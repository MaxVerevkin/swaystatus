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

use serde_derive::Deserialize;
use swayipc_async::{Connection, Event, EventType, WindowChange, WorkspaceChange};
use tokio_stream::StreamExt;

use super::prelude::*;

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct FocusedWindowConfig {
    format: FormatTemplate,
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
        let block_config =
            FocusedWindowConfig::deserialize(block_config).config_error()?;
        let format = block_config.format.clone().or_default("{title^21}")?;
        let mut widget = api.new_widget();

        let mut title = None;
        let mut marks = Vec::new();

        let conn = Connection::new()
            .await
            .error( "failed to open connection with swayipc")?;

        let mut events = conn
            .subscribe(&[EventType::Window, EventType::Workspace])
            .await
            .error( "could not subscribe to window events")?;

        // Main loop
        loop {
            let event = events
                .next()
                .await
                .error( "swayipc channel closed")?
                .error( "bad event")?;

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
                let mut widgets = vec![];
                if title.is_some() || !block_config.autohide {
                    widget.set_text(format.render(&map! {
                        "title" => Value::from_string(title.clone().unwrap_or_default()),
                        "marks" => Value::from_string(marks.iter().map(|m| format!("[{}]",m)).collect()),
                        "visible_marks" => Value::from_string(marks.iter().filter(|m| !m.starts_with('_')).map(|m| format!("[{}]",m)).collect()),
                    })?);
                    widgets.push(widget.get_data());
                }
                api.send_widgets(widgets).await?;
            }
        }
    })
}
