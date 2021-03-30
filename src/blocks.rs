pub mod memory;
pub mod sway_kbd;
pub mod temperature;
pub mod time;

use serde::de::Deserialize;
use std::collections::HashMap;
use tokio::sync::mpsc;
use toml::value::{Table, Value};

use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_block::I3BarBlock;
use crate::protocol::i3bar_event::{I3BarEvent, MouseButton};
use crate::signals::Signal;
use crate::subprocess::spawn_child_async;

#[derive(serde_derive::Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Time,
    Memory,
    SwayKbd,
    Temperature,
}

#[derive(Debug)]
pub struct BlockMessage {
    pub id: usize,
    pub widgets: Vec<I3BarBlock>,
}

#[derive(Debug, Clone, Copy)]
pub enum BlockEvent {
    I3Bar(I3BarEvent),
    Signal(Signal),
}

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct CommonConfig {
    #[serde(default)]
    pub on_click: Option<String>,
    #[serde(default)]
    pub on_right_click: Option<String>,
    #[serde(default)]
    pub icons_format: Option<String>,
    #[serde(default)]
    pub theme_overrides: Option<HashMap<String, String>>,
}

impl CommonConfig {
    pub fn new(from: &mut Value) -> Result<Self> {
        const FIELDS: &[&str] = &[
            "on_click",
            "on_right_click",
            "theme_overrides",
            "icons_format",
        ];

        // FIXME (?): this function is to paper over https://github.com/serde-rs/serde/issues/1957
        let mut common_table = Table::new();
        if let Some(table) = from.as_table_mut() {
            for &field in FIELDS {
                if let Some(it) = table.remove(field) {
                    common_table.insert(field.to_string(), it);
                }
            }
        }
        let common_value: Value = common_table.into();
        CommonConfig::deserialize(common_value)
            .configuration_error("failed to deserialize common config")
    }
}

pub async fn run_block(
    id: usize,
    block_type: BlockType,
    mut block_config: Value,
    mut shared_config: SharedConfig,
    message_tx: mpsc::Sender<BlockMessage>,
    mut events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let common_config = CommonConfig::new(&mut block_config)?;

    if let Some(icons_format) = common_config.icons_format {
        shared_config.icons_format_override(icons_format);
    }
    if let Some(theme_overrides) = common_config.theme_overrides {
        shared_config.theme_override(&theme_overrides)?;
    }
    let on_click = common_config.on_click;
    let on_right_click = common_config.on_right_click;

    let (evets_tx, events_rx) = mpsc::channel(64);
    tokio::task::spawn(async move {
        loop {
            let event = match events_reciever.recv().await {
                Some(e) => e,
                None => break,
            };
            match event {
                BlockEvent::I3Bar(click) => {
                    if let Some(ref on_click) = on_click {
                        if click.button == MouseButton::Left {
                            let _ = spawn_child_async("sh", &["-c", on_click]);
                        }
                    }
                    if let Some(ref on_right_click) = on_right_click {
                        if click.button == MouseButton::Right {
                            let _ = spawn_child_async("sh", &["-c", on_right_click]);
                        }
                    }
                }
                BlockEvent::Signal(_signal) => {
                    // TODO handle signals
                }
            }
            // Reciever might be droped -- but we don't care
            let _ = evets_tx.send(event).await;
        }
    });

    match block_type {
        BlockType::Time => time::run(id, block_config, shared_config, message_tx, events_rx).await,
        BlockType::Memory => {
            memory::run(id, block_config, shared_config, message_tx, events_rx).await
        }
        BlockType::SwayKbd => {
            sway_kbd::run(id, block_config, shared_config, message_tx, events_rx).await
        }
        BlockType::Temperature => {
            temperature::run(id, block_config, shared_config, message_tx, events_rx).await
        }
    }
}
