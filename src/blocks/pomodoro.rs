use serde::de::Deserialize;
use std::convert::TryInto;
use std::time::Duration;
use tokio::sync::mpsc;

use chrono::offset::{Local, Utc};
use chrono::Locale;
use chrono_tz::Tz;

use super::{BlockEvent, BlockMessage};
use crate::click::MouseButton;
use crate::config::SharedConfig;
use crate::errors::*;
use crate::widgets::widget::Widget;

#[derive(serde_derive::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct PomodoroConfig {
    // FIXME
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {}
    }
}

pub async fn run(
    id: usize,
    block_config: toml::Value,
    shared_config: SharedConfig,
    message_sender: mpsc::Sender<BlockMessage>,
    mut events_receiver: mpsc::Receiver<BlockEvent>,
) -> Result<()> {
    let _block_config = PomodoroConfig::deserialize(block_config).block_config_error("pomodoro")?;
    let mut text = Widget::new(id, shared_config).with_icon("pomodoro")?;

    // Send collaped block
    message_sender
        .send(BlockMessage {
            id,
            widgets: vec![text.get_data()],
        })
        .await
        .internal_error("pomodoro", "failed to send message")?;

    // Wait for left click
    loop {
        if let Some(BlockEvent::I3Bar(click)) = events_receiver.recv().await {
            if click.button == MouseButton::Left {
                break;
            }
        }
    }

    // Read task length
    let task_len = read_usize(
        id,
        &mut text,
        &message_sender,
        &mut events_receiver,
        25,
        "Task length:",
    )
    .await?;

    // Read break length
    let break_len = read_usize(
        id,
        &mut text,
        &message_sender,
        &mut events_receiver,
        5,
        "Break length:",
    )
    .await?;

    update(
        id,
        &mut text,
        &&message_sender,
        format!("tark_len={} and break_len={}", task_len, break_len),
    )
    .await?;

    Ok(())
}

async fn read_usize(
    id: usize,
    widget: &mut Widget,
    sender: &mpsc::Sender<BlockMessage>,
    receiver: &mut mpsc::Receiver<BlockEvent>,
    mut number: usize,
    msg: &str,
) -> Result<usize> {
    loop {
        update(id, widget, sender, format!("{} {}", msg, number)).await?;
        if let Some(BlockEvent::I3Bar(click)) = receiver.recv().await {
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

async fn update(
    id: usize,
    widget: &mut Widget,
    sender: &mpsc::Sender<BlockMessage>,
    text: String,
) -> Result<()> {
    widget.set_text(text);
    sender
        .send(BlockMessage {
            id,
            widgets: vec![widget.get_data()],
        })
        .await
        .internal_error("pomodoro", "failed to send message")?;
    Ok(())
}
