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

use super::prelude::*;
use crate::signals::Signal;
use tokio::process::Command;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct CustomConfig {
    command: Option<StdString>,
    cycle: Option<Vec<StdString>>,
    interval: u64,
    json: bool,
    hide_when_empty: bool,
    shell: Option<StdString>,
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

pub async fn run(config: toml::Value, mut api: CommonApi) -> Result<()> {
    let mut events = api.get_events().await?;
    let config = CustomConfig::deserialize(config).config_error()?;
    let CustomConfig {
        command,
        cycle,
        interval,
        json,
        hide_when_empty,
        shell,
        one_shot,
        signal,
    } = config;

    let interval = Duration::from_secs(interval);

    // Choose the shell in this priority:
    // 1) `shell` config option
    // 2) `SHELL` environment varialble
    // 3) `"sh"`
    let shell = shell
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "sh".to_string());

    let mut cycle = cycle
        .or_else(|| command.clone().map(|cmd| vec![cmd]))
        .error("either 'command' or 'cycle' must be specified")?
        .into_iter()
        .cycle();

    loop {
        // Run command
        let output = Command::new(&shell)
            .args(&["-c", &cycle.next().unwrap()])
            .output()
            .await
            .error("failed to run command")?;
        let stdout = std::str::from_utf8(&output.stdout)
            .error("the output of command is invalid UTF-8")?
            .trim();

        // {"icon": "ICON", "state": "STATE", "text": "YOURTEXT", "short_text": "YOUR SHORT TEXT"}
        if stdout.is_empty() && hide_when_empty {
            api.hide();
        } else if json {
            let input: Input = serde_json::from_str(stdout).error("invalid JSON")?;

            api.show();
            api.set_icon(&input.icon)?;
            api.set_state(input.state);
            if let Some(short) = input.short_text {
                api.set_texts(input.text, short);
            } else {
                api.set_text(input.text);
            }
        } else {
            api.show();
            api.set_text(stdout.into());
        };
        api.flush().await?;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {
                    if !one_shot {
                        break;
                    }
                },
                Some(event) = events.recv() => {
                    match (event, signal) {
                        (BlockEvent::Signal(Signal::Custom(s)), Some(signal)) if s == signal => break,
                        (BlockEvent::Click(_), _) => break,
                        _ => (),
                    }
                },
            }
        }
    }
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct Input {
    icon: String,
    state: State,
    text: String,
    short_text: Option<String>,
}
