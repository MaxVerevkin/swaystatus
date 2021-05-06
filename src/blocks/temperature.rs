//! The system temperature
//!
//! This block is based on lm_sensors' `sensors -j` output. Requires `lm_sensors` and appropriate kernel modules for your hardware.
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
//! `chip` | Narrows the results to a given chip name. `*` may be used as a wildcard | No | None
//!
//! Placeholder  | Value                                 | Type    | Unit
//! -------------|---------------------------------------|---------|--------
//! `{min}`      | Minimum temperature among all sensors | Integer | Degrees
//! `{average}`  | Average temperature among all sensors | Integer | Degrees
//! `{max}`      | Maximum temperature among all sensors | Integer | Degrees
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "temperature"
//! interval = 10
//! format = "{min} min, {max} max, {average} avg"
//! chip = "*-isa-*"
//! ```
//!
//! # TODO
//! - `inputs` config option
//! - `fahrenheit` config option

use serde::de::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widgets::widget::Widget;
use crate::widgets::State;

type SensorsOutput = HashMap<String, HashMap<String, serde_json::Value>>;
type InputReadings = HashMap<String, f64>;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct TemperatureConfig {
    format: FormatTemplate,
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    collapsed: bool,
    good: i64,
    idle: i64,
    info: i64,
    warning: i64,
    chip: Option<String>,
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
            chip: None,
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

    // Construct a command
    let mut command = Command::new("sensors");
    command.arg("-j");
    if let Some(ref chip) = block_config.chip {
        command.arg(chip);
    }

    loop {
        // Run command and get output
        let output = String::from_utf8(
            command
                .output()
                .await
                .block_error("temperature", "failed to run 'sensors'")?
                .stdout,
        )
        .block_error("temperature", "'sensors' command produced invalid UTF-8")?;

        // Parse output
        let mut temps = Vec::new();
        let output_json: SensorsOutput =
            serde_json::from_str(&output).block_error("temperature", "failed to parse JSON")?;
        for (_chip, inputs) in output_json {
            for (input_name, input_values) in inputs {
                let readings: InputReadings = match serde_json::from_value(input_values) {
                    Ok(values) => values,
                    Err(_) => continue, // probably the "Adapter" key, just ignore.
                };
                if input_name.contains("Core") {
                    for (name, value) in readings {
                        if name.contains("input") {
                            temps.push(value as i64);
                        }
                    }
                }
            }
        }

        let min_temp = temps.iter().min().cloned().unwrap_or(0);
        let max_temp = temps.iter().max().cloned().unwrap_or(0);
        let avg_temp = (temps.iter().sum::<i64>() as f64) / (temps.len() as f64);

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
