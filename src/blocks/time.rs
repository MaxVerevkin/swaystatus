use chrono::offset::Local;
use serde::de::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::widgets::text::TextWidget;
use crate::widgets::I3BarWidget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct TimeConfig {
    /// Format string.
    /// See [chrono docs](https://docs.rs/chrono/0.3.0/chrono/format/strftime/index.html#specifiers) for all options.
    pub format: String,

    /// Update interval in seconds
    pub interval: u64,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            format: "%a %d/%m %R".to_string(),
            interval: 5,
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

    let block_config =
        TimeConfig::deserialize(block_config).block_error("time", "failed to fase config")?;
    let interval = Duration::from_secs(block_config.interval);

    let mut text = TextWidget::new(id, 0, shared_config).with_icon("time")?;

    loop {
        text.set_text(Local::now().format(&block_config.format).to_string());
        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("time", "failed to send message")?;

        tokio::time::sleep(interval).await;
    }
}
