use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use semver::Version;
use shilpo_ext_api::{Capability, ExtensionManifest};
use shilpo_registry_contract::{
    OFFICIAL_SOURCE_ID, REGISTRY_SCHEMA_VERSION, RegistryIndex, RegistryRelease, ReleaseChannel,
    SignedRegistryIndex, capabilities_hash, hash_file,
};

use crate::manifest::{is_official_extension, parse_and_validate_manifest};
use crate::owners::OwnersConfig;

#[derive(Clone, Debug)]
pub struct GeneratorOptions {
    pub extensions_dir: PathBuf,
    pub dist_dir: Option<PathBuf>,
    pub owners_path: PathBuf,
    pub previous_index_path: Option<PathBuf>,
    pub base_url: String,
    pub source_id: String,
    pub commit_timestamp: Option<String>,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        Self {
            extensions_dir: PathBuf::from("extensions"),
            dist_dir: None,
            owners_path: PathBuf::from("owners.toml"),
            previous_index_path: None,
            base_url: "https://github.com/shilpo-rs/extensions/releases/download".into(),
            source_id: OFFICIAL_SOURCE_ID.into(),
            commit_timestamp: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct ValidationReport {
    pub extensions_count: usize,
    pub warnings: Vec<String>,
    pub capability_additions: Vec<String>,
}

pub fn scan_and_validate(
    extensions_dir: &Path,
    owners_path: &Path,
    pr_author: Option<&str>,
    base_index_path: Option<&Path>,
) -> Result<ValidationReport, String> {
    if !extensions_dir.is_dir() {
        return Err(format!(
            "extensions directory '{}' not found",
            extensions_dir.display()
        ));
    }

    let owners = OwnersConfig::load_from_file(owners_path)?;
    let mut report = ValidationReport::default();
    let mut scanned_ids = HashSet::new();

    let entries = fs::read_dir(extensions_dir).map_err(|err| {
        format!(
            "failed to read extensions directory '{}': {err}",
            extensions_dir.display()
        )
    })?;

    let mut scanned_manifests = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("invalid directory name at '{}'", path.display()))?;

        // 1. Validate manifest
        let manifest = parse_and_validate_manifest(&path, Some(dir_name))?;

        // 2. Validate namespace ownership
        owners.verify_ownership(&manifest.id, pr_author)?;

        if !scanned_ids.insert(manifest.id.clone()) {
            return Err(format!(
                "duplicate extension ID declared: '{}'",
                manifest.id
            ));
        }

        report.extensions_count += 1;
        scanned_manifests.push(manifest);
    }

    // 3. Capability addition diff check against base_index if provided
    if let Some(base_path) = base_index_path {
        if base_path.is_file() {
            let base_index = load_index_file(base_path)?;
            for manifest in &scanned_manifests {
                check_capability_additions(manifest, &base_index, &mut report);
            }
        }
    }

    Ok(report)
}

fn check_capability_additions(
    manifest: &ExtensionManifest,
    base_index: &RegistryIndex,
    report: &mut ValidationReport,
) {
    let mut previous_releases: Vec<&RegistryRelease> = base_index
        .releases
        .iter()
        .filter(|r| r.id == manifest.id)
        .collect();

    if previous_releases.is_empty() {
        return;
    }

    previous_releases.sort_by(|a, b| b.version.cmp(&a.version));
    let latest_prev = previous_releases[0];

    let new_caps: Vec<&Capability> = manifest
        .capabilities
        .iter()
        .filter(|c| !latest_prev.capabilities.contains(c))
        .collect();

    if !new_caps.is_empty() {
        report.capability_additions.push(format!(
            "Extension '{}' version {} adds new capabilities not in latest release {}: {:?}",
            manifest.id, manifest.version, latest_prev.version, new_caps
        ));
    }
}

