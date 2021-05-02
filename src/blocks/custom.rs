use serde::de::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::subprocess::spawn_shell;
use crate::widgets::widget::Widget;
use crate::widgets::Spacing;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct CustomConfig {
    /// The command to run
    command: String,

    /// Format string
    format: String,

    /// Interval between command runs
    interval: u64,

    /// Json support
    json: bool,
}

impl Default for CustomConfig {
    fn default() -> Self {
        Self {
            command: "uname -r".to_string(),
            format: "{stdout}".to_string(),
            interval: 5,
            json: false,
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
    let block_config = CustomConfig::deserialize(block_config).block_config_error("custom")?;
    let format = FormatTemplate::from_string(&block_config.format)?;
    let interval = Duration::from_secs(block_config.interval);

    loop {
        // Run command
        let output = Command::new("sh")
            .args(&["-c", block_config.command.as_str()])
            .output()
            .await
            .block_error("custom", "failed to run command")?;

        // Get basic info
        let stdout = std::str::from_utf8(&output.stdout)
            .block_error("custom", "the output of command is invalid UTF-8")?
            .trim();
        let stderr = std::str::from_utf8(&output.stderr)
            .block_error("custom", "the output of command is invalid UTF-8")?
            .trim();
        let exit_code = output
            .status
            .code()
            .block_error("custom", "failed to get command's exit code")?;

        // TODO add `state` into JSON
        let (widgets, click_handlers) = if block_config.json {
            // If JSON is enabled, stdout should contain this data:
            // ```json
            // [
            //     {
            //         "text": "<text>",
            //         "icon": "<icon>",
            //         "on_click": "<command>",
            //         "on_right_click": "<command>"
            //         "on_scroll_up": "<command>",
            //         "on_scroll_down": "<command>",
            //     },
            //     { ... },
            //     { ... }
            // ]
            // ```
            // All the fields are optional
            let json_data: Vec<HashMap<String, String>> =
                serde_json::from_str(stdout).block_error("custom", "failed to parse JSON")?;
            let mut widgets = Vec::new();
            let mut click_handlers = HashMap::new(); // TODO use `crate::click::ClickHandler`

            for (instance, widget_data) in json_data.into_iter().enumerate() {
                // Add click handlers
                widget_data
                    .get("on_click")
                    .cloned()
                    .map(|cmd| click_handlers.insert((instance, MouseButton::Left), cmd));
                widget_data
                    .get("on_right_click")
                    .cloned()
                    .map(|cmd| click_handlers.insert((instance, MouseButton::Right), cmd));
                widget_data
                    .get("on_scroll_up")
                    .cloned()
                    .map(|cmd| click_handlers.insert((instance, MouseButton::WheelUp), cmd));
                widget_data
                    .get("on_scroll_down")
                    .cloned()
                    .map(|cmd| click_handlers.insert((instance, MouseButton::WheelDown), cmd));

                // Create widget
                let mut widget = Widget::new(id, shared_config.clone()).with_instance(instance);

                // Maybe set text
                if let Some(text) = widget_data.get("text") {
                    widget.set_full_text(text.clone());
                }

                // Maybe set icon
                if let Some(icon) = widget_data.get("icon") {
                    widget.set_icon(icon)?;
                }

                // Add widget
                widgets.push(widget.with_spacing(Spacing::Hidden).get_data());
            }

            (widgets, Some(click_handlers))
        } else {
            // No JSON, use standard output
            let text = Widget::new(id, shared_config.clone()).with_text(format.render(&map! {
                "stdout" => Value::from_string(stdout.to_string()),
                "stderr" => Value::from_string(stderr.to_string()),
                "exit_code" => Value::from_integer(exit_code as i64),
            })?);
            (vec![text.get_data()], None)
        };

        message_sender
            .send(BlockMessage { id, widgets })
            .await
            .internal_error("custom", "failed to send message")?;

        tokio::select! {
            _ = tokio::time::sleep(interval) =>(),
            Some(BlockEvent::I3Bar(click)) = events_reciever.recv() => {
                if let Some(click_handlers) = click_handlers {
                    if let Some(instance) = click.instance {
                        if let Some(cmd) = click_handlers.get(&(instance, click.button)) {
                            spawn_shell(cmd).block_error("custom", "failed to run command")?;
                        }
                    }
                }
            }
        }
    }
}
