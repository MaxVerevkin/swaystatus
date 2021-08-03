use std::path::Path;
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
use crate::util::read_file;
use crate::widgets::widget::Widget;
use crate::widgets::State;

const CPU_BOOST_PATH: &str = "/sys/devices/system/cpu/cpufreq/boost";
const CPU_NO_TURBO_PATH: &str = "/sys/devices/system/cpu/intel_pstate/no_turbo";

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct CpuConfig {
    pub format: FormatTemplate,
    pub format_alt: Option<FormatTemplate>,
    pub interval: u64,
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self {
            format: Default::default(),
            format_alt: None,
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
    let block_config = CpuConfig::deserialize(block_config).block_config_error("cpu")?;
    let mut format = block_config.format.or_default("{utilization}")?;
    let mut format_alt = block_config.format_alt;

    let boost_icon_on = shared_config.get_icon("cpu_boost_on")?;
    let boost_icon_off = shared_config.get_icon("cpu_boost_off")?;

    let mut text = Widget::new(id, shared_config).with_icon("cpu")?;
    let interval = Duration::from_secs(block_config.interval);

    // Store previous /proc/stat state
    let mut cputime = read_proc_stat().await?;
    let cores = cputime.1.len();

    loop {
        let freqs = read_frequencies().await?;
        let freq_avg = freqs.iter().sum::<f64>() / (freqs.len() as f64);

        // Compute utilizations
        let new_cputime = read_proc_stat().await?;
        let utilization_avg = new_cputime.0.utilization(cputime.0);
        let mut utilizations = Vec::new();
        if new_cputime.1.len() != cores {
            return block_error("cpu", "new cputime length is incorrect");
        }
        for i in 0..cores {
            utilizations.push(new_cputime.1[i].utilization(cputime.1[i]));
        }
        cputime = new_cputime;

        // Set state
        text.set_state(match utilization_avg {
            x if x > 0.9 => State::Critical,
            x if x > 0.6 => State::Warning,
            x if x > 0.3 => State::Info,
            _ => State::Idle,
        });

        // Create barchart indicating per-core utilization
        let mut barchart = String::new();
        const BOXCHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        for utilization in &utilizations {
            barchart.push(BOXCHARS[(7.5 * utilization) as usize]);
        }

        // Read boot state on intel CPUs
        let boost = match boost_status().await {
            Some(true) => &boost_icon_on,
            Some(false) => &boost_icon_off,
            _ => "",
        };

        let mut values = map!(
            "barchart" => Value::from_string(barchart),
            "boost" => Value::from_string(boost.to_string()),
            "frequency" => Value::from_float(freq_avg).hertz(),
            "utilization" => Value::from_integer((utilization_avg * 100.) as i64).percents(),
        );
        let mut frequency_keys = vec![]; // There should be a better way to dynamically crate keys?
        for i in 0..freqs.len() {
            frequency_keys.push(format!("frequency{}", i + 1));
        }
        for (i, freq) in freqs.iter().enumerate() {
            values.insert(&frequency_keys[i], Value::from_float(*freq).hertz());
        }
        let mut utilization_keys = vec![]; // There should be a better way to dynamically crate keys?
        for i in 0..utilizations.len() {
            utilization_keys.push(format!("utilization{}", i + 1));
        }
        for (i, utilization) in utilizations.iter().enumerate() {
            values.insert(
                &utilization_keys[i],
                Value::from_integer((utilization * 100.) as i64).percents(),
            );
        }

        text.set_text(format.render(&values)?);

        message_sender
            .send(BlockMessage {
                id,
                widgets: vec![text.get_data()],
            })
            .await
            .internal_error("cpu", "failed to send message")?;

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

// Read frequencies (read in MHz, store in Hz)
async fn read_frequencies() -> Result<Vec<f64>> {
    let mut freqs = Vec::with_capacity(32);

    let file = File::open("/proc/cpuinfo")
        .await
        .block_error("cpu", "failed to read /proc/cpuinfo")?;
    let mut file = BufReader::new(file);

    let mut line = String::new();
    while file
        .read_line(&mut line)
        .await
        .block_error("cpu", "failed to read /proc/cpuinfo")?
        != 0
    {
        if line.starts_with("cpu MHz") {
            let slice = line
                .trim_end()
                .trim_start_matches(|c: char| !c.is_digit(10));
            freqs.push(
                f64::from_str(slice).block_error("cpu", "failed to parse /proc/cpuinfo")? * 1e6,
            );
        }
        line.clear();
    }

    Ok(freqs)
}

#[derive(Debug, Clone, Copy)]
struct CpuTime {
    idle: u64,
    non_idle: u64,
}

impl CpuTime {
    fn from_str(s: &str) -> Option<Self> {
        let mut s = s.trim().split_ascii_whitespace();
        let user = u64::from_str(s.next()?).ok()?;
        let nice = u64::from_str(s.next()?).ok()?;
        let system = u64::from_str(s.next()?).ok()?;
        let idle = u64::from_str(s.next()?).ok()?;
        let iowait = u64::from_str(s.next()?).ok()?;
        let irq = u64::from_str(s.next()?).ok()?;
        let softirq = u64::from_str(s.next()?).ok()?;

        Some(Self {
            idle: idle + iowait,
            non_idle: user + nice + system + irq + softirq,
        })
    }

    fn utilization(&self, old: Self) -> f64 {
        let elapsed = (self.idle + self.non_idle) as f64 - (old.idle + old.non_idle) as f64;
        ((self.non_idle - old.non_idle) as f64 / elapsed).clamp(0., 1.)
    }
}

async fn read_proc_stat() -> Result<(CpuTime, Vec<CpuTime>)> {
    let mut utilizations = Vec::with_capacity(32);
    let mut total = None;

    let file = File::open("/proc/stat")
        .await
        .block_error("cpu", "failed to read /proc/stat")?;
    let mut file = BufReader::new(file);

    let mut line = String::new();
    while file
        .read_line(&mut line)
        .await
        .block_error("cpu", "failed to read /proc/sta")?
        != 0
    {
        // Total time
        let data = line.trim_start_matches(|c: char| !c.is_ascii_whitespace());
        if line.starts_with("cpu ") {
            total = Some(CpuTime::from_str(data).block_error("cpu", "failed to parse /proc/stat")?);
        } else if line.starts_with("cpu") {
            utilizations
                .push(CpuTime::from_str(data).block_error("cpu", "failed to parse /proc/stat")?);
        }
        line.clear();
    }

    Ok((
        total.block_error("cpu", "failed to parse /proc/stat")?,
        utilizations,
    ))
}

/// Read the cpu turbo boost status from kernel sys interface
/// or intel pstate interface
async fn boost_status() -> Option<bool> {
    if let Ok(boost) = read_file(Path::new(CPU_BOOST_PATH)).await {
        Some(boost.starts_with('1'))
    } else if let Ok(no_turbo) = read_file(Path::new(CPU_NO_TURBO_PATH)).await {
        Some(no_turbo.starts_with('0'))
    } else {
        None
    }
}
