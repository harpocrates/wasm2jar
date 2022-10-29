use std::fmt::{Display, Formatter, Result, Write};

/// Wrapper struct whose [`Display`] implementation renders as a Java string literal
pub struct JavaStringLiteral<'a>(pub &'a str);

impl Display for JavaStringLiteral<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.write_char('"')?;
        let mut code_points = [0; 2];
        for c in self.0.chars() {
            match c {
                '\t' => f.write_str("\\t")?,
                '\r' => f.write_str("\\r")?,
                '\n' => f.write_str("\\n")?,
                '\\' | '\'' | '"' => {
                    f.write_char('\\')?;
                    f.write_char(c)?;
                }
                '\x20'..='\x7e' => f.write_char(c)?,
                _ => {
                    for code_point in c.encode_utf16(&mut code_points) {
                        f.write_fmt(format_args!("\\u{:04x}", code_point))?;
                    }
                }
            }
        }
        f.write_char('"')
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ascii_only() {
        assert_eq!(
            format!("{}", JavaStringLiteral("hello world!")),
            "\"hello world!\""
        );
        assert_eq!(format!("{}", JavaStringLiteral("abcdefg")), "\"abcdefg\"");
        assert_eq!(
            format!("{}", JavaStringLiteral("alpha123*")),
            "\"alpha123*\""
        );
    }

    #[test]
    fn special_escapes() {
        assert_eq!(
            format!("{}", JavaStringLiteral("\"hel\\o\tworld!\n\"")),
            "\"\\\"hel\\\\o\\tworld!\\n\\\"\""
        );
        assert_eq!(format!("{}", JavaStringLiteral("\'foo\'")), "\"\\'foo\\'\"");
    }

    #[test]
    fn general_unicode() {
        assert_eq!(
            format!("{}", JavaStringLiteral("a\x15\u{0082}\u{2764}b")),
            "\"a\\u0015\\u0082\\u2764b\""
        );
        assert_eq!(
            format!("{}", JavaStringLiteral("a\u{101234}b")),
            "\"a\\udbc4\\ude34b\""
        );
    }
}
