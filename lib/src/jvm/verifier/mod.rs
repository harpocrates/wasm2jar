//! Bytecode verification utilities
//!
//! For any specific instruction inside a method body, the stack and locals should have the same
//! structure, regardless of which control flow was used to reach that instruction. in other words:
//! although the values on the stack and in the locals may obviously be different, the types and
//! order of the stack and local variables cannot. This information is referred to as the _stack
//! map frame_ (represented using [`VerifierFrame`]) and the set of stack map frames for all
//! possible jump targets in a method is the _stack map table_.
//!
//! Knowing the stack map frame at a point in the code makes it possible to verify that the next
//! instruction makes sense (eg. `dadd` only makes sense if the top two elements on the stack are
//! of type `double`). The "types" used in verification (represented using [`VerificationType`])
//! are slightly augumented to take into account initialization and null.
//!
//! The process of verifying a program is referred to as [verification by type-checking][0], and it
//! is something that the JVM itself does when loading a class. Although verifying straight-line
//! instructions is pretty simple (see [`Frame::verify_instruction`]), things get more complicated
//! when an instruction can be reach from multiple locations (eg. it is the target of jumps). In
//! those cases, the frames from the different source locations need to be unified. This ends up
//! being a fix-point algorithm which converges towards the right answer (if there is one). Since
//! inferring the stack map table of a method is potentially quite expensive, method code must be
//! annotated with a [`crate::jvm::class_file::StackMapTable`] attribute which stores the stack map
//! frame for every offset that is the target of a jump.
//!
//! [0]: https://docs.oracle.com/javase/specs/jvms/se17/html/jvms-4.html#jvms-4.10.1

mod frame;
mod types;

pub use frame::*;
pub use types::*;
