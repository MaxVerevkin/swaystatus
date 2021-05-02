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

    /// Same as `format` but used when i3bar/swaystatus doesn't have enough space for every block
    format_short: Option<String>,

    /// Update interval in seconds
    interval: u64,

    timezone: Option<Tz>,

    locale: Option<String>,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            format: "%a %d/%m %R".to_string(),
            format_short: None,
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

    let format = block_config.format.as_str();
    let format_short = block_config.format_short.as_deref();
    let timezone = block_config.timezone;
    let locale = match block_config.locale.as_deref() {
        Some(locale) => Some(
            locale
                .try_into()
                .ok()
                .block_error("time", "invalid locale")?,
        ),
        None => None,
    };

    loop {
        let full_time = get_time(format, timezone, locale)?;
        match format_short {
            Some(format_short) => {
                text.set_text((full_time, Some(get_time(format_short, timezone, locale)?)))
            }
            None => text.set_full_text(full_time),
        }

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

fn get_time(format: &str, timezone: Option<Tz>, locale: Option<Locale>) -> Result<String> {
    Ok(match locale {
        Some(locale) => match timezone {
            Some(tz) => Utc::now()
                .with_timezone(&tz)
                .format_localized(format, locale)
                .to_string(),
            None => Local::now().format_localized(format, locale).to_string(),
        },
        None => match timezone {
            Some(tz) => Utc::now().with_timezone(&tz).format(format).to_string(),
            None => Local::now().format(format).to_string(),
        },
    })
}
