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
//! `hide_missing` | Hide the block if battery is full | No | `false`
//! `info` | Minimum battery level, where state is set to info | No | `60`
//! `good` | Minimum battery level, where state is set to good | No | `60`
//! `warning` | Minimum battery level, where state is set to warning | No | `30`
//! `critical` | Minimum battery level, where state is set to critical | No | `15`
//! `full_threshold` | Percentage at which the battery is considered full (`full_format` shown) | No | `100`
//!
//! Placeholder    | Value                                                                   | Type              | Unit
//! ---------------|-------------------------------------------------------------------------|-------------------|-----
//! `{percentage}` | Battery level, in percent                                               | String or Integer | Percents
//! `{time}`       | Time remaining until (dis)charge is complete                            | String            | -
//! `{power}`      | Power consumption by the battery or from the power supply when charging | String or Float   | Watts

use std::convert::TryInto;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;

use serde_derive::Deserialize;
use tokio::fs::{read_dir, read_to_string};
use tokio::time::{Instant, Interval};
use zbus::fdo::DBusProxy;
use zbus::MessageStream;

use super::prelude::*;
use crate::de::deserialize_duration;
use crate::util::read_file;

mod zbus_upower;

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
    hide_full: bool,
    info: u8,
    good: u8,
    warning: u8,
    critical: u8,
    full_threshold: u8,
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
            hide_full: false,
            info: 60,
            good: 60,
            warning: 30,
            critical: 15,
            full_threshold: 100,
        }
    }
}

/// Read value from a file, return None if the file does not exist
async fn read_value_from_file<P: AsRef<Path>, T: FromStr>(path: P) -> Result<Option<T>>
where
    T::Err: StdError + Send + Sync + 'static,
{
    match read_file(path.as_ref()).await {
        Ok(raw) => Ok(Some(raw.parse().error("failed to parse file")?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).error("failed while try to read charge_full"),
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
            .error("No file to read current charge")??)
    }

    async fn charge_full(&self) -> Result<u64> {
        let (charge, energy) = tokio::join!(
            read_value_from_file(self.device_path.join("charge_full")),
            read_value_from_file(self.device_path.join("energy_full"))
        );

        Ok((charge.transpose())
            .or_else(|| energy.transpose())
            .error("No file to read full charge")??)
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
            .error("status is not available")
    }

    async fn capacity(&self) -> Result<u8> {
        let (capacity, charge_now, charge_full) = tokio::join!(
            read_value_from_file(self.device_path.join("capacity")),
            self.charge_now(),
            self.charge_full(),
        );

        let capacity = (capacity.ok().flatten())
            .or_else(|| Some(100 * charge_now.ok()? / charge_full.ok()?))
            .error("Failed to read capacity, charge, or energy")?;

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
            _ => Err(Error::new("Device does not support power consumption")),
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
                .error("No method supported to calculate time to empty"),
            BatteryStatus::Charging => time_to_full
                .or_else(|| {
                    Some((60. * (charge_full.ok()? - charge_now.ok()?) as f64 / usage.ok()?) as u64)
                })
                .error("No method supported to calculate time to full"),
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

pub struct UPowerDevice<'a> {
    device_proxy: zbus_upower::DeviceProxy<'a>,
    changes: MessageStream,
}

impl<'a> UPowerDevice<'a> {
    async fn from_device(
        device: &str,
        dbus_conn: &'a zbus::Connection,
    ) -> Result<UPowerDevice<'a>> {
        // Fetch device path
        let device_path = {
            if device == "DisplayDevice" {
                "/org/freedesktop/UPower/devices/DisplayDevice"
                    .try_into()
                    .unwrap()
            } else {
                zbus_upower::UPowerProxy::new(dbus_conn)
                    .await
                    .error("Failed to create UPwerProxy")?
                    .enumerate_devices()
                    .await
                    .error("Failed to retrieve UPower devices")?
                    .into_iter()
                    .find(|entry| entry.ends_with(device))
                    .error("UPower device could not be found")?
            }
        };

        let device_proxy = zbus_upower::DeviceProxy::builder(dbus_conn)
            .path(device_path.clone())
            .error("Failed to set proxy's path")?
            .build()
            .await
            .error("Failed to create DeviceProxy")?;

        // Verify device name
        // https://upower.freedesktop.org/docs/Device.html#Device:Type
        // consider any peripheral, UPS and internal battery
        let device_type = device_proxy
            .type_()
            .await
            .error("Failed to get device's type")?;
        if device_type == 1 {
            return Err(Error::new("UPower device is not a battery."));
        }

        DBusProxy::new(dbus_conn)
            .await
            .error("failed to cerate DBusProxy")?
            .add_match(&format!("type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='{}'", device_path.as_str()))
            .await
            .error("Failed to add match")?;
        let changes = MessageStream::from(dbus_conn);

        Ok(Self {
            device_proxy,
            changes,
        })
    }
}

