//! System load average
//!
//! # Configuration
//!
//! Key        | Values                                                                                | Required | Default
//! -----------|---------------------------------------------------------------------------------------|----------|--------
//! `format`   | A string to customise the output of this block. See below for available placeholders. | No       | `"$1m"`
//! `interval` | Update interval in seconds                                                            | No       | `3`
//! `info`     | Minimum load, where state is set to info                                              | No       | `0.3`
//! `warning`  | Minimum load, where state is set to warning                                           | No       | `0.6`
//! `critical` | Minimum load, where state is set to critical                                          | No       | `0.9`
//!
//! Placeholder  | Value                  | Type   | Unit
//! -------------|------------------------|--------|-----
//! `1m`         | 1 minute load average  | Number | -
//! `5m`         | 5 minute load average  | Number | -
//! `15m`        | 15 minute load average | Number | -
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "load"
//! format = "1min avg: $1m"
//! interval = 1
//! ```
//!
//! # Icons Used
//! - `cogs`

use std::path::Path;
use std::time::Duration;

use super::prelude::*;
use crate::de::deserialize_duration;

use crate::util;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct LoadConfig {
    format: FormatConfig,
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    info: f64,
    warning: f64,
    critical: f64,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            format: Default::default(),
            interval: Duration::from_secs(3),
            info: 0.3,
            warning: 0.6,
            critical: 0.9,
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = LoadConfig::deserialize(block_config).config_error()?;
        let mut interval = tokio::time::interval(block_config.interval);
        api.set_format(block_config.format.init("$1m", &api)?);
        api.set_icon("cogs")?;

        // borrowed from https://docs.rs/cpuinfo/0.1.1/src/cpuinfo/count/logical.rs.html#4-6
        let logical_cores = util::read_file(Path::new("/proc/cpuinfo"))
            .await
            .error("Your system doesn't support /proc/cpuinfo")?
            .lines()
            .filter(|l| l.starts_with("processor"))
            .count() as f64;

        let loadavg_path = Path::new("/proc/loadavg");
        loop {
            let loadavg = util::read_file(loadavg_path).await.error(
                "Your system does not support reading the load average from /proc/loadavg",
            )?;
            let mut values = loadavg.split(' ');
            let m1: f64 = values
                .next()
                .map(|x| x.parse().ok())
                .flatten()
                .error("bad /proc/loadavg file")?;
            let m5: f64 = values
                .next()
                .map(|x| x.parse().ok())
                .flatten()
                .error("bad /proc/loadavg file")?;
            let m15: f64 = values
                .next()
                .map(|x| x.parse().ok())
                .flatten()
                .error("bad /proc/loadavg file")?;

            api.set_state(match m1 / logical_cores {
                x if x > block_config.critical => WidgetState::Critical,
                x if x > block_config.warning => WidgetState::Warning,
                x if x > block_config.info => WidgetState::Info,
                _ => WidgetState::Idle,
            });
            api.set_values(map! {
                "1m" => Value::number(m1),
                "5m" => Value::number(m5),
                "15m" => Value::number(m15),
            });

            api.render();
            api.flush().await?;
            interval.tick().await;
        }
    })
}
