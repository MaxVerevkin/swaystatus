use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;

use serde::de::Deserialize;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::formatting::value::Value;
use crate::formatting::FormatTemplate;
use crate::util;
use crate::widgets::widget::Widget;
use crate::widgets::State;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct LoadConfig {
    /// Format string
    format: String,

    /// Format string (short)
    format_short: Option<String>,

    /// Inerval of updates
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,

    /// Minimum load, where state is set to info
    info: f64,

    /// Minimum load, where state is set to warning
    warning: f64,

    /// Minimum load, where state is set to critical
    critical: f64,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            format: "{1m}".to_string(),
            format_short: None,
            interval: Duration::from_secs(5),
            info: 0.3,
            warning: 0.6,
            critical: 0.9,
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

    let block_config = LoadConfig::deserialize(block_config).block_config_error("cpu")?;
    let mut text = Widget::new(id, shared_config).with_icon("cogs")?;
    let format = FormatTemplate::new(&block_config.format, block_config.format_short.as_deref())?;
    let mut interval = tokio::time::interval(block_config.interval);

    dbg!(&format);

    // borrowed from https://docs.rs/cpuinfo/0.1.1/src/cpuinfo/count/logical.rs.html#4-6
    let logical_cores = util::read_file(Path::new("/proc/cpuinfo"))
        .await
        .block_error("load", "Your system doesn't support /proc/cpuinfo")?
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count() as u32;

    let loadavg_path = Path::new("/proc/loadavg");
    loop {
        let loadavg = util::read_file(loadavg_path).await.block_error(
            "load",
            "Your system does not support reading the load average from /proc/loadavg",
        )?;
        let mut values = loadavg.split(' ');
        let m1: f64 = values
            .next()
            .map(|x| x.parse().ok())
            .flatten()
            .block_error("load", "bad /proc/loadavg file")?;
        let m5: f64 = values
            .next()
            .map(|x| x.parse().ok())
            .flatten()
            .block_error("load", "bad /proc/loadavg file")?;
        let m15: f64 = values
            .next()
            .map(|x| x.parse().ok())
            .flatten()
            .block_error("load", "bad /proc/loadavg file")?;

        text.set_state(match m1 / (logical_cores as f64) {
            x if x > block_config.critical => State::Critical,
            x if x > block_config.warning => State::Warning,
            x if x > block_config.info => State::Info,
            _ => State::Idle,
        });
        text.set_text(format.render(&map!(
            "1m" => Value::from_float(m1),
            "5m" => Value::from_float(m5),
            "15m" => Value::from_float(m15),
        ))?);

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("load", "failed to send message")?;

        interval.tick().await;
    }
}
