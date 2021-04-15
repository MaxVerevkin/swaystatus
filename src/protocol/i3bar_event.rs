use std::fmt;
use std::option::Option;
use std::string::*;

use serde::{de, Deserializer};
use serde_derive::Deserialize;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    Forward,
    Back,
    Unknown,
}

#[derive(Deserialize, Debug, Clone)]
struct I3BarEventInternal {
    pub name: Option<String>,
    pub instance: Option<String>,
    pub x: u64,
    pub y: u64,

    #[serde(deserialize_with = "deserialize_mousebutton")]
    pub button: MouseButton,
}

#[derive(Debug, Clone, Copy)]
pub struct I3BarEvent {
    pub id: Option<usize>,
    pub instance: Option<usize>,
    pub button: MouseButton,
}

pub async fn process_events(sender: Sender<I3BarEvent>) {
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut input = String::new();

    loop {
        stdin.read_line(&mut input).await.unwrap();

        // Take only the valid JSON object betweem curly braces (cut off leading bracket, commas and whitespace)
        let slice = input.trim_start_matches(|c| c != '{');
        let slice = slice.trim_end_matches(|c| c != '}');

        if !slice.is_empty() {
            let e: I3BarEventInternal = serde_json::from_str(slice).unwrap();
            sender
                .send(I3BarEvent {
                    id: e.name.map(|x| x.parse::<usize>().unwrap()),
                    instance: e.instance.map(|x| x.parse::<usize>().unwrap()),
                    button: e.button,
                })
                .await
                .expect("channel closed while sending event");
        }

        input.clear();
    }
}

fn deserialize_mousebutton<'de, D>(deserializer: D) -> Result<MouseButton, D::Error>
where
    D: Deserializer<'de>,
{
    struct MouseButtonVisitor;

    impl<'de> de::Visitor<'de> for MouseButtonVisitor {
        type Value = MouseButton;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("u64")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // TODO: put this behind `--debug` flag
            //eprintln!("{}", value);
            Ok(match value {
                1 => MouseButton::Left,
                2 => MouseButton::Middle,
                3 => MouseButton::Right,
                4 => MouseButton::WheelUp,
                5 => MouseButton::WheelDown,
                9 => MouseButton::Forward,
                8 => MouseButton::Back,
                _ => MouseButton::Unknown,
            })
        }
    }

    deserializer.deserialize_any(MouseButtonVisitor)
}