pub fn generate_index(options: &GeneratorOptions) -> Result<RegistryIndex, String> {
    let owners = OwnersConfig::load_from_file(&options.owners_path)?;
    let mut scanned_releases = Vec::new();

    let entries = fs::read_dir(&options.extensions_dir).map_err(|err| {
        format!(
            "failed to read extensions directory '{}': {err}",
            options.extensions_dir.display()
        )
    })?;

    let published_at = options
        .commit_timestamp
        .clone()
        .unwrap_or_else(|| Utc::now().to_rfc3339());

    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("invalid directory name at '{}'", path.display()))?;

        let manifest = parse_and_validate_manifest(&path, Some(dir_name))?;
        owners.verify_ownership(&manifest.id, None)?;

        let package_filename = format!("{}-{}.shilpo-ext", manifest.id, manifest.version);
        let (package_hash, package_url) = if let Some(dist_dir) = &options.dist_dir {
            let pkg_path = dist_dir.join(&package_filename);
            let fallback_pkg_path = dist_dir.join(manifest.id.as_str()).join(&package_filename);
            let final_pkg_path = if pkg_path.is_file() {
                pkg_path
            } else if fallback_pkg_path.is_file() {
                fallback_pkg_path
            } else {
                return Err(format!(
                    "package archive '{}' not found in dist directory '{}'",
                    package_filename,
                    dist_dir.display()
                ));
            };

            let hash = hash_file(&final_pkg_path).map_err(|err| {
                format!(
                    "failed to hash package '{}': {err}",
                    final_pkg_path.display()
                )
            })?;
            let url = format!(
                "{}/{}-v{}/{}",
                options.base_url.trim_end_matches('/'),
                manifest.id,
                manifest.version,
                package_filename
            );
            (hash, url)
        } else {
            // Unsigned/dry-run placeholder
            (
                "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
                format!(
                    "{}/{}-v{}/{}",
                    options.base_url.trim_end_matches('/'),
                    manifest.id,
                    manifest.version,
                    package_filename
                ),
            )
        };

        let cap_hash = capabilities_hash(&manifest.capabilities)
            .map_err(|err| format!("failed to compute capabilities hash: {err}"))?;

        let official = is_official_extension(&manifest);
        let publisher = manifest
            .authors
            .first()
            .cloned()
            .unwrap_or_else(|| "Shilpo Contributor".into());

        let release = RegistryRelease {
            id: manifest.id,
            name: manifest.name,
            description: manifest.description,
            publisher,
            version: manifest.version,
            api_version: manifest.api_version,
            min_shilpo_version: manifest.min_shilpo_version,
            channel: ReleaseChannel::Stable,
            package_url,
            package_hash,
            publisher_public_key: String::new(),
            publisher_signature: String::new(),
            capabilities_hash: cap_hash,
            capabilities: manifest.capabilities,
            published_at: published_at.clone(),
            yanked: false,
            official,
            verified_publisher: true,
            open_source: true,
            data_only: manifest.library.is_none(),
            key_rotation: None,
        };

        scanned_releases.push(release);
    }

    // Merge with previous index releases
    let mut all_releases_map: BTreeMap<(String, Version), RegistryRelease> = BTreeMap::new();

    if let Some(prev_path) = &options.previous_index_path {
        if prev_path.is_file() {
            let prev_index = load_index_file(prev_path)?;
            for release in prev_index.releases {
                let key = (release.id.to_string(), release.version.clone());
                all_releases_map.insert(key, release);
            }
        }
    }

    for new_release in scanned_releases {
        let key = (new_release.id.to_string(), new_release.version.clone());
        if let Some(existing) = all_releases_map.get(&key) {
            // Immutability check: if options.dist_dir is present and hash differs, error!
            if options.dist_dir.is_some() && existing.package_hash != new_release.package_hash {
                return Err(format!(
                    "cannot overwrite published release '{}' v{}! Existing hash is {}, newly built hash is {}.",
                    new_release.id,
                    new_release.version,
                    existing.package_hash,
                    new_release.package_hash
                ));
            }
            // Idempotent: preserve existing release with original signatures and timestamps
            continue;
        }
        all_releases_map.insert(key, new_release);
    }

    let mut releases: Vec<RegistryRelease> = all_releases_map.into_values().collect();
    // Sort releases by ID, then descending version
    releases.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| b.version.cmp(&a.version)));

    Ok(RegistryIndex {
        schema_version: REGISTRY_SCHEMA_VERSION,
        source_id: options.source_id.clone(),
        generated_at: published_at,
        releases,
    })
}

pub fn load_index_file(path: &Path) -> Result<RegistryIndex, String> {
    let content = fs::read_to_string(path)
        .map_err(|err| format!("failed to read index file at '{}': {err}", path.display()))?;

    // Try parsing as SignedRegistryIndex first
    if let Ok(signed) = serde_json::from_str::<SignedRegistryIndex>(&content) {
        return Ok(signed.index);
    }

    // Fall back to plain RegistryIndex
    serde_json::from_str::<RegistryIndex>(&content).map_err(|err| {
        format!(
            "failed to parse registry index at '{}': {err}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempdir().unwrap();
        let ext_dir = root.path().join("extensions");
        let owners_path = root.path().join("owners.toml");
        fs::create_dir_all(&ext_dir).unwrap();

        fs::write(
            &owners_path,
            r#"
[namespaces]
"org.shilpo" = ["sayeed205"]
"io.github.alice" = ["alice"]
"#,
        )
        .unwrap();

        let wallpaper = ext_dir.join("org.shilpo.wallpaper");
        fs::create_dir_all(&wallpaper).unwrap();
        fs::write(
            wallpaper.join("extension.toml"),
            r#"
id = "org.shilpo.wallpaper"
name = "Wallpaper"
version = "0.1.0"
schema_version = 1
api_version = "0.1.0"
min_shilpo_version = "0.1.0"
authors = ["Sayeed Ahmed <sayeed205@gmail.com>"]
description = "Wallpaper extension"

[library]
path = "extension.wasm"
"#,
        )
        .unwrap();

        (root, ext_dir, owners_path)
    }

    #[test]
    fn test_scan_and_validate_success() {
        let (_root, ext_dir, owners_path) = setup_fixture();
        let report = scan_and_validate(&ext_dir, &owners_path, Some("sayeed205"), None).unwrap();
        assert_eq!(report.extensions_count, 1);
    }

    #[test]
    fn test_generate_index_success() {
        let (_root, ext_dir, owners_path) = setup_fixture();
        let options = GeneratorOptions {
            extensions_dir: ext_dir,
            owners_path,
            ..Default::default()
        };

        let index = generate_index(&options).unwrap();
        assert_eq!(index.releases.len(), 1);
        assert_eq!(index.releases[0].id.as_str(), "org.shilpo.wallpaper");
        assert!(index.releases[0].official);
    }
}
