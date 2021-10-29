//! System's uptime
//!
//! This block displays system uptime in terms of two biggest units, so minutes and seconds, or
//! hours and minutes or days and hours or weeks and days.
//!
//! # Configuration
//!
//! Key        | Values                     | Required | Default
//! -----------|----------------------------|----------|--------
//! `interval` | Update interval in seconds | No       | `60`
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "uptime"
//! interval = "3600" # update every hour
//! ```
//!
//! # TODO:
//! - Add `time` or `dur` formatter to `src/formatting/formatter.rs`

use super::prelude::*;
use std::time::Duration;
use tokio::fs::read_to_string;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct UptimeConfig {
    interval: u64,
}

impl Default for UptimeConfig {
    fn default() -> Self {
        Self { interval: 60 }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = UptimeConfig::deserialize(block_config).config_error()?;
        let mut interval = tokio::time::interval(Duration::from_secs(block_config.interval));
        let mut widget = api.new_widget().with_icon("uptime")?;

        loop {
            let uptime = read_to_string("/proc/uptime")
                .await
                .error("Failed to read /proc/uptime")?;
            let mut seconds: u64 = uptime
                .split('.')
                .next()
                .and_then(|u| u.parse().ok())
                .error("/proc/uptime has invalid content")?;

            let weeks = seconds / 604_800;
            seconds %= 604_800;
            let days = seconds / 86_400;
            seconds %= 86_400;
            let hours = seconds / 3_600;
            seconds %= 3_600;
            let minutes = seconds / 60;
            seconds %= 60;

            let text = if weeks > 0 {
                format!("{}w {}d", weeks, days)
            } else if days > 0 {
                format!("{}d {}h", days, hours)
            } else if hours > 0 {
                format!("{}h {}m", hours, minutes)
            } else {
                format!("{}m {}s", minutes, seconds)
            };

            widget.set_full_text(text);
            api.send_widget(widget.get_data()).await?;
            interval.tick().await;
        }
    })
}
