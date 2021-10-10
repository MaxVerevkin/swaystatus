//! The brightness of a backlight device
//!
//! This block reads brightness information directly from the filesystem, so it works under both
//! X11 and Wayland. The block uses `inotify` to listen for changes in the device's brightness
//! directly, so there is no need to set an update interval. This block uses DBus to set brightness
//! level using the mouse wheel.
//!
//! # Root scaling
//!
//! Some devices expose raw values that are best handled with nonlinear scaling. The human perception of lightness is close to the cube root of relative luminance, so settings for `root_scaling` between 2.4 and 3.0 are worth trying. For devices with few discrete steps this should be 1.0 (linear). More information: <https://en.wikipedia.org/wiki/Lightness>
//!
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `device` | The `/sys/class/backlight` device to read brightness information from.  When there is no `device` specified, this block will display information from the first device found in the `/sys/class/backlight` directory. If you only have one display, this approach should find it correctly.| No | Default device
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"{brightness}"`
//! `step_width` | The brightness increment to use when scrolling, in percent | No | `5`
//! `root_scaling` | Scaling exponent reciprocal (ie. root) | No | `1.0`
//! `invert_icons` | Invert icons' ordering, useful if you have colorful emoji | No | `false`
//!
//! Placeholder    | Value              | Type     | Unit
//! ---------------|--------------------|----------|---------------
//! `{brightness}` | Current brightness | Interger | Percents
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "backlight"
//! device = "intel_backlight"
//! ```

use std::cmp::max;
use std::convert::TryInto;
use std::ops::Range;
use std::path::{Path, PathBuf};

use inotify::{Inotify, WatchMask};
use serde_derive::Deserialize;
use tokio::fs::read_dir;
use tokio_stream::StreamExt;

use super::prelude::*;
use crate::util::read_file;

#[zbus::dbus_proxy(
    interface = "org.mpris.MediaPlayer2",
    default_service = "org.freedesktop.login1",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Session {
    fn set_brightness(&self, subsystem: &str, name: &str, brightness: u32) -> zbus::Result<()>;
}

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

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct BacklightConfig {
    pub device: Option<String>,
    pub format: FormatTemplate,
    pub step_width: u8,
    pub root_scaling: f64,
    pub invert_icons: bool,
}

impl Default for BacklightConfig {
    fn default() -> Self {
        Self {
            device: None,
            format: Default::default(),
            step_width: 5,
            root_scaling: 1f64,
            invert_icons: false,
        }
    }
}

/// Read a brightness value from the given path.
async fn read_brightness_raw(device_file: &Path) -> Result<u64> {
    read_file(device_file)
        .await
        .block_error("backlight", "Failed to read brightness file")?
        .parse::<u64>()
        .block_error("backlight", "Failed to read value from brightness file")
}

/// Represents a physical backlit device whose brightness level can be queried.
pub struct BacklightDevice<'a> {
    device_name: String,
    brightness_file: PathBuf,
    max_brightness: u64,
    root_scaling: f64,
    dbus_proxy: SessionProxy<'a>,
}

impl<'a> BacklightDevice<'a> {
    async fn new(
        device_path: PathBuf,
        root_scaling: f64,
        dbus_conn: &'a zbus::Connection,
    ) -> Result<BacklightDevice<'a>> {
        Ok(Self {
            brightness_file: device_path.join({
                if device_path.ends_with("amdgpu_bl0") {
                    FILE_BRIGHTNESS_AMD
                } else {
                    FILE_BRIGHTNESS
                }
            }),
            device_name: device_path
                .file_name()
                .map(|x| x.to_str().unwrap().to_string())
                .block_error("backlight", "Malformed device path")?,
            max_brightness: read_brightness_raw(&device_path.join(FILE_MAX_BRIGHTNESS)).await?,
            root_scaling: root_scaling.clamp(ROOT_SCALDING_RANGE.start, ROOT_SCALDING_RANGE.end),
            dbus_proxy: SessionProxy::new(dbus_conn)
                .await
                .block_error("backlight", "failed to create SessionProxy")?,
        })
    }

    /// Use the default backlit device, i.e. the first one found in the
    /// `/sys/class/backlight` directory.
    pub async fn default(
        root_scaling: f64,
        dbus_conn: &'a zbus::Connection,
    ) -> Result<BacklightDevice<'a>> {
        let device = read_dir(DEVICES_PATH)
            .await
            .block_error("backlight", "Failed to read backlight device directory")?
            .next_entry()
            .await
            .block_error("backlight", "No backlit devices found")?
            .block_error("backlight", "Failed to read default device file")?;
        Self::new(device.path(), root_scaling, dbus_conn).await
    }

    /// Use the backlit device `device`. Returns an error if a directory for
    /// that device is not found.
    pub async fn from_device(
        device: &str,
        root_scaling: f64,
        dbus_conn: &'a zbus::Connection,
    ) -> Result<BacklightDevice<'a>> {
        Self::new(
            Path::new(DEVICES_PATH).join(device),
            root_scaling,
            dbus_conn,
        )
        .await
    }

    /// Query the brightness value for this backlit device, as a percent.
    pub async fn brightness(&self) -> Result<u8> {
        let raw = read_brightness_raw(&self.brightness_file).await?;

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
        let raw = max(1, (ratio * (self.max_brightness as f64)).round() as u32);
        self.dbus_proxy
            .set_brightness("backlight", &self.device_name, raw)
            .await
            .block_error("backlight", "Failed to send D-Bus message")
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, events: EventsRxGetter) -> BlockHandle {
    let mut events = events();
    tokio::spawn(async move {
        let block_config =
            BacklightConfig::deserialize(block_config).block_config_error("backlight")?;
        let format = block_config.format.or_default("{brightness}")?;
        let dbus_conn = api.shared_dbus_connection().await?;

        let device = match &block_config.device {
            None => BacklightDevice::default(block_config.root_scaling, &dbus_conn).await?,
            Some(path) => {
                BacklightDevice::from_device(path, block_config.root_scaling, &dbus_conn).await?
            }
        };

        // Watch for brightness changes
        let mut notify = Inotify::init().block_error("backlight", "Failed to start inotify")?;
        let mut buffer = [0; 1024];

        notify
            .add_watch(&device.brightness_file, WatchMask::MODIFY)
            .block_error("backlight", "Failed to watch brightness file")?;

        let mut file_changes = notify
            .event_stream(&mut buffer)
            .block_error("backlight", "Failed to create event stream")?;

        let mut text = api.new_widget();

        loop {
            let brightness = device.brightness().await?;
            let mut icon_index = (usize::from(brightness) * BACKLIGHT_ICONS.len()) / 101;
            if block_config.invert_icons {
                icon_index = BACKLIGHT_ICONS.len() - icon_index;
            }

            text.set_icon(BACKLIGHT_ICONS[icon_index])?;
            text.set_text(format.render(&map! {
                "brightness" => Value::from_integer(brightness as i64).percents(),
            })?);
            api.send_widgets(vec![text.get_data()]).await?;

            tokio::select! {
                _ = file_changes.next() => (),
                Some(BlockEvent::I3Bar(event)) = events.recv() => {
                    let brightness = device.brightness().await?;
                    match event.button {
                        MouseButton::WheelUp => {
                            device
                                .set_brightness(brightness + block_config.step_width)
                                .await?;
                        }
                        MouseButton::WheelDown => {
                            device
                                .set_brightness(brightness.saturating_sub(block_config.step_width))
                                .await?;
                        }
                        _ => (),
                    }
                }
            }
        }
    })
}
