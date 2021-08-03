//! The system temperature
//!
//! This block simply reads temperatures form `/sys/class/hwmon` direcory.
//!
//! This block has two modes: "collapsed", which uses only color as an indicator, and "expanded", which shows the content of a `format` string. The average, minimum, and maximum temperatures are computed using all sensors displayed by `sensors`, or optionally filtered by `chip` and `inputs`.
//!
//! Note that the colour of the block is always determined by the maximum temperature across all sensors, not the average. You may need to keep this in mind if you have a misbehaving sensor.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders | No | `"{average} avg, {max} max"`
//! `interval` | Update interval in seconds | No | `5`
//! `collapsed` | Whether the block will be collapsed by default | No | `false`
//! `good` | Maximum temperature to set state to good | No | `20` °C (`68` °F)
//! `idle` | Maximum temperature to set state to idle | No | `45` °C (`113` °F)
//! `info` | Maximum temperature to set state to info | No | `60` °C (`140` °F)
//! `warning` | Maximum temperature to set state to warning. Beyond this temperature, state is set to critical | No | `80` °C (`176` °F)
//! `chip` | Chip name as shown by `cat /sys/class/hwmon/*/name` | No | `"coretemp"`
//!
//! Placeholder  | Value                                 | Type    | Unit
//! -------------|---------------------------------------|---------|--------
//! `{min}`      | Minimum temperature among all inputs | Integer | Degrees
//! `{average}`  | Average temperature among all inputs | Integer | Degrees
//! `{max}`      | Maximum temperature among all inputs | Integer | Degrees
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "temperature"
//! interval = 10
//! format = "{min} min, {max} max, {average} avg"
//! ```

use serde::de::Deserialize;
use std::time::Duration;
use tokio::fs::{read_dir, read_to_string};
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widgets::widget::Widget;
use crate::widgets::State;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct TemperatureConfig {
    format: FormatTemplate,
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
            chip: "coretemp".to_string(),
        }
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    mut events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let block_config =
        TemperatureConfig::deserialize(block_config).block_config_error("temperature")?;
    let format = block_config.format.or_default("{average} avg, {max} max")?;
    let mut text = Widget::new(id, shared_config).with_icon("thermometer")?;
    let mut collapsed = block_config.collapsed;

    loop {
        // Get chip info
        let temp = ChipInfo::new(&block_config.chip).await?.temp;
        let min_temp = temp.iter().min().cloned().unwrap_or(0);
        let max_temp = temp.iter().max().cloned().unwrap_or(0);
        let avg_temp = (temp.iter().sum::<i32>() as f64) / (temp.len() as f64);

        // Render!
        let values = map! {
            "avg" => Value::from_integer(avg_temp.round() as i64).degrees(),
            "min" => Value::from_integer(min_temp as i64).degrees(),
            "max" => Value::from_integer(max_temp as i64).degrees(),
        };
        text.set_text(if collapsed {
            (String::new(), None)
        } else {
            format.render(&values)?
        });

        // Set state
        text.set_state(match max_temp {
            x if x <= block_config.good => State::Good,
            x if x <= block_config.idle => State::Idle,
            x if x <= block_config.info => State::Info,
            x if x <= block_config.warning => State::Warning,
            _ => State::Critical,
        });

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("temperature", "failed to send message")?;

        tokio::select! {
            _ = tokio::time::sleep(block_config.interval) => (),
            Some(BlockEvent::I3Bar(click)) = events_reciever.recv() => {
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
            .block_error("temperature", "failed to read /sys/class/hwmon direcory")?;
        while let Some(dir) = sysfs_dir
            .next_entry()
            .await
            .block_error("temperature", "failed to read /sys/class/hwmon direcory")?
        {
            if read_to_string(dir.path().join("name"))
                .await
                .map(|t| t.trim() == name)
                .unwrap_or(false)
            {
                let mut chip_dir = read_dir(dir.path())
                    .await
                    .block_error("temperature", "failed to read chip's sysfs direcory")?;
                let mut temp = Vec::new();
                while let Some(entry) = chip_dir
                    .next_entry()
                    .await
                    .block_error("temperature", "failed to read chip's sysfs direcory")?
                {
                    let entry_str = entry.file_name().to_str().unwrap().to_string();
                    if entry_str.starts_with("temp") && entry_str.ends_with("_input") {
                        let val: i32 = read_to_string(entry.path())
                            .await
                            .block_error("temperature", "failed to read chip's temperature")?
                            .trim()
                            .parse()
                            .block_error("temperature", "temperature is not an integer")?;
                        temp.push(val / 1000);
                    }
                }
                return Ok(Self { temp });
            }
        }
        block_error("temperature", &format!("chip '{}' not found", name))
    }
}
