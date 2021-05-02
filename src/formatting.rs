pub mod placeholder;
pub mod prefix;
pub mod unit;
pub mod value;

use std::collections::HashMap;
use std::convert::TryInto;

use crate::errors::*;
use placeholder::unexpected_token;
use placeholder::Placeholder;
use value::Value;

#[derive(Debug, Clone)]
pub struct FormatTemplate {
    tokens: Vec<Token>,
    short_tokens: Option<Vec<Token>>,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Text(String),
    Var(Placeholder),
}

impl FormatTemplate {
    pub fn new(full: &str, short: Option<&str>) -> Result<Self> {
        let mut retval = Self::from_string(full)?;
        if let Some(short) = short {
            let short = Self::from_string(short)?;
            retval.short_tokens = Some(short.tokens);
        }
        Ok(retval)
    }

    pub fn from_string(s: &str) -> Result<Self> {
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

        Ok(FormatTemplate {
            tokens,
            short_tokens: None,
        })
    }

    pub fn render(&self, vars: &HashMap<&str, Value>) -> Result<(String, Option<String>)> {
        let full = Self::render_tokens(&self.tokens, vars)?;
        Ok((
            full,
            match &self.short_tokens {
                Some(tokens) => Some(Self::render_tokens(tokens, vars)?),
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
