mod backlight;
mod battery;
mod cpu;
mod custom;
mod custom_dbus;
mod disk_space;
mod focused_window;
mod github;
mod load;
mod memory;
mod music;
mod net;
mod pomodoro;
mod speedtest;
mod sway_kbd;
mod temperature;
mod time;
mod weather;

use serde::de::Deserialize;
use std::collections::HashMap;
use tokio::sync::mpsc;
use toml::value::{Table, Value};

use crate::click::ClickHandler;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_block::I3BarBlock;
use crate::protocol::i3bar_event::I3BarEvent;
use crate::signals::Signal;

#[derive(serde_derive::Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Backlight,
    Battery,
    Cpu,
    Custom,
    CustomDbus,
    DiskSpace,
    FocusedWindow,
    Github,
    Load,
    Memory,
    Music,
    Net,
    Pomodoro,
    Speedtest,
    SwayKbd,
    Temperature,
    Time,
    Weather,
}

#[derive(Debug, Clone)]
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
struct CommonConfig {
    #[serde(default)]
    click: ClickHandler,
    #[serde(default)]
    icons_format: Option<String>,
    #[serde(default)]
    theme_overrides: Option<HashMap<String, String>>,
}

impl CommonConfig {
    pub fn new(from: &mut Value) -> Result<Self> {
        const FIELDS: &[&str] = &["click", "theme_overrides", "icons_format"];
        let mut common_table = Table::new();
        if let Some(table) = from.as_table_mut() {
            for &field in FIELDS {
                if let Some(it) = table.remove(field) {
                    common_table.insert(field.to_string(), it);
                }
            }
        }
        let common_value: Value = common_table.into();
        CommonConfig::deserialize(common_value).config_error()
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
    let click_handler = common_config.click;

    // Spawn event handler
    let (evets_tx, events_rx) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = events_reciever.recv().await {
            if let BlockEvent::I3Bar(click) = event {
                let update = click_handler.handle(click.button).await;
                if !update {
                    continue;
                }
            }
            // Reciever might be droped -- but we don't care
            let _ = evets_tx.send(event).await;
        }
    });

    use BlockType::*;
    match block_type {
        Backlight => backlight::run(id, block_config, shared_config, message_tx, events_rx).await,
        Battery => battery::run(id, block_config, shared_config, message_tx, events_rx).await,
        Cpu => cpu::run(id, block_config, shared_config, message_tx, events_rx).await,
        Custom => custom::run(id, block_config, shared_config, message_tx, events_rx).await,
        CustomDbus => {
            custom_dbus::run(id, block_config, shared_config, message_tx, events_rx).await
        }
        DiskSpace => disk_space::run(id, block_config, shared_config, message_tx, events_rx).await,
        FocusedWindow => {
            focused_window::run(id, block_config, shared_config, message_tx, events_rx).await
        }
        Github => github::run(id, block_config, shared_config, message_tx, events_rx).await,
        Load => load::run(id, block_config, shared_config, message_tx, events_rx).await,
        Memory => memory::run(id, block_config, shared_config, message_tx, events_rx).await,
        Music => music::run(id, block_config, shared_config, message_tx, events_rx).await,
        Net => net::run(id, block_config, shared_config, message_tx, events_rx).await,
        Pomodoro => pomodoro::run(id, block_config, shared_config, message_tx, events_rx).await,
        Speedtest => speedtest::run(id, block_config, shared_config, message_tx, events_rx).await,
        SwayKbd => sway_kbd::run(id, block_config, shared_config, message_tx, events_rx).await,
        Temperature => {
            temperature::run(id, block_config, shared_config, message_tx, events_rx).await
        }
        Time => time::run(id, block_config, shared_config, message_tx, events_rx).await,
        Weather => weather::run(id, block_config, shared_config, message_tx, events_rx).await,
    }
}
