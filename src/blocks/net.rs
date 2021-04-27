use serde::de::Deserialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::netlink::{default_interface, wifi_info};
use crate::util;
use crate::widgets::widget::Widget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct NetConfig {
    /// Format string for `Net` block.
    pub format: String,

    /// Format string that is applied afted a click
    pub format_alt: Option<String>,

    /// Format string for `Net` block.
    pub device: Option<String>,

    /// The delay in seconds between updates.
    pub interval: u64,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            format: "{speed_down;K}{speed_up;k}".to_string(),
            format_alt: None,
            device: None,
            interval: 2,
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

    let mut text = Widget::new(id, shared_config.clone());
    let interval = Duration::from_secs(block_config.interval);

    // Stats
    let mut stats = None;
    let mut timer = Instant::now();
    let mut tx_hist = [0f64; 8];
    let mut rx_hist = [0f64; 8];

    loop {
        let mut speed_down: f64 = 0.0;
        let mut speed_up: f64 = 0.0;

        // Get interface name
        let interface = block_config
            .device
            .clone()
            .or_else(default_interface)
            .unwrap_or_else(|| "lo".to_string());

        // Calculate speed
        match (stats, read_stats(&interface).await) {
            // No previous stats available
            (None, new_stats) => stats = new_stats,
            // No new stats available
            (Some(_), None) => stats = None,
            // All stats available
            (Some(old_stats), Some(new_stats)) => {
                let rx_bytes = new_stats.0.saturating_sub(old_stats.0);
                let tx_bytes = new_stats.1.saturating_sub(old_stats.1);
                let elapsed = timer.elapsed().as_secs_f64();
                timer = Instant::now();
                speed_down = rx_bytes as f64 / elapsed;
                speed_up = tx_bytes as f64 / elapsed;
                stats = Some(new_stats);
            }
        }
        push_to_hist(&mut rx_hist, speed_down);
        push_to_hist(&mut tx_hist, speed_up);

        // Get WiFi information
        let wifi = wifi_info(&interface)?;

        text.set_text(format.render(&map! {
            "ssid" => Value::from_string(wifi.0.unwrap_or_else(|| "N/A".to_string())),
            "signal_stength" => Value::from_integer(wifi.2.unwrap_or_default()).percents(),
            "frequency" => Value::from_float(wifi.1.unwrap_or_default()).hertz(),
            "speed_down" => Value::from_float(speed_down).bytes().icon(shared_config.get_icon("net_down")?),
            "speed_up" => Value::from_float(speed_up).bytes().icon(shared_config.get_icon("net_up")?),
            "graph_down" => Value::from_string(util::format_vec_to_bar_graph(&rx_hist)),
            "graph_up" => Value::from_string(util::format_vec_to_bar_graph(&tx_hist)),
            "device" => Value::from_string(interface),
        })?);

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("net", "failed to send message")?;

        tokio::select! {
            _ = tokio::time::sleep(interval) =>(),
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

async fn read_stats(interface: &str) -> Option<(u64, u64)> {
    let mut path = PathBuf::from("/sys/class/net");
    path = path.join(interface);
    let mut buf = String::new();

    let mut file = BufReader::new(File::open(path.join("statistics/rx_bytes")).await.ok()?);
    file.read_to_string(&mut buf).await.ok()?;
    let rx: u64 = buf.trim().parse().ok()?;
    buf.clear();

    let mut file = BufReader::new(File::open(path.join("statistics/tx_bytes")).await.ok()?);
    file.read_to_string(&mut buf).await.ok()?;
    let tx: u64 = buf.trim().parse().ok()?;

    Some((rx, tx))
}

fn push_to_hist<T>(hist: &mut [T], elem: T) {
    hist[0] = elem;
    hist.rotate_left(1);
}

#[cfg(test)]
mod tests {
    use super::push_to_hist;

    #[test]
    fn test_push_to_hist() {
        let mut hist = [0; 4];
        assert_eq!(&hist, &[0, 0, 0, 0]);
        push_to_hist(&mut hist, 1);
        assert_eq!(&hist, &[0, 0, 0, 1]);
        push_to_hist(&mut hist, 3);
        assert_eq!(&hist, &[0, 0, 1, 3]);
        push_to_hist(&mut hist, 0);
        assert_eq!(&hist, &[0, 1, 3, 0]);
        push_to_hist(&mut hist, 10);
        assert_eq!(&hist, &[1, 3, 0, 10]);
        push_to_hist(&mut hist, 2);
        assert_eq!(&hist, &[3, 0, 10, 2]);
    }
}
