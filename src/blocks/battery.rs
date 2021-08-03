//! Information about an internal power supply
//!
//! This block can display the current battery state (Full, Charging or Discharging), percentage
//! charged and estimate time until (dis)charged for an internal power supply.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `device` | The device in `/sys/class/power_supply/` to read from. When using UPower, this can also be `"DisplayDevice"`. | No | Any battery device
//! `driver` | One of `"sysfs"` or `"upower"` | No | `"sysfs"`
//! `interval` | Update interval, in seconds. Only relevant for `driver = "sysfs"`. | No | `10`
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"{percentage}"`
//! `full_format` | Same as `format` but for when the battery is full | No | `""`
//! `missing_format` | Same as `format` but for when the specified battery is missing | No | `"{percentage}"`
//! `allow_missing` | Don't display errors when the battery cannot be found. Only works with the `sysfs` driver. | No | `false`
//! `hide_missing` | Completely hide this block if the battery cannot be found. Only works in combination with `allow_missing`. | No | `false`
//! `info` | Minimum battery level, where state is set to info | No | `60`
//! `good` | Minimum battery level, where state is set to good | No | `60`
//! `warning` | Minimum battery level, where state is set to warning | No | `30`
//! `critical` | Minimum battery level, where state is set to critical | No | `15`
//!
//! Placeholder    | Value                                                                   | Type              | Unit
//! ---------------|-------------------------------------------------------------------------|-------------------|-----
//! `{percentage}` | Battery level, in percent                                               | String or Integer | Percents
//! `{time}`       | Time remaining until (dis)charge is complete                            | String            | -
//! `{power}`      | Power consumption by the battery or from the power supply when charging | String or Float   | Watts
//!
//! # TODO
//! - Use `inotify` for `sysfs` dirver
//! - remove `interval` option

use std::convert::TryInto;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
use futures::StreamExt;
use serde::de::Deserialize;
use serde_derive::Deserialize;
use tokio::fs::{read_dir, read_to_string};
use tokio::sync::mpsc;
use tokio::time::{Instant, Interval};

use crate::blocks::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::formatting::value::Value;
use crate::formatting::FormatTemplate;
use crate::util::read_file;
use crate::widgets::widget::Widget;
use crate::widgets::{Spacing, State};

/// Path for the power supply devices
const POWER_SUPPLY_DEVICES_PATH: &str = "/sys/class/power_supply";

/// Ordered list of icons used to display battery charge
const BATTERY_CHARGE_ICONS: &[&str] = &[
    "bat_empty",
    "bat_quarter",
    "bat_half",
    "bat_three_quarters",
    "bat_full",
];

// Specialized battery icons
const BATTERY_EMPTY_ICON: &str = "bat_empty";
const BATTERY_FULL_ICON: &str = "bat_full";
const BATTERY_UNAVAILABLE_ICON: &str = "bat_not_available";

// DBUS properties for UPower
const UPOWER_DBUS_NAME: &str = "org.freedesktop.UPower";
const UPOWER_DBUS_ROOT_INTERFACE: &str = "org.freedesktop.UPower";
const UPOWER_DBUS_DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
const UPOWER_DBUS_PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const UPOWER_DBUS_ROOT_PATH: &str = "/org/freedesktop/UPower";

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct BatteryConfig {
    device: Option<String>,
    driver: BatteryDriver,
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    format: FormatTemplate,
    full_format: FormatTemplate,
    missing_format: FormatTemplate,
    allow_missing: bool,
    hide_missing: bool,
    info: u8,
    good: u8,
    warning: u8,
    critical: u8,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        Self {
            device: None,
            driver: BatteryDriver::Sysfs,
            interval: Duration::from_secs(10),
            format: Default::default(),
            full_format: Default::default(),
            missing_format: Default::default(),
            allow_missing: false,
            hide_missing: false,
            info: 60,
            good: 60,
            warning: 30,
            critical: 15,
        }
    }
}

/// Read value from a file, return None if the file does not exist
async fn read_value_from_file<P: AsRef<Path>, T: FromStr>(path: P) -> Result<Option<T>>
where
    T::Err: std::error::Error,
{
    match read_file(path.as_ref()).await {
        Ok(raw) => Ok(Some(
            raw.parse().block_error("battery", "failed to parse file")?,
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).block_error("battery", "Failed while try to read charge_full"),
    }
}

// ---
// --- BatteryDevice
// ---

/// A battery device can be queried for a few properties relevant to the user.
#[async_trait]
trait BatteryDevice {
    /// Query whether the device is available. Batteries can be hot-swappable
    /// and configurations may be used for multiple devices (desktop AND laptop).
    async fn is_available(&self) -> bool;

    async fn capacity(&self) -> Result<u8>;
    async fn usage(&self) -> Result<f64>;
    async fn status(&self) -> Result<BatteryStatus>;
    async fn time_remaining(&self) -> Result<u64>;
    async fn wait_for_change(&mut self) -> Result<()>;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BatteryStatus {
    Charging,
    Discharging,
    Empty,
    Full,
    NotCharging,
    Unknown,
}

impl Default for BatteryStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

impl FromStr for BatteryStatus {
    type Err = crate::errors::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "Charging" => Self::Charging,
            "Discharging" => Self::Discharging,
            "Empty" => Self::Empty,
            "Full" => Self::Full,
            "Not charging" => Self::NotCharging,
            _ => Self::Unknown,
        })
    }
}

// ---
// --- PowerSupplyDevice
// ---

/// Represents a physical power supply device, as known to sysfs.
struct PowerSupplyDevice {
    device_path: PathBuf,
    interval: Interval,
}

impl PowerSupplyDevice {
    fn from_device(device: &str, interval: Duration) -> Self {
        let interval = tokio::time::interval_at(Instant::now() + interval, interval);

        Self {
            device_path: Path::new(POWER_SUPPLY_DEVICES_PATH).join(device),
            interval,
        }
    }

    async fn charge_now(&self) -> Result<u64> {
        let (charge, energy) = tokio::join!(
            read_value_from_file(self.device_path.join("charge_now")),
            read_value_from_file(self.device_path.join("energy_now"))
        );

        Ok((charge.transpose())
            .or_else(|| energy.transpose())
            .block_error("battery", "No file to read current charge")??)
    }

    async fn charge_full(&self) -> Result<u64> {
        let (charge, energy) = tokio::join!(
            read_value_from_file(self.device_path.join("charge_full")),
            read_value_from_file(self.device_path.join("energy_full"))
        );

        Ok((charge.transpose())
            .or_else(|| energy.transpose())
            .block_error("battery", "No file to read full charge")??)
    }
}

#[async_trait]
impl BatteryDevice for PowerSupplyDevice {
    async fn is_available(&self) -> bool {
        read_dir(&self.device_path).await.is_ok()
    }

    async fn status(&self) -> Result<BatteryStatus> {
        read_value_from_file(self.device_path.join("status"))
            .await?
            .block_error("battery", "status is not available")
    }

    async fn capacity(&self) -> Result<u8> {
        let (capacity, charge_now, charge_full) = tokio::join!(
            read_value_from_file(self.device_path.join("capacity")),
            self.charge_now(),
            self.charge_full(),
        );

        let capacity = (capacity.ok().flatten())
            .or_else(|| Some(100 * charge_now.ok()? / charge_full.ok()?))
            .block_error("battery", "Failed to read capacity, charge, or energy")?;

        Ok(capacity.clamp(0, 100) as u8)
    }

    async fn usage(&self) -> Result<f64> {
        match tokio::join!(
            read_value_from_file(self.device_path.join("power_now")), // µWh
            read_value_from_file::<_, f64>(self.device_path.join("current_now")), // µA
            read_value_from_file::<_, f64>(self.device_path.join("voltage_now")), // µV
        ) {
            (Ok(Some(power)), _, _) => Ok(power),
            (_, Ok(Some(current)), Ok(Some(voltage))) => Ok((current * voltage) / 1e6),
            _ => block_error("battery", "Device does not support power consumption"),
        }
    }

    async fn time_remaining(&self) -> Result<u64> {
        let (time_to_empty, time_to_full, status, charge_now, charge_full, usage) = tokio::join!(
            read_value_from_file(self.device_path.join("time_to_empty_now")),
            read_value_from_file(self.device_path.join("time_to_full_now")),
            self.status(),
            self.charge_now(),
            self.charge_full(),
            self.usage(),
        );

        let time_to_empty = time_to_empty.ok().flatten();
        let time_to_full = time_to_full.ok().flatten();

        match status? {
            BatteryStatus::Discharging => time_to_empty
                .or_else(|| Some((60. * charge_now.ok()? as f64 / usage.ok()?) as u64))
                .block_error("battery", "No method supported to calculate time to empty"),
            BatteryStatus::Charging => time_to_full
                .or_else(|| {
                    Some((60. * (charge_full.ok()? - charge_now.ok()?) as f64 / usage.ok()?) as u64)
                })
                .block_error("battery", "No method supported to calculate time to full"),
            _ => {
                // TODO: What should we return in this case? It seems that under
                // some conditions sysfs will return 0 for some readings (energy
                // or power), so perhaps the most natural thing to do is emulate
                // that.
                Ok(0)
            }
        }
    }

    async fn wait_for_change(&mut self) -> Result<()> {
        self.interval.tick().await;
        Ok(())
    }
}

// ---
// --- UpowerDevice
// ---

pub struct UPowerDevice {
    dbus_conn: Arc<dbus::nonblock::SyncConnection>,
    dbus_proxy: dbus::nonblock::Proxy<'static, Arc<dbus::nonblock::SyncConnection>>,
    device_path: dbus::Path<'static>,
}

impl UPowerDevice {
    async fn from_device(device: &str) -> Result<Self> {
        let (ressource, dbus_conn) = dbus_tokio::connection::new_system_sync()
            .block_error("battery", "Failed to open dbus connection")?;

        tokio::spawn(async move {
            let err = ressource.await;
            panic!("Lost connection to D-Bus: {}", err);
        });

        // Fetch device name
        let device_path = {
            if device == "DisplayDevice" {
                format!("{}/devices/DisplayDevice", UPOWER_DBUS_ROOT_PATH).into()
            } else {
                let (paths,): (Vec<dbus::Path>,) = {
                    dbus::nonblock::Proxy::new(
                        UPOWER_DBUS_NAME,
                        UPOWER_DBUS_ROOT_PATH,
                        Duration::from_secs(2),
                        dbus_conn.clone(),
                    )
                    .method_call(UPOWER_DBUS_ROOT_INTERFACE, "EnumerateDevices", ())
                    .await
                    .block_error("battery", "Failed to retrieve DBus devices")?
                };

                paths
                    .into_iter()
                    .find(|entry| entry.ends_with(device))
                    .block_error("battery", "UPower device could not be found")?
            }
        };

        let dbus_proxy = dbus::nonblock::Proxy::new(
            UPOWER_DBUS_NAME,
            device_path.clone(),
            Duration::from_secs(2),
            dbus_conn.clone(),
        );

        // Verify device name
        let upower_type: u32 = dbus_proxy
            .get(UPOWER_DBUS_DEVICE_INTERFACE, "Type")
            .await
            .block_error("battery", "Failed to read UPower Type property")?;

        // https://upower.freedesktop.org/docs/Device.html#Device:Type
        // consider any peripheral, UPS and internal battery
        if upower_type == 1 {
            return block_error("battery", "UPower device is not a battery.");
        }

        Ok(Self {
            dbus_conn,
            dbus_proxy,
            device_path,
        })
    }
}

#[async_trait]
impl BatteryDevice for UPowerDevice {
    async fn is_available(&self) -> bool {
        true
    }

    async fn capacity(&self) -> Result<u8> {
        let capacity: f64 = self
            .dbus_proxy
            .get(UPOWER_DBUS_DEVICE_INTERFACE, "Percentage")
            .await
            .block_error("battery", "Failed to read UPower Percentage property.")?;

        Ok(capacity.clamp(0., 100.) as u8)
    }

    async fn usage(&self) -> Result<f64> {
        let usage: f64 = self
            .dbus_proxy
            .get(UPOWER_DBUS_DEVICE_INTERFACE, "EnergyRate")
            .await
            .block_error("battery", "Failed to read UPower EnergyRate property.")?;

        Ok(1e6 * usage)
    }

    async fn status(&self) -> Result<BatteryStatus> {
        let status: u32 = self
            .dbus_proxy
            .get(UPOWER_DBUS_DEVICE_INTERFACE, "State")
            .await
            .block_error("battery", "Failed to read UPower State property.")?;

        Ok(match status {
            1 => BatteryStatus::Charging,
            2 | 6 => BatteryStatus::Discharging,
            3 => BatteryStatus::Empty,
            4 => BatteryStatus::Full,
            5 => BatteryStatus::NotCharging,
            _ => BatteryStatus::Unknown,
        })
    }

    async fn time_remaining(&self) -> Result<u64> {
        let property = match self.status().await? {
            BatteryStatus::Charging => "TimeToFull",
            _ => "TimeToEmpty",
        };

        let time_to_empty: i64 = self
            .dbus_proxy
            .get(UPOWER_DBUS_DEVICE_INTERFACE, property)
            .await
            .block_error("battery", "Failed to read UPower Time")?;

        Ok((time_to_empty / 60)
            .try_into()
            .block_error("battery", "Got a negative time to completion fro DBus")?)
    }

    async fn wait_for_change(&mut self) -> Result<()> {
        // Setup signal monitoring
        let mut match_rule = dbus::message::MatchRule::new_signal(
            UPOWER_DBUS_PROPERTIES_INTERFACE,
            "PropertiesChanged",
        );

        match_rule.path.replace(self.device_path.clone());

        let (incoming_signal, mut stream) = self
            .dbus_conn
            .add_match(match_rule)
            .await
            .block_error("battery", "Failed to add D-Bus match rule.")?
            .msg_stream();

        // Wait for signal
        stream.next().await;

        // Release match rule
        self.dbus_conn
            .remove_match(incoming_signal.token())
            .await
            .block_error("battery", "Failed to remove D-Bus match rule.")?;

        Ok(())
    }
}

// ---
// --- Block
// ---

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
enum BatteryDriver {
    Sysfs,
    Upower,
}

impl Default for BatteryDriver {
    fn default() -> Self {
        BatteryDriver::Sysfs
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_receiver: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    std::mem::drop(events_receiver);
    let block_config = BatteryConfig::deserialize(block_config).block_config_error("battery")?;

    let format = block_config.format.clone().or_default("{percentage}")?;
    let format_full = block_config.full_format.clone().or_default("")?;
    let format_missing = block_config
        .missing_format
        .clone()
        .or_default("{percentage}")?;

    // Get _any_ battery device if not set in the config
    let device = match block_config.device {
        Some(d) => d,
        None => {
            let mut sysfs_dir = read_dir("/sys/class/power_supply")
                .await
                .block_error("battery", "failed to read /sys/class/power_supply direcory")?;
            let mut device = None;
            while let Some(dir) = sysfs_dir
                .next_entry()
                .await
                .block_error("battery", "failed to read /sys/class/power_supply direcory")?
            {
                if read_to_string(dir.path().join("type"))
                    .await
                    .map(|t| t.trim() == "Battery")
                    .unwrap_or(false)
                {
                    device = Some(dir.file_name().to_str().unwrap().to_string());
                    break;
                }
            }
            device.block_error("battery", "failed to determine default battery - please set your battery device in the configuration file")?
        }
    };

    let mut device: Box<dyn BatteryDevice + Send> = match block_config.driver {
        BatteryDriver::Sysfs => Box::new(PowerSupplyDevice::from_device(
            &device,
            block_config.interval,
        )),
        BatteryDriver::Upower => Box::new(UPowerDevice::from_device(&device).await?),
    };

    loop {
        let (is_available, status, capacity, time, power) = tokio::join!(
            device.is_available(),
            device.status(),
            device.capacity(),
            device.time_remaining(),
            device.usage()
        );

        let fmt = match status.clone() {
            Err(_) => &format_missing,
            Ok(BatteryStatus::Full) => &format_full,
            _ => &format,
        };

        let vars = {
            if !is_available && block_config.allow_missing {
                map! {
                    "percentage" => Value::from_string("X".to_string()),
                    "time" => Value::from_string("xx:xx".to_string()),
                    "power" => Value::from_string("N/A".to_string()),
                }
            } else {
                map! {
                    "percentage" => capacity.clone()
                        .map(|c| Value::from_integer(c as i64).percents())
                        .unwrap_or_else(|_| Value::from_string("×".to_string())),
                    "time" => time
                        .map(|time| {
                            if time == 0 {
                                Value::from_string("".to_string())
                            } else {
                                Value::from_string(format!(
                                    "{}:{:02}",
                                    (time / 60).clamp(0, 99),
                                    time % 60,
                                ))
                            }
                        })
                        .unwrap_or_else(|_| Value::from_string("×".to_string())),
                    "power" => power
                        .map(|power| Value::from_float(power / 1e6).watts())
                        .unwrap_or_else(|_| Value::from_string("×".to_string())),
                }
            }
        };

        let widget = match (
            status.unwrap_or_default(),
            capacity.ok().map(|c| c.clamp(0, 100)),
        ) {
            (BatteryStatus::Empty, _) => Widget::new(id, shared_config.clone())
                .with_icon(BATTERY_EMPTY_ICON)?
                .with_state(State::Critical)
                .with_spacing(Spacing::Hidden),
            (BatteryStatus::Full, _) => Widget::new(id, shared_config.clone())
                .with_icon(BATTERY_FULL_ICON)?
                .with_spacing(Spacing::Hidden),
            (status, Some(charge)) => {
                let index = (charge as usize * BATTERY_CHARGE_ICONS.len()) / 101;
                let icon = BATTERY_CHARGE_ICONS[index];

                let state = {
                    if status == BatteryStatus::Charging {
                        State::Good
                    } else if charge <= block_config.critical {
                        State::Critical
                    } else if charge <= block_config.warning {
                        State::Warning
                    } else if charge <= block_config.info {
                        State::Info
                    } else if charge > block_config.good {
                        State::Good
                    } else {
                        State::Idle
                    }
                };

                Widget::new(id, shared_config.clone())
                    .with_text(fmt.render(&vars)?)
                    .with_icon(icon)?
                    .with_state(state)
            }
            _ => Widget::new(id, shared_config.clone())
                .with_icon(BATTERY_UNAVAILABLE_ICON)?
                .with_state(State::Warning)
                .with_spacing(Spacing::Hidden),
        };

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![widget.get_data()],
            })
            .await
            .internal_error("backlight", "failed to send message")?;

        let x = device.wait_for_change();
        x.await?;
    }
}
