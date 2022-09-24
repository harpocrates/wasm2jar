use std::fmt;

/// Opaque label
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct SynLabel(usize);

impl SynLabel {
    /// Label for the first block in the method
    pub const START: SynLabel = SynLabel(0);

    /// Get the next fresh label
    pub fn next(&self) -> SynLabel {
        SynLabel(self.0 + 1)
    }
}

/// Generates new labels
pub trait LabelGenerator<Label> {
    /// Generate a fresh label
    fn fresh_label(&mut self) -> Label;
}

/// Label generator for [`SynLabel`]
///
/// Cloning does not split the generator source - the cloned generator will produce the same
/// sequence of labels as the original.
#[derive(Clone)]
pub struct SynLabelGenerator(SynLabel);

impl SynLabelGenerator {
    pub fn new(start: SynLabel) -> SynLabelGenerator {
        SynLabelGenerator(start)
    }
}

impl LabelGenerator<SynLabel> for SynLabelGenerator {
    fn fresh_label(&mut self) -> SynLabel {
        let to_return = self.0;
        self.0 = self.0.next();
        to_return
    }
}

impl fmt::Debug for SynLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_fmt(format_args!("l{}", self.0))
    }
}
