//! A [pomodoro timer](https://en.wikipedia.org/wiki/Pomodoro_Technique)
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
//! `message` | Message when timer expires | No | `Pomodoro over! Take a break!`
//! `break_message` | Message when break is over | No | `Break over! Time to work!`
//! `notify_cmd` | A shell command to run as a notifier. `{msg}` will be substituted with either `message` or `break_message`. | No | `swaynag -m '{msg}'`
//! `blocking_cmd` | Is `notify_cmd` blocking? If it is, then pomodoro block will wait until the command finishes before proceeding. Otherwise, you will have to click on the block in order to proceed. | No | `true`
//!
//! # Example
//!
//! Use `notify-send` as a notifier:
//!
//! ```toml
//! [[block]]
//! block = "pomodoro"
//! notify_cmd = "notify-send '{msg}'"
//! blocking_cmd = false
//! ```
//!
//! # TODO
//!
//! - Use different icons.
//! - Use format strings.

use serde::de::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::subprocess::{spawn_shell, spawn_shell_sync};
use crate::widgets::widget::Widget;
use crate::widgets::State;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct PomodoroConfig {
    message: String,
    break_message: String,
    notify_cmd: Option<String>,
    blocking_cmd: bool,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {
            message: "Pomodoro over! Take a break!".to_string(),
            break_message: "Break over! Time to work!".to_string(),
            notify_cmd: Some("swaynag -m '{msg}'".to_string()),
            blocking_cmd: true,
        }
    }
}

struct Block<'a> {
    id: usize,
    widget: Widget<'a>,
    block_config: PomodoroConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    events_receiver: mpsc::Receiver<BlockEvent>,
}

impl<'a> Block<'a> {
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
            if let Some(cmd) = &self.block_config.notify_cmd {
                let cmd = cmd.replace("{msg}", &self.block_config.message);
                if self.block_config.blocking_cmd {
                    spawn_shell_sync(&cmd)
                        .await
                        .block_error("pomodoro", "failed to run notify_cmd")?;
                } else {
                    spawn_shell(&cmd).block_error("pomodoro", "failed to run notify_cmd")?;
                    self.wait_for_click(MouseButton::Left).await;
                }
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
            if let Some(cmd) = &self.block_config.notify_cmd {
                let cmd = cmd.replace("{msg}", &self.block_config.break_message);
                if self.block_config.blocking_cmd {
                    spawn_shell_sync(&cmd)
                        .await
                        .block_error("pomodoro", "failed to run notify_cmd")?;
                } else {
                    spawn_shell(&cmd).block_error("pomodoro", "failed to run notify_cmd")?;
                    self.wait_for_click(MouseButton::Left).await;
                }
            } else {
                self.wait_for_click(MouseButton::Left).await;
            }
        }

        Ok(())
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig<'_>,
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
