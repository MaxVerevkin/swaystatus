//! The current time.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | Format string. See [chrono docs](https://docs.rs/chrono/0.3.0/chrono/format/strftime/index.html#specifiers) for all options. | No | `"%a %d/%m %R"`
//! `format_short` | Same as `format` but used when there is no enough space on the bar | No | None
//! `interval` | Update interval in seconds | No | 10
//! `timezone` | A timezone specifier (e.g. "Europe/Lisbon") | No | Local timezone
//! `locale` | Locale to apply when formatting the time | No | System locale
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "time"
//! interval = 60
//! locale = "fr_BE"
//! [block.format]
//! full = "%d/%m %R"
//! short = "%R"
//! ```
//!
//! # Icons Used
//! - `time`

use std::collections::HashMap;
use std::time::Duration;

use chrono::offset::{Local, Utc};
use chrono::Locale;
use chrono_tz::Tz;

use super::prelude::*;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct TimeConfig {
    format: FormatConfig,
    interval: u64,
    timezone: Option<Tz>,
    locale: Option<String>,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            format: Default::default(),
            interval: 10,
            timezone: None,
            locale: None,
        }
    }
}

pub async fn run(config: toml::Value, mut api: CommonApi) -> Result<()> {
    let config = TimeConfig::deserialize(config).config_error()?;
    api.set_icon("time")?;

    let mut interval = tokio::time::interval(Duration::from_secs(config.interval));

    // `FormatTemplate` doesn't do much stuff here - we just want to get the original "full" and
    // "short" formats, so we "render" it without providing any placeholders.
    let (format, format_short) = config
        .format
        .init("%a %d/%m %R", &api)?
        .render(&HashMap::new())?;
    let format = format.as_str();
    let format_short = format_short.as_deref();

    let timezone = config.timezone;
    let locale = match config.locale.as_deref() {
        Some(locale) => Some(locale.try_into().ok().error("invalid locale")?),
        None => None,
    };

    loop {
        let full_time = get_time(format, timezone, locale);
        let short_time = format_short.map(|f| get_time(f, timezone, locale));

        api.set_text((full_time, short_time));
        api.flush().await?;

        interval.tick().await;
    }
}

fn get_time(format: &str, timezone: Option<Tz>, locale: Option<Locale>) -> String {
    match locale {
        Some(locale) => match timezone {
            Some(tz) => Utc::now()
                .with_timezone(&tz)
                .format_localized(format, locale)
                .to_string()
                .into(),
            None => Local::now()
                .format_localized(format, locale)
                .to_string()
                .into(),
        },
        None => match timezone {
            Some(tz) => Utc::now()
                .with_timezone(&tz)
                .format(format)
                .to_string()
                .into(),
            None => Local::now().format(format).to_string().into(),
        },
    }
}
