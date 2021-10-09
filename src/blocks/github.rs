use reqwest::header;

use std::time::Duration;

use super::prelude::*;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GithubConfig {
    /// Update interval in seconds
    #[serde(default = "default_interval")]
    pub interval: u64,

    #[serde(default)]
    pub format: FormatTemplate,

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

pub fn spawn(id: usize, block_config: toml::Value, swaystatus: &mut Swaystatus) -> BlockHandle {
    let shared_config = swaystatus.shared_config.clone();
    let message_sender = swaystatus.message_sender.clone();
    tokio::spawn(async move {
        let block_config = GithubConfig::deserialize(block_config).block_config_error("github")?;
        let interval = Duration::from_secs(block_config.interval);
        let format = block_config.format.or_default("{total:1}")?;
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
                None => ("x".to_string(), None),
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
    })
}

async fn get_total(request: &reqwest::RequestBuilder) -> Option<i64> {
    // Send request
    let result = request.try_clone()?.send().await.ok()?.text().await.ok()?;
    // Convert to JSON
    let notifications: Vec<serde_json::Value> = serde_json::from_str(&result).ok()?;
    // The total number of notifications is just the length of a list
    Some(notifications.len() as i64)
}
