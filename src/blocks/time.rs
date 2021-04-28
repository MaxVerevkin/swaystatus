use serde::de::Deserialize;
use std::convert::TryInto;
use std::time::Duration;
use tokio::sync::mpsc;

use chrono::offset::{Local, Utc};
use chrono::Locale;
use chrono_tz::Tz;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::widgets::widget::Widget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct TimeConfig {
    /// Format string.
    /// See [chrono docs](https://docs.rs/chrono/0.3.0/chrono/format/strftime/index.html#specifiers) for all options.
    format: String,

    /// Update interval in seconds
    interval: u64,

    pub timezone: Option<Tz>,

    pub locale: Option<String>,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            format: "%a %d/%m %R".to_string(),
            interval: 5,
            timezone: None,
            locale: None,
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

    let block_config = TimeConfig::deserialize(block_config).block_config_error("time")?;
    let mut interval = tokio::time::interval(Duration::from_secs(block_config.interval));
    let mut text = Widget::new(id, shared_config).with_icon("time")?;

    loop {
        let time = match &block_config.locale {
            Some(l) => {
                let locale: Locale = l
                    .as_str()
                    .try_into()
                    .ok()
                    .block_error("time", "invalid locale")?;
                match block_config.timezone {
                    Some(tz) => Utc::now()
                        .with_timezone(&tz)
                        .format_localized(&block_config.format, locale),
                    None => Local::now().format_localized(&block_config.format, locale),
                }
            }
            None => match block_config.timezone {
                Some(tz) => Utc::now().with_timezone(&tz).format(&block_config.format),
                None => Local::now().format(&block_config.format),
            },
        };
        text.set_text(time.to_string());

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("time", "failed to send message")?;

        interval.tick().await;
    }
}
