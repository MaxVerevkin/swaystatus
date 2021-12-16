//! Sway's keyboard layout indicator
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"$layout"`
//! `mappings` | Layouts' names can be mapped to custom names. See below for an example. | No | None
//!
//! Placeholder | Value          | Type   | Unit
//! ------------|----------------|--------|-----
//! `layout`    | Current layout | Text   | -
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! [block.mappings]
//! "English (Workman)" = "EN"
//! "Russian" = "RU"
//! ```

use super::prelude::*;
use futures::stream::StreamExt;
use std::collections::HashMap;
use swayipc_async::{Connection, Event, EventType};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct SwayKbdConfig {
    #[serde(default)]
    pub format: FormatConfig,
    #[serde(default)]
    pub mappings: Option<HashMap<StdString, StdString>>,
}

pub async fn run(block_config: toml::Value, mut api: CommonApi) -> Result<()> {
    let block_config = SwayKbdConfig::deserialize(block_config).config_error()?;
    api.set_format(block_config.format.init("$layout", &api)?);

    // New connection
    let mut connection = Connection::new()
        .await
        .error("failed to open swayipc connection")?;

    // Get current layout
    let mut layout = connection
        .get_inputs()
        .await
        .error("failed to get current input")?
        .iter()
        .find(|i| i.input_type == "keyboard")
        .map(|i| i.xkb_active_layout_name.clone())
        .flatten()
        .error("failed to get current input")?;

    // Subscribe to events
    let mut events = connection
        .subscribe(&[EventType::Input])
        .await
        .error("failed to subscribe to events")?;

    loop {
        let layout_mapped = if let Some(ref mappings) = block_config.mappings {
            mappings.get(&layout).unwrap_or(&layout).into()
        } else {
            (&layout).into()
        };

        api.set_values(map! {
            "layout" => Value::text(layout_mapped),
        });
        api.render();
        api.flush().await?;

        // Wait for new event
        loop {
            let event = events
                .next()
                .await
                .error("swayipc channel closed")?
                .error("bad event")?;
            if let Event::Input(event) = event {
                if let Some(new_layout) = event.input.xkb_active_layout_name {
                    // Update only if layout has changed
                    if new_layout != layout {
                        layout = new_layout;
                        break;
                    }
                }
            }
        }
    }
}
