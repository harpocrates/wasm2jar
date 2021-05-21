use super::StackType;

/// Information about a WASM block structure that is being visited
///
/// The specification of WASM execution defines the stack as containing values or labels. The
/// labels end up playing a role sort of like bookmarks: you `br lbl` by popping the stack until
/// you get back to the `lbl` label. For translation purposes, it is more useful to keep the labels
/// in their own stack and to track more state along with each label. `ControlFrame` is that
/// information (and the "label" itself turns out to be implicit: it is the index of the frame into
/// the stack of active frames).
#[derive(Debug)]
pub enum ControlFrame<Lbl> {
    If {
        /// Label for the else block
        else_block: Lbl,

        /// Label for the end of the block
        end_block: Lbl,

        /// What to return from the block
        return_values: Vec<StackType>,

        /// Height of WASM operand stack under this (used to decide how far to pop when breaking)
        base_stack_height: u32,
    },
    Else {
        /// Label for the end of the block
        end_block: Lbl,

        /// What to return from the block
        return_values: Vec<StackType>,

        /// Height of WASM operand stack under this (used to decide how far to pop when breaking)
        base_stack_height: u32,
    },
    Loop {
        /// Label for the top of the loop
        start_loop: Lbl,

        /// Label for after the end of the loop
        after_block: Lbl,

        /// What arguments come to this block
        input_values: Vec<StackType>,

        /// What to return from the block
        return_values: Vec<StackType>,

        /// Height of WASM operand stack under this (used to decide how far to pop when breaking)
        base_stack_height: u32,
    },
    Block {
        /// Label for the end of the block
        end_block: Lbl,

        /// What to return from the block
        return_values: Vec<StackType>,

        /// Height of WASM operand stack under this (used to decide how far to pop when breaking)
        base_stack_height: u32,
    },
}

impl<Lbl: Copy> ControlFrame<Lbl> {
    /// Get the label that should be jumped to by branching instructions targeting this block
    pub fn branch_label(&self) -> Lbl {
        match self {
            ControlFrame::If { end_block, .. } => *end_block,
            ControlFrame::Else { end_block, .. } => *end_block,
            ControlFrame::Loop { start_loop, .. } => *start_loop,
            ControlFrame::Block { end_block, .. } => *end_block,
        }
    }

    /// How many values should be on the stack before branching to this frame?
    ///
    /// Note: this is subtly different than `return_values` for loops, since in those the branch
    /// jumps to the beginning of the loop, not the end!
    pub fn branch_values(&self) -> &[StackType] {
        match self {
            ControlFrame::If { return_values, .. } => return_values,
            ControlFrame::Else { return_values, .. } => return_values,
            ControlFrame::Loop { input_values, .. } => input_values,
            ControlFrame::Block { return_values, .. } => return_values,
        }
    }

    /// Get the label that should be placed at the end of the block
    pub fn end_label(&self) -> Lbl {
        match self {
            ControlFrame::If { end_block, .. } => *end_block,
            ControlFrame::Else { end_block, .. } => *end_block,
            ControlFrame::Loop { after_block, .. } => *after_block,
            ControlFrame::Block { end_block, .. } => *end_block,
        }
    }

    /// How many return values should be on the stack when naturally ending this frame?
    pub fn return_values(&self) -> &[StackType] {
        match self {
            ControlFrame::If { return_values, .. } => return_values,
            ControlFrame::Else { return_values, .. } => return_values,
            ControlFrame::Loop { return_values, .. } => return_values,
            ControlFrame::Block { return_values, .. } => return_values,
        }
    }

    /// Size of the WASM stack on top of which this frame is placed
    ///
    /// When breaking out of the block, continue unwinding the stack until this is its size
    pub fn base_stack_height(&self) -> u32 {
        match self {
            ControlFrame::If {
                base_stack_height, ..
            } => *base_stack_height,
            ControlFrame::Else {
                base_stack_height, ..
            } => *base_stack_height,
            ControlFrame::Loop {
                base_stack_height, ..
            } => *base_stack_height,
            ControlFrame::Block {
                base_stack_height, ..
            } => *base_stack_height,
        }
    }
}
