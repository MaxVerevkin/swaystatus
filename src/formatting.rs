pub mod formatter;
pub mod prefix;
pub mod template;
pub mod unit;
pub mod value;

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use serde::de::{MapAccess, Visitor};
use serde::{de, Deserialize, Deserializer};

use crate::errors::*;
use template::FormatTemplate;
use value::Value;

#[derive(Debug, Default)]
// TODO: pub struct FormatConfig<const DEFAULT: &str> {
pub struct FormatConfig {
    full: Option<FormatTemplate>,
    short: Option<FormatTemplate>,
}

pub trait FormatMapKey: Borrow<str> + Eq + Hash {}
impl<T> FormatMapKey for T where T: Borrow<str> + Eq + Hash {}

impl FormatConfig {
    pub fn new(full: Option<&str>, short: Option<&str>) -> Result<Self> {
        let full = match full {
            Some(v) => Some(v.parse()?),
            None => None,
        };
        let short = match short {
            Some(v) => Some(v.parse()?),
            None => None,
        };
        Ok(Self { full, short })
    }

    /// Initialize `full` field if it is `None`
    pub fn or_default(mut self, default_full: &str) -> Result<Self> {
        if self.full.is_none() {
            self.full = Some(default_full.parse()?);
        }
        Ok(self)
    }

    /// Whether the format string contains a given placeholder
    #[allow(dead_code)]
    pub fn contains_key(&self, key: &str) -> bool {
        self.full
            .as_ref()
            .map(|tl| tl.contains_key(key))
            .unwrap_or(false)
            || self
                .short
                .as_ref()
                .map(|tl| tl.contains_key(key))
                .unwrap_or(false)
    }

    pub fn render(
        &self,
        vars: &HashMap<impl FormatMapKey, Value>,
    ) -> Result<(String, Option<String>)> {
        let full = match &self.full {
            Some(tl) => tl.render(vars).error("Failed to render full text")?,
            None => String::new(), // TODO: throw an error that says that it's a bug?
        };
        let short = match &self.short {
            Some(tl) => Some(tl.render(vars).error("Failed to render short text")?),
            None => None,
        };
        Ok((full, short))
    }
}

impl<'de> Deserialize<'de> for FormatConfig {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Full,
            Short,
        }

        struct FormatTemplateVisitor;

        impl<'de> Visitor<'de> for FormatTemplateVisitor {
            type Value = FormatConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("format structure")
            }

            /// Handle configs like:
            ///
            /// ```toml
            /// format = "{layout}"
            /// ```
            fn visit_str<E>(self, full: &str) -> StdResult<FormatConfig, E>
            where
                E: de::Error,
            {
                FormatConfig::new(Some(full), None).serde_error()
            }

            /// Handle configs like:
            ///
            /// ```toml
            /// [block.format]
            /// full = "{layout}"
            /// short = "{layout^2}"
            /// ```
            fn visit_map<V>(self, mut map: V) -> StdResult<FormatConfig, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut full: Option<String> = None;
                let mut short: Option<String> = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Full => {
                            if full.is_some() {
                                return Err(de::Error::duplicate_field("full"));
                            }
                            full = Some(map.next_value()?);
                        }
                        Field::Short => {
                            if short.is_some() {
                                return Err(de::Error::duplicate_field("short"));
                            }
                            short = Some(map.next_value()?);
                        }
                    }
                }
                FormatConfig::new(full.as_deref(), short.as_deref()).serde_error()
            }
        }

        deserializer.deserialize_any(FormatTemplateVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render() {
        let ft = FormatConfig::new(
            Some("some text {var} var again {var}{new_var:3} {bar:2#100} {freq;1}."),
            None,
        );
        assert!(ft.is_ok());

        let values = map!(
            "var" => Value::text("|var value|".to_string()),
            "new_var" => Value::from_integer(12),
            "bar" => Value::from_integer(25),
            "freq" => Value::from_float(0.01).hertz(),
        );

        assert_eq!(
            ft.unwrap().render(&values).unwrap().0.as_str(),
            "some text |var value| var again |var value| 12 \u{258c}  0.0Hz."
        );
    }

    #[test]
    fn contains() {
        let format = FormatConfig::new(Some("some text {foo} {bar:1} foobar"), None);
        assert!(format.is_ok());
        let format = format.unwrap();
        assert!(format.contains_key("foo"));
        assert!(format.contains_key("bar"));
        assert!(!format.contains_key("foobar"));
        assert!(!format.contains_key("random string"));
    }
}
