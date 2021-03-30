use std::str::FromStr;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use serde::de::Deserialize;

use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::protocol::i3bar_event::MouseButton;
use crate::widgets::text::TextWidget;
use crate::widgets::I3BarWidget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct MemoryConfig {
    /// Format string for Memory view. All format values are described below.
    pub format_mem: String,

    /// Format string for Swap view.
    pub format_swap: String,

    /// Default view displayed on startup. Options are <br/> memory, swap
    pub display_type: Memtype,

    /// Whether the format string should be prepended with Icons. Options are <br/> true, false
    pub icons: bool,

    /// Whether the view should switch between memory and swap on click. Options are <br/> true, false
    pub clickable: bool,

    /// The delay in seconds between an update. If `clickable`, an update is triggered on click. Integer values only.
    pub interval: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            format_mem: "{mem_free;M}/{mem_total;M}({mem_total_used_percents})".to_string(),
            format_swap: "{swap_free;M}/{swap_total;M}({swap_used_percents})".to_string(),
            display_type: Memtype::Memory,
            icons: true,
            clickable: true,
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
    let block_config =
        MemoryConfig::deserialize(block_config).configuration_error("failed to parse config")?;

    let format = (
        FormatTemplate::from_string(&block_config.format_mem)?,
        FormatTemplate::from_string(&block_config.format_swap)?,
    );

    let mut text_mem = TextWidget::new(id, 0, shared_config.clone()).with_icon("memory_mem")?;
    let mut text_swap = TextWidget::new(id, 0, shared_config.clone()).with_icon("memory_swap")?;

    let mut memtype = block_config.display_type;
    let clickable = block_config.clickable;

    let interval = Duration::from_secs(block_config.interval);

    loop {
        let mem_state = Memstate::new().await?;
        let mem_total = mem_state.mem_total as f64 * 1024.;
        let mem_free = mem_state.mem_free as f64 * 1024.;
        let swap_total = mem_state.swap_total as f64 * 1024.;
        let swap_free = mem_state.swap_free as f64 * 1024.;
        let swap_used = swap_total - swap_free;
        let mem_total_used = mem_total - mem_free;
        let buffers = mem_state.buffers as f64 * 1024.;
        let cached =
            // TODO revisit
            (mem_state.cached + mem_state.s_reclaimable - mem_state.shmem) as f64 * 1024.;
        let mem_used = mem_total_used - (buffers + cached);
        let mem_avail = mem_total - mem_used;

        let values = map!(
            "mem_total" => Value::from_float(mem_total).bytes(),
            "mem_free" => Value::from_float(mem_free).bytes(),
            "mem_free_percents" => Value::from_float(mem_free / mem_total * 100.).percents(),
            "mem_total_used" => Value::from_float(mem_total_used).bytes(),
            "mem_total_used_percents" => Value::from_float(mem_total_used / mem_total * 100.).percents(),
            "mem_used" => Value::from_float(mem_used).bytes(),
            "mem_used_percents" => Value::from_float(mem_used / mem_total * 100.).percents(),
            "mem_avail" => Value::from_float(mem_avail).bytes(),
            "mem_avail_percents" => Value::from_float(mem_avail / mem_total * 100.).percents(),
            "swap_total" => Value::from_float(swap_total).bytes(),
            "swap_free" => Value::from_float(swap_free).bytes(),
            "swap_free_percents" => Value::from_float(swap_free / swap_total * 100.).percents(),
            "swap_used" => Value::from_float(swap_used).bytes(),
            "swap_used_percents" => Value::from_float(swap_used / swap_total * 100.).percents(),
            "buffers" => Value::from_float(buffers).bytes(),
            "buffers_percent" => Value::from_float(buffers / mem_total * 100.).percents(),
            "cached" => Value::from_float(cached).bytes(),
            "cached_percent" => Value::from_float(cached / mem_total * 100.).percents(),
        );

        text_mem.set_text(format.0.render(&values)?);
        text_swap.set_text(format.1.render(&values)?);

        let widgets = match memtype {
            Memtype::Memory => vec![text_mem.get_data()],
            Memtype::Swap => vec![text_swap.get_data()],
        };

        message_sender
            .send(BlockMessage { id, widgets })
            .await
            .internal_error("memory", "failed to send message")?;

        tokio::select! {
            _ = tokio::time::sleep(interval) =>(),
            event = events_reciever.recv() => {
                if let BlockEvent::I3Bar(click) = event.unwrap() {
                    if click.button == MouseButton::Left && clickable {
                        memtype = match memtype {
                            Memtype::Swap => Memtype::Memory,
                            Memtype::Memory => Memtype::Swap,
                        };
                    }
                }
            }
        }
    }
}

#[derive(serde_derive::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Memtype {
    Swap,
    Memory,
}

#[derive(Clone, Copy, Debug)]
// Not following naming convention, because of naming in /proc/meminfo
struct Memstate {
    mem_total: u64,
    mem_free: u64,
    buffers: u64,
    cached: u64,
    s_reclaimable: u64,
    shmem: u64,
    swap_total: u64,
    swap_free: u64,
}

impl Memstate {
    async fn new() -> Result<Self> {
        let mut file = BufReader::new(
            File::open("/proc/meminfo")
                .await
                .block_error("memory", "/proc/meminfo does not exist")?,
        );

        let mut mem_state = Memstate {
            mem_total: 0,
            mem_free: 0,
            buffers: 0,
            cached: 0,
            s_reclaimable: 0,
            shmem: 0,
            swap_total: 0,
            swap_free: 0,
        };

        let mut line = String::new();
        while file
            .read_line(&mut line)
            .await
            .block_error("memory", "failed to read /proc/meminfo")?
            != 0
        {
            let mut words = line.trim().split_whitespace();

            let name = match words.next() {
                Some(name) => name,
                None => {
                    line.clear();
                    continue;
                }
            };
            let val = words
                .next()
                .map(|x| u64::from_str(x).ok())
                .flatten()
                .block_error("memory", "failed to parse /proc/meminfo")?;

            match name {
                "MemTotal:" => mem_state.mem_total = val,
                "MemFree:" => mem_state.mem_free = val,
                "Buffers:" => mem_state.buffers = val,
                "Cached:" => mem_state.cached = val,
                "SReclaimable:" => mem_state.s_reclaimable = val,
                "Shmem:" => mem_state.shmem = val,
                "SwapTotal:" => mem_state.swap_total = val,
                "SwapFree:" => mem_state.swap_free = val,
                _ => (),
            }

            line.clear();
        }
        Ok(mem_state)
    }
}
