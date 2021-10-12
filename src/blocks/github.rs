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

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = GithubConfig::deserialize(block_config).config_error()?;
        let interval = Duration::from_secs(block_config.interval);
        let format = block_config.format.or_default("{total:1}")?;
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
                text.set_text(match total {
                    Some(total) => format.render(&map! {
                        "total" => Value::from_integer(total as i64),
                    })?,
                    None => ("x".to_string(), None),
                });
                widgets.push(text.get_data());
            }

            api.send_widgets(widgets).await?;
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
