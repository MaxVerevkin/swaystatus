use crate::errors::*;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::prelude::v1::String;
use std::process::Command;

use tokio::io::AsyncReadExt;

pub const USR_SHARE_PATH: &str = "/usr/share/swaystatus";

/// Tries to find a file in standard locations:
/// - Fist try to find a file by full path
/// - Then try XDG_CONFIG_HOME
/// - Then try `~/.local/share/`
/// - Then try `/usr/share/`
///
/// Automaticaly append an extension if not presented.
pub fn find_file(file: &str, subdir: Option<&str>, extension: Option<&str>) -> Option<PathBuf> {
    // Append an extension if `file` does not have it
    let mut file = String::from(file);
    if let Some(extension) = extension {
        if !file.ends_with(extension) {
            file.push_str(extension);
        }
    }

    // Try full path
    let full_path = PathBuf::from(&file);
    if full_path.exists() {
        return Some(full_path);
    }

    // Try XDG_CONFIG_HOME
    let mut xdg_config_path = xdg_config_home().join("swaystatus");
    if let Some(subdir) = subdir {
        xdg_config_path = xdg_config_path.join(subdir);
    }
    xdg_config_path = xdg_config_path.join(&file);
    if xdg_config_path.exists() {
        return Some(xdg_config_path);
    }

    // Try `~/.local/share/`
    if let Ok(home) = std::env::var("HOME") {
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
    let mut usr_share_path = PathBuf::from(USR_SHARE_PATH);
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

pub fn xdg_config_home() -> PathBuf {
    // In the unlikely event that $HOME is not set, it doesn't really matter
    // what we fall back on, so use /.config.
    PathBuf::from(std::env::var("XDG_CONFIG_HOME").unwrap_or(format!(
        "{}/.config",
        std::env::var("HOME").unwrap_or_default()
    )))
}

pub fn deserialize_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let file = path.to_str().unwrap();
    let mut contents = String::new();
    let mut file = BufReader::new(
        File::open(file).internal_error("util", &format!("failed to open file: {}", file))?,
    );
    file.read_to_string(&mut contents)
        .internal_error("util", "failed to read file")?;
    toml::from_str(&contents).config_error()
}

pub async fn read_file(path: &Path) -> std::result::Result<String, std::io::Error> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    Ok(content.trim_end().to_string())
}

#[allow(dead_code)]
pub fn has_command(block_name: &str, command: &str) -> Result<bool> {
    let exit_status = Command::new("sh")
        .args(&[
            "-c",
            format!("command -v {} >/dev/null 2>&1", command).as_ref(),
        ])
        .status()
        .block_error(
            block_name,
            format!("failed to start command to check for {}", command).as_ref(),
        )?;
    Ok(exit_status.success())
}

macro_rules! map {
    ($($key:expr => $value:expr),+ $(,)*) => {{
        let mut m = ::std::collections::HashMap::new();
        $(m.insert($key, $value);)+
        m
    }};
}

macro_rules! map_to_owned {
    { $($key:expr => $value:expr),+ } => {
        {
            let mut m = ::std::collections::HashMap::new();
            $(
                m.insert($key.to_owned(), $value.to_owned());
            )+
            m
        }
     };
}

pub fn color_from_rgba(
    color: &str,
) -> ::std::result::Result<(u8, u8, u8, u8), Box<dyn std::error::Error>> {
    Ok((
        u8::from_str_radix(&color.get(1..3).ok_or("invalid rgba color")?, 16)?,
        u8::from_str_radix(&color.get(3..5).ok_or("invalid rgba color")?, 16)?,
        u8::from_str_radix(&color.get(5..7).ok_or("invalid rgba color")?, 16)?,
        u8::from_str_radix(&color.get(7..9).unwrap_or("FF"), 16)?,
    ))
}

pub fn color_to_rgba(color: (u8, u8, u8, u8)) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color.0, color.1, color.2, color.3
    )
}

// TODO: Allow for other non-additive tints
pub fn add_colors(
    a: Option<&str>,
    b: Option<&str>,
) -> ::std::result::Result<Option<String>, Box<dyn std::error::Error>> {
    match (a, b) {
        (None, _) => Ok(None),
        (Some(a), None) => Ok(Some(a.to_string())),
        (Some(a), Some(b)) => {
            let (r_a, g_a, b_a, a_a) = color_from_rgba(a)?;
            let (r_b, g_b, b_b, a_b) = color_from_rgba(b)?;

            Ok(Some(color_to_rgba((
                r_a.saturating_add(r_b),
                g_a.saturating_add(g_b),
                b_a.saturating_add(b_b),
                a_a.saturating_add(a_b),
            ))))
        }
    }
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
    use crate::util::{color_from_rgba, has_command};

    #[test]
    // we assume sh is always available
    fn test_has_command_ok() {
        let has_command = has_command("none", "sh");
        assert!(has_command.is_ok());
        let has_command = has_command.unwrap();
        assert!(has_command);
    }

    #[test]
    // we assume thequickbrownfoxjumpsoverthelazydog command does not exist
    fn test_has_command_err() {
        let has_command = has_command("none", "thequickbrownfoxjumpsoverthelazydog");
        assert!(has_command.is_ok());
        let has_command = has_command.unwrap();
        assert!(!has_command)
    }
    #[test]
    fn test_color_from_rgba() {
        let valid_rgb = "#AABBCC"; //rgb
        let rgba = color_from_rgba(valid_rgb);
        assert!(rgba.is_ok());
        assert_eq!(rgba.unwrap(), (0xAA, 0xBB, 0xCC, 0xFF));
        let valid_rgba = "#AABBCC00"; // rgba
        let rgba = color_from_rgba(valid_rgba);
        assert!(rgba.is_ok());
        assert_eq!(rgba.unwrap(), (0xAA, 0xBB, 0xCC, 0x00));
    }

    #[test]
    fn test_color_from_rgba_invalid() {
        let invalid = "invalid";
        let rgba = color_from_rgba(invalid);
        assert!(rgba.is_err());
        let invalid = "AA"; // too short
        let rgba = color_from_rgba(invalid);
        assert!(rgba.is_err());
        let invalid = "AABBCC"; // invalid rgba (missing #)
        let rgba = color_from_rgba(invalid);
        assert!(rgba.is_err());
    }
}
