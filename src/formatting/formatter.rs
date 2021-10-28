use super::prefix::Prefix;
use super::unit::Unit;
use super::value::Value;
use crate::errors::*;
use std::fmt::Debug;
use std::str::FromStr;

enum StrArgs {
    MinWidth,
    MaxWidth,
}

enum BarArgs {
    Width,
    MaxValue,
}

enum EngFixArgs {
    Width,
    Unit,
    Prefix,
}

pub trait Formatter: Debug {
    fn format(&self, val: &Value) -> Result<String>;
}

pub fn new_formatter(name: &str, args: &[String]) -> Result<Box<dyn Formatter + Send + Sync>> {
    match name {
        "str" => {
            let min_width: usize = match args.get(StrArgs::MinWidth as usize) {
                Some(v) => v.parse().error("Width must be a positive integer")?,
                None => 0,
            };
            let max_width: Option<usize> =
                match args.get(StrArgs::MaxWidth as usize).map(|x| x.as_str()) {
                    Some("inf") | None => None,
                    Some(v) => Some(v.parse().error("Width must be a positive integer")?),
                };
            if let Some(max_width) = max_width {
                if max_width < min_width {
                    return Err(Error::new(
                        "Max width must be greater of equal to min width",
                    ));
                }
            }
            Ok(Box::new(StrFormatter {
                min_width,
                max_width,
            }))
        }
        "bar" => {
            let width: usize = match args.get(BarArgs::Width as usize) {
                Some(v) => v.parse().error("Width must be a positive integer")?,
                None => 5,
            };
            let max_value: f64 = match args.get(BarArgs::MaxValue as usize) {
                Some(v) => v.parse().error("Max value must be a number")?,
                None => 100.,
            };
            Ok(Box::new(BarFormatter { width, max_value }))
        }
        "eng" => Ok(Box::new(EngFormatter(EngFixConfig::from_args(args)?))),
        "fix" => Ok(Box::new(FixFormatter(EngFixConfig::from_args(args)?))),
        _ => Err(Error::new(format!("Unknown formatter: '{}'", name))),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StrFormatter {
    min_width: usize,
    max_width: Option<usize>,
}

impl Formatter for StrFormatter {
    fn format(&self, val: &Value) -> Result<String> {
        match val {
            Value::Text(text) => {
                let width = text.chars().count();
                if width < self.min_width {
                    let mut text = text.clone();
                    for _ in 0..(self.min_width - width) {
                        text.push(' ');
                    }
                    return Ok(text);
                }
                if let Some(max_width) = self.max_width {
                    if width > max_width {
                        return Ok(text.chars().take(max_width).collect());
                    }
                }
                Ok(text.to_string())
            }
            Value::Number { .. } => Err(Error::new_format(
                "A number cannot be formatted with 'str' formatter",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BarFormatter {
    width: usize,
    max_value: f64,
}

const VERTICAL_BAR_CHARS: [char; 9] = [
    ' ', '\u{258f}', '\u{258e}', '\u{258d}', '\u{258c}', '\u{258b}', '\u{258a}', '\u{2589}',
    '\u{2588}',
];

impl Formatter for BarFormatter {
    fn format(&self, val: &Value) -> Result<String> {
        match val {
            Value::Number { mut val, .. } => {
                val = (val / self.max_value).clamp(0., 1.);
                let chars_to_fill = val * self.width as f64;
                Ok((0..self.width)
                    .map(|i| {
                        VERTICAL_BAR_CHARS[((chars_to_fill - i as f64).clamp(0., 1.) * 8.) as usize]
                    })
                    .collect())
            }
            Value::Text(_) => Err(Error::new_format(
                "Text cannot be formatted with 'bar' formatter",
            )),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct PrefixConfig {
    pub prefix: Option<(Prefix, bool)>,
    pub has_space: bool,
    pub hidden: bool,
}

impl FromStr for PrefixConfig {
    type Err = Error;

    fn from_str(mut s: &str) -> Result<Self> {
        let has_space = if s.starts_with(' ') {
            s = &s[1..];
            true
        } else {
            false
        };

        let hidden = if s.starts_with('_') {
            s = &s[1..];
            true
        } else {
            false
        };

        let forced = if s.starts_with('!') {
            s = &s[1..];
            true
        } else {
            false
        };

        Ok(Self {
            prefix: Some((s.parse()?, forced)),
            has_space,
            hidden,
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct UnitConfig {
    pub unit: Option<Unit>,
    pub has_space: bool,
    pub hidden: bool,
}

impl FromStr for UnitConfig {
    type Err = Error;

    fn from_str(mut s: &str) -> Result<Self> {
        let has_space = if s.starts_with(' ') {
            s = &s[1..];
            true
        } else {
            false
        };

        let hidden = if s.starts_with('_') {
            s = &s[1..];
            true
        } else {
            false
        };

        Ok(Self {
            unit: Some(s.parse()?),
            has_space,
            hidden,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct EngFixConfig {
    width: usize,
    unit: UnitConfig,
    prefix: PrefixConfig,
}

impl EngFixConfig {
    fn from_args(args: &[String]) -> Result<Self> {
        let width: usize = match args.get(EngFixArgs::Width as usize) {
            Some(v) => v.parse().error("Width must be a positive integer")?,
            None => 3,
        };
        let unit: UnitConfig = match args.get(EngFixArgs::Unit as usize).map(|x| x.as_str()) {
            Some("auto") | None => Default::default(),
            Some(v) => v.parse()?,
        };
        let prefix: PrefixConfig = match args.get(EngFixArgs::Prefix as usize).map(|x| x.as_str()) {
            Some("auto") | None => Default::default(),
            Some(v) => v.parse()?,
        };
        Ok(Self {
            width,
            unit,
            prefix,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EngFormatter(EngFixConfig);

impl Formatter for EngFormatter {
    fn format(&self, val: &Value) -> Result<String> {
        match val {
            Value::Number {
                mut val,
                mut unit,
                icon,
            } => {
                if let Some(new_unit) = self.0.unit.unit {
                    val = unit.convert(val, new_unit)?;
                    unit = new_unit;
                }

                let (min_prefix, max_prefix) = match self.0.prefix.prefix {
                    Some((prefix, true)) => (prefix, prefix),
                    Some((prefix, false)) => (prefix, Prefix::max_available()),
                    None => (Prefix::min_available(), Prefix::max_available()),
                };

                let prefix = unit.clamp_prefix(
                    Prefix::from_exp_level(val.log10().div_euclid(3.) as i32)
                        .clamp(min_prefix, max_prefix),
                );
                val = prefix.apply(val);

                let mut digits = (val.max(1.).log10().floor() + 1.0) as isize;
                if val < 0. {
                    digits += 1;
                }

                let mut retval = icon.clone();
                retval.push_str(&match self.0.width as isize - digits {
                    isize::MIN..=0 => format!("{:.0}", val),
                    1 => format!(" {:.0}", val),
                    rest => format!("{:.*}", rest as usize - 1, val),
                });
                if !self.0.prefix.hidden {
                    if self.0.prefix.has_space {
                        retval.push(' ');
                    }
                    retval.push_str(&prefix.to_string());
                }
                if !self.0.unit.hidden {
                    if self.0.unit.has_space {
                        retval.push(' ');
                    }
                    retval.push_str(&unit.to_string());
                }

                Ok(retval)
            }
            Value::Text(_) => Err(Error::new_format(
                "Text cannot be formatted with 'eng' formatter",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixFormatter(EngFixConfig);

impl Formatter for FixFormatter {
    fn format(&self, val: &Value) -> Result<String> {
        match val {
            Value::Number {
                ..
                // mut val,
                // unit,
                // icon,
            } => Err(Error::new_format("'fix' formatter is not implemented yet")),
            Value::Text(_) => Err(Error::new_format(
                "Text cannot be formatted with 'eng' formatter",
            )),
        }
    }
}
