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
mod sound;
mod speedtest;
mod sway_kbd;
mod taskwarrior;
mod temperature;
mod time;
mod weather;

use serde::de::Deserialize;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
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
    Sound,
    Speedtest,
    SwayKbd,
    Taskwarrior,
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

pub fn spawn_block(
    id: usize,
    block_type: BlockType,
    mut block_config: Value,
    mut shared_config: SharedConfig,
    message_tx: mpsc::Sender<BlockMessage>,
    mut events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<JoinHandle<Result<()>>> {
    let common_config = CommonConfig::new(&mut block_config)?;

    if let Some(icons_format) = common_config.icons_format {
        *shared_config.icons_format.to_mut() = icons_format;
    }
    if let Some(theme_overrides) = common_config.theme_overrides {
        shared_config
            .theme
            .to_mut()
            .apply_overrides(&theme_overrides)?;
    }
    let click_handler = common_config.click;

    // Spawn event handler
    let (events_tx, events_rx) = mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = events_reciever.recv().await {
            if let BlockEvent::I3Bar(click) = event {
                let update = click_handler.handle(click.button).await;
                if !update {
                    continue;
                }
            }
            // If events_rx is dropped then the best we can do here is just end this task
            if events_tx.send(event).await.is_err() {
                break;
            }
        }
    });

    use BlockType::*;
    Ok(match block_type {
        Backlight => backlight::spawn(id, block_config, shared_config, message_tx, events_rx),
        Battery => battery::spawn(id, block_config, shared_config, message_tx, events_rx),
        Cpu => cpu::spawn(id, block_config, shared_config, message_tx, events_rx),
        Custom => custom::spawn(id, block_config, shared_config, message_tx, events_rx),
        CustomDbus => custom_dbus::spawn(id, block_config, shared_config, message_tx, events_rx),
        DiskSpace => disk_space::spawn(id, block_config, shared_config, message_tx, events_rx),
        #[rustfmt::skip]
        FocusedWindow => focused_window::spawn(id, block_config, shared_config, message_tx, events_rx),
        Github => github::spawn(id, block_config, shared_config, message_tx, events_rx),
        Load => load::spawn(id, block_config, shared_config, message_tx, events_rx),
        Memory => memory::spawn(id, block_config, shared_config, message_tx, events_rx),
        Music => music::spawn(id, block_config, shared_config, message_tx, events_rx),
        Net => net::spawn(id, block_config, shared_config, message_tx, events_rx),
        Pomodoro => pomodoro::spawn(id, block_config, shared_config, message_tx, events_rx),
        Sound => sound::spawn(id, block_config, shared_config, message_tx, events_rx),
        Speedtest => speedtest::spawn(id, block_config, shared_config, message_tx, events_rx),
        SwayKbd => sway_kbd::spawn(id, block_config, shared_config, message_tx, events_rx),
        Taskwarrior => taskwarrior::spawn(id, block_config, shared_config, message_tx, events_rx),
        Temperature => temperature::spawn(id, block_config, shared_config, message_tx, events_rx),
        Time => time::spawn(id, block_config, shared_config, message_tx, events_rx),
        Weather => weather::spawn(id, block_config, shared_config, message_tx, events_rx),
    })
}
