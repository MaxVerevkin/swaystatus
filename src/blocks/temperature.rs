use async_process::Command;
use serde::de::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widgets::text::TextWidget;
use crate::widgets::{I3BarWidget, State};

type SensorsOutput = HashMap<String, HashMap<String, serde_json::Value>>;
type InputReadings = HashMap<String, f64>;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct TemperatureConfig {
    pub format: String,

    /// Update interval in seconds
    pub interval: u64,

    /// Narrows the results to a given chip name
    pub chip: Option<String>,

    /// Maximum temperature, below which state is set to good
    pub good: i64,

    /// Maximum temperature, below which state is set to idle
    pub idle: i64,

    /// Maximum temperature, below which state is set to info
    pub info: i64,

    /// Maximum temperature, below which state is set to warning
    pub warning: i64,
}

impl Default for TemperatureConfig {
    fn default() -> Self {
        Self {
            format: "{avg}".to_string(),
            interval: 5,
            chip: None,
            good: 20,
            idle: 45,
            info: 60,
            warning: 80,
        }
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    // Drop the reciever if we don't what to recieve events
    drop(events_reciever);

    let block_config = TemperatureConfig::deserialize(block_config)
        .configuration_error("failed to parse config")?;
    let format = FormatTemplate::from_string(&block_config.format)?;
    let interval = Duration::from_secs(block_config.interval);

    let mut text = TextWidget::new(id, 0, shared_config).with_icon("thermometer")?;

    loop {
        // Construct a command
        let mut command = Command::new("sensors");
        command.arg("-j");
        if let Some(ref chip) = block_config.chip {
            command.arg(chip);
        }

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
        text.set_text(format.render(&values)?);

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
            .internal_error("time block", "failed to send message")?;

        tokio::time::sleep(interval).await;
    }
}
