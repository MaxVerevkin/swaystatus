use std::time::{Duration, Instant};

use super::prelude::*;

use crate::netlink::{default_interface, NetDevice};
use crate::util;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct NetConfig {
    /// Format string for `Net` block.
    pub format: FormatTemplate,

    /// Format string that is applied afted a click
    pub format_alt: Option<FormatTemplate>,

    /// Format string for `Net` block.
    pub device: Option<String>,

    /// The delay in seconds between updates.
    pub interval: u64,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            format: Default::default(),
            format_alt: None,
            device: None,
            interval: 2,
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, events: EventsRxGetter) -> BlockHandle {
    let mut events = events();
    tokio::spawn(async move {
        let block_config = NetConfig::deserialize(block_config).block_config_error("net")?;
        let mut format = block_config
            .format
            .or_default("{speed_down;K}{speed_up;k}")?;
        let mut format_alt = block_config.format_alt;

        let mut text = api.new_widget();
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
            let device = NetDevice::from_interface(
                block_config
                    .device
                    .clone()
                    .or_else(default_interface)
                    .unwrap_or_else(|| "lo".to_string()),
            )
            .await;

            // Calculate speed
            match (stats, device.read_stats().await) {
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
            let wifi = device.wifi_info()?;

            text.set_icon(device.icon)?;
            text.set_text(format.render(&map! {
                "ssid" => Value::from_string(wifi.0.unwrap_or_else(|| "N/A".to_string())),
                "signal_strength" => Value::from_integer(wifi.2.unwrap_or_default()).percents(),
                "frequency" => Value::from_float(wifi.1.unwrap_or_default()).hertz(),
                "speed_down" => Value::from_float(speed_down).bytes().icon(api.get_icon("net_down")?),
                "speed_up" => Value::from_float(speed_up).bytes().icon(api.get_icon("net_up")?),
                "graph_down" => Value::from_string(util::format_vec_to_bar_graph(&rx_hist)),
                "graph_up" => Value::from_string(util::format_vec_to_bar_graph(&tx_hist)),
                "device" => Value::from_string(device.interface),
            })?);

            api.send_widgets(vec![text.get_data()]).await?;

            tokio::select! {
                _ = tokio::time::sleep(interval) =>(),
                Some(BlockEvent::I3Bar(click)) = events.recv() => {
                    if click.button == MouseButton::Left {
                        if let Some(ref mut format_alt) = format_alt {
                            std::mem::swap(format_alt, &mut format);
                        }
                    }
                }
            }
        }
    })
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
