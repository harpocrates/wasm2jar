use super::{Error, JavaRenamer, Renamer};
use crate::jvm::{BinaryName, Name, UnqualifiedName};
use std::panic::AssertUnwindSafe;
use wasmparser::WasmFeatures;

pub struct Settings {
    /// Output class name, written as `my/output/Klass`
    pub output_full_class_name: BinaryName,

    /// Name given to the start function
    pub start_function_name: UnqualifiedName,

    /// Function name prefix (eg. `func`)
    pub wasm_function_name_prefix: UnqualifiedName,

    /// Name for fields associated with imports
    pub wasm_import_name_prefix: UnqualifiedName,

    /// Global name prefix (eg. `global`)
    pub wasm_global_name_prefix: UnqualifiedName,

    /// Table name prefix (eg. `table`)
    pub wasm_table_name_prefix: UnqualifiedName,

    /// Memory name prefix (eg. `memory`)
    pub wasm_memory_name_prefix: UnqualifiedName,

    /// Data name prefix (eg. `data`)
    pub wasm_data_name_prefix: UnqualifiedName,

    /// Data name prefix (eg. `element`)
    pub wasm_element_name_prefix: UnqualifiedName,

    /// Utilities class strategy
    pub utilities_strategy: UtilitiesStrategy,

    /// Inner part class name
    ///
    /// Each part is a nested class which has no fields - just carries a bunch of static functions
    /// and a `<clinit>` that registers those static functions into a static functions array.
    /// The _sole_ purpose of part classes is to support many more functions than can fit in a
    /// single class. The functions array ensures that we can also _call_ many more functions than
    /// would fit in a class constant pool.
    pub part_short_class_name: UnqualifiedName,

    /// Field name for arrays of `funcref` tables
    ///
    /// This has type `[[Ljava/lang/invoke/MethodHandle;` with values in the outer array being
    /// `null` whenever the WASM table at that index doesn't have element type `funcref`. The field
    /// itself is `ACC_FINAL` since the number of tables doesn't change at runtime.
    pub funcref_array_table_field_name: UnqualifiedName,

    /// Field name for arrays of `externref` tables
    ///
    /// This has type `[[Ljava/lang/Object;` with values in the outer array being `null` whenever
    /// the WASM table at that index doesn't have element type `externref`. The field itself is
    /// `ACC_FINAL` since the number of tables doesn't change at runtime.
    pub externref_array_table_field_name: UnqualifiedName,

    /// WASM features
    ///
    /// Note: some features are not supported, so a start with `SUPPORTED_WASM_FEATURES` and then
    /// disable fields you don't want.
    pub wasm_features: WasmFeatures,

    /// How should exports be handled
    pub export_strategy: ExportStrategy,

    /// Trap on division of `i32` and `i64` minimum values by -1
    ///
    /// This is an edge case. WASM dictates a trap (since technically there has been an overflow)
    /// while the `idiv` and `ldiv` instructions just return the overflowed value.
    pub trap_integer_division_overflow: bool,

    /// Make absolute value bitwise on NaN
    ///
    /// This is an edge case. WASM dictates that `f32.abs` and `f64.abs` should negate event the
    /// sign of NaN, contrary to the behaviour of Java's `Math.abs`.
    pub bitwise_floating_abs: bool,

    /// Renaming strategy for exports
    ///
    /// TODO: remove `AssertUnwindSafe` after we weed out panics that make catching necessary
    pub renamer: AssertUnwindSafe<Box<dyn Renamer>>,

    /// Should extra public (non-static) methods be generated for exported functions? If so, the
    /// renamer is how those method names will be determined.
    pub methods_for_function_exports: bool,
}

/// Strategy for handling utility functions
pub enum UtilitiesStrategy {
    GenerateNested {
        inner_class: UnqualifiedName,

        /// Generate all utilities (even if they aren't used)
        generate_all: bool,
    },
    ReferenceExisting(BinaryName),
}

// TODO: add a method to validadte that the settings are all possible (eg. the names are valid in
// the JVM)
impl Settings {
    /// Supported WASM features
    pub const SUPPORTED_WASM_FEATURES: WasmFeatures = WasmFeatures {
        mutable_global: true,
        saturating_float_to_int: true,
        sign_extension: true,
        reference_types: true,
        multi_value: true,
        bulk_memory: true,
        simd: false,
        relaxed_simd: false,
        threads: false,
        tail_call: false,
        deterministic_only: true,
        multi_memory: false,
        exceptions: false,
        memory64: false,
        extended_const: false,
        component_model: false,
    };

