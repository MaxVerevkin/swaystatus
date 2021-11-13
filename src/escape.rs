use std::fmt::{self, Display, Write};

pub trait IterEscape: Iterator<Item = char> {
    fn collect_pango<T: Write>(self, out: &mut T);

    fn collect_json<T: Write>(self, out: &mut T);
}

impl<I: Iterator<Item = char>> IterEscape for I {
    fn collect_pango<T: Write>(self, out: &mut T) {
        for c in self {
            match c {
                '&' => out.write_str("&amp;"),
                '<' => out.write_str("&lt;"),
                '>' => out.write_str("&gt;"),
                '\'' => out.write_str("&#39;"),
                x => out.write_char(x),
            }
            .unwrap();
        }
    }

    fn collect_json<T: Write>(self, out: &mut T) {
        for c in self {
            match c {
                '"' => out.write_str("\\\""),
                '\\' => out.write_str("\\\\"),
                '\t' => out.write_str("\\t"),
                '\n' => out.write_str("\\n"),
                x => out.write_char(x),
            }
            .unwrap();
        }
    }
}

pub struct JsonStr<'a>(pub &'a str);

impl<'a> Display for JsonStr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.chars().collect_json(f);
        Ok(())
    }
}
