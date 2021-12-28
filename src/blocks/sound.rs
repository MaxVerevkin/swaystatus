use std::cmp::{max, min};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStdout, Command};

use super::prelude::*;

const FILTER: &[char] = &['[', ']', '%'];

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
struct SoundConfig {
    driver: SoundDriver,
    name: Option<String>,
    device: Option<String>,
    device_kind: DeviceKind,
    natural_mapping: bool,
    step_width: u32,
    format: FormatConfig,
    show_volume_when_muted: bool,
    mappings: Option<HashMap<String, String>>,
    max_vol: Option<u32>,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum SoundDriver {
    Auto,
    Alsa,
    PulseAudio,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum DeviceKind {
    Sink,
    Source,
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            driver: SoundDriver::Auto,
            name: None,
            device: None,
            device_kind: DeviceKind::Sink,
            natural_mapping: false,
            step_width: 5,
            format: FormatConfig::default(),
            show_volume_when_muted: false,
            mappings: None,
            max_vol: None,
        }
    }
}

pub async fn run(config: toml::Value, mut api: CommonApi) -> Result<()> {
    let mut events = api.get_events().await?;
    let config = SoundConfig::deserialize(config).config_error()?;
    api.set_format(config.format.with_default("$volume.eng(2)|")?);

    let device_kind = config.device_kind;
    let icon = |volume: u32, headphones: bool| -> String {
        if headphones && config.device_kind == DeviceKind::Sink && headphones {
            "headphones".into()
        } else {
            let mut icon = String::new();
            let _ = write!(
                icon,
                "{}_{}",
                match device_kind {
                    DeviceKind::Source => "microphone",
                    DeviceKind::Sink => "volume",
                },
                match volume {
                    0 => "muted",
                    1..=20 => "empty",
                    21..=70 => "half",
                    _ => "full",
                }
            );
            icon
        }
    };

    let step_width = config.step_width.clamp(0, 50) as i32;

    let mut device = AlsaSoundDevice::new(
        config.name.unwrap_or_else(|| "Master".into()),
        config.device.unwrap_or_else(|| "default".into()),
        config.natural_mapping,
    )?;

    loop {
        device.get_info().await?;
        let volume = device.volume();

        let mut output_name = device.output_name();
        if let Some(m) = &config.mappings {
            if let Some(mapped) = m.get(&output_name) {
                output_name = mapped.clone();
            }
        }

        let output_description = device
            .output_description()
            .unwrap_or_else(|| output_name.clone());

        // TODO: Query port names instead? See https://github.com/greshake/i3status-rust/pull/1363#issue-1069904082
        // Reference: PulseAudio port name definitions are the first item in the well_known_descriptions struct:
        // https://gitlab.freedesktop.org/pulseaudio/pulseaudio/-/blob/0ce3008605e5f644fac4bb5edbb1443110201ec1/src/modules/alsa/alsa-mixer.c#L2709-L2731
        let headphones = device
            .active_port()
            .map(|p| p.contains("headphones"))
            .unwrap_or(false);

        let mut values = map! {
            "volume" => Value::percents(volume),
            "output_name" => Value::text(output_name),
            "output_description" => Value::text(output_description),
        };

        if device.muted() {
            api.set_icon(&icon(0, headphones))?;
            api.set_state(State::Warning);
            if !config.show_volume_when_muted {
                values.remove("volume");
            }
        } else {
            api.set_icon(&icon(volume, headphones))?;
            api.set_state(State::Idle);
        }

        api.set_values(values);
        api.flush().await?;

        tokio::select! {
            val = device.wait_for_update() => val?,
            Some(BlockEvent::Click(click)) = events.recv() => {
                match click.button {
                    MouseButton::Right => {
                        device.toggle().await?;
                    }
                    MouseButton::WheelUp => {
                        device.set_volume(step_width, config.max_vol).await?;
                    }
                    MouseButton::WheelDown => {
                        device.set_volume(-step_width, config.max_vol).await?;
                    }
                    _ => ()
                }
            }
        }
    }
}

