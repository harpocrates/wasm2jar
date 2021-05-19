use std::fmt::Arguments;
use std::io;
use wasm2jar::translate::JavaRenamer;

/// Simplified utility to facilitate writing pretty-printed Java code with idiomatic block
/// indentation around curly braces.
pub struct JavaWriter<W: io::Write> {
    open_blocks: usize,
    line_in_progress: bool,
    inner: W,
    pub export_renamer: JavaRenamer,
}

impl<W: io::Write> JavaWriter<W> {
    pub fn new(inner: W) -> JavaWriter<W> {
        JavaWriter {
            open_blocks: 0,
            line_in_progress: false,
            inner,
            export_renamer: JavaRenamer::new(),
        }
    }

    /// If we are on a fresh line, make sure the indent is present
    fn ensure_line_indented(&mut self) -> io::Result<()> {
        if !self.line_in_progress {
            for _ in 0..self.open_blocks {
                self.inner.write(b"   ")?;
            }
            self.line_in_progress = true;
        }
        Ok(())
    }

    /// Append a snippet of formatted inline code (no newlines - use `newline` for that)
    pub fn inline_code_fmt(&mut self, code_fmt: Arguments) -> io::Result<()> {
        self.ensure_line_indented()?;
        self.inner.write_fmt(code_fmt)?;
        Ok(())
    }

    /// Append a snippet of inline code (no newlines - use `newline` for that)
    pub fn inline_code(&mut self, code: impl AsRef<str>) -> io::Result<()> {
        self.ensure_line_indented()?;
        self.inner.write(code.as_ref().as_bytes())?;
        Ok(())
    }

    /// Start a new line
    pub fn newline(&mut self) -> io::Result<()> {
        self.inner.write(b"\n")?;
        self.line_in_progress = false;
        Ok(())
    }

    /// Open a new curly brace block
    ///
    /// If we were mid line, this tacks on a ` {` to the current line then opens a new line.
    pub fn open_curly_block(&mut self) -> io::Result<()> {
        self.inline_code(if self.line_in_progress { " {" } else { "{" })?;
        self.newline()?;
        self.open_blocks += 1;
        Ok(())
    }

    /// Close a curly brace block
    ///
    /// This will put the `}` on a fresh line (and put another new line after that):
    pub fn close_curly_block(&mut self) -> io::Result<()> {
        assert!(self.open_blocks > 0, "no blocks to close");
        if self.line_in_progress {
            self.newline()?;
        }
        self.open_blocks -= 1;
        self.inline_code("}")?;
        self.newline()?;
        Ok(())
    }

    /// Close the writer
    pub fn close(mut self) -> io::Result<()> {
        assert_eq!(self.open_blocks, 0, "un-closed blocks remain");
        self.inner.flush()?;
        Ok(())
    }
}
