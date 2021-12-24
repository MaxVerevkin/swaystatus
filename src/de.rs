use std::time::Duration;

use serde::de::{self, Deserialize, Deserializer};

pub fn deserialize_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    struct DurationWrapper;

    impl<'de> de::Visitor<'de> for DurationWrapper {
        type Value = Duration;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("i64, f64 or map")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Duration::from_secs(value as u64))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Duration::new(0, (value * 1_000_000_000f64) as u32))
        }

        fn visit_map<A>(self, visitor: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(visitor))
        }
    }

    deserializer.deserialize_any(DurationWrapper)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnceDuration {
    Once,
    Duration(Duration),
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
                Ok(OnceDuration::Duration(Duration::from_secs(v as u64)))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(OnceDuration::Duration(Duration::from_secs_f64(v)))
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
