//! Currently focused window
//!
//! This block displays the title or the active marks of the currently focused window. Uses push
//! updates from i3 IPC, so no need to worry about resource usage. The block only updates when the
//! focused window changes title or the focus changes. Also works with sway, due to it having
//! compatibility with i3's IPC.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"{window}"`
//!
//! Placeholder      | Value                                     | Type   | Unit
//! -----------------|-------------------------------------------|--------|-----
//! `{title}`        | Window's titile                           | String | -
//! `{marks}`        | Window's marks                            | String | -
//! `{visible_marks}`| Window's marks that do not start with `_` | String | -
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "focused_window"
//! [block.format]
//! full = "{title^40}"
//! short = "{title^20}"
//! ```

use serde::de::Deserialize;
use serde_derive::Deserialize;
use swayipc_async::{Connection, Event, EventType, Node, WindowChange, WorkspaceChange};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::blocks::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widgets::widget::Widget;

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct FocusedWindowConfig {
    #[serde(default)]
    pub format: Option<FormatTemplate>,
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_receiver: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    drop(events_receiver);

    let block_config =
        FocusedWindowConfig::deserialize(block_config).block_config_error("focused_window")?;
    let format = default_format!(block_config.format.clone(), "{window^21}")?;
    let mut widget = Widget::new(id, shared_config);

    let mut title = "".to_string();
    let mut marks = Vec::new();

    let conn = Connection::new()
        .await
        .block_error("focused_window", "failed to open connection with swayipc")?;

    let mut events = conn
        .subscribe(&[EventType::Window, EventType::Workspace])
        .await
        .block_error("focused_window", "could not subscribe to window events")?;

    // Main loop
    loop {
        let event = events
            .next()
            .await
            .block_error("focused_window", "swayipc channel closed")?
            .block_error("focused_window", "bad event")?;

        dbg!(&event);

        let updated = match event {
            Event::Window(e) => match (e.change, e.container) {
                (
                    WindowChange::Mark,
                    Node {
                        marks: new_marks, ..
                    },
                ) => {
                    marks = new_marks;
                    true
                }
                (
                    WindowChange::Focus,
                    Node {
                        name,
                        marks: new_marks,
                        ..
                    },
                ) => {
                    title = name.unwrap_or_default();
                    marks = new_marks;
                    true
                }
                (
                    WindowChange::Title,
                    Node {
                        focused: true,
                        name: Some(name),
                        ..
                    },
                ) => {
                    title = name;
                    true
                }
                (
                    WindowChange::Close,
                    Node {
                        name: Some(name), ..
                    },
                ) if name == title => {
                    title.clear();
                    marks.clear();
                    true
                }
                _ => false,
            },
            Event::Workspace(e) if e.change == WorkspaceChange::Init => {
                title.clear();
                marks.clear();
                true
            }
            _ => false,
        };

        // Render and send widget (send empty vec when text is empty to hide the block)
        if updated {
            let mut widgets = vec![];
            widget.set_text(format.render(&map! {
                "title" => Value::from_string(title.clone()),
                "marks" => Value::from_string(marks.iter().map(|m| format!("[{}]",m)).collect()),
                "visible_marks" => Value::from_string(marks.iter().filter(|m| !m.starts_with('_')).map(|m| format!("[{}]",m)).collect()),
            })?);
            widgets.push(widget.get_data());
            message_sender
                .send(BlockMessage { id, widgets })
                .await
                .internal_error("focused_window", "failed to send message")?;
        }
    }
}
