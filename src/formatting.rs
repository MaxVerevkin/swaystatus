pub mod placeholder;
pub mod prefix;
pub mod unit;
pub mod value;

use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{de, Deserialize, Deserializer};

use crate::errors::*;
use placeholder::unexpected_token;
use placeholder::Placeholder;
use value::Value;

macro_rules! default_format {
    ($format:expr,$default:expr) => {
        match $format {
            Some(format) => Ok(format),
            None => crate::formatting::FormatTemplate::new($default, None),
        }
    };
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Text(String),
    Var(Placeholder),
}

#[derive(Debug, Clone)]
pub struct FormatTemplate {
    full: Vec<Token>,
    short: Option<Vec<Token>>,
}

impl FormatTemplate {
    pub fn new(full: &str, short: Option<&str>) -> Result<Self> {
        let full = Self::tokens_from_string(full)?;
        let short = match short {
            Some(short) => Some(Self::tokens_from_string(short)?),
            None => None,
        };
        Ok(Self { full, short })
    }

    fn tokens_from_string(s: &str) -> Result<Vec<Token>> {
        let mut tokens = vec![];

        let mut text_buf = String::new();
        let mut var_buf = String::new();
        let mut inside_var = false;

        let mut current_buf = &mut text_buf;

        for c in s.chars() {
            match c {
                '{' => {
                    if inside_var {
                        return unexpected_token(c);
                    }
                    if !text_buf.is_empty() {
                        tokens.push(Token::Text(text_buf.clone()));
                        text_buf.clear();
                    }
                    current_buf = &mut var_buf;
                    inside_var = true;
                }
                '}' => {
                    if !inside_var {
                        return unexpected_token(c);
                    }
                    tokens.push(Token::Var(var_buf.as_str().try_into()?));
                    var_buf.clear();
                    current_buf = &mut text_buf;
                    inside_var = false;
                }
                x => current_buf.push(x),
            }
        }
        if inside_var {
            return Err(InternalError {
                context: "format parser".to_string(),
                message: "missing '}'".to_string(),
                cause: None,
                cause_dbg: None,
            });
        }
        if !text_buf.is_empty() {
            tokens.push(Token::Text(text_buf.clone()));
        }

        Ok(tokens)
    }

    pub fn render(&self, vars: &HashMap<&str, Value>) -> Result<(String, Option<String>)> {
        let full = Self::render_tokens(&self.full, vars)?;
        Ok((
            full,
            match &self.short {
                Some(short) => Some(Self::render_tokens(short, vars)?),
                None => None,
            },
        ))
    }

    fn render_tokens(tokens: &[Token], vars: &HashMap<&str, Value>) -> Result<String> {
        let mut rendered = String::new();
        for token in tokens {
            match token {
                Token::Text(text) => rendered.push_str(&text),
                Token::Var(var) => rendered.push_str(
                    &vars
                        .get(&*var.name)
                        .internal_error(
                            "util",
                            &format!("Unknown placeholder in format string: {}", var.name),
                        )?
                        .format(&var)?,
                ),
            }
        }
        Ok(rendered)
    }
}

impl<'de> Deserialize<'de> for FormatTemplate {
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
            type Value = FormatTemplate;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("format structure")
            }

            /// Handle configs like:
            ///
            /// ```toml
            /// format = "{layout}"
            /// ```
            fn visit_str<E>(self, full: &str) -> StdResult<FormatTemplate, E>
            where
                E: de::Error,
            {
                FormatTemplate::new(full, None).map_err(|e| de::Error::custom(e.to_string()))
            }

            /// Handle configs like:
            ///
            /// ```toml
            /// [block.format]
            /// full = "{layout}"
            /// short = "{layout^2}"
            /// ```
            fn visit_map<V>(self, mut map: V) -> StdResult<FormatTemplate, V::Error>
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

                let full = full.ok_or_else(|| de::Error::missing_field("full"))?;
                FormatTemplate::new(&full, short.as_deref())
                    .map_err(|e| de::Error::custom(e.to_string()))
            }
        }

        deserializer.deserialize_any(FormatTemplateVisitor)
    }
}
