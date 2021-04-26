use reqwest::header;
use serde::de::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widgets::widget::Widget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GithubConfig {
    /// Update interval in seconds
    #[serde(default = "default_interval")]
    pub interval: u64,

    #[serde(default = "default_format")]
    pub format: String,

    // A GitHub personal access token with the "notifications" scope is requried
    pub token: String,

    // Hide this block if the total count of notifications is zero
    #[serde(default = "default_hide")]
    pub hide: bool,
}

fn default_interval() -> u64 {
    30
}
fn default_format() -> String {
    "{total:1}".to_string()
}
fn default_hide() -> bool {
    true
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    // Drop the reciever if we don't what to recieve events
    drop(events_reciever);

    let block_config = GithubConfig::deserialize(block_config).block_config_error("github")?;
    let interval = Duration::from_secs(block_config.interval);
    let format = FormatTemplate::from_string(&block_config.format)?;
    let mut text = Widget::new(id, shared_config).with_icon("github")?;

    // Http client
    let client = reqwest::Client::new();
    let request = client
        .get("https://api.github.com/notifications")
        .header("Authorization", &format!("token {}", block_config.token))
        .header(header::USER_AGENT, "swaystatus");

    loop {
        let total = get_total(&request).await;

        text.set_text(match total {
            Some(total) => format.render(&map! {
                "total" => Value::from_integer(total as i64),
            })?,
            None => "x".to_string(),
        });

        message_sender
            .send(BlockMessage {
                id,
                widgets: if total == Some(0) && block_config.hide {
                    vec![]
                } else {
                    vec![text.get_data()]
                },
            })
            .await
            .internal_error("github", "failed to send message")?;

        tokio::time::sleep(interval).await;
    }
}

async fn get_total(request: &reqwest::RequestBuilder) -> Option<i64> {
    // Send request
    let result = request.try_clone()?.send().await.ok()?.text().await.ok()?;
    // Convert to JSON
    let notifications: Vec<serde_json::Value> = serde_json::from_str(&result).ok()?;
    // The total number of notifications is just the length of a list
    Some(notifications.len() as i64)
}
