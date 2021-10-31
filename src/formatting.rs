pub mod config;
pub mod formatter;
pub mod prefix;
pub mod template;
pub mod unit;
pub mod value;

use crate::errors::*;
use smartstring::alias::String;
use std::collections::HashMap;
use template::FormatTemplate;
use value::Value;

#[derive(Debug)]
pub struct Format {
    pub full: FormatTemplate,
    pub short: Option<FormatTemplate>,
}

impl Format {
    /// Whether the format string contains a given placeholder
    #[allow(dead_code)]
    pub fn contains_key(&self, key: &str) -> bool {
        self.full.contains_key(key)
            || self
                .short
                .as_ref()
                .map(|tl| tl.contains_key(key))
                .unwrap_or(false)
    }

    pub fn render(&self, vars: &HashMap<String, Value>) -> Result<(String, Option<String>)> {
        let full = self.full.render(vars).error("Failed to render full text")?;
        let short = match &self.short {
            Some(short) => Some(short.render(vars).error("Failed to render short text")?),
            None => None,
        };
        Ok((full, short))
    }
}
