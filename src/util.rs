use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::prelude::v1::String;

use serde::de::DeserializeOwned;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::errors::*;

/// Tries to find a file in standard locations:
/// - Fist try to find a file by full path
/// - Then try XDG_CONFIG_HOME
/// - Then try `~/.local/share/`
/// - Then try `/usr/share/`
///
/// Automaticaly append an extension if not presented.
pub fn find_file(file: &str, subdir: Option<&str>, extension: Option<&str>) -> Option<PathBuf> {
    // Set (or update) the extension
    let mut file = PathBuf::from(file);
    if let Some(extension) = extension {
        file.set_extension(extension);
    }

    // Try full path
    if file.exists() {
        return Some(file);
    }

    // Try XDG_CONFIG_HOME
    if let Some(xdg_config) = xdg_config_home() {
        let mut xdg_config = xdg_config.join("swaystatus");
        if let Some(subdir) = subdir {
            xdg_config = xdg_config.join(subdir);
        }
        xdg_config = xdg_config.join(&file);
        if xdg_config.exists() {
            return Some(xdg_config);
        }
    }

    // Try `~/.local/share/`
    if let Ok(home) = env::var("HOME") {
        let mut local_share_path = PathBuf::from(home).join(".local/share/swaystatus");
        if let Some(subdir) = subdir {
            local_share_path = local_share_path.join(subdir);
        }
        local_share_path = local_share_path.join(&file);
        if local_share_path.exists() {
            return Some(local_share_path);
        }
    }

    // Try `/usr/share/`
    let mut usr_share_path = PathBuf::from("/usr/share/swaystatus");
    if let Some(subdir) = subdir {
        usr_share_path = usr_share_path.join(subdir);
    }
    usr_share_path = usr_share_path.join(&file);
    if usr_share_path.exists() {
        return Some(usr_share_path);
    }

    None
}

pub fn escape_pango_text(text: String) -> String {
    text.chars()
        .map(|x| match x {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '\'' => "&#39;".to_string(),
            _ => x.to_string(),
        })
        .collect()
}

pub fn xdg_config_home() -> Option<PathBuf> {
    // If XDG_CONFIG_HOME is not set, fall back to use HOME/.config
    env::var("XDG_CONFIG_HOME")
        .ok()
        .or_else(|| {
            env::var("HOME")
                .ok()
                .map(|home| format!("{}/.config", home))
        })
        .map(PathBuf::from)
}

pub fn deserialize_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let file = path.to_str().unwrap();
    let mut contents = String::new();
    let mut file =
        BufReader::new(File::open(file).error(format!("failed to open file: {}", file))?);
    file.read_to_string(&mut contents)
        .error("failed to read file")?;
    toml::from_str(&contents).config_error()
}

pub async fn read_file(path: &Path) -> StdResult<String, std::io::Error> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    Ok(content.trim_end().to_string())
}

#[allow(dead_code)]
pub async fn has_command(command: &str) -> Result<bool> {
    Command::new("sh")
        .args(&[
            "-c",
            format!("command -v {} >/dev/null 2>&1", command).as_ref(),
        ])
        .status()
        .await
        .error(format!("failed to start command to check for {}", command))
        .map(|status| status.success())
}

macro_rules! map {
    ($($key:expr => $value:expr),+ $(,)*) => {{
        let mut m = ::std::collections::HashMap::new();
        $(m.insert($key, $value);)+
        m
    }};
}

macro_rules! map_to_owned {
    ($($key:expr => $value:expr),+ $(,)*) => {{
        let mut m = ::std::collections::HashMap::new();
        $(m.insert($key.to_owned(), $value.to_owned());)+
        m
    }};
}

pub fn format_vec_to_bar_graph(content: &[f64]) -> String {
    // (x * one eighth block) https://en.wikipedia.org/wiki/Block_Elements
    static BARS: [char; 8] = [
        '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
        '\u{2588}',
    ];

    // Find min and max
    let mut min = std::f64::INFINITY;
    let mut max = -std::f64::INFINITY;
    for v in content {
        if *v < min {
            min = *v;
        }
        if *v > max {
            max = *v;
        }
    }

    let range = max - min;
    content
        .iter()
        .map(|x| BARS[((x - min) / range * 7.).clamp(0., 7.) as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::util::has_command;

    #[test]
    // we assume sh is always available
    fn test_has_command_ok() {
        let has_command = tokio_test::block_on(has_command("sh"));
        assert!(has_command.is_ok());
        let has_command = has_command.unwrap();
        assert!(has_command);
    }

    #[test]
    // we assume thequickbrownfoxjumpsoverthelazydog command does not exist
    fn test_has_command_err() {
        let has_command = tokio_test::block_on(has_command("thequickbrownfoxjumpsoverthelazydog"));
        assert!(has_command.is_ok());
        let has_command = has_command.unwrap();
        assert!(!has_command)
    }
}
