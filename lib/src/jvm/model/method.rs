use crate::jvm::{SynLabel, MethodData, BasicBlock, ClassData};
use crate::util::Offset;
use std::collections::HashMap;

/// In-memory representation of a method
pub struct Method<'g> {
    /// The current method
    pub method: &'g MethodData<'g>,

    /// Method code implementation
    pub code_impl: Option<Code<'g>>,

    /// Which exceptions can this method throw?
    ///
    /// Note: this does not need to include `RuntimeException`, `Error`, or subclasses
    pub exceptions: Vec<&'g ClassData<'g>>,

    /// Generic method signature
    ///
    /// [Format](https://docs.oracle.com/javase/specs/jvms/se11/html/jvms-4.html#jvms-4.7.9.1)
    pub generic_signature: Option<String>,
}

/// Method code.
pub struct Code<'g> {

    /// Maximum size of locals through the method
    pub max_locals: Offset,

    /// Maximum size of stack through the method
    pub min_locals: Offset,

    /// Basic blocks in the code
    pub blocks: HashMap<SynLabel, BasicBlock<'g, SynLabel, SynLabel, SynLabel>>,

    /// Order of basic blocks in the code (elements are unique and exactly match keys of `blocks`)
    pub block_order: Vec<SynLabel>,

}

