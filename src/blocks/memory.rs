//! Memory and swap usage
//!
//! This module keeps track of both Swap and Memory. By default, a click switches between them.
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `format_mem` | A string to customise the output of this block when in "Memory" view. See below for available placeholders. | No | `"{mem_free;M}/{mem_total;M}({mem_total_used_percents})"`
//! `format_swap` | A string to customise the output of this block when in "Swap" view. See below for available placeholders. | No | `"{swap_free;M}/{swap_total;M}({swap_used_percents})"`
//! `display_type` | Default view displayed on startup: "`memory`" or "`swap`" | No | `"memory"`
//! `clickable` | Whether the view should switch between memory and swap on click | No | `true`
//! `interval` | Update interval in seconds | No | `5`
//! `warning_mem` | Percentage of memory usage, where state is set to warning | No | `80.0`
//! `warning_swap` | Percentage of swap usage, where state is set to warning | No | `80.0`
//! `critical_mem` | Percentage of memory usage, where state is set to critical | No | `95.0`
//! `critical_swap` | Percentage of swap usage, where state is set to critical | No | `95.0`
//!
//! Placeholder                 | Value                                                                         | Type  | Unit
//! ----------------------------|-------------------------------------------------------------------------------|-------|-------
//! `{mem_total}`               | Memory total                                                                  | Float | Bytes
//! `{mem_free}`                | Memory free                                                                   | Float | Bytes
//! `{mem_free_percents}`       | Memory free                                                                   | Float | Percents
//! `{mem_total_used}`          | Total memory used                                                             | Float | Bytes
//! `{mem_total_used_percents}` | Total memory used                                                             | Float | Percents
//! `{mem_used}`                | Memory used, excluding cached memory and buffers; similar to htop's green bar | Float | Bytes
//! `{mem_used_percents}`       | Memory used, excluding cached memory and buffers; similar to htop's green bar | Float | Percents
//! `{mem_avail}`               | Available memory, including cached memory and buffers                         | Float | Bytes
//! `{mem_avail_percents}`      | Available memory, including cached memory and buffers                         | Float | Percents
//! `{swap_total}`              | Swap total                                                                    | Float | Bytes
//! `{swap_free}`               | Swap free                                                                     | Float | Bytes
//! `{swap_free_percents}`      | Swap free                                                                     | Float | Percents
//! `{swap_used}`               | Swap used                                                                     | Float | Bytes
//! `{swap_used_percents}`      | Swap used                                                                     | Float | Percents
//! `{buffers}`                 | Buffers, similar to htop's blue bar                                           | Float | Bytes
//! `{buffers_percent}`         | Buffers, similar to htop's blue bar                                           | Float | Percents
//! `{cached}`                  | Cached memory, similar to htop's yellow bar                                   | Float | Bytes
//! `{cached_percent}`          | Cached memory, similar to htop's yellow bar                                   | Float | Percents
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "memory"
//! format_mem = "{mem_used_percents:1}"
//! clickable = false
//! interval = 30
//! warning_mem = 70
//! critical_mem = 90
//! ```

use std::str::FromStr;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use serde::de::Deserialize;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::formatting::{value::Value, FormatTemplate};
use crate::widgets::widget::Widget;
use crate::widgets::State;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct MemoryConfig {
    format_mem: Option<FormatTemplate>,
    format_swap: Option<FormatTemplate>,
    display_type: Memtype,
    clickable: bool,
    interval: u64,
    warning_mem: f64,
    warning_swap: f64,
    critical_mem: f64,
    critical_swap: f64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            format_mem: None,
            format_swap: None,
            display_type: Memtype::Memory,
            clickable: true,
            interval: 5,
            warning_mem: 80.,
            warning_swap: 80.,
            critical_mem: 95.,
            critical_swap: 95.,
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
    let block_config = MemoryConfig::deserialize(block_config).block_config_error("memory")?;

    let format = (
        default_format!(
            block_config.format_mem,
            "{mem_free;M}/{mem_total;M}({mem_total_used_percents})"
        )?,
        default_format!(
            block_config.format_swap,
            "{swap_free;M}/{swap_total;M}({swap_used_percents})"
        )?,
    );

    let mut text_mem = Widget::new(id, shared_config.clone()).with_icon("memory_mem")?;
    let mut text_swap = Widget::new(id, shared_config).with_icon("memory_swap")?;

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

        let text = match memtype {
            Memtype::Memory => &mut text_mem,
            Memtype::Swap => &mut text_swap,
        };

        text.set_state(match memtype {
            Memtype::Memory => match mem_used / mem_total * 100. {
                x if x > block_config.critical_mem => State::Critical,
                x if x > block_config.warning_mem => State::Warning,
                _ => State::Idle,
            },
            Memtype::Swap => match swap_used / swap_total * 100. {
                x if x > block_config.critical_swap => State::Critical,
                x if x > block_config.warning_swap => State::Warning,
                _ => State::Idle,
            },
        });

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
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
