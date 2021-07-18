use std::collections::HashMap;
use std::sync::Arc;

use serde::de::{Deserialize, Deserializer};
use serde_derive::Deserialize;
use toml::value;

use crate::blocks::BlockType;
use crate::errors;
use crate::icons::Icons;
use crate::themes::Theme;

// TODO use `Cow` insted of `Arc`?
#[derive(Debug)]
pub struct SharedConfig {
    pub theme: Arc<Theme>,
    icons: Arc<Icons>,
    icons_format: String,
}

impl SharedConfig {
    pub fn new(config: &Config) -> Self {
        Self {
            theme: Arc::new(config.theme.clone()),
            icons: Arc::new(config.icons.clone()),
            icons_format: config.icons_format.clone(),
        }
    }

    pub fn icons_format_override(&mut self, icons_format: String) {
        self.icons_format = icons_format;
    }

    pub fn theme_override(&mut self, overrides: &HashMap<String, String>) -> errors::Result<()> {
        let mut theme = self.theme.as_ref().clone();
        theme.apply_overrides(overrides)?;
        self.theme = Arc::new(theme);
        Ok(())
    }

    pub fn get_icon(&self, icon: &str) -> crate::errors::Result<String> {
        use crate::errors::OptionExt;
        Ok(self.icons_format.clone().replace(
            "{icon}",
            self.icons
                .0
                .get(icon)
                .internal_error("get_icon()", &format!("icon '{}' not found: please check your icon file or open a new issue on GitHub if you use a precompiled icons.", icon))?,
        ))
    }
}

impl Default for SharedConfig {
    fn default() -> Self {
        Self {
            theme: Arc::new(Theme::default()),
            icons: Arc::new(Icons::default()),
            icons_format: " {icon} ".to_string(),
        }
    }
}

impl Clone for SharedConfig {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            icons: Arc::clone(&self.icons),
            icons_format: self.icons_format.clone(),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default)]
    pub icons: Icons,

    #[serde(default)]
    pub theme: Theme,

    #[serde(default = "Config::default_icons_format")]
    pub icons_format: String,

    /// Set to `true` to invert mouse wheel direction
    #[serde(default)]
    pub invert_scrolling: bool,

    #[serde(rename = "block", deserialize_with = "deserialize_blocks")]
    pub blocks: Vec<(BlockType, value::Value)>,
}

impl Config {
    fn default_icons_format() -> String {
        " {icon} ".to_string()
    }
}

fn deserialize_blocks<'de, D>(deserializer: D) -> Result<Vec<(BlockType, value::Value)>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut blocks: Vec<(BlockType, value::Value)> = Vec::new();
    let raw_blocks: Vec<value::Table> = Deserialize::deserialize(deserializer)?;
    for mut entry in raw_blocks {
        if let Some(name) = entry.remove("block") {
            let name_str = name.to_string();
            let block = BlockType::deserialize(name)
                .map_err(|_| serde::de::Error::custom(format!("unknown block {}", name_str)))?;
            blocks.push((block, value::Value::Table(entry)));
        }
    }

    Ok(blocks)
}
