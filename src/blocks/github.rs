//! The number of GitHub notifications
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"$total.eng(1)|X"`
//! `interval` | Update interval in seconds | No | `30`
//! `token` | A GitHub personal access token with the "notifications" scope | Yes | -
//! `hide` | Hide this block if the total count of notifications is zero | No | `true`
//!
//! Placeholder  | Value          | Type   | Unit
//! -------------|----------------|--------|---------------
//! `total`      | The total number of notifications. Absent if something went wrong, e.g. no internet connection or token is invalid. TODO: handle invalid token differently. | Number | None
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "github"
//! format = "$total.eng(2)|N/A"
//! interval = 60
//! token = "..."
//! ```

use super::prelude::*;
use reqwest::header;
use std::{collections::HashMap, time::Duration};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct GithubConfig {
    /// Update interval in seconds
    #[serde(default = "default_interval")]
    pub interval: u64,

    #[serde(default)]
    pub format: FormatConfig,

    // A GitHub personal access token with the "notifications" scope is requried
    pub token: String,

    // Hide this block if the total count of notifications is zero
    #[serde(default = "default_hide")]
    pub hide: bool,
}

fn default_interval() -> u64 {
    30
}
fn default_hide() -> bool {
    true
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = GithubConfig::deserialize(block_config).config_error()?;
        let interval = Duration::from_secs(block_config.interval);
        let format = block_config.format.or_default("$total.eng(1)|X")?;
        let mut text = api.new_widget().with_icon("github")?;

        // Http client
        let client = reqwest::Client::new();
        let request = client
            .get("https://api.github.com/notifications")
            .header("Authorization", &format!("token {}", block_config.token))
            .header(header::USER_AGENT, "swaystatus");

        loop {
            let total = get_total(&request).await;

            let mut widgets = Vec::new();
            if total != Some(0) || !block_config.hide {
                let mut values = HashMap::new();
                total.map(|t| values.insert("total", Value::number(t)));
                text.set_text(format.render(&values)?);
                widgets.push(text.get_data());
            }

            api.send_widgets(widgets).await?;
            tokio::time::sleep(interval).await;
        }
    })
}

async fn get_total(request: &reqwest::RequestBuilder) -> Option<usize> {
    // Send request
    let result = request.try_clone()?.send().await.ok()?.text().await.ok()?;
    // Convert to JSON
    let notifications: Vec<serde_json::Value> = serde_json::from_str(&result).ok()?;
    // The total number of notifications is just the length of a list
    Some(notifications.len())
}
