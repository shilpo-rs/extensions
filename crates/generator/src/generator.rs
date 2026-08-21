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
    /// Restrict the scan to a single extension directory name. Used by the per-extension
    /// tag-triggered release workflow: a release only ever builds and re-signs the one
    /// extension whose tag was pushed, merging that single new release into the existing
    /// signed index rather than re-scanning (and re-building) every extension in the repo.
    pub only_id: Option<String>,
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
            only_id: None,
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
    changed_dirs: Option<&HashSet<String>>,
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

        // 2. Validate namespace ownership — but only for extensions this PR actually changed.
        // Pre-existing, untouched extensions were already vetted when they were originally
        // merged; re-checking them against the current PR's author would reject every PR that
        // doesn't happen to touch every namespace already in the repo. `changed_dirs: None`
        // means "no diff information available" (e.g. a direct/manual invocation), in which
        // case every extension is checked, matching the previous behavior.
        let should_check_ownership =
            changed_dirs.map_or(true, |changed| changed.contains(dir_name));
        if should_check_ownership {
            owners.verify_ownership(&manifest.id, pr_author)?;
        }

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

        if options
            .only_id
            .as_deref()
            .is_some_and(|id| id != dir_name)
        {
            continue;
        }

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

    if let Some(id) = &options.only_id {
        if scanned_releases.is_empty() {
            return Err(format!(
                "only_id '{id}' matched no directory under '{}'",
                options.extensions_dir.display()
            ));
        }
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

pub fn pack_extensions(
    extensions_dir: &Path,
    wasm_target_dir: &Path,
    output_dir: &Path,
    only_id: Option<&str>,
) -> Result<Vec<PathBuf>, String> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fs::create_dir_all(output_dir)
        .map_err(|err| format!("failed to create output directory '{}': {err}", output_dir.display()))?;

    let entries = fs::read_dir(extensions_dir)
        .map_err(|err| format!("failed to read extensions dir '{}': {err}", extensions_dir.display()))?;

    let mut packed = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read entry: {err}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("invalid directory name at '{}'", path.display()))?;

        if only_id.is_some_and(|id| id != dir_name) {
            continue;
        }

        let manifest = parse_and_validate_manifest(&path, Some(dir_name))?;

        let archive_name = format!("{}-{}.shilpo-ext", manifest.id, manifest.version);
        let archive_path = output_dir.join(&archive_name);

        let file = fs::File::create(&archive_path)
            .map_err(|err| format!("failed to create archive file '{}': {err}", archive_path.display()))?;
        let enc = GzEncoder::new(file, Compression::default());
        let mut tar = Builder::new(enc);

        // 1. Add extension.toml
        tar.append_path_with_name(path.join("extension.toml"), "extension.toml")
            .map_err(|err| format!("failed to append extension.toml: {err}"))?;

        // 2. Add WASM binary if declared in manifest.library
        if let Some(library) = &manifest.library {
            let wasm_file = find_wasm_binary(&path, wasm_target_dir)?;
            tar.append_path_with_name(&wasm_file, &library.path)
                .map_err(|err| format!("failed to append {}: {err}", library.path))?;
        }

        // 3. Add settings.schema.json if exists
        let settings = path.join("settings.schema.json");
        if settings.is_file() {
            tar.append_path_with_name(&settings, "settings.schema.json")
                .map_err(|err| format!("failed to append settings.schema.json: {err}"))?;
        }

        // 4. Add assets/ if exists
        let assets = path.join("assets");
        if assets.is_dir() {
            tar.append_dir_all("assets", &assets)
                .map_err(|err| format!("failed to append assets: {err}"))?;
        }

        // 5. Add README.md if exists
        let readme = path.join("README.md");
        if readme.is_file() {
            tar.append_path_with_name(&readme, "README.md")
                .map_err(|err| format!("failed to append README.md: {err}"))?;
        }

        // 6. Add LICENSE if exists
        let license = path.join("LICENSE");
        if license.is_file() {
            tar.append_path_with_name(&license, "LICENSE")
                .map_err(|err| format!("failed to append LICENSE: {err}"))?;
        }

        tar.into_inner()
            .map_err(|err| format!("failed to finalize tar archive: {err}"))?
            .finish()
            .map_err(|err| format!("failed to finish gzip compression: {err}"))?;

        packed.push(archive_path);
    }

    if let Some(id) = only_id {
        if packed.is_empty() {
            return Err(format!(
                "--only '{id}' matched no directory under '{}'",
                extensions_dir.display()
            ));
        }
    }

    Ok(packed)
}

