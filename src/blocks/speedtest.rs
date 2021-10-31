//! Ping, download, and upload speeds
//!
//! This block which requires [`speedtest-cli`](https://github.com/sivel/speedtest-cli).
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"$ping.eng()$speed_down.eng()$speed_up.eng() "`
//! `interval` | Update interval in seconds | No | `1800`
//!
//! Placeholder  | Value          | Type   | Unit
//! -------------|----------------|--------|---------------
//! `ping`       | Ping delay     | Number | Seconds
//! `speed_down` | Download speed | Number | Bits per second
//! `speed_up`   | Upload speed   | Number | Bits per second
//!
//! # Example
//!
//! Hide ping and display speed in bytes per second each using 4 characters
//!
//! ```toml
//! [[block]]
//! block = "speedtest"
//! interval = 1800
//! format = "$speed_down.eng(4,B)$speed_up(4,B)"
//! ```

use super::prelude::*;
use crate::de::deserialize_duration;
use std::time::Duration;
use tokio::process::Command;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct SpeedtestConfig {
    format: FormatConfig,
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
        let block_config = SpeedtestConfig::deserialize(block_config).config_error()?;
        api.set_format(
            block_config
                .format
                .init("$ping.eng()$speed_down.eng()$speed_up.eng() ", &api)?,
        );

        let icon_ping = api.get_icon("ping")?;
        let icon_down = api.get_icon("net_down")?;
        let icon_up = api.get_icon("net_up")?;

        let mut command = Command::new("speedtest-cli");
        command.arg("--json");

        loop {
            let output = command
                .output()
                .await
                .error("failed to run 'speedtest-cli'")?
                .stdout;
            let output =
                std::str::from_utf8(&output).error("'speedtest-cli' produced non-UTF8 outupt")?;
            let output: SpeedtestCliOutput =
                serde_json::from_str(output).error("'speedtest-cli' produced wrong JSON")?;

            api.set_values(map! {
                "ping" => Value::seconds(output.ping * 1e-3).icon(icon_ping.clone()),
                "speed_down" => Value::bits(output.download).icon(icon_down.clone()),
                "speed_up" => Value::bits(output.upload).icon(icon_up.clone()),
            });
            api.render();
            api.flush().await?;
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