#[async_trait]
impl<'a> BatteryDevice for UPowerDevice<'a> {
    async fn is_available(&self) -> bool {
        true
    }

    async fn capacity(&self) -> Result<u8> {
        self.device_proxy
            .percentage()
            .await
            .error("Failed to get capacity")
            .map(|p| p.clamp(0., 100.) as u8)
    }

    async fn usage(&self) -> Result<f64> {
        self.device_proxy
            .energy_rate()
            .await
            .error("Failed to get usage")
            .map(|u| u * 1e6)
    }

    async fn status(&self) -> Result<BatteryStatus> {
        let state = self
            .device_proxy
            .state()
            .await
            .error("Failed to get state")?;
        Ok(match state {
            1 => BatteryStatus::Charging,
            2 | 6 => BatteryStatus::Discharging,
            3 => BatteryStatus::Empty,
            4 => BatteryStatus::Full,
            5 => BatteryStatus::NotCharging,
            _ => BatteryStatus::Unknown,
        })
    }

    async fn time_remaining(&self) -> Result<u64> {
        let time = match self.status().await? {
            BatteryStatus::Charging => self
                .device_proxy
                .time_to_full()
                .await
                .error("Failed to get time to full")?,
            _ => self
                .device_proxy
                .time_to_empty()
                .await
                .error("Failed to get time to empty")?,
        };
        // TODO: do we need this check?
        Ok((time / 60)
            .try_into()
            .error("Got a negative time from DBus")?)
    }

    async fn wait_for_change(&mut self) -> Result<()> {
        self.changes.next().await;
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

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = BatteryConfig::deserialize(block_config).config_error()?;
        let dbus_conn = api.dbus_connection().await?;

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
                    .error("failed to read /sys/class/power_supply direcory")?;
                let mut device = None;
                while let Some(dir) = sysfs_dir
                    .next_entry()
                    .await
                    .error("failed to read /sys/class/power_supply direcory")?
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
                device.error("failed to determine default battery - please set your battery device in the configuration file")?
            }
        };

        let mut device: Box<dyn BatteryDevice + Send> = match block_config.driver {
            BatteryDriver::Sysfs => Box::new(PowerSupplyDevice::from_device(
                &device,
                block_config.interval,
            )),
            BatteryDriver::Upower => {
                Box::new(UPowerDevice::from_device(&device, &dbus_conn).await?)
            }
        };

        loop {
            let (is_available, mut status, capacity, time, power) = tokio::join!(
                device.is_available(),
                device.status(),
                device.capacity(),
                device.time_remaining(),
                device.usage()
            );

            if let Ok(c) = capacity {
                if c > block_config.full_threshold {
                    if let Ok(s) = &mut status {
                        dbg!(&s);
                        if *s != BatteryStatus::Discharging {
                            *s = BatteryStatus::Full;
                        }
                    }
                }
            }

            let fmt = match status {
                Err(_) if block_config.hide_missing => {
                    api.send_empty_widget().await?;
                    continue;
                }
                Ok(BatteryStatus::Full) if block_config.hide_full => {
                    api.send_empty_widget().await?;
                    continue;
                }
                Ok(BatteryStatus::Full | BatteryStatus::NotCharging) => &format_full,
                Err(_) => &format_missing,
                Ok(_) => &format,
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
                (BatteryStatus::Empty, _) => api
                    .new_widget()
                    .with_icon(BATTERY_EMPTY_ICON)?
                    .with_state(WidgetState::Critical)
                    .with_spacing(WidgetSpacing::Hidden),
                (BatteryStatus::Full, _) => api
                    .new_widget()
                    .with_icon(BATTERY_FULL_ICON)?
                    .with_spacing(WidgetSpacing::Hidden),
                (status, Some(charge)) => {
                    let index = (charge as usize * BATTERY_CHARGE_ICONS.len()) / 101;
                    let icon = BATTERY_CHARGE_ICONS[index];

                    let state = {
                        if status == BatteryStatus::Charging {
                            WidgetState::Good
                        } else if charge <= block_config.critical {
                            WidgetState::Critical
                        } else if charge <= block_config.warning {
                            WidgetState::Warning
                        } else if charge <= block_config.info {
                            WidgetState::Info
                        } else if charge > block_config.good {
                            WidgetState::Good
                        } else {
                            WidgetState::Idle
                        }
                    };

                    api.new_widget()
                        .with_text(fmt.render(&vars)?)
                        .with_icon(icon)?
                        .with_state(state)
                }
                _ => api
                    .new_widget()
                    .with_icon(BATTERY_UNAVAILABLE_ICON)?
                    .with_state(WidgetState::Warning)
                    .with_spacing(WidgetSpacing::Hidden),
            };

            api.send_widgets(vec![widget.get_data()]).await?;
            eprintln!("update");
            device.wait_for_change().await?
        }
    })
}
