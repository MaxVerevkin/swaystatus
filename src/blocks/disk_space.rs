use std::path::Path;
use std::time::Duration;

use nix::sys::statvfs::statvfs;

use serde_derive::Deserialize;

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

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct DiskSpaceConfig {
    /// Path to collect information from
    path: String,

    /// Currently supported options are available, free, total and used
    /// Sets value used for {percentage} calculation
    /// total is the same as used, use format to set format string for output
    info_type: InfoType,

    /// Format string for output
    format: FormatTemplate,

    /// Unit that is used to display disk space. Options are B, KB, MB, GB and TB
    unit: String,

    /// Update interval in seconds
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,

    /// Diskspace warning (yellow)
    warning: f64,

    /// Diskspace alert (red)
    alert: f64,

    /// use absolute (unit) values for disk space alerts
    alert_absolute: bool,
}

impl Default for DiskSpaceConfig {
    fn default() -> Self {
        Self {
            path: "/".to_string(),
            info_type: InfoType::Available,
            format: Default::default(),
            unit: "GB".to_string(),
            interval: Duration::from_secs(20),
            warning: 20.,
            alert: 10.,
            alert_absolute: false,
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = DiskSpaceConfig::deserialize(block_config).config_error()?;

        let icon = api.get_icon("disk_drive")?;
        let icon = icon.trim();

        let mut text = api.new_widget();
        let format = block_config.format.or_default("{available}")?;

        let unit = match block_config.unit.as_str() {
            "TB" => Prefix::Tera,
            "GB" => Prefix::Giga,
            "MB" => Prefix::Mega,
            "KB" => Prefix::Kilo,
            "B" => Prefix::One,
            x => return Err(Error::new(format!("Unknown unit: '{}'", x))),
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
            let values = map!(
                "percentage" => Value::from_float(percentage).percents(),
                "path" => Value::from_string(block_config.path.clone()),
                "total" => Value::from_float(total as f64).bytes(),
                "used" => Value::from_float(used as f64).bytes(),
                "available" => Value::from_float(available as f64).bytes(),
                "free" => Value::from_float(free as f64).bytes(),
                "icon" => Value::from_string(icon.to_string()),
            );
            text.set_text(format.render(&values)?);

            // Send percentage to alert check if we don't want absolute alerts
            let alert_val = if block_config.alert_absolute {
                result
                    / match unit {
                        Prefix::Tera => 1u64 << 40,
                        Prefix::Giga => 1u64 << 30,
                        Prefix::Mega => 1u64 << 20,
                        Prefix::Kilo => 1u64 << 10,
                        Prefix::One => 1u64,
                        _ => unreachable!(),
                    } as f64
            } else {
                percentage
            };

            // Compute state
            let state = match block_config.info_type {
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
            };
            text.set_state(state);

            api.send_widget(text.get_data()).await?;
            interval.tick().await;
        }
    })
}
