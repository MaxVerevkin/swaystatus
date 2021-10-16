pub mod prelude;

use serde::de::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use toml::value::{Table, Value};

use crate::click::ClickHandler;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::protocol::i3bar_block::I3BarBlock;
use crate::protocol::i3bar_event::I3BarEvent;
use crate::signals::Signal;
use crate::widget::Widget;

macro_rules! define_blocks {
    ($($type:ident $mod:ident,)*) => {
        $(mod $mod;)*

        #[derive(serde_derive::Deserialize, Debug, Clone, Copy, PartialEq)]
        #[serde(rename_all = "snake_case")]
        pub enum BlockType {
            $($type,)*
        }

        const BLOCK_NAMES: &[&str] = &[
            $(stringify!($mod),)*
        ];

        const BLOCK_SPAWNERS: &[&BlockSpawnerFn] = &[
            $(&$mod::spawn as &BlockSpawnerFn,)*
        ];

        /// Matches the block's type to block's name
        #[inline(always)]
        pub fn block_name(block: BlockType) -> &'static str {
            // SAFETY: The length of BlockType and BLOCK_NAMES is equal because the number of $type
            // is equal to the number of $mod (provided by the macro declaration)
            unsafe { BLOCK_NAMES.get_unchecked(block as usize) }
        }

        /// Matches the block's type to block's spawner function
        #[inline(always)]
        pub fn block_spawner(block: BlockType) -> &'static BlockSpawnerFn {
            // SAFETY: The length of BlockType and BLOCK_SPAWNERS is equal because the number of
            // $type is equal to the number of $mod (provided by the macro declaration)
            unsafe { BLOCK_SPAWNERS.get_unchecked(block as usize) }
        }
    };
}

define_blocks!(
    Backlight backlight,
    Battery battery,
    Bluetooth bluetooth,
    Cpu cpu,
    Custom custom,
    CustomDbus custom_dbus,
    DiskSpace disk_space,
    FocusedWindow focused_window,
    Github github,
    Load load,
    Memory memory,
    Music music,
    Net net,
    Pomodoro pomodoro,
    Sound sound,
    Speedtest speedtest,
    SwayKbd sway_kbd,
    Taskwarrior taskwarrior,
    Temperature temperature,
    Time time,
    Toggle toggle,
    Uptime uptime,
    Weather weather,
);

pub type EventsRxGetter<'a> = &'a mut dyn FnMut() -> mpsc::Receiver<BlockEvent>;

pub type BlockSpawnerFn = dyn Fn(Value, CommonApi, EventsRxGetter) -> BlockHandle;

pub type BlockHandle = tokio::task::JoinHandle<std::result::Result<(), crate::errors::Error>>;

#[derive(Debug, Clone)]
pub enum BlockMessage {
    None(usize),
    Single(usize, I3BarBlock),
    Many(usize, Vec<I3BarBlock>),
}

#[derive(Debug, Clone, Copy)]
pub enum BlockEvent {
    I3Bar(I3BarEvent),
    Signal(Signal),
}

#[derive(serde_derive::Deserialize, Debug, Clone)]
pub struct CommonConfig {
    #[serde(default)]
    pub click: ClickHandler,
    #[serde(default)]
    pub icons_format: Option<String>,
    #[serde(default)]
    pub theme_overrides: Option<HashMap<String, String>>,
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

pub struct CommonApi {
    pub id: usize,
    pub shared_config: SharedConfig,
    pub message_sender: mpsc::Sender<BlockMessage>,

    pub dbus_connection: Arc<async_lock::Mutex<Option<zbus::Connection>>>,
    pub system_dbus_connection: Arc<async_lock::Mutex<Option<zbus::Connection>>>,
}

impl CommonApi {
    pub fn new_widget(&self) -> Widget {
        Widget::new(self.id, self.shared_config.clone())
    }

    pub fn get_icon(&self, icon: &str) -> Result<String> {
        self.shared_config.get_icon(icon)
    }

    pub async fn send_empty_widget(&mut self) -> Result<()> {
        self.message_sender
            .send(BlockMessage::None(self.id))
            .await
            .error("Failed to send empty widget")
    }

    pub async fn send_widget(&mut self, widget: I3BarBlock) -> Result<()> {
        self.message_sender
            .send(BlockMessage::Single(self.id, widget))
            .await
            .error("Failed to send widget")
    }

    pub async fn send_widgets(&mut self, widgets: Vec<I3BarBlock>) -> Result<()> {
        self.message_sender
            .send(BlockMessage::Many(self.id, widgets))
            .await
            .error("Failed to send widgets")
    }

    pub async fn system_dbus_connection(&self) -> Result<zbus::Connection> {
        let mut guard = self.system_dbus_connection.lock().await;
        match &*guard {
            Some(conn) => Ok(conn.clone()),
            None => {
                let conn = zbus::ConnectionBuilder::system()
                    .unwrap()
                    .internal_executor(false)
                    .build()
                    .await
                    .error("Failed to open system DBus connection")?;
                {
                    let conn = conn.clone();
                    tokio::spawn(async move {
                        loop {
                            conn.executor().tick().await;
                        }
                    });
                }
                *guard = Some(conn.clone());
                Ok(conn)
            }
        }
    }

    pub async fn dbus_connection(&self) -> Result<zbus::Connection> {
        let mut guard = self.dbus_connection.lock().await;
        match &*guard {
            Some(conn) => Ok(conn.clone()),
            None => {
                let conn = zbus::ConnectionBuilder::session()
                    .unwrap()
                    .internal_executor(false)
                    .build()
                    .await
                    .error("Failed to open DBus connection")?;
                {
                    let conn = conn.clone();
                    tokio::spawn(async move {
                        loop {
                            conn.executor().tick().await;
                        }
                    });
                }
                conn.request_name(crate::DBUS_WELL_KNOWN_NAME)
                    .await
                    .error("Failed to reuqest DBus name")?;
                *guard = Some(conn.clone());
                Ok(conn)
            }
        }
    }
}