    pub fn new(
        output_full_class_name: &str,
        external_utilities: Option<&str>,
    ) -> Result<Settings, Error> {
        let mut wasm_features = Self::SUPPORTED_WASM_FEATURES;
        wasm_features.deterministic_only = false;

        fn make_name<N: Name>(name: impl Into<String>) -> Result<N, Error> {
            N::from_string(name.into()).map_err(Error::MalformedName)
        }

        let utilities_strategy = if let Some(external_name) = external_utilities {
            UtilitiesStrategy::ReferenceExisting(make_name(external_name)?)
        } else {
            UtilitiesStrategy::GenerateNested {
                inner_class: make_name("Utils")?,
                generate_all: false,
            }
        };

        Ok(Settings {
            output_full_class_name: make_name(output_full_class_name)?,
            start_function_name: make_name("initialize")?,
            wasm_function_name_prefix: make_name("func")?,
            wasm_import_name_prefix: make_name("imprt")?,
            wasm_global_name_prefix: make_name("global")?,
            wasm_table_name_prefix: make_name("table")?,
            wasm_memory_name_prefix: make_name("memory")?,
            wasm_data_name_prefix: make_name("data")?,
            wasm_element_name_prefix: make_name("element")?,
            utilities_strategy,
            part_short_class_name: make_name("Part")?,
            funcref_array_table_field_name: make_name("funcref_tables")?,
            externref_array_table_field_name: make_name("externref_tables")?,
            wasm_features,
            export_strategy: ExportStrategy::Members,
            trap_integer_division_overflow: true,
            bitwise_floating_abs: true,
            renamer: AssertUnwindSafe(Box::new(JavaRenamer::new())),
            methods_for_function_exports: true,
        })
    }

    pub fn wasm_function_name(&self, func_idx: usize) -> UnqualifiedName {
        self.wasm_function_name_prefix
            .concat(&UnqualifiedName::number(func_idx))
    }

    /// Name given to the `MethodHandle` fields that store imported functions
    pub fn wasm_import_name(&self, import_idx: usize) -> UnqualifiedName {
        self.wasm_import_name_prefix
            .concat(&UnqualifiedName::number(import_idx))
    }

    pub fn wasm_global_name(&self, import_idx: usize) -> UnqualifiedName {
        self.wasm_global_name_prefix
            .concat(&UnqualifiedName::number(import_idx))
    }

    pub fn wasm_memory_name(&self, memory_idx: usize) -> UnqualifiedName {
        self.wasm_memory_name_prefix
            .concat(&UnqualifiedName::number(memory_idx))
    }

    pub fn wasm_table_name(&self, import_idx: usize) -> UnqualifiedName {
        self.wasm_table_name_prefix
            .concat(&UnqualifiedName::number(import_idx))
    }

    pub fn wasm_data_getter_name(&self, data_idx: usize) -> UnqualifiedName {
        UnqualifiedName::GET
            .concat(&UnqualifiedName::UNDERSCORE)
            .concat(&self.wasm_data_name_prefix)
            .concat(&UnqualifiedName::number(data_idx))
    }

    pub fn wasm_data_name(&self, data_idx: usize) -> UnqualifiedName {
        self.wasm_data_name_prefix
            .concat(&UnqualifiedName::number(data_idx))
    }

    pub fn wasm_element_getter_name(&self, element_idx: usize) -> UnqualifiedName {
        UnqualifiedName::GET
            .concat(&UnqualifiedName::UNDERSCORE)
            .concat(&self.wasm_element_name_prefix)
            .concat(&UnqualifiedName::number(element_idx))
    }

    pub fn wasm_element_name(&self, element_idx: usize) -> UnqualifiedName {
        self.wasm_element_name_prefix
            .concat(&UnqualifiedName::number(element_idx))
    }
}

pub enum ExportStrategy {
    /// Each export as a member, named appropriately
    Members,

    /// Exports packed into `Map<String, ?>`
    Exports,
}
