use serde::de::Deserialize;
use serde_derive::Deserialize;
use swayipc_async::{Connection, Event, EventType, Node, WindowChange, WorkspaceChange};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::blocks::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::{BlockError, Result, ResultExt};
use crate::widgets::text::TextWidget;
use crate::widgets::I3BarWidget;

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarksType {
    All,
    Visible,
    None,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct FocusedWindowConfig {
    /// Truncates titles if longer than max-width
    /// TODO replace with `format`
    pub max_width: usize,

    /// Show marks in place of title (if exist)
    pub show_marks: MarksType,
}

impl Default for FocusedWindowConfig {
    fn default() -> Self {
        Self {
            max_width: 21,
            show_marks: MarksType::None,
        }
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_receiver: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    std::mem::drop(events_receiver);

    let block_config =
        FocusedWindowConfig::deserialize(block_config).block_config_error("focused_window")?;

    let mut title = "".to_string();
    let mut marks = Vec::new();

    let conn = Connection::new()
        .await
        .block_error("focused_window", "failed to open connection with swayipc")?;

    let mut events = conn
        .subscribe(&[EventType::Window, EventType::Workspace])
        .await
        .block_error("focused_window", "could not subscribe to window events")?;

    // Render text for marks
    let marks_str = |marks: &[String]| -> String {
        let mut result = "".to_string();

        for mark in marks {
            match block_config.show_marks {
                MarksType::All => {
                    result.push_str(&format!("[{}]", mark));
                }
                MarksType::Visible if !mark.starts_with('_') => {
                    result.push_str(&format!("[{}]", mark));
                }
                _ => {}
            }
        }

        result
    };

    // Render and send widget
    let update = |title: &str, marks: &[String]| {
        let text: String = match block_config.show_marks {
            MarksType::None => title.to_string(),
            _ => marks_str(marks),
        }
        .chars()
        .take(block_config.max_width)
        .collect();

        let widget = TextWidget::new(id, 0, shared_config.clone())
            .with_text(&text)
            .get_data();

        async {
            message_sender
                .send(BlockMessage {
                    id,
                    widgets: vec![widget],
                })
                .await
                .internal_error("focused_window", "failed to send message")?;

            Ok(())
        }
    };

    // Main loop
    while let Some(event) = events.next().await {
        let event =
            event.block_error("focused_window", "could not read event in `window` block")?;

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

        if updated {
            update(&title, &marks).await?;
        }
    }

    Err(BlockError {
        block: "focused_window".to_string(),
        message: "swayipc channel closed".to_string(),
        cause: None,
        cause_dbg: None,
    })
}
