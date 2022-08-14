//! Bytecode representation and generation
//!
//! ### Structure
//!
//! Despite being pushed off into [just another method attribute](crate::jvm::class_file::Code),
//! the bytecode is arguably the most important part of the class file - it contains the actual
//! executable instructions. Method bodies are essentially just a CFG of basic blocks, with an
//! operand stack and a stack of local variables. We split up the [list of bytecode
//! instructions][0] into two groups:
//!
//!   - [`Instruction`] for straight-line instructions (the body of the basic blocks)
//!   - [`BranchInstruction`] for instructions that may branch (the end of the basic blocks)
//!
//! With these, we can literally represent the method [`Code`] as an ordered sequence of
//! [`BasicBlock`]s.
//!
//! ### Code generation
//!
//! Since there is actually a little bit more that the JVM needs (see [`crate::jvm::verifier`]), it
//! can get quite tedious and error prone to generate valid bytecode. In order to aid in this
//! process, [`CodeBuilder`] provides an interface for generating method code from top to bottom
//! and doing the verification incrementally.
//!
//! [0]: https://docs.oracle.com/javase/specs/jvms/se18/html/jvms-6.html#jvms-6.5

mod basic_block;
mod code;
mod code_builder;
mod code_builder_exts;
mod instructions;
pub mod jump_encoding;
mod label;

pub use basic_block::*;
pub use code::*;
pub use code_builder::*;
pub use code_builder_exts::*;
pub use instructions::*;
pub use label::*;
