//! The current time.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format` | Format string. See [chrono docs](https://docs.rs/chrono/0.3.0/chrono/format/strftime/index.html#specifiers) for all options. | No | `%a %d/%m %R`
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

use std::collections::HashMap;
use std::convert::TryInto;
use std::time::Duration;

use chrono::offset::{Local, Utc};
use chrono::Locale;
use chrono_tz::Tz;

use super::prelude::*;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct TimeConfig {
    format: FormatTemplate,
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

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = TimeConfig::deserialize(block_config).config_error()?;
        let mut interval = tokio::time::interval(Duration::from_secs(block_config.interval));
        let mut text = api.new_widget().with_icon("time")?;
        // `FormatTemplate` doesn't do much stuff here - we just want to get the original "full" and
        // "short" formats, so we "render" it without providing any placeholders.
        let (format, format_short) = block_config.format.or_default("")?.render(&HashMap::<
            &str,
            _,
        >::new(
        ))?;
        let format = format.as_str();
        let format_short = format_short.as_deref();

        let timezone = block_config.timezone;
        let locale = match block_config.locale.as_deref() {
            Some(locale) => Some(locale.try_into().ok().error("invalid locale")?),
            None => None,
        };

        loop {
            let full_time = get_time(format, timezone, locale);
            let short_time = format_short.map(|f| get_time(f, timezone, locale));
            text.set_text((full_time, short_time));
            api.send_widgets(vec![text.get_data()]).await?;
            interval.tick().await;
        }
    })
}

fn get_time(format: &str, timezone: Option<Tz>, locale: Option<Locale>) -> String {
    match locale {
        Some(locale) => match timezone {
            Some(tz) => Utc::now()
                .with_timezone(&tz)
                .format_localized(format, locale)
                .to_string(),
            None => Local::now().format_localized(format, locale).to_string(),
        },
        None => match timezone {
            Some(tz) => Utc::now().with_timezone(&tz).format(format).to_string(),
            None => Local::now().format(format).to_string(),
        },
    }
}
