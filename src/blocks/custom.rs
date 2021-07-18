//! The output of a custom shell command
//!
//! For further customisation, use the `json` option and have the shell command output valid JSON in the schema below:  
//! `{"icon": "ICON", "state": "STATE", "text": "YOURTEXT", "short_text": "YOUR SHORT TEXT"}
//! `{"icon": "ICON", "state": "STATE", "text": "YOURTEXT"}`  
//! `icon` is optional (TODO add a link to the docs) (default "")  
//! `state` is optional, it may be Idle, Info, Good, Warning, Critical (default Idle)  
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `command` | Shell command to execute & display | No | None
//! `cycle` | Commands to execute and change when the button is clicked | No | None
//! `interval` | Update interval in seconds | No | `10`
//! `one_shot` | Whether to run the command only once (if set to `true`, `interval` will be ignored) | No | `false`
//! `json` | Use JSON from command output to format the block. If the JSON is not valid, the block will error out. | No | `false`
//! `signal` | Signal value that causes an update for this block with 0 corresponding to `-SIGRTMIN+0` and the largest value being `-SIGRTMAX` | No | None
//! `hide_when_empty` | Hides the block when the command output (or json text field) is empty | No | false
//! `shell` | Specify the shell to use when running commands | No | `$SHELL` if set, otherwise fallback to `sh`
//!
//! # Examples
//!
//! Display temperature, update every 10 seconds:
//!
//! ```toml
//! [[block]]
//! block = "custom"
//! command = ''' cat /sys/class/thermal/thermal_zone0/temp | awk '{printf("%.1f\n",$1/1000)}' '''
//! ```
//!
//! Cycle between "ON" and "OFF", update every 1 second, run `<command>` when block is clicked:
//!
//! ```toml
//! [[block]]
//! block = "custom"
//! cycle = ["echo ON", "echo OFF"]
//! interval = 1
//! ```
//!
//! Use JSON output:
//!
//! ```toml
//! [[block]]
//! block = "custom"
//! command = "echo '{\"icon\":\"weather_thunder\",\"state\":\"Critical\", \"text\": \"Danger!\"}'"
//! json = true
//! ```
//!
//! Display kernel, update the block only once:
//!
//! ```toml
//! [[block]]
//! block = "custom"
//! command = "uname -r"
//! one_shot = true
//! ```
//!
//! Display the screen brightness on an intel machine and update this only when `pkill -SIGRTMIN+4 i3status-rs` is called:
//!
//! ```toml
//! [[block]]
//! block = "custom"
//! command = ''' cat /sys/class/backlight/intel_backlight/brightness | awk '{print $1}' '''
//! signal = 4
//! one_shot = true
//! ```

use serde::de::Deserialize;
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::signals::Signal;
use crate::widgets::widget::Widget;
use crate::widgets::State;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct CustomConfig {
    command: Option<String>,
    cycle: Option<Vec<String>>,
    interval: u64,
    json: bool,
    hide_when_empty: bool,
    shell: Option<String>,
    one_shot: bool,
    signal: Option<i32>,
}

impl Default for CustomConfig {
    fn default() -> Self {
        Self {
            command: None,
            cycle: None,
            interval: 10,
            json: false,
            hide_when_empty: false,
            shell: None,
            one_shot: false,
            signal: None,
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
    let CustomConfig {
        command,
        cycle,
        interval,
        json,
        hide_when_empty,
        shell,
        one_shot,
        signal,
    } = block_config;

    let interval = Duration::from_secs(interval);
    let mut widget = Widget::new(id, shared_config);

    // Choose the shell in this priority:
    // 1) `shell` config option
    // 2) `SHELL` environment varialble
    // 3) `"sh"`
    let shell = shell
        .or_else(|| env::var("SHELL").ok())
        .unwrap_or_else(|| "sh".to_string());

    let mut cycle = cycle
        .or_else(|| command.clone().map(|cmd| vec![cmd]))
        .block_error("custom", "either 'command' or 'cycle' must be specified")?
        .into_iter()
        .cycle();

    loop {
        // Run command
        let output = Command::new(&shell)
            .args(&["-c", &cycle.next().unwrap()])
            .output()
            .await
            .block_error("custom", "failed to run command")?;
        let stdout = std::str::from_utf8(&output.stdout)
            .block_error("custom", "the output of command is invalid UTF-8")?
            .trim();

        // {"icon": "ICON", "state": "STATE", "text": "YOURTEXT", "short_text": "YOUR SHORT TEXT"}
        let widgets = if stdout.is_empty() && hide_when_empty {
            vec![]
        } else if json {
            let vals: HashMap<String, String> =
                serde_json::from_str(stdout).block_error("custom", "invalid JSON")?;
            widget.set_icon(vals.get("icon").map(|s| s.as_str()).unwrap_or(""))?;
            widget.set_state(match vals.get("state").map(|s| s.as_str()).unwrap_or("") {
                "Info" => State::Info,
                "Good" => State::Good,
                "Warning" => State::Warning,
                "Critical" => State::Critical,
                _ => State::Idle,
            });
            let text = vals.get("text").cloned().unwrap_or_default();
            let short_text = vals.get("short_text").cloned();
            widget.set_text((text, short_text));
            vec![widget.get_data()]
        } else {
            widget.set_full_text(stdout.to_string());
            vec![widget.get_data()]
        };

        message_sender
            .send(BlockMessage { id, widgets })
            .await
            .internal_error("custom", "failed to send message")?;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {
                    if !one_shot {
                        break;
                    }
                },
                Some(event) = events_reciever.recv() => {
                    match (event, signal) {
                        (BlockEvent::Signal(Signal::Custom(s)), Some(signal)) if s == signal => break,
                        (BlockEvent::I3Bar(_), _) => break,
                        _ => (),
                    }
                },
            }
        }
    }
}
