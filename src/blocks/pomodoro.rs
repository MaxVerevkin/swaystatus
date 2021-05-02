//! A block which runs a [pomodoro timer](https://en.wikipedia.org/wiki/Pomodoro_Technique).
//!
//! # Technique
//!
//! There are six steps in the original technique:
//! 1) Decide on the task to be done.
//! 2) Set the pomodoro timer (traditionally to 25 minutes).
//! 3) Work on the task.
//! 4) End work when the timer rings and put a checkmark on a piece of paper.
//! 5) If you have fewer than four checkmarks, take a short break (3–5 minutes) and then return to step 2.
//! 6) After four pomodoros, take a longer break (15–30 minutes), reset your checkmark count to zero, then go to step 1.
//!
//!
//! # Configuration
//!
//! Key | Values | Required | Default
//! ----|--------|----------|--------
//! `use_nag` | i3-nagbar enabled | No | `false`
//! `nag_path` | i3-nagbar binary path | No | `i3-nagbar`
//! `message` | Message when timer expires | No | `Pomodoro over! Take a break!`
//! `break_message` | Message when break is over | No | `Break over! Time to work!`
//!
//! # Example
//!
//! ```toml
//! [[block]]
//! block = "pomodoro"
//! use_nag = true
//! nag_path = "swaynag"
//! ```
//!
//! # TODO
//!
//! - Automaticaly select between "i3-nagbar" and "swaynag".
//! - Use different icons.
//! - Use format strings.

use serde::de::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::widgets::widget::Widget;
use crate::widgets::State;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct PomodoroConfig {
    use_nag: bool,
    nag_path: String,
    message: String,
    break_message: String,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {
            use_nag: false,
            nag_path: "i3-nagbar".to_string(),
            message: "Pomodoro over! Take a break!".to_string(),
            break_message: "Break over! Time to work!".to_string(),
        }
    }
}

struct Block {
    id: usize,
    widget: Widget,
    block_config: PomodoroConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_receiver: mpsc::Receiver<BlockEvent>,
}

impl Block {
    async fn set_text(&mut self, text: String) -> Result<()> {
        self.widget.set_full_text(text);
        self.message_sender
            .send(BlockMessage {
                id: self.id,
                widgets: vec![self.widget.get_data()],
            })
            .await
            .internal_error("pomodoro", "failed to send message")?;
        Ok(())
    }

    async fn wait_for_click(&mut self, button: MouseButton) {
        loop {
            if let Some(BlockEvent::I3Bar(click)) = self.events_receiver.recv().await {
                if click.button == button {
                    break;
                }
            }
        }
    }

    async fn read_params(&mut self) -> Result<(Duration, Duration, u64)> {
        let task_len = self.read_u64(25, "Task length:").await?;
        let break_len = self.read_u64(5, "Break length:").await?;
        let pomodoros = self.read_u64(4, "Pomodoros:").await?;
        Ok((
            Duration::from_secs(task_len * 60),
            Duration::from_secs(break_len * 60),
            pomodoros,
        ))
    }

    async fn read_u64(&mut self, mut number: u64, msg: &str) -> Result<u64> {
        loop {
            self.set_text(format!("{} {}", msg, number)).await?;
            if let Some(BlockEvent::I3Bar(click)) = self.events_receiver.recv().await {
                match click.button {
                    MouseButton::Left => break,
                    MouseButton::WheelUp => number += 1,
                    MouseButton::WheelDown => number = number.saturating_sub(1),
                    _ => (),
                }
            }
        }
        Ok(number)
    }

    async fn run_pomodoro(
        &mut self,
        task_len: Duration,
        break_len: Duration,
        pomodoros: u64,
    ) -> Result<()> {
        for pomodoro in 0..pomodoros {
            // Task timer
            self.widget.set_state(State::Idle);
            let timer = Instant::now();
            loop {
                let elapsed = timer.elapsed();
                if elapsed >= task_len {
                    break;
                }
                let left = task_len - elapsed;
                let text = if pomodoro == 0 {
                    format!("{} min", (left.as_secs() + 59) / 60,)
                } else {
                    format!(
                        "{} {} min",
                        "|".repeat(pomodoro as usize),
                        (left.as_secs() + 59) / 60,
                    )
                };
                self.set_text(text).await?;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(10)) => (),
                    Some(BlockEvent::I3Bar(click)) = self.events_receiver.recv() => {
                        if click.button == MouseButton::Middle {
                            return Ok(());
                        }
                    }
                }
            }

            // Show break message
            self.widget.set_state(State::Good);
            self.set_text(self.block_config.message.clone()).await?;
            if self.block_config.use_nag {
                self.nag(&self.block_config.message).await?;
            } else {
                self.wait_for_click(MouseButton::Left).await;
            }

            // No break after the last pomodoro
            if pomodoro == pomodoros - 1 {
                break;
            }

            // Break timer
            let timer = Instant::now();
            loop {
                let elapsed = timer.elapsed();
                if elapsed >= break_len {
                    break;
                }
                let left = break_len - elapsed;
                self.set_text(format!("Break: {} min", (left.as_secs() + 59) / 60,))
                    .await?;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(10)) => (),
                    Some(BlockEvent::I3Bar(click)) = self.events_receiver.recv() => {
                        if click.button == MouseButton::Middle {
                            return Ok(());
                        }
                    }
                }
            }

            // Show task message
            self.widget.set_state(State::Good);
            self.set_text(self.block_config.break_message.clone())
                .await?;
            if self.block_config.use_nag {
                self.nag(&self.block_config.break_message).await?;
            } else {
                self.wait_for_click(MouseButton::Left).await;
            }
        }

        Ok(())
    }

    async fn nag(&self, msg: &str) -> Result<()> {
        tokio::process::Command::new(&self.block_config.nag_path)
            .arg("-m")
            .arg(msg)
            .spawn()
            .block_error("pomodoro", "failed to run nag command")?
            .wait()
            .await
            .block_error("pomodoro", "failed to wait for nag command to run")?;
        Ok(())
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_receiver: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let block_config = PomodoroConfig::deserialize(block_config).block_config_error("pomodoro")?;
    let widget = Widget::new(id, shared_config).with_icon("pomodoro")?;
    let mut block = Block {
        id,
        widget,
        block_config,
        message_sender,
        events_receiver,
    };

    loop {
        // Send collaped block
        block.widget.set_state(State::Idle);
        block.set_text(String::new()).await?;

        // Wait for left click
        block.wait_for_click(MouseButton::Left).await;

        // Read params
        let (task_len, break_len, pomodoros) = block.read_params().await?;

        // Run!
        block.run_pomodoro(task_len, break_len, pomodoros).await?;
    }
}
