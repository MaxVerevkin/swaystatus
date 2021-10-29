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
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"{title^21}"`
//! `autohide` | Whether to hide the block when no title is available | No | `true`
//!
//! Placeholder     | Value                                     | Type | Unit
//! ----------------|-------------------------------------------|------|-----
//! `title`         | Window's titile (may be absent)           | Text | -
//! `marks`         | Window's marks                            | Text | -
//! `visible_marks` | Window's marks that do not start with `_` | Text | -
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "focused_window"
//! [block.format]
//! full = "$title.str(0,40)"
//! short = "$title.str(0,20)"
//! ```

use super::prelude::*;
use swayipc_async::{Connection, Event, EventType, WindowChange, WorkspaceChange};
use tokio_stream::StreamExt;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct FocusedWindowConfig {
    format: FormatConfig,
    autohide: bool,
}

impl Default for FocusedWindowConfig {
    fn default() -> Self {
        Self {
            format: Default::default(),
            autohide: true,
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = FocusedWindowConfig::deserialize(block_config).config_error()?;
        let format = block_config.format.or_default("{title^21}")?;
        let mut widget = api.new_widget();

        let mut title = None;
        let mut marks = Vec::new();

        let conn = Connection::new()
            .await
            .error("failed to open connection with swayipc")?;

        let mut events = conn
            .subscribe(&[EventType::Window, EventType::Workspace])
            .await
            .error("could not subscribe to window events")?;

        // Main loop
        loop {
            let event = events
                .next()
                .await
                .error("swayipc channel closed")?
                .error("bad event")?;

            let updated = match event {
                Event::Window(e) => match e.change {
                    WindowChange::Mark => {
                        marks = e.container.marks;
                        true
                    }
                    WindowChange::Focus => {
                        title = e.container.name;
                        marks = e.container.marks;
                        true
                    }
                    WindowChange::Title => {
                        if e.container.focused {
                            title = e.container.name;
                            true
                        } else {
                            false
                        }
                    }
                    WindowChange::Close => {
                        title = None;
                        marks.clear();
                        true
                    }
                    _ => false,
                },
                Event::Workspace(e) if e.change == WorkspaceChange::Init => {
                    title = None;
                    marks.clear();
                    true
                }
                _ => false,
            };

            // Render and send widget
            if updated {
                if title.is_some() || !block_config.autohide {
                    let mut values = map! {
                        "marks" => Value::text(marks.iter().map(|m| format!("[{}]",m)).collect()),
                        "visible_marks" => Value::text(marks.iter().filter(|m| !m.starts_with('_')).map(|m| format!("[{}]",m)).collect()),
                    };
                    title
                        .clone()
                        .map(|t| values.insert("title", Value::text(t)));
                    widget.set_text(format.render(&values)?);
                    api.send_widget(widget.get_data()).await?;
                } else {
                    api.send_empty_widget().await?;
                }
            }
        }
    })
}
