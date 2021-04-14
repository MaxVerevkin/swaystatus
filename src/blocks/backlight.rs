//! A block for displaying the brightness of a backlit device.
//!
//! This module contains the [`Backlight`](./struct.Backlight.html) block, which
//! can display the brightness level of physical backlit devices. Brightness
//! levels are read from and written to the `sysfs` filesystem, so this block
//! does not depend on `xrandr` (and thus it works on Wayland). To set
//! brightness levels using `xrandr`, see the
//! [`Xrandr`](../xrandr/struct.Xrandr.html) block.

use std::cmp::max;
use std::convert::TryInto;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use inotify::{Inotify, WatchMask};
use serde::de::Deserialize;
use serde_derive::Deserialize;
use tokio::fs::{read_dir, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::blocks::{BlockEvent, BlockMessage};
use crate::config::LogicalDirection;
use crate::config::SharedConfig;
use crate::errors::{OptionExt, Result, ResultExt};
use crate::widgets::text::TextWidget;
use crate::widgets::I3BarWidget;

/// Location of backlight devices
const DEVICES_PATH: &str = "/sys/class/backlight";

/// Filename for device's max brightness
const FILE_MAX_BRIGHTNESS: &str = "max_brightness";

/// Filename for current brightness.
const FILE_BRIGHTNESS: &str = "actual_brightness";

/// amdgpu drivers set the actual_brightness in a different scale than
/// [0, max_brightness], so we have to use the 'brightness' file instead.
/// This may be fixed in the new 5.7 kernel?
const FILE_BRIGHTNESS_AMD: &str = "brightness";

/// Range of valid values for `root_scaling`
const ROOT_SCALDING_RANGE: Range<f64> = 0.1..10.;

/// Ordered list of icons used to display lighting progress
const BACKLIGHT_ICONS: &[&str] = &[
    "backlight_empty",
    "backlight_1",
    "backlight_2",
    "backlight_3",
    "backlight_4",
    "backlight_5",
    "backlight_6",
    "backlight_7",
    "backlight_8",
    "backlight_9",
    "backlight_10",
    "backlight_11",
    "backlight_12",
    "backlight_13",
    "backlight_full",
];

/// Read a brightness value from the given path.
async fn read_brightness_raw(device_file: &Path) -> Result<u64> {
    let mut content = String::new();
    let mut file = OpenOptions::new()
        .read(true)
        .open(device_file)
        .await
        .block_error("backlight", "Failed to open brightness file")?;

    file.read_to_string(&mut content)
        .await
        .block_error("backlight", "Failed to read brightness file")?;

    content
        .trim_end()
        .parse::<u64>()
        .block_error("backlight", "Failed to read value from brightness file")
}

/// Represents a physical backlit device whose brightness level can be queried.
pub struct BacklitDevice {
    device_path: PathBuf,
    max_brightness: u64,
    root_scaling: f64,
    dbus_proxy: dbus::nonblock::Proxy<'static, Arc<dbus::nonblock::LocalConnection>>,
}

impl BacklitDevice {
    fn new(max_brightness: u64, device_path: PathBuf, root_scaling: f64) -> Result<Self> {
        let (ressource, dbus_conn) = dbus_tokio::connection::new_system_local()
            .block_error("backlight", "Failed to open dbus connection")?;

        tokio::task::spawn_local(async move {
            let err = ressource.await;
            panic!("Lost connection to D-Bus: {}", err);
        });

        let dbus_proxy = dbus::nonblock::Proxy::new(
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            Duration::from_secs(2),
            dbus_conn,
        );

        Ok(Self {
            max_brightness,
            device_path,
            root_scaling: {
                if ROOT_SCALDING_RANGE.contains(&root_scaling) {
                    root_scaling
                } else {
                    ROOT_SCALDING_RANGE.end
                }
            },
            dbus_proxy,
        })
    }

    async fn from_path(device_path: PathBuf, root_scaling: f64) -> Result<Self> {
        let max_brightness = read_brightness_raw(&device_path.join(FILE_MAX_BRIGHTNESS)).await?;
        Self::new(max_brightness, device_path, root_scaling)
    }

    /// Use the default backlit device, i.e. the first one found in the
    /// `/sys/class/backlight` directory.
    pub async fn default(root_scaling: f64) -> Result<Self> {
        let device = read_dir(DEVICES_PATH)
            .await
            .block_error("backlight", "Failed to read backlight device directory")?
            .next_entry()
            .await
            .block_error("backlight", "No backlit devices found")?
            .block_error("backlight", "Failed to read default device file")?;

        Self::from_path(device.path(), root_scaling).await
    }

    /// Use the backlit device `device`. Returns an error if a directory for
    /// that device is not found.
    pub async fn from_device(device: &str, root_scaling: f64) -> Result<Self> {
        Self::from_path(Path::new(DEVICES_PATH).join(device), root_scaling).await
    }

    /// Query the brightness value for this backlit device, as a percent.
    pub async fn brightness(&self) -> Result<u8> {
        let raw = read_brightness_raw(&self.brightness_file()).await?;

        let brightness_ratio =
            (raw as f64 / self.max_brightness as f64).powf(self.root_scaling.recip());

        ((brightness_ratio * 100.0).round() as i64)
            .try_into()
            .ok()
            .filter(|brightness| (0..=100).contains(brightness))
            .block_error("backlight", "Brightness is not in [0, 100]")
    }

    /// Set the brightness value for this backlit device, as a percent.
    pub async fn set_brightness(&self, value: u8) -> Result<()> {
        let value = value.clamp(0, 100);
        let ratio = (value as f64 / 100.0).powf(self.root_scaling);
        let raw = max(1, (ratio * (self.max_brightness as f64)).round() as u64);

        eprintln!("{}", self.device_path.join("brightness").display());

        if let Ok(mut file) = OpenOptions::new()
            .write(true)
            .open(self.brightness_file())
            .await
            .map_err(|err| eprintln!("{:?}", err))
        {
            file.write_all(format!("{}", raw).as_bytes())
                .await
                .block_error("backlight", "Failed to write into brightness file")?;

            Ok(())
        } else {
            self.set_brightness_via_dbus(raw).await
        }
    }

    async fn set_brightness_via_dbus(&self, raw_value: u64) -> Result<()> {
        let device_name = self
            .device_path
            .file_name()
            .and_then(|x| x.to_str())
            .block_error("backlight", "Malformed device path")?;

        self.dbus_proxy
            .method_call(
                "org.freedesktop.login1.Session",
                "SetBrightness",
                ("backlight", device_name, raw_value as u32),
            )
            .await
            .block_error("backlight", "Failed to send D-Bus message")?;

        Ok(())
    }

    /// The brightness file itself.
    fn brightness_file(&self) -> PathBuf {
        self.device_path.join({
            if self.device_path.ends_with("amdgpu_bl0") {
                FILE_BRIGHTNESS_AMD
            } else {
                FILE_BRIGHTNESS
            }
        })
    }
}

/// Configuration for the [`Backlight`](./struct.Backlight.html) block.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct BacklightConfig {
    /// The backlight device in `/sys/class/backlight/` to read brightness from.
    pub device: Option<String>,

    /// The steps brightness is in/decreased for the selected screen.
    /// When greater than 50 it gets limited to 50.
    pub step_width: u8,

    /// Scaling exponent reciprocal (ie. root). Some devices expose raw values
    /// that are best handled with nonlinear scaling. The human perception of
    /// lightness is close to the cube root of relative luminance. Settings
    /// between 2.4 and 3.0 are worth trying.
    /// More information: <https://en.wikipedia.org/wiki/Lightness>
    ///
    /// For devices with few discrete steps this should be 1.0 (linear).
    pub root_scaling: f64,

    /// Invert the ordering of displayed icons.
    pub invert_icons: bool,
}

