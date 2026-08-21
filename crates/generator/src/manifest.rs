use std::fs;
use std::path::Path;

use shilpo_ext_api::{ExtensionManifest, OFFICIAL_AUTHOR, validate_author};

pub fn parse_and_validate_manifest(
    dir: &Path,
    expected_id: Option<&str>,
) -> Result<ExtensionManifest, String> {
    let manifest_path = dir.join("extension.toml");
    if !manifest_path.is_file() {
        return Err(format!(
            "extension directory '{}' is missing a valid 'extension.toml' file",
            dir.display()
        ));
    }

    let content = fs::read_to_string(&manifest_path).map_err(|err| {
        format!(
            "failed to read manifest at '{}': {err}",
            manifest_path.display()
        )
    })?;

    let manifest: ExtensionManifest = toml::from_str(&content).map_err(|err| {
        format!(
            "failed to parse manifest at '{}': {err}",
            manifest_path.display()
        )
    })?;

    if let Some(expected) = expected_id {
        if manifest.id.as_str() != expected {
            return Err(format!(
                "manifest id '{}' in '{}' does not match directory name '{}'",
                manifest.id,
                manifest_path.display(),
                expected
            ));
        }
    }

    if manifest.authors.is_empty() {
        return Err(format!(
            "manifest '{}' must specify at least one author in mailbox format",
            manifest_path.display()
        ));
    }

    for author in &manifest.authors {
        validate_author(author).map_err(|err| {
            format!(
                "invalid author '{author}' in '{}': {err}",
                manifest_path.display()
            )
        })?;
    }

    Ok(manifest)
}

pub fn is_official_extension(manifest: &ExtensionManifest) -> bool {
    manifest.id.as_str().starts_with("org.shilpo.")
        && manifest.authors.iter().any(|author| {
            author.trim() == OFFICIAL_AUTHOR
                || author.trim() == "Sayeed Ahmed <sayeed205@gmail.com>"
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_valid_manifest() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("extension.toml");
        fs::write(
            &manifest_path,
            r#"
id = "org.shilpo.wallpaper"
name = "Wallpaper"
version = "0.1.0"
schema_version = 1
api_version = "0.1.0"
min_shilpo_version = "0.1.0"
authors = ["Sayeed Ahmed <sayeed205@gmail.com>"]
description = "Official host-supervised wallpaper provider"

[library]
path = "extension.wasm"
"#,
        )
        .unwrap();

        let manifest =
            parse_and_validate_manifest(dir.path(), Some("org.shilpo.wallpaper")).unwrap();
        assert_eq!(manifest.id.as_str(), "org.shilpo.wallpaper");
        assert!(is_official_extension(&manifest));
    }

    #[test]
    fn test_script_bundle_rejected() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("extension.toml");
        fs::write(
            &manifest_path,
            r#"
id = "local.script.test"
name = "Test Script"
version = "0.1.0"
schema_version = 1
authors = ["Alice <alice@example.com>"]

[runtime]
executable = "script.sh"
"#,
        )
        .unwrap();

        assert!(parse_and_validate_manifest(dir.path(), None).is_err());
    }
}
