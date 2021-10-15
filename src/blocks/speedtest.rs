//! Ping, download, and upload speeds
//!
//! This block which requires [`speedtest-cli`](https://github.com/sivel/speedtest-cli).
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"{ping}{speed_down}{speed_up}"`
//! `interval` | Update interval in seconds | No | `1800`
//!
//! Placeholder    | Value          | Type  | Unit
//! ---------------|----------------|-------|---------------
//! `{ping}`       | Ping delay     | Float | Seconds
//! `{speed_down}` | Download speed | Float | Bits per second
//! `{speed_up}`   | Upload speed   | Float | Bits per second
//!
//! # Example
//!
//! Display speed in bytes per second using 4 digits
//!
//! ```toml
//! [[block]]
//! block = "speedtest"
//! interval = 1800
//! format = "{ping}{speed_down:4*B}{speed_up:4*B}"
//! ```

use std::time::Duration;
use tokio::process::Command;

use super::prelude::*;
use crate::de::deserialize_duration;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct SpeedtestConfig {
    format: FormatTemplate,
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
}

impl Default for SpeedtestConfig {
    fn default() -> Self {
        Self {
            format: Default::default(),
            interval: Duration::from_secs(1800),
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let icon_ping = api.get_icon("ping")?;
        let icon_down = api.get_icon("net_down")?;
        let icon_up = api.get_icon("net_up")?;
        let block_config =
            SpeedtestConfig::deserialize(block_config).config_error()?;
        let format = block_config
            .format
            .or_default("{ping}{speed_down}{speed_up}")?;
        let mut text = api.new_widget();

        let mut command = Command::new("speedtest-cli");
        command.arg("--json");

        loop {
            let output = command
                .output()
                .await
                .error( "failed to run 'speedtest-cli'")?
                .stdout;
            let output = String::from_utf8(output)
                .error( "'speedtest-cli' produced non-UTF8 outupt")?;
            let output: SpeedtestCliOutput = serde_json::from_str(&output)
                .error( "'speedtest-cli' produced wrong JSON")?;

            text.set_text(format.render(&map! {
                "ping" => Value::from_float(output.ping * 1e-3).seconds().icon(icon_ping.clone()),
                "speed_down" => Value::from_float(output.download).bits().icon(icon_down.clone()),
                "speed_up" => Value::from_float(output.upload).bits().icon(icon_up.clone()),
            })?);

            api.send_widget(text.get_data()).await?;
            tokio::time::sleep(block_config.interval).await;
        }
    })
}

#[derive(serde_derive::Deserialize, Debug, Clone, Copy)]
struct SpeedtestCliOutput {
    /// Download speed in bits per second
    download: f64,
    /// Upload speed in bits per second
    upload: f64,
    /// Ping time in ms
    ping: f64,
    // TODO add more
}
