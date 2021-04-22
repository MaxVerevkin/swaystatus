use std::option::Option;
use std::string::*;

use serde_derive::Deserialize;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc::Sender;

use crate::click::MouseButton;

#[derive(Deserialize, Debug, Clone)]
struct I3BarEventInternal {
    pub name: Option<String>,
    pub instance: Option<String>,
    pub button: MouseButton,
}

#[derive(Debug, Clone, Copy)]
pub struct I3BarEvent {
    pub id: Option<usize>,
    pub instance: Option<usize>,
    pub button: MouseButton,
}

pub async fn process_events(sender: Sender<I3BarEvent>, invert_scrolling: bool) {
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut input = String::new();

    loop {
        stdin.read_line(&mut input).await.unwrap();

        // Take only the valid JSON object betweem curly braces (cut off leading bracket, commas and whitespace)
        let slice = input.trim_start_matches(|c| c != '{');
        let slice = slice.trim_end_matches(|c| c != '}');

        if !slice.is_empty() {
            let e: I3BarEventInternal = serde_json::from_str(slice).unwrap();
            let mut e = I3BarEvent {
                id: e.name.map(|x| x.parse().unwrap()),
                instance: e.instance.map(|x| x.parse::<usize>().unwrap()),
                button: e.button,
            };
            if invert_scrolling {
                if e.button == MouseButton::WheelUp {
                    e.button = MouseButton::WheelDown;
                } else if e.button == MouseButton::WheelDown {
                    e.button = MouseButton::WheelUp;
                }
            }
            sender
                .send(e)
                .await
                .expect("channel closed while sending event");
        }

        input.clear();
    }
}
