use super::{JavaRenamer, Renamer};
use std::panic::AssertUnwindSafe;
use wasmparser::WasmFeatures;

pub struct Settings {
    /// Output class name, written as `my/output/Klass`
    pub output_full_class_name: String,

    /// Name given to the start function
    pub start_function_name: String,

    /// Function name prefix (eg. `func`)
    pub wasm_function_name_prefix: String,

    /// Utilities class name (external for now, possibly generated in the future)
    pub utilities_full_class_name: String,

    /// Inner part class name
    ///
    /// Each part is a nested class which has no fields - just carries a bunch of static functions
    /// and a `<clinit>` that registers those static functions into a static functions array.
    /// The _sole_ purpose of part classes is to support many more functions than can fit in a
    /// single class. The functions array ensures that we can also _call_ many more functions than
    /// would fit in a class constant pool.
    pub part_short_class_name: String,

    /// Field name for arrays of `funcref` tables
    ///
    /// This has type `[[Ljava/lang/invoke/MethodHandle;` with values in the outer array being
    /// `null` whenever the WASM table at that index doesn't have element type `funcref`. The field
    /// itself is `ACC_FINAL` since the number of tables doesn't change at runtime.
    pub funcref_array_table_field_name: String,

    /// Field name for arrays of `externref` tables
    ///
    /// This has type `[[Ljava/lang/Object;` with values in the outer array being `null` whenever
    /// the WASM table at that index doesn't have element type `externref`. The field itself is
    /// `ACC_FINAL` since the number of tables doesn't change at runtime.
    pub externref_array_table_field_name: String,

    /// WASM features
    ///
    /// Note: some features are not supported, so a start with `SUPPORTED_WASM_FEATURES` and then
    /// disable fields you don't want.
    pub wasm_features: WasmFeatures,

    /// How should exports be handled
    pub export_strategy: ExportStrategy,

    /// Renaming strategy for exports
    ///
    /// TODO: remove `AssertUnwindSafe` after we weed out panics that make catching necessary
    pub renamer: AssertUnwindSafe<Box<dyn Renamer>>,
}

impl Settings {
    /// Supported WASM features
    pub const SUPPORTED_WASM_FEATURES: WasmFeatures = WasmFeatures {
        reference_types: true,
        multi_value: true,
        bulk_memory: true,
        module_linking: false,
        simd: false,
        threads: false,
        tail_call: false,
        deterministic_only: true,
        multi_memory: false,
        exceptions: false,
        memory64: false,
    };

    pub fn new(output_full_class_name: String, utilities_full_class_name: String) -> Settings {
        let mut wasm_features = Self::SUPPORTED_WASM_FEATURES;
        wasm_features.deterministic_only = false;

        Settings {
            output_full_class_name,
            start_function_name: String::from("initialize"),
            wasm_function_name_prefix: String::from("func"),
            utilities_full_class_name,
            part_short_class_name: String::from("Part"),
            funcref_array_table_field_name: String::from("funcref_tables"),
            externref_array_table_field_name: String::from("externref_tables"),
            wasm_features,
            export_strategy: ExportStrategy::Members,
            renamer: AssertUnwindSafe(Box::new(JavaRenamer::new())),
        }
    }
}

pub enum ExportStrategy {
    /// Each export as a member, named appropriately
    Members,

    /// Exports packed into `Map<String, ?>`
    Exports,
}
