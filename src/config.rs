use std::collections::HashMap;
use std::rc::Rc;

use serde::de::{Deserialize, Deserializer};
use serde_derive::Deserialize;
use toml::value;

use crate::blocks::BlockType;
use crate::errors;
use crate::icons::Icons;
use crate::themes::Theme;

#[derive(Debug)]
pub struct SharedConfig {
    pub theme: Rc<Theme>,
    icons: Rc<Icons>,
    icons_format: String,
}

impl SharedConfig {
    pub fn new(config: &Config) -> Self {
        Self {
            theme: Rc::new(config.theme.clone()),
            icons: Rc::new(config.icons.clone()),
            icons_format: config.icons_format.clone(),
        }
    }

    pub fn icons_format_override(&mut self, icons_format: String) {
        self.icons_format = icons_format;
    }

    pub fn theme_override(&mut self, overrides: &HashMap<String, String>) -> errors::Result<()> {
        let mut theme = self.theme.as_ref().clone();
        for entry in overrides {
            match entry.0.as_str() {
                "idle_fg" => theme.idle_fg = Some(entry.1.to_string()),
                "idle_bg" => theme.idle_bg = Some(entry.1.to_string()),
                "info_fg" => theme.info_fg = Some(entry.1.to_string()),
                "info_bg" => theme.info_bg = Some(entry.1.to_string()),
                "good_fg" => theme.good_fg = Some(entry.1.to_string()),
                "good_bg" => theme.good_bg = Some(entry.1.to_string()),
                "warning_fg" => theme.warning_fg = Some(entry.1.to_string()),
                "warning_bg" => theme.warning_bg = Some(entry.1.to_string()),
                "critical_fg" => theme.critical_fg = Some(entry.1.to_string()),
                "critical_bg" => theme.critical_bg = Some(entry.1.to_string()),
                x => {
                    return Err(errors::Error::InternalError {
                        context: "theme overrider".to_string(),
                        message: format!("theme element \"{}\" cannot be overriden", x),
                        cause: None,
                        cause_dbg: None,
                    })
                }
            }
        }
        self.theme = Rc::new(theme);
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
            theme: Rc::new(Theme::default()),
            icons: Rc::new(Icons::default()),
            icons_format: " {icon} ".to_string(),
        }
    }
}

impl Clone for SharedConfig {
    fn clone(&self) -> Self {
        Self {
            theme: Rc::clone(&self.theme),
            icons: Rc::clone(&self.icons),
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

    /// Direction of scrolling, "natural" or "reverse".
    ///
    /// Configuring natural scrolling on input devices changes the way i3status-rust
    /// processes mouse wheel events: pushing the wheen away now is interpreted as downward
    /// motion which is undesired for sliders. Use "natural" to invert this.
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

impl Default for Config {
    fn default() -> Self {
        Config {
            icons: Icons::default(),
            theme: Theme::default(),
            icons_format: Config::default_icons_format(),
            blocks: Vec::new(),
            invert_scrolling: false,
        }
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
