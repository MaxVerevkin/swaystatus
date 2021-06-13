use std::time::Duration;

use serde_derive::Deserialize;

use tokio::io::{AsyncBufReadExt, BufReader, Stdin};
use tokio::sync::mpsc::Sender;

use crate::click::MouseButton;

#[derive(Deserialize, Debug, Clone)]
struct I3BarEventInternal {
    pub name: Option<String>,
    pub instance: Option<String>,
    pub button: MouseButton,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct I3BarEvent {
    pub id: usize,
    pub instance: Option<usize>,
    pub button: MouseButton,
}

async fn get_event(input: &mut BufReader<Stdin>, invert_scrolling: bool) -> I3BarEvent {
    let mut buf = String::new();
    loop {
        input.read_line(&mut buf).await.unwrap();

        // Take only the valid JSON object betweem curly braces (cut off leading bracket, commas and whitespace)
        let slice = buf.trim_start_matches(|c| c != '{');
        let slice = slice.trim_end_matches(|c| c != '}');

        if !slice.is_empty() {
            let event: I3BarEventInternal = serde_json::from_str(slice).unwrap();
            let id = match event.name {
                Some(name) => name.parse().unwrap(),
                None => continue,
            };
            let instance = event.instance.map(|x| x.parse::<usize>().unwrap());

            use MouseButton::*;
            let button = match (event.button, invert_scrolling) {
                (WheelUp, false) | (WheelDown, true) => WheelUp,
                (WheelUp, true) | (WheelDown, false) => WheelDown,
                (other, _) => other,
            };

            return I3BarEvent {
                id,
                instance,
                button,
            };
        }
    }
}

pub async fn process_events(sender: Sender<I3BarEvent>, invert_scrolling: bool) {
    let mut stdin = BufReader::new(tokio::io::stdin());

    loop {
        // Get next event
        let mut event = get_event(&mut stdin, invert_scrolling).await;

        // Handle double left click. Max delay between two clicks is 150ms.
        // TODO: make delay configurable
        if event.button == MouseButton::Left {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(150)) => (),
                new_event = get_event(&mut stdin, invert_scrolling) => {
                    if event == new_event {
                        event.button = MouseButton::DoubleLeft;
                    } else {
                        sender.send(event).await.unwrap();
                        event = new_event;
                    }
                }
            }
        }

        sender
            .send(event)
            .await
            .expect("channel closed while sending event");
    }
}
