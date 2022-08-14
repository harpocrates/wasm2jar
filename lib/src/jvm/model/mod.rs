//! Semantic representations of classes
//!
//! This is usually the representation to use while working with classes for purposes of codegen or
//! analysis. It keeps all of the semantic information around and queryable.
//!
//!   - __Class__ is represented using [`Class`]
//!   - __Method__ is represented using [`Method`]
//!   - __Field__ is represented using [`Field`]
//!
//! In all of these cases, the classes have an `id` field to query the class graph representation.

mod class;
mod field;
mod method;

pub use class::*;
pub use field::*;
pub use method::*;