impl Default for BacklightConfig {
    fn default() -> Self {
        Self {
            device: None,
            step_width: 5,
            root_scaling: 1f64,
            invert_icons: false,
        }
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    mut events_receiver: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let block_config =
        BacklightConfig::deserialize(block_config).block_config_error("backlight")?;

    let device = match &block_config.device {
        None => BacklitDevice::default(block_config.root_scaling).await?,
        Some(path) => BacklitDevice::from_device(path, block_config.root_scaling).await?,
    };

    // Render and send widget
    let update = || async {
        let brightness = device.brightness().await?;
        let mut icon_index = (usize::from(brightness) * BACKLIGHT_ICONS.len()) / 101;

        if block_config.invert_icons {
            icon_index = BACKLIGHT_ICONS.len() - icon_index;
        }

        let widget = TextWidget::new(id, 0, shared_config.clone())
            .with_text(&format!("{}%", brightness))
            .with_icon(BACKLIGHT_ICONS[icon_index])?
            .get_data();

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![widget],
            })
            .await
            .internal_error("backlight", "failed to send message")?;

        Ok(())
    };

    // Initial block value
    update().await?;

    // Watch for brightness changes
    let mut notify = Inotify::init().block_error("backlight", "Failed to start inotify")?;
    let mut buffer = [0; 1024];

    notify
        .add_watch(device.brightness_file(), WatchMask::MODIFY)
        .block_error("backlight", "Failed to watch brightness file")?;

    let mut file_changes = notify
        .event_stream(&mut buffer)
        .block_error("backlight", "Failed to create event stream")?;

    loop {
        tokio::select! {
            _ = file_changes.next() => update().await?,
            Some(BlockEvent::I3Bar(event)) = events_receiver.recv() => {
                let brightness = device.brightness().await?;

                match shared_config.scrolling.to_logical_direction(event.button) {
                    Some(LogicalDirection::Up) => {
                        device
                            .set_brightness(brightness + block_config.step_width)
                            .await?;
                    }
                    Some(LogicalDirection::Down) => {
                        device
                            .set_brightness(brightness.saturating_sub(block_config.step_width))
                            .await?;
                    }
                    None => {}
                }
            }
        }
    }
}
