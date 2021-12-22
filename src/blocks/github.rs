//! The number of GitHub notifications
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | A string to customise the output of this block. See below for available placeholders. | No | `"$total.eng(1)"`
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
//!
//! # Icons Used
//! - `github`

use super::prelude::*;
use reqwest::header;

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

pub async fn run(config: toml::Value, mut api: CommonApi) -> Result<()> {
    let config = GithubConfig::deserialize(config).config_error()?;
    let interval = Duration::from_secs(config.interval);
    api.set_format(config.format.with_default("$total.eng(1)")?);
    api.set_icon("github")?;

    // Http client
    let request = REQWEST_CLIENT
        .get("https://api.github.com/notifications")
        .header("Authorization", &format!("token {}", config.token))
        .header(header::USER_AGENT, "swaystatus");

    loop {
        let total = api.recoverable(|| get_total(&request), "X").await?;

        if total != 0 || !config.hide {
            api.set_values(map! {"total" => Value::number(total)});
            api.show();
        } else {
            api.hide();
        }
        api.flush().await?;

        tokio::time::sleep(interval).await;
    }
}

async fn get_total(request: &reqwest::RequestBuilder) -> Result<usize> {
    // Send request
    let result = request
        .try_clone()
        .error("Failed to clone request")?
        .send()
        .await
        .error("Failed to send request")?
        .text()
        .await
        .error("Failed to get response")?;
    // Convert to JSON
    let notifications: Vec<serde_json::Value> =
        serde_json::from_str(&result).error("Failed to deserialize JSON")?;
    // The total number of notifications is just the length of a list
    Ok(notifications.len())
}
