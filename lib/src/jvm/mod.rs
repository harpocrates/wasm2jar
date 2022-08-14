//! Manipulate JVM classes
//!
//! ### Simple example
//!
//! Consider the following simple Java class:
//!
//! ```java,ignore,no_run
//! public class Point {
//!     public final int x;
//!     public final int y;
//!
//!     public Point(int x, int y) {
//!         this.x = x;
//!         this.y = y;
//!     }
//! }
//! ```
//!
//! Generating an analogous class file can be done as follows:
//!
//! ```
//! use wasm2jar::jvm::class_graph::*;
//! use wasm2jar::jvm::model::{Method, Class, Field};
//! use wasm2jar::jvm::code::{CodeBuilder, InvokeType, Instruction::*, BranchInstruction::*};
//! use wasm2jar::jvm::class_file::{ClassFile, Serialize, Version};
//! use wasm2jar::jvm::*;
//!
//! # fn generate_class() -> Result<(), Error> {
//! // Setup the class graph, add in Java standard library types
//! let class_graph_arenas = ClassGraphArenas::new();
//! let class_graph = ClassGraph::new(&class_graph_arenas);
//! let java = class_graph.insert_java_library_types();
//!
//! // Declare the class and all its members in the class graph
//! let class = class_graph.add_class(ClassData::new(
//!     BinaryName::from_string(String::from("me/alec/Point")).unwrap(),
//!     java.classes.lang.object,
//!     ClassAccessFlags::PUBLIC,
//!     None,
//! ));
//! let field_x = class_graph.add_field(FieldData {
//!     class,
//!     name: UnqualifiedName::from_string(String::from("x")).unwrap(),
//!     descriptor: FieldType::int(),
//!     access_flags: FieldAccessFlags::PUBLIC,
//! });
//! let field_y = class_graph.add_field(FieldData {
//!     class,
//!     name: UnqualifiedName::from_string(String::from("y")).unwrap(),
//!     descriptor: FieldType::int(),
//!     access_flags: FieldAccessFlags::PUBLIC,
//! });
//! let constructor = class_graph.add_method(MethodData {
//!     class,
//!     name: UnqualifiedName::INIT,
//!     descriptor: MethodDescriptor {
//!         parameters: vec![FieldType::int(), FieldType::int()],
//!         return_type: None,
//!     },
//!     access_flags: MethodAccessFlags::PUBLIC,
//! });
//!
//! // Make the class
//! let mut class = Class::new(class);
//!
//! // Add the fields to the class
//! class.add_field(Field::new(field_x));
//! class.add_field(Field::new(field_y));
//!
//! // Generate the constructor method body
//! let mut code = CodeBuilder::new(&class_graph, &java, constructor);
//! code.push_instruction(ALoad(0))?;
//! code.push_instruction(Invoke(InvokeType::Special, java.members.lang.object.init))?;
//! code.push_instruction(ALoad(0))?;
//! code.push_instruction(ILoad(1))?;
//! code.push_instruction(PutField(field_x))?;
//! code.push_instruction(ALoad(0))?;
//! code.push_instruction(ILoad(2))?;
//! code.push_instruction(PutField(field_y))?;
//! code.push_branch_instruction(Return)?;
//!
//! // Add the constructor method to the class
//! let mut constructor = Method::new(constructor);
//! constructor.code_impl = Some(code.result()?);
//! class.add_method(constructor);
//!
//! // Finally, encode the class into bytes
//! let class_file: ClassFile = class.serialize(Version::JAVA11)?;
//! let mut class_bytes: Vec<u8> = vec![];
//! class_file.serialize(&mut class_bytes).map_err(Error::IoError)?;
//! # Ok(())
//! # }
//! ```

mod access_flags;
pub mod class_file;
pub mod class_graph;
pub mod code;
mod descriptors;
mod errors;
pub mod model;
mod names;
pub mod verifier;

pub use access_flags::*;
pub use descriptors::*;
pub use errors::*;
pub use names::*;
