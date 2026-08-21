pub mod generator;
pub mod manifest;
pub mod owners;
pub mod schema;

pub use generator::{GeneratorOptions, ValidationReport, generate_index, scan_and_validate};
pub use manifest::{is_official_extension, parse_and_validate_manifest};
pub use owners::OwnersConfig;
pub use schema::{check_schema_drift, generate_schema, write_schema_file};
