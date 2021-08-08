use futures::stream::StreamExt;
use serde::de::Deserialize;
use std::collections::HashMap;
use tokio::sync::mpsc;

use swayipc_async::{Connection, Event, EventType};

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widget::Widget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct SwayKbdConfig {
    #[serde(default)]
    pub format: FormatTemplate,
    #[serde(default)]
    pub mappings: Option<HashMap<String, String>>,
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

    let block_config = SwayKbdConfig::deserialize(block_config).block_config_error("sway_kbd")?;
    let format = block_config.format.or_default("{layout}")?;
    let mut text = Widget::new(id, shared_config);

    // New connection
    let mut connection = Connection::new()
        .await
        .block_error("sway_kbd", "failed to open swayipc connection")?;

    // Get current layout
    let mut layout = connection
        .get_inputs()
        .await
        .block_error("sway_kbd", "failed to get current input")?
        .iter()
        .find(|i| i.input_type == "keyboard")
        .map(|i| i.xkb_active_layout_name.clone())
        .flatten()
        .block_error("sway_kbd", "failed to get current input")?;

    // Subscribe to events
    let mut events = connection
        .subscribe(&[EventType::Input])
        .await
        .block_error("sway_kbd", "failed to subscribe to events")?;

    loop {
        let layout_mapped = if let Some(ref mappings) = block_config.mappings {
            mappings.get(&layout).unwrap_or(&layout).to_string()
        } else {
            layout.clone()
        };

        text.set_text(format.render(&map! {
            "layout" => Value::from_string(layout_mapped),
        })?);

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("sway_kbd", "failed to send message")?;

        // Wait for new event
        loop {
            let event = events
                .next()
                .await
                .block_error("sway_kbd", "swayipc channel closed")?
                .block_error("sway_kbd", "bad event")?;
            if let Event::Input(event) = event {
                if let Some(new_layout) = event.input.xkb_active_layout_name {
                    // Update only if layout has changed
                    if new_layout != layout {
                        layout = new_layout;
                        break;
                    }
                }
            }
        }
    }
}
