use std::str::FromStr;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use serde::de::Deserialize;

use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::protocol::i3bar_event::MouseButton;
use crate::widgets::text::TextWidget;
use crate::widgets::I3BarWidget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct NetConfig {
    /// Format string for `Net` block.
    pub format: String,

    /// Format string that is applied afted a click
    pub format_alt: Option<String>,

    /// Format string for `Net` block.
    pub interface: String,

    /// The delay in seconds between updates.
    pub interval: u64,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            format: "{speed_down;K}{speed_up;k}".to_string(),
            format_alt: None,
            interface: "lo".to_string(), // FIXME detect automatically
            interval: 2,
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
    let block_config = NetConfig::deserialize(block_config).block_config_error("net")?;
    let mut format = FormatTemplate::from_string(&block_config.format)?;
    let mut format_alt = match block_config.format_alt {
        Some(ref format_alt) => Some(FormatTemplate::from_string(format_alt)?),
        None => None,
    };

    let mut text = TextWidget::new(id, 0, shared_config.clone()).with_icon("net_wireless")?; // FIXME select icont automatically
    let interval = Duration::from_secs(block_config.interval);

    loop {
        // FIXME
        let speed_down: f64 = 0.0;
        let speed_up: f64 = 0.0;

        text.set_text(format.render(&map! {
            "speed_down" => Value::from_float(speed_down).bytes().icon(shared_config.get_icon("net_down")?),
            "speed_up" => Value::from_float(speed_up).bytes().icon(shared_config.get_icon("net_up")?),
        })?);

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("net", "failed to send message")?;

        tokio::select! {
            _ = tokio::time::sleep(interval) =>(),
            event = events_reciever.recv() => {
                if let BlockEvent::I3Bar(click) = event.unwrap() {
                    if click.button == MouseButton::Left {
                        if let Some(ref mut format_alt) = format_alt {
                            std::mem::swap(format_alt, &mut format);
                        }
                    }
                }
            }
        }
    }
}
