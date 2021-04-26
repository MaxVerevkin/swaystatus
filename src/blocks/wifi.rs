use serde::de::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

use nl80211::Socket;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::util::escape_pango_text;
use crate::widgets::widget::Widget;
use crate::widgets::{I3BarWidget, State};

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct NetConfig {
    /// Format string for `Net` block.
    pub format: String,

    /// Format string that is applied afted a click
    pub format_alt: Option<String>,

    /// Format string that is applied afted a click
    pub format_unavailable: Option<String>,

    /// The delay in seconds between updates.
    pub interval: u64,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            format: "{signal}".to_string(),
            format_alt: None,
            format_unavailable: None,
            interval: 5,
        }
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    mut events_reciever: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let block_config = NetConfig::deserialize(block_config).block_config_error("net")?;
    let mut format = FormatTemplate::from_string(&block_config.format)?;
    let mut format_alt = match block_config.format_alt {
        Some(ref format_alt) => Some(FormatTemplate::from_string(format_alt)?),
        None => None,
    };

    let mut text = Widget::new(id, shared_config).with_icon("net_wireless")?;
    let interval = Duration::from_secs(block_config.interval);

    loop {
        // List of interfaces
        let interfaces = Socket::connect()
            .block_error("wifi", "failed to connect to the socket")?
            .get_interfaces_info()
            .block_error("wifi", "failed to get interfaces' information")?;

        let mut output = None;
        for interface in interfaces {
            if let Ok(ap) = interface.get_station_info() {
                // SSID is `None` when not connected
                if let Some(ssid) = interface.ssid {
                    let ssid = escape_pango_text(decode_escaped_unicode(&ssid));
                    let freq = nl80211::parse_u32(
                        &interface
                            .frequency
                            .block_error("wifi", "failed to get frequency")?,
                    ) as f64
                        * 1_000_000.;
                    let signal = signal_percents(nl80211::parse_i8(
                        &ap.signal.block_error("net", "failed to get signal")?,
                    ));

                    let values = map! {
                        "ssid" => Value::from_string(ssid),
                        "signal" => Value::from_integer(signal).percents(),
                        "frequency" => Value::from_float(freq).hertz(),
                    };

                    output = Some(format.render(&values)?);
                    break;
                }
            }
        }

        match output {
            Some(output) => {
                text.set_text(output);
                text.set_state(State::Idle);
            }
            None => {
                text.set_text(block_config.format_unavailable.clone().unwrap_or_default());
                text.set_state(State::Critical);
            }
        }

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("net", "failed to send message")?;

        tokio::select! {
            _ = tokio::time::sleep(interval) => (),
            Some(BlockEvent::I3Bar(click)) = events_reciever.recv() => {
                if click.button == MouseButton::Left {
                    if let Some(ref mut format_alt) = format_alt {
                        std::mem::swap(format_alt, &mut format);
                    }
                }
            }
        }
    }
}

fn signal_percents(raw: i8) -> i64 {
    let raw = raw as f64;

    let perfect = -20.;
    let worst = -85.;
    let d = perfect - worst;

    // https://github.com/torvalds/linux/blob/9ff9b0d392ea08090cd1780fb196f36dbb586529/drivers/net/wireless/intel/ipw2x00/ipw2200.c#L4322-L4334
    let percents = 100. - (perfect - raw) * (15. * d + 62. * (perfect - raw)) / (d * d);

    (percents as i64).clamp(0, 100)
}

fn decode_escaped_unicode(raw: &[u8]) -> String {
    let mut result: Vec<u8> = Vec::new();

    let mut idx = 0;
    while idx < raw.len() {
        if raw[idx] == b'\\' {
            idx += 2; // skip "\x"
            let hex = std::str::from_utf8(&raw[idx..idx + 2]).unwrap();
            result.extend(Some(u8::from_str_radix(hex, 16).unwrap()));
            idx += 2;
        } else {
            result.extend(Some(&raw[idx]));
            idx += 1;
        }
    }

    String::from_utf8_lossy(&result).to_string()
}
