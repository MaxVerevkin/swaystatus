pub mod prelude;

use serde::de::Deserialize;
use smallvec::SmallVec;
use smartstring::alias::String;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use toml::value::Table;

use crate::click::ClickHandler;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, Format};
use crate::protocol::i3bar_event::I3BarEvent;
use crate::signals::Signal;
use crate::widget::WidgetState;
use crate::{Request, RequestCmd};

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
    // Weather weather,
);

pub type EventsRxGetter<'a> = &'a mut dyn FnMut() -> mpsc::Receiver<BlockEvent>;

pub type BlockSpawnerFn = dyn Fn(toml::Value, CommonApi, EventsRxGetter) -> BlockHandle;

pub type BlockHandle = tokio::task::JoinHandle<std::result::Result<(), crate::errors::Error>>;

#[derive(Debug, Clone, Copy)]
pub enum BlockEvent {
    Click(I3BarEvent),
    Signal(Signal),
}

pub struct CommonApi {
    pub id: usize,
    pub shared_config: SharedConfig,

    pub request_sender: mpsc::Sender<Request>,
    pub cmd_buf: SmallVec<[RequestCmd; 4]>,

    pub dbus_connection: Arc<async_lock::Mutex<Option<zbus::Connection>>>,
    pub system_dbus_connection: Arc<async_lock::Mutex<Option<zbus::Connection>>>,
}

impl CommonApi {
    pub fn hide(&mut self) {
        self.cmd_buf.push(RequestCmd::Hide);
    }

    pub fn collapse(&mut self) {
        self.cmd_buf.push(RequestCmd::Collapse);
    }

    pub fn show(&mut self) {
        self.cmd_buf.push(RequestCmd::Show);
    }

    pub fn set_icon(&mut self, icon: &str) -> Result<()> {
        let icon = if icon.is_empty() {
            String::new()
        } else {
            self.get_icon(icon)?
        };
        self.cmd_buf.push(RequestCmd::SetIcon(icon));
        Ok(())
    }

    pub fn set_state(&mut self, state: WidgetState) {
        self.cmd_buf.push(RequestCmd::SetState(state));
    }

    pub fn set_text(&mut self, text: (String, Option<String>)) {
        self.cmd_buf.push(RequestCmd::SetText(text))
    }

    pub fn set_values(&mut self, values: HashMap<String, Value>) {
        self.cmd_buf.push(RequestCmd::SetValues(values));
    }

    pub fn set_format(&mut self, format: Arc<Format>) {
        self.cmd_buf.push(RequestCmd::SetFormat(format));
    }

    pub fn add_button(&mut self, instance: usize, icon: &str) -> Result<()> {
        self.cmd_buf
            .push(RequestCmd::AddButton(instance, self.get_icon(icon)?));
        Ok(())
    }

    pub fn set_button(&mut self, instance: usize, icon: &str) -> Result<()> {
        self.cmd_buf
            .push(RequestCmd::SetButton(instance, self.get_icon(icon)?));
        Ok(())
    }

    pub fn render(&mut self) {
        self.cmd_buf.push(RequestCmd::Render);
    }

    pub async fn flush(&mut self) -> Result<()> {
        let cmds = std::mem::replace(&mut self.cmd_buf, SmallVec::new());
        self.request_sender
            .send(Request {
                block_id: self.id,
                cmds,
            })
            .await
            .error("Failed to send Request")?;
        Ok(())
    }
}

impl CommonApi {
    pub fn get_icon(&self, icon: &str) -> Result<String> {
        self.shared_config.get_icon(icon)
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

#[derive(serde_derive::Deserialize, Debug)]
pub struct CommonConfig {
    #[serde(default)]
    pub click: ClickHandler,
    #[serde(default)]
    pub icons_format: Option<String>,
    #[serde(default)]
    pub theme_overrides: Option<HashMap<String, String>>,
}

impl CommonConfig {
    pub fn new(from: &mut toml::Value) -> Result<Self> {
        const FIELDS: &[&str] = &["click", "theme_overrides", "icons_format"];
        let mut common_table = Table::new();
        if let Some(table) = from.as_table_mut() {
            for &field in FIELDS {
                if let Some(it) = table.remove(field) {
                    common_table.insert(field.to_string(), it);
                }
            }
        }
        let common_value: toml::Value = common_table.into();
        CommonConfig::deserialize(common_value).config_error()
    }
}