#[async_trait::async_trait]
trait SoundDevice {
    fn volume(&self) -> u32;
    fn muted(&self) -> bool;
    fn output_name(&self) -> String;
    fn output_description(&self) -> Option<String>;
    fn active_port(&self) -> Option<String>;

    async fn get_info(&mut self) -> Result<()>;
    async fn set_volume(&mut self, step: i32, max_vol: Option<u32>) -> Result<()>;
    async fn toggle(&mut self) -> Result<()>;
    async fn wait_for_update(&mut self) -> Result<()>;
}

struct AlsaSoundDevice {
    name: String,
    device: String,
    natural_mapping: bool,
    volume: u32,
    muted: bool,

    monitor: ChildStdout,
    buffer: [u8; 2048],
}

impl AlsaSoundDevice {
    fn new(name: String, device: String, natural_mapping: bool) -> Result<Self> {
        Ok(AlsaSoundDevice {
            name,
            device,
            natural_mapping,
            volume: 0,
            muted: false,

            monitor: Command::new("stdbuf")
                .args(&["-oL", "alsactl", "monitor"])
                .stdout(Stdio::piped())
                .spawn()
                .error("Failed to start alsactl monitor")?
                .stdout
                .error("Failed to pipe alsactl monitor output")?,
            buffer: [0; 2048],
        })
    }
}

#[async_trait::async_trait]
impl SoundDevice for AlsaSoundDevice {
    fn volume(&self) -> u32 {
        self.volume
    }

    fn muted(&self) -> bool {
        self.muted
    }

    fn output_name(&self) -> String {
        self.name.clone()
    }

    fn output_description(&self) -> Option<String> {
        // TODO Does Alsa has something similar like descripitons in Pulse?
        None
    }

    fn active_port(&self) -> Option<String> {
        None
    }

    async fn get_info(&mut self) -> Result<()> {
        let mut args = Vec::new();
        if self.natural_mapping {
            args.push("-M")
        };
        args.extend(&["-D", &self.device, "get", &self.name]);

        let output: String = Command::new("amixer")
            .args(&args)
            .output()
            .await
            .map(|o| std::str::from_utf8(&o.stdout).unwrap().trim().into())
            .error("could not run amixer to get sound info")?;

        let last_line = &output.lines().last().error("could not get sound info")?;

        let mut last = last_line
            .split_whitespace()
            .filter(|x| x.starts_with('[') && !x.contains("dB"))
            .map(|s| s.trim_matches(FILTER));

        self.volume = last
            .next()
            .error("could not get volume")?
            .parse::<u32>()
            .error("could not parse volume to u32")?;

        self.muted = last.next().map(|muted| muted == "off").unwrap_or(false);

        Ok(())
    }

    async fn set_volume(&mut self, step: i32, max_vol: Option<u32>) -> Result<()> {
        let new_vol = max(0, self.volume as i32 + step) as u32;
        let capped_volume = if let Some(vol_cap) = max_vol {
            min(new_vol, vol_cap)
        } else {
            new_vol
        };
        let mut args = Vec::new();
        if self.natural_mapping {
            args.push("-M")
        };
        let vol_str = format!("{}%", capped_volume);
        args.extend(&["-D", &self.device, "set", &self.name, &vol_str]);

        Command::new("amixer")
            .args(&args)
            .output()
            .await
            .error("failed to set volume")?;

        self.volume = capped_volume;

        Ok(())
    }

    async fn toggle(&mut self) -> Result<()> {
        let mut args = Vec::new();
        if self.natural_mapping {
            args.push("-M")
        };
        args.extend(&["-D", &self.device, "set", &self.name, "toggle"]);

        Command::new("amixer")
            .args(&args)
            .output()
            .await
            .error("failed to toggle mute")?;

        self.muted = !self.muted;

        Ok(())
    }

    async fn wait_for_update(&mut self) -> Result<()> {
        self.monitor
            .read(&mut self.buffer)
            .await
            .error("Failed to read stdbuf output")
            .map(|_| ())
    }
}
