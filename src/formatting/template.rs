use super::formatter::{new_formatter, Formatter};
use super::value::Value;
use super::FormatMapKey;
use crate::errors::*;
use std::collections::HashMap;
use std::iter::Peekable;
use std::str::FromStr;

#[derive(Debug)]
pub struct FormatTemplate(pub Vec<TokenList>);

#[derive(Debug)]
pub struct TokenList(pub Vec<Token>);

#[derive(Debug)]
pub enum Token {
    Text(String),
    Recursive(FormatTemplate),
    Var {
        name: String,
        formatter: Box<dyn Formatter + Send + Sync>,
    },
}

impl FormatTemplate {
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.iter().any(|token_list| {
            token_list.0.iter().any(|token| match token {
                Token::Var { name, .. } => name == key,
                Token::Recursive(rec) => rec.contains_key(key),
                _ => false,
            })
        })
    }

    pub fn render(&self, vars: &HashMap<impl FormatMapKey, Value>) -> Result<String> {
        for (i, token_list) in self.0.iter().enumerate() {
            match token_list.render(vars) {
                Ok(res) => return Ok(res),
                Err(e) if e.kind != ErrorKind::Format => return Err(e),
                Err(e) if i == self.0.len() - 1 => return Err(e),
                _ => (),
            }
        }
        Ok(String::new())
    }
}

impl TokenList {
    pub fn render(&self, vars: &HashMap<impl FormatMapKey, Value>) -> Result<String> {
        let mut retval = String::new();
        for token in &self.0 {
            match token {
                Token::Text(text) => retval.push_str(text),
                Token::Recursive(rec) => retval.push_str(&rec.render(vars)?),
                Token::Var { name, formatter } => retval.push_str(
                    &formatter.format(
                        vars.get(name)
                            .format_error(format!("Placeholder with name '{}' not found", name))?,
                    )?,
                ),
            }
        }
        Ok(retval)
    }
}

impl FromStr for FormatTemplate {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut it = s.chars().chain(std::iter::once('}')).peekable();
        let template = read_format_template(&mut it)?;
        if it.next().is_some() {
            Err(Error::new("Unexpected '}'"))
        } else {
            Ok(template)
        }
    }
}

fn read_format_template(it: &mut Peekable<impl Iterator<Item = char>>) -> Result<FormatTemplate> {
    let mut token_lists = Vec::new();
    let mut cur_list = Vec::new();
    loop {
        match *it.peek().error("Missing '}'")? {
            '{' => {
                let _ = it.next();
                cur_list.push(Token::Recursive(read_format_template(it)?));
            }
            '}' => {
                let _ = it.next();
                token_lists.push(TokenList(cur_list));
                return Ok(FormatTemplate(token_lists));
            }
            '|' => {
                let _ = it.next();
                token_lists.push(TokenList(cur_list));
                cur_list = Vec::new();
            }
            '$' => {
                let _ = it.next();
                let name = read_placeholder_name(it)?;
                let formatter = read_formatter(it)?;
                let args = read_args(it)?;
                cur_list.push(Token::Var {
                    name,
                    formatter: new_formatter(&formatter, &args)?,
                });
            }
            _ => {
                cur_list.push(Token::Text(read_text(it)?));
            }
        }
    }
}

fn read_text(it: &mut Peekable<impl Iterator<Item = char>>) -> Result<String> {
    let mut retval = String::new();
    let mut escaped = false;
    while let Some(&c) = it.peek() {
        if escaped {
            escaped = false;
            retval.push(c);
            let _ = it.next();
            continue;
        }
        match c {
            '\\' => {
                let _ = it.next();
                escaped = true;
            }
            '{' | '}' | '$' | '|' => break,
            x => {
                let _ = it.next();
                retval.push(x);
            }
        }
    }
    Ok(retval)
}

fn read_placeholder_name(it: &mut impl Iterator<Item = char>) -> Result<String> {
    let mut retval = String::new();
    let mut escaped = false;
    while let Some(c) = it.next() {
        if escaped {
            escaped = false;
            retval.push(c);
            continue;
        }
        match c {
            '\\' => escaped = true,
            '.' => return Ok(retval),
            x => retval.push(x),
        }
    }
    Err(Error::new("Missing '.'"))
}

fn read_formatter(it: &mut impl Iterator<Item = char>) -> Result<String> {
    let mut retval = String::new();
    let mut escaped = false;
    while let Some(c) = it.next() {
        if escaped {
            escaped = false;
            retval.push(c);
            continue;
        }
        match c {
            '\\' => escaped = true,
            '(' => return Ok(retval),
            x => retval.push(x),
        }
    }
    Err(Error::new("Missing '.'"))
}

fn read_args(it: &mut impl Iterator<Item = char>) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut cur_arg = String::new();
    let mut escaped = false;
    while let Some(c) = it.next() {
        if escaped {
            escaped = false;
            cur_arg.push(c);
            continue;
        }
        match c {
            '\\' => escaped = true,
            ',' => {
                args.push(cur_arg);
                cur_arg = String::new();
            }
            ')' => {
                if !cur_arg.is_empty() || !args.is_empty() {
                    args.push(cur_arg);
                }
                return Ok(args);
            }
            x => cur_arg.push(x),
        }
    }
    Err(Error::new("Missing ')'"))
}
