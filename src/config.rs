use std::borrow::Cow;

use serde::de::{Deserialize, Deserializer};
use serde_derive::Deserialize;
use toml::value;

use crate::blocks::BlockType;
use crate::icons::Icons;
use crate::themes::Theme;

#[derive(Debug, Clone)]
pub struct SharedConfig<'a> {
    pub theme: Cow<'a, Theme>,
    pub icons: Cow<'a, Icons>,
    pub icons_format: Cow<'a, String>,
}

impl<'a> SharedConfig<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            theme: Cow::Borrowed(&config.theme),
            icons: Cow::Borrowed(&config.icons),
            icons_format: Cow::Borrowed(&config.icons_format),
        }
    }

    pub fn into_owned(self) -> SharedConfig<'static> {
        SharedConfig {
            theme: Cow::Owned(self.theme.into_owned()),
            icons: Cow::Owned(self.icons.into_owned()),
            icons_format: Cow::Owned(self.icons_format.into_owned()),
        }
    }

    pub fn get_icon(&self, icon: &str) -> crate::errors::Result<String> {
        use crate::errors::OptionExt;
        Ok(self.icons_format.replace(
            "{icon}",
            self.icons
                .0
                .get(icon)
                .internal_error("get_icon()", &format!("icon '{}' not found: please check your icon file or open a new issue on GitHub if you use a precompiled icons.", icon))?,
        ))
    }
}

impl Default for SharedConfig<'_> {
    fn default() -> Self {
        Self {
            theme: Cow::Owned(Theme::default()),
            icons: Cow::Owned(Icons::default()),
            icons_format: Cow::Owned(" {icon} ".to_string()),
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