/// Resolves the exact WASM component belonging to one extension. Never guesses: either it
/// finds the binary this specific extension actually produced, or it fails loudly. A
/// previous version of this function fell back to scanning `wasm_target_dir` for any
/// `.wasm` file whose name loosely matched (e.g. containing the literal substring
/// "extension"), which every Rust-built extension's filename does — so a non-Rust
/// extension with no resolvable binary silently got packaged with whichever unrelated
/// extension's binary happened to appear first in that directory listing, signed and
/// published as if it were its own.
fn find_wasm_binary(ext_dir: &Path, wasm_target_dir: &Path) -> Result<PathBuf, String> {
    let local_wasm = ext_dir.join("extension.wasm");
    if local_wasm.is_file() {
        return Ok(local_wasm);
    }

    let cargo_toml = ext_dir.join("Cargo.toml");
    if cargo_toml.is_file() {
        let content = fs::read_to_string(&cargo_toml)
            .map_err(|err| format!("failed to read Cargo.toml in '{}': {err}", ext_dir.display()))?;
        let toml_val: toml::Value = toml::from_str(&content)
            .map_err(|err| format!("failed to parse Cargo.toml in '{}': {err}", ext_dir.display()))?;
        let pkg_name = toml_val
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .ok_or_else(|| {
                format!(
                    "Cargo.toml in '{}' has no [package].name to resolve a WASM binary from",
                    ext_dir.display()
                )
            })?;
        let wasm_name = format!("{}.wasm", pkg_name.replace('-', "_"));
        let candidate = wasm_target_dir.join(&wasm_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "extension '{}' declares a Cargo package '{pkg_name}', but '{}' was not found in '{}' \
             — did the build step run and produce this exact filename?",
            ext_dir.display(),
            wasm_name,
            wasm_target_dir.display()
        ));
    }

    Err(format!(
        "extension '{}' has no local extension.wasm and no Cargo.toml, so there is no \
         deterministic way to resolve its WASM binary. Non-Rust extensions must either commit \
         a pre-built extension.wasm, or the pipeline must gain a real build step for their \
         toolchain before they can be scanned here.",
        ext_dir.display()
    ))
}

pub fn sign_index_and_packages(
    unsigned_index: RegistryIndex,
    dist_dir: Option<&Path>,
    package_signing_key: Option<&str>,
    index_signing_key: &str,
) -> Result<SignedRegistryIndex, String> {
    use shilpo_registry_contract::{sign_package, sign_release, sign_registry_index};

    let mut index = unsigned_index;

    if let Some(pkg_key) = package_signing_key {
        for release in &mut index.releases {
            if let Some(dist) = dist_dir {
                let pkg_name = format!("{}-{}.shilpo-ext", release.id, release.version);
                let pkg_path = dist.join(&pkg_name);
                if pkg_path.is_file() {
                    sign_package(&pkg_path, &release.publisher, pkg_key)
                        .map_err(|err| format!("failed to sign package '{}': {err}", pkg_path.display()))?;
                }
            }
            sign_release(release, pkg_key)
                .map_err(|err| format!("failed to sign release '{}': {err}", release.id))?;
        }
    }

    sign_registry_index(index, index_signing_key)
        .map_err(|err| format!("failed to sign registry index: {err}"))
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
        let report = scan_and_validate(&ext_dir, &owners_path, Some("sayeed205"), None, None).unwrap();
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
