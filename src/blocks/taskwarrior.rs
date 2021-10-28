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
//! Placeholder     | Value                                       | Type   | Unit
//! ----------------|---------------------------------------------|--------|-----
//! `{count}`       | The number of tasks matching current filter | Number | -
//! `{filter_name}` | The name of current filter                  | Text   | -
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

use std::time::Duration;
use tokio::process::Command;

use super::prelude::*;

use crate::de::deserialize_duration;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct TaskwarriorConfig {
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    warning_threshold: u32,
    critical_threshold: u32,
    hide_when_zero: bool,
    filters: Vec<Filter>,
    format: FormatConfig,
    format_singular: FormatConfig,
    format_everything_done: FormatConfig,
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
            format: FormatConfig::default(),
            format_singular: FormatConfig::default(),
            format_everything_done: FormatConfig::default(),
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, events: EventsRxGetter) -> BlockHandle {
    let mut events = events();
    tokio::spawn(async move {
        let block_config = TaskwarriorConfig::deserialize(block_config).config_error()?;
        let format = block_config.format.or_default("{count}")?;
        let format_singular = block_config.format_singular.or_default("{count}")?;
        let format_everything_done = block_config.format_everything_done.or_default("{count}")?;
        let mut widget = api.new_widget().with_icon("tasks")?;

        let mut filters = block_config.filters.iter().cycle();
        let mut filter = filters.next().error("failed to get next filter")?;

        loop {
            let number_of_tasks = get_number_of_tasks(&filter.filter).await?;
            let values = map!(
                "count" => Value::number(number_of_tasks),
                "filter_name" => Value::text(filter.name.clone()),
            );
            widget.set_text(match number_of_tasks {
                0 => format_everything_done.render(&values)?,
                1 => format_singular.render(&values)?,
                _ => format.render(&values)?,
            });
            widget.set_state(if number_of_tasks >= block_config.critical_threshold {
                WidgetState::Critical
            } else if number_of_tasks >= block_config.warning_threshold {
                WidgetState::Warning
            } else {
                WidgetState::Idle
            });

            let mut widgets = Vec::new();
            if number_of_tasks != 0 || !block_config.hide_when_zero {
                widgets.push(widget.get_data());
            }
            api.send_widgets(widgets).await?;

            tokio::select! {
                _ = tokio::time::sleep(block_config.interval) =>(),
                Some(BlockEvent::I3Bar(click)) = events.recv() => {
                    if click.button == MouseButton::Right {
                        filter = filters.next().error("failed to get next filter")?;
                    }
                }
            }
        }
    })
}

async fn get_number_of_tasks(filter: &str) -> Result<u32> {
    String::from_utf8(
        Command::new("sh")
            .args(&["-c", &format!("task rc.gc=off {} count", filter)])
            .output()
            .await
            .error("failed to run taskwarrior for getting the number of tasks")?
            .stdout,
    )
    .error("failed to get the number of tasks from taskwarrior (invalid UTF-8)")?
    .trim()
    .parse::<u32>()
    .error("could not parse the result of taskwarrior")
}

#[derive(serde_derive::Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct Filter {
    pub name: String,
    pub filter: String,
}
