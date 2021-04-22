use serde::de::{self, Deserialize, Deserializer, Visitor};
use std::fmt;

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

impl<'de> Deserialize<'de> for MouseButton {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MouseButtonVisitor;

        impl<'de> Visitor<'de> for MouseButtonVisitor {
            type Value = MouseButton;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("u64 or string")
            }

            /// ```toml
            /// button = "left"
            /// ```
            fn visit_str<E>(self, name: &str) -> Result<MouseButton, E>
            where
                E: de::Error,
            {
                use MouseButton::*;
                Ok(match name {
                    "left" => Left,
                    "middle" => Middle,
                    "Right" => Right,
                    "up" => WheelUp,
                    "down" => WheelDown,
                    "forward" => Forward,
                    "back" => Back,
                    _ => Unknown,
                })
            }

            /// ```toml
            /// button = 1
            /// ```
            fn visit_u64<E>(self, number: u64) -> Result<MouseButton, E>
            where
                E: de::Error,
            {
                use MouseButton::*;
                Ok(match number {
                    1 => Left,
                    2 => Middle,
                    3 => Right,
                    4 => WheelUp,
                    5 => WheelDown,
                    9 => Forward,
                    8 => Back,
                    _ => Unknown,
                })
            }
        }

        deserializer.deserialize_any(MouseButtonVisitor)
    }
}
