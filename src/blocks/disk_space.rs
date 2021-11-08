//! Disk usage statistics
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `path` | Path to collect information from | No | `"/"`
//! `interval` | Update time in seconds | No | `20`
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"$available"`
//! `warning` | A value which will trigger warning block state | No | `20.0`
//! `alert` | A value which will trigger critical block state | No | `10.0`
//! `info_type` | Determines which information will affect the block state. Possible values are `"available"`, `"free"` and `"used"` | No | `"available"`
//! `alert_unit` | The unit of `alert` and `warning` options. If not set, percents are uesd. Possible values are `"B"`, `"KB"`, `"MB"`, `"GB"` and `"TB"` | No | None
//!
//! Placeholder  | Value                                                              | Type   | Unit
//! -------------|--------------------------------------------------------------------|--------|-------
//! `path`       | The value of `path` option                                         | Text   | -
//! `percentage` | Free or used percentage. Depends on `info_type`                    | Number | %
//! `total`      | Total disk space                                                   | Number | Bytes
//! `used`       | Dused disk space                                                   | Number | Bytes
//! `free`       | Free disk space                                                    | Number | Bytes
//! `available`  | Available disk space (free disk space minus reserved system space) | Number | Bytes
//! `icon`       | Disk drive icon                                                    | Text   | -
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "disk_space"
//! info_type = "available"
//! alert_unit = "GB"
//! alert = 10.0
//! warning = 15.0
//! format = "$icon.str() $available.eng(2)"
//! ```
//! # Icons Used
//! - `disk_drive`

use std::path::Path;
use std::time::Duration;

use nix::sys::statvfs::statvfs;

use super::prelude::*;
use crate::de::deserialize_duration;
use crate::formatting::prefix::Prefix;

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InfoType {
    Available,
    Free,
    Used,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct DiskSpaceConfig {
    path: String,
    info_type: InfoType,
    format: FormatConfig,
    alert_unit: Option<String>,
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    warning: f64,
    alert: f64,
}

impl Default for DiskSpaceConfig {
    fn default() -> Self {
        Self {
            path: "/".into(),
            info_type: InfoType::Available,
            format: Default::default(),
            alert_unit: None,
            interval: Duration::from_secs(20),
            warning: 20.,
            alert: 10.,
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = DiskSpaceConfig::deserialize(block_config).config_error()?;

        let icon = api.get_icon("disk_drive")?;
        let icon = icon.trim();

        let format = block_config.format.init("$available", &api)?;
        api.set_format(format);

        let unit = match block_config.alert_unit.as_deref() {
            Some("TB") => Some(Prefix::Tera),
            Some("GB") => Some(Prefix::Giga),
            Some("MB") => Some(Prefix::Mega),
            Some("KB") => Some(Prefix::Kilo),
            Some("B") => Some(Prefix::One),
            Some(x) => return Err(Error::new(format!("Unknown unit: '{}'", x))),
            None => None,
        };

        let path = Path::new(block_config.path.as_str());
        let mut interval = tokio::time::interval(block_config.interval);

        loop {
            let statvfs = statvfs(path).error("failed to retrieve statvfs")?;

            let total = (statvfs.blocks() as u64) * (statvfs.fragment_size() as u64);
            let used = ((statvfs.blocks() as u64) - (statvfs.blocks_free() as u64))
                * (statvfs.fragment_size() as u64);
            let available = (statvfs.blocks_available() as u64) * (statvfs.block_size() as u64);
            let free = (statvfs.blocks_free() as u64) * (statvfs.block_size() as u64);

            let result = match block_config.info_type {
                InfoType::Available => available,
                InfoType::Free => free,
                InfoType::Used => used,
            } as f64;

            let percentage = result / (total as f64) * 100.;
            api.set_values(map!(
                "path" => Value::text(block_config.path.clone()),
                "percentage" => Value::percents(percentage),
                "total" => Value::bytes(total as f64),
                "used" => Value::bytes(used as f64),
                "available" => Value::bytes(available as f64),
                "free" => Value::bytes(free as f64),
                "icon" => Value::text(icon.into()),
            ));

            // Send percentage to alert check if we don't want absolute alerts
            let alert_val = match unit {
                Some(Prefix::Tera) => result * 1e12,
                Some(Prefix::Giga) => result * 1e9,
                Some(Prefix::Mega) => result * 1e6,
                Some(Prefix::Kilo) => result * 1e3,
                Some(_) => result,
                None => percentage,
            };

            // Compute state
            api.set_state(match block_config.info_type {
                InfoType::Used => {
                    if alert_val > block_config.alert {
                        WidgetState::Critical
                    } else if alert_val <= block_config.alert && alert_val > block_config.warning {
                        WidgetState::Warning
                    } else {
                        WidgetState::Idle
                    }
                }
                InfoType::Free | InfoType::Available => {
                    if 0. <= alert_val && alert_val < block_config.alert {
                        WidgetState::Critical
                    } else if block_config.alert <= alert_val && alert_val < block_config.warning {
                        WidgetState::Warning
                    } else {
                        WidgetState::Idle
                    }
                }
            });

            api.render();
            api.flush().await?;

            interval.tick().await;
        }
    })
}
