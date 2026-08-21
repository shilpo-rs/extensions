use std::fs;
use std::path::Path;

use schemars::schema_for;
use shilpo_registry_contract::SignedRegistryIndex;

pub fn generate_schema() -> Result<String, String> {
    let schema = schema_for!(SignedRegistryIndex);
    serde_json::to_string_pretty(&schema)
        .map_err(|err| format!("failed to serialize generated schema: {err}"))
}

pub fn check_schema_drift(schema_path: &Path) -> Result<(), String> {
    if !schema_path.exists() {
        return Err(format!(
            "schema file '{}' does not exist; run schema generation first",
            schema_path.display()
        ));
    }

    let existing = fs::read_to_string(schema_path).map_err(|err| {
        format!(
            "failed to read schema at '{}': {err}",
            schema_path.display()
        )
    })?;

    let generated = generate_schema()?;

    let existing_val: serde_json::Value = serde_json::from_str(&existing)
        .map_err(|err| format!("existing schema is invalid JSON: {err}"))?;
    let generated_val: serde_json::Value = serde_json::from_str(&generated)
        .map_err(|err| format!("generated schema is invalid JSON: {err}"))?;

    if existing_val != generated_val {
        return Err(format!(
            "schema drift detected in '{}'! The checked-in schema differs from canonical contract types. Run with `--emit-schema` to update.",
            schema_path.display()
        ));
    }

    Ok(())
}

pub fn write_schema_file(schema_path: &Path) -> Result<(), String> {
    let schema_json = generate_schema()?;
    if let Some(parent) = schema_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create schema parent dir: {err}"))?;
    }
    fs::write(schema_path, format!("{schema_json}\n")).map_err(|err| {
        format!(
            "failed to write schema to '{}': {err}",
            schema_path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_and_check_schema() {
        let dir = tempdir().unwrap();
        let schema_path = dir.path().join("registry-index-v1.schema.json");

        write_schema_file(&schema_path).unwrap();
        assert!(check_schema_drift(&schema_path).is_ok());

        // Modify file to test drift detection
        fs::write(&schema_path, "{}").unwrap();
        assert!(check_schema_drift(&schema_path).is_err());
    }
}
