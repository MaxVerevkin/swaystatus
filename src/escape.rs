//! Simple pango and json escaping

use std::fmt::{self, Display, Write};

pub trait CollectEscaped {
    /// Write escaped version of `self` to `out`
    fn collect_pango_into<T: Write>(self, out: &mut T);

    /// Write escaped version of `self` to a new buffer
    #[inline]
    fn collect_pango<T: Write + Default>(self) -> T
    where
        Self: Sized,
    {
        let mut out = T::default();
        self.collect_pango_into(&mut out);
        out
    }

    /// Write escaped version of `self` to `out`
    fn collect_json_into<T: Write>(self, out: &mut T);

    /// Write escaped version of `self` to a new buffer
    #[inline]
    fn collect_json<T: Write + Default>(self) -> T
    where
        Self: Sized,
    {
        let mut out = T::default();
        self.collect_json_into(&mut out);
        out
    }
}

impl<I: Iterator<Item = char>> CollectEscaped for I {
    fn collect_pango_into<T: Write>(self, out: &mut T) {
        for c in self {
            let _ = match c {
                '&' => out.write_str("&amp;"),
                '<' => out.write_str("&lt;"),
                '>' => out.write_str("&gt;"),
                '\'' => out.write_str("&#39;"),
                x => out.write_char(x),
            };
        }
    }

    fn collect_json_into<T: Write>(self, out: &mut T) {
        for c in self {
            let _ = match c {
                '"' => out.write_str("\\\""),
                '\\' => out.write_str("\\\\"),
                '\t' => out.write_str("\\t"),
                '\n' => out.write_str("\\n"),
                '\r' => out.write_str("\\r"),
                x => out.write_char(x),
            };
        }
    }
}

pub struct JsonStr<'a>(pub &'a str);

impl<'a> Display for JsonStr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.chars().collect_json_into(f);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pango() {
        let orig = "&my 'text' <>";
        let escaped: String = orig.chars().collect_pango();
        assert_eq!(escaped, "&amp;my &#39;text&#39; &lt;&gt;");
    }

    #[test]
    fn json() {
        let orig = "my\ntest\t";
        let escaped: String = orig.chars().collect_json();
        assert_eq!(escaped, "my\\ntest\\t");
    }

    #[test]
    fn json_display() {
        let orig = "my\ntest\t";
        let escaped = format!("{}", JsonStr(&orig));
        assert_eq!(escaped, "my\\ntest\\t");
    }
}
