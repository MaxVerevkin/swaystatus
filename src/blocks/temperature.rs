//! The system temperature
//!
//! This block simply reads temperatures form `/sys/class/hwmon` direcory.
//!
//! This block has two modes: "collapsed", which uses only color as an indicator, and "expanded",
//! which shows the content of a `format` string. The average, minimum, and maximum temperatures
//! are computed using all sensors displayed by `sensors`, or optionally filtered by `chip` and
//! `inputs`.
//!
//! Note that the colour of the block is always determined by the maximum temperature across all
//! sensors, not the average. You may need to keep this in mind if you have a misbehaving sensor.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders | No | `"$average avg, $max max"`
//! `interval` | Update interval in seconds | No | `5`
//! `collapsed` | Whether the block will be collapsed by default | No | `false`
//! `good` | Maximum temperature to set state to good | No | `20` °C (`68` °F)
//! `idle` | Maximum temperature to set state to idle | No | `45` °C (`113` °F)
//! `info` | Maximum temperature to set state to info | No | `60` °C (`140` °F)
//! `warning` | Maximum temperature to set state to warning. Beyond this temperature, state is set to critical | No | `80` °C (`176` °F)
//! `chip` | Chip name as shown by `cat /sys/class/hwmon/*/name` | No | `"coretemp"`
//!
//! Placeholder  | Value                                | Type   | Unit
//! -------------|--------------------------------------|--------|--------
//! `{min}`      | Minimum temperature among all inputs | Number | Degrees
//! `{average}`  | Average temperature among all inputs | Number | Degrees
//! `{max}`      | Maximum temperature among all inputs | Number | Degrees
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "temperature"
//! interval = 10
//! format = "{min} min, {max} max, {average} avg"
//! ```
//!
//! # Icons Used
//! - `thermometer`
//!
//! # TODO
//! - Support Fahrenheit scale

use super::prelude::*;
use crate::de::deserialize_duration;
use std::time::Duration;
use tokio::fs::{read_dir, read_to_string};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct TemperatureConfig {
    format: FormatConfig,
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    collapsed: bool,
    good: i32,
    idle: i32,
    info: i32,
    warning: i32,
    chip: String,
}

impl Default for TemperatureConfig {
    fn default() -> Self {
        Self {
            format: Default::default(),
            interval: Duration::from_secs(5),
            collapsed: false,
            good: 20,
            idle: 45,
            info: 60,
            warning: 80,
            chip: "coretemp".into(),
        }
    }
}

pub async fn run(block_config: toml::Value, mut api: CommonApi) -> Result<()> {
    let mut events = api.get_events().await?;
    let block_config = TemperatureConfig::deserialize(block_config).config_error()?;
    let mut collapsed = block_config.collapsed;
    api.set_format(block_config.format.init("$average avg, $max max", &api)?);
    api.set_icon("thermometer")?;

    loop {
        // Get chip info
        let temp = ChipInfo::new(&block_config.chip).await?.temp;
        let min_temp = temp.iter().min().cloned().unwrap_or(0);
        let max_temp = temp.iter().max().cloned().unwrap_or(0);
        let avg_temp = (temp.iter().sum::<i32>() as f64) / (temp.len() as f64);

        api.set_state(match max_temp {
            x if x <= block_config.good => WidgetState::Good,
            x if x <= block_config.idle => WidgetState::Idle,
            x if x <= block_config.info => WidgetState::Info,
            x if x <= block_config.warning => WidgetState::Warning,
            _ => WidgetState::Critical,
        });

        if collapsed {
            api.collapse();
        } else {
            api.set_values(map! {
                "average" => Value::degrees(avg_temp),
                "min" => Value::degrees(min_temp),
                "max" => Value::degrees(max_temp),
            });
            api.show();
            api.render();
        }

        api.flush().await?;

        tokio::select! {
            _ = tokio::time::sleep(block_config.interval) => (),
            Some(BlockEvent::Click(click)) = events.recv() => {
                if click.button == MouseButton::Left {
                    collapsed = !collapsed;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ChipInfo {
    temp: Vec<i32>,
}

impl ChipInfo {
    async fn new(name: &str) -> Result<Self> {
        let mut sysfs_dir = read_dir("/sys/class/hwmon")
            .await
            .error("failed to read /sys/class/hwmon direcory")?;
        while let Some(dir) = sysfs_dir
            .next_entry()
            .await
            .error("failed to read /sys/class/hwmon direcory")?
        {
            if read_to_string(dir.path().join("name"))
                .await
                .map(|t| t.trim() == name)
                .unwrap_or(false)
            {
                let mut chip_dir = read_dir(dir.path())
                    .await
                    .error("failed to read chip's sysfs direcory")?;
                let mut temp = Vec::new();
                while let Some(entry) = chip_dir
                    .next_entry()
                    .await
                    .error("failed to read chip's sysfs direcory")?
                {
                    let entry_str = entry.file_name().to_str().unwrap().to_string();
                    if entry_str.starts_with("temp") && entry_str.ends_with("_input") {
                        let val: i32 = read_to_string(entry.path())
                            .await
                            .error("failed to read chip's temperature")?
                            .trim()
                            .parse()
                            .error("temperature is not an integer")?;
                        temp.push(val / 1000);
                    }
                }
                return Ok(Self { temp });
            }
        }
        Err(Error::new(format!("chip '{}' not found", name)))
    }
}
