//! The number of tasks from the taskwarrior list
//!
//! Clicking on the block updates the number of tasks immediately. Clicking the right mouse button on the icon cycles the view of the block through the user's filters.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `interval` | Update interval in seconds | No | `600` (10min)
//! `warning_threshold` | The threshold of pending (or started) tasks when the block turns into a warning state | No | `10`
//! `critical_threshold` | The threshold of pending (or started) tasks when the block turns into a critical state | No | `20`
//! `hide_when_zero` | Whethere to hide the block when the number of tasks is zero | No | `false`
//! `filters` | A list of tables with the keys `name` and `filter`. `filter` specifies the criteria that must be met for a task to be counted towards this filter. | No | ```[{name = "pending", filter = "-COMPLETED -DELETED"}]```
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"{count}"`
//! `format_singular` | Same as `format` but for when exactly one task is pending | No | `"{count}"`
//! `format_everything_done` | Same as `format` but for when all tasks are completed | No | `"{count}"`
//!
//! Placeholder     | Value                                       | Type    | Unit
//! ----------------|---------------------------------------------|---------|-----
//! `{count}`       | The number of tasks matching current filter | Integer | -
//! `{filter_name}` | The name of current filter                  | String  | -
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "taskwarrior"
//! interval = 60
//! format = "{count} open tasks ({filter_name})"
//! format_singular = "{count} open task ({filter_name})"
//! format_everything_done = "nothing to do!"
//! warning_threshold = 10
//! critical_threshold = 20
//! [[block.filters]]
//! name = "today"
//! filter = "+PENDING +OVERDUE or +DUETODAY"
//! [[block.filters]]
//! name = "some-project"
//! filter = "project:some-project +PENDING"
//! ```

use serde::de::Deserialize;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widget::{State, Widget};

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct TaskwarriorConfig {
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    warning_threshold: u32,
    critical_threshold: u32,
    hide_when_zero: bool,
    filters: Vec<Filter>,
    format: FormatTemplate,
    format_singular: FormatTemplate,
    format_everything_done: FormatTemplate,
}

impl Default for TaskwarriorConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(600),
            warning_threshold: 10,
            critical_threshold: 20,
            hide_when_zero: false,
            filters: vec![Filter {
                name: "pending".to_string(),
                filter: "-COMPLETED -DELETED".to_string(),
            }],
            format: FormatTemplate::default(),
            format_singular: FormatTemplate::default(),
            format_everything_done: FormatTemplate::default(),
        }
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    mut events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let block_config =
        TaskwarriorConfig::deserialize(block_config).block_config_error("taskwarrior")?;
    let format = block_config.format.or_default("{count}")?;
    let format_singular = block_config.format_singular.or_default("{count}")?;
    let format_everything_done = block_config.format_everything_done.or_default("{count}")?;
    let mut widget = Widget::new(id, shared_config).with_icon("tasks")?;

    let mut filters = block_config.filters.iter().cycle();
    let mut filter = filters
        .next()
        .block_error("taskwarrior", "failed to get next filter")?;

    loop {
        let number_of_tasks = get_number_of_tasks(&filter.filter).await?;
        let values = map!(
            "count" => Value::from_integer(number_of_tasks as i64),
            "filter_name" => Value::from_string(filter.name.clone()),
        );
        widget.set_text(match number_of_tasks {
            0 => format_everything_done.render(&values)?,
            1 => format_singular.render(&values)?,
            _ => format.render(&values)?,
        });
        widget.set_state(if number_of_tasks >= block_config.critical_threshold {
            State::Critical
        } else if number_of_tasks >= block_config.warning_threshold {
            State::Warning
        } else {
            State::Idle
        });

        let widgets = if number_of_tasks == 0 && block_config.hide_when_zero {
            vec![]
        } else {
            vec![widget.get_data()]
        };

        message_sender
            .send(BlockMessage { id, widgets })
            .await
            .internal_error("taskwarrior", "failed to send message")?;

        tokio::select! {
            _ = tokio::time::sleep(block_config.interval) =>(),
            Some(BlockEvent::I3Bar(click)) = events_reciever.recv() => {
                if click.button == MouseButton::Right {
                    filter = filters.next().block_error("taskwarrior", "failed to get next filter")?;
                }
            }
        }
    }
}

async fn get_number_of_tasks(filter: &str) -> Result<u32> {
    String::from_utf8(
        Command::new("sh")
            .args(&["-c", &format!("task rc.gc=off {} count", filter)])
            .output()
            .await
            .block_error(
                "taskwarrior",
                "failed to run taskwarrior for getting the number of tasks",
            )?
            .stdout,
    )
    .block_error(
        "taskwarrior",
        "failed to get the number of tasks from taskwarrior (invalid UTF-8)",
    )?
    .trim()
    .parse::<u32>()
    .block_error("taskwarrior", "could not parse the result of taskwarrior")
}

#[derive(serde_derive::Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct Filter {
    pub name: String,
    pub filter: String,
}
