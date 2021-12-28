use chrono::{DateTime, Local};
use serde::de::{self, Deserialize, Deserializer};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnceDuration {
    Once,
    Duration(Seconds),
}

impl<'de> Deserialize<'de> for OnceDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OnceDurationVisitor;

        impl<'de> de::Visitor<'de> for OnceDurationVisitor {
            type Value = OnceDuration;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("\"once\", i64 or f64")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OnceDuration::Duration(Seconds(Duration::from_secs(
                    v as u64,
                ))))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OnceDuration::Duration(Seconds(Duration::from_secs_f64(v))))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v == "once" {
                    Ok(OnceDuration::Once)
                } else {
                    Err(E::custom(format!("'{}' is not a valid interval", v)))
                }
            }
        }

        deserializer.deserialize_any(OnceDurationVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seconds(pub Duration);

impl Seconds {
    pub fn new(value: u64) -> Self {
        Self(Duration::from_secs(value))
    }

    pub fn timer(self) -> tokio::time::Interval {
        let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + self.0, self.0);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        timer
    }
}

impl<'de> Deserialize<'de> for Seconds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SecondsVisitor;

        impl<'de> de::Visitor<'de> for SecondsVisitor {
            type Value = Seconds;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("i64 or f64")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Seconds(Duration::from_secs(v as u64)))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Seconds(Duration::from_secs_f64(v)))
            }
        }

        deserializer.deserialize_any(SecondsVisitor)
    }
}

pub fn deserialize_local_timestamp<'de, D>(deserializer: D) -> Result<DateTime<Local>, D::Error>
where
    D: Deserializer<'de>,
{
    use chrono::TimeZone;
    i64::deserialize(deserializer).map(|seconds| Local.timestamp(seconds, 0))
}
