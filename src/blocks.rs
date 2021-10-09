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

pub mod prelude;

use serde::de::Deserialize;
use std::collections::HashMap;
use tokio::task::JoinHandle;
use toml::value::{Table, Value};

use crate::click::ClickHandler;
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

pub type BlockHandle = tokio::task::JoinHandle<std::result::Result<(), crate::errors::Error>>;

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
    swaystatus: &mut crate::Swaystatus,
) -> Result<(JoinHandle<Result<()>>, ClickHandler)> {
    let common_config = CommonConfig::new(&mut block_config)?;

    if let Some(icons_format) = common_config.icons_format {
        *swaystatus.shared_config.icons_format.to_mut() = icons_format;
    }
    if let Some(theme_overrides) = common_config.theme_overrides {
        swaystatus
            .shared_config
            .theme
            .to_mut()
            .apply_overrides(&theme_overrides)?;
    }
    let click_handler = common_config.click;

    use BlockType::*;
    Ok((
        match block_type {
            Backlight => backlight::spawn(id, block_config, swaystatus),
            Battery => battery::spawn(id, block_config, swaystatus),
            Cpu => cpu::spawn(id, block_config, swaystatus),
            Custom => custom::spawn(id, block_config, swaystatus),
            CustomDbus => custom_dbus::spawn(id, block_config, swaystatus),
            DiskSpace => disk_space::spawn(id, block_config, swaystatus),
            FocusedWindow => focused_window::spawn(id, block_config, swaystatus),
            Github => github::spawn(id, block_config, swaystatus),
            Load => load::spawn(id, block_config, swaystatus),
            Memory => memory::spawn(id, block_config, swaystatus),
            Music => music::spawn(id, block_config, swaystatus),
            Net => net::spawn(id, block_config, swaystatus),
            Pomodoro => pomodoro::spawn(id, block_config, swaystatus),
            Sound => sound::spawn(id, block_config, swaystatus),
            Speedtest => speedtest::spawn(id, block_config, swaystatus),
            SwayKbd => sway_kbd::spawn(id, block_config, swaystatus),
            Taskwarrior => taskwarrior::spawn(id, block_config, swaystatus),
            Temperature => temperature::spawn(id, block_config, swaystatus),
            Time => time::spawn(id, block_config, swaystatus),
            Weather => weather::spawn(id, block_config, swaystatus),
        },
        click_handler,
    ))
}
