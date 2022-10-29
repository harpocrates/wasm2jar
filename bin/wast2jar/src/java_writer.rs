//! Utilities for writing Java-style (curly blocks + indented) source code.

use std::io::{Result, Write};

/// Simplified utility to facilitate writing pretty-printed Java code with idiomatic block
/// indentation around curly braces.
pub struct JavaWriter<W: Write> {
    /// How many blocks have been opened but not closed? This determines how indented new lines
    /// should be.
    open_blocks: usize,

    /// Is there a line already in progress?
    line_in_progress: bool,

    /// Inner writer
    inner: W,
}

/// Indentation aware writer
///
/// Automatically adds indentation after newlines in the text written. This does _not_ have special
/// handling for detecting curly braces (those will get written out literally).
impl<W: Write> Write for JavaWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        for line in buf.split_inclusive(|b| *b == b'\n') {
            if line.first().filter(|b| b.is_ascii_whitespace()).is_none() {
                self.ensure_line_indented()?;
            }
            self.inner.write_all(line)?;
            self.line_in_progress = false;
        }
        self.line_in_progress = buf.last().copied() != Some(b'\n');
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

impl<W: Write> JavaWriter<W> {
    pub fn new(inner: W) -> JavaWriter<W> {
        JavaWriter {
            open_blocks: 0,
            line_in_progress: false,
            inner,
        }
    }

    /// If we are on a fresh line, make sure the indent is present
    fn ensure_line_indented(&mut self) -> Result<()> {
        if !self.line_in_progress {
            for _ in 0..self.open_blocks {
                self.inner.write_all(b"    ")?;
            }
            self.line_in_progress = true;
        }
        Ok(())
    }

    /// Start a new line
    pub fn newline(&mut self) -> Result<()> {
        self.inner.write_all(b"\n")?;
        self.line_in_progress = false;
        Ok(())
    }

    /// Open a new curly brace block
    ///
    /// If we were mid line, this tacks on a ` {` to the current line then opens a new line.
    pub fn open_curly_block(&mut self) -> Result<()> {
        if self.line_in_progress {
            write!(self, " ")?;
        }
        writeln!(self, "{{")?;
        self.open_blocks += 1;
        Ok(())
    }

    /// Close a curly brace block
    ///
    /// This will put the `}` on a fresh line (and put another new line after that).
    pub fn close_curly_block(&mut self) -> Result<()> {
        assert!(self.open_blocks > 0, "no blocks to close");
        if self.line_in_progress {
            self.newline()?;
        }
        self.open_blocks -= 1;
        writeln!(self, "}}")?;
        Ok(())
    }

    /// Close the writer
    pub fn close(mut self) -> Result<()> {
        assert_eq!(self.open_blocks, 0, "un-closed blocks remain");
        self.inner.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn pass_through() -> Result<()> {
        let mut output = vec![];
        let mut java_writer = JavaWriter::new(&mut output);

        write!(&mut java_writer, "hello")?;
        write!(&mut java_writer, " world")?;
        writeln!(&mut java_writer, "!")?;
        java_writer.close()?;

        assert_eq!(std::str::from_utf8(&output).unwrap(), "hello world!\n");
        Ok(())
    }

    #[test]
    fn simple_blocks() -> Result<()> {
        let mut output = vec![];
        let mut java_writer = JavaWriter::new(&mut output);

        write!(&mut java_writer, "public static void main(String [] args)")?;
        java_writer.open_curly_block()?;
        writeln!(
            &mut java_writer,
            "System.out.println(\"Total args :\" + args.length);"
        )?;
        java_writer.close_curly_block()?;
        java_writer.close()?;

        assert_eq!(
            std::str::from_utf8(&output).unwrap(),
            r#"public static void main(String [] args) {
    System.out.println("Total args :" + args.length);
}
"#
        );
        Ok(())
    }

    #[test]
    fn nested_blocks() -> Result<()> {
        let mut output = vec![];
        let mut java_writer = JavaWriter::new(&mut output);

        write!(&mut java_writer, "public static void main(String [] args)")?;
        java_writer.open_curly_block()?;
        write!(&mut java_writer, "for (String arg : args)")?;
        java_writer.open_curly_block()?;
        writeln!(&mut java_writer, "System.out.println(\"ANOTHER ARG\");")?;
        write!(&mut java_writer, "System.out.println(arg);")?;
        java_writer.close_curly_block()?;
        writeln!(&mut java_writer, "System.out.println(\"DONE\");")?;
        java_writer.close_curly_block()?;
        java_writer.close()?;

        assert_eq!(
            std::str::from_utf8(&output).unwrap(),
            r#"public static void main(String [] args) {
    for (String arg : args) {
        System.out.println("ANOTHER ARG");
        System.out.println(arg);
    }
    System.out.println("DONE");
}
"#
        );
        Ok(())
    }
}
