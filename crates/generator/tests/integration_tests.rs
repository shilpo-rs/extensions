use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

use shilpo_registry_generator::*;

#[test]
fn test_real_extensions_directory_validates() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let extensions_dir = repo_root.join("extensions");
    let owners_file = repo_root.join("owners.toml");

    let report = scan_and_validate(&extensions_dir, &owners_file, None, None).unwrap();
    assert!(report.extensions_count >= 3);
}

#[test]
fn test_script_bundle_in_extensions_is_rejected() {
    let temp = tempdir().unwrap();
    let ext_dir = temp.path().join("extensions");
    let owners_file = temp.path().join("owners.toml");
    fs::create_dir_all(&ext_dir).unwrap();

    fs::write(
        &owners_file,
        r#"
[namespaces]
"org.shilpo" = ["sayeed205"]
"#,
    )
    .unwrap();

    let script_ext = ext_dir.join("org.shilpo.script");
    fs::create_dir_all(&script_ext).unwrap();
    fs::write(
        script_ext.join("extension.toml"),
        r#"
id = "org.shilpo.script"
name = "Script"
version = "0.1.0"
schema_version = 1
authors = ["Sayeed Ahmed <sayeed205@gmail.com>"]

[runtime]
executable = "run.sh"
"#,
    )
    .unwrap();

    let result = scan_and_validate(&ext_dir, &owners_file, Some("sayeed205"), None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("unknown field `runtime`") || err.contains("runtime"));
}

#[test]
fn test_unauthorized_namespace_rejected() {
    let temp = tempdir().unwrap();
    let ext_dir = temp.path().join("extensions");
    let owners_file = temp.path().join("owners.toml");
    fs::create_dir_all(&ext_dir).unwrap();

    fs::write(
        &owners_file,
        r#"
[namespaces]
"org.shilpo" = ["sayeed205"]
"io.github.alice" = ["alice"]
"#,
    )
    .unwrap();

    // Attacker tries to publish to alice's namespace
    let alice_ext = ext_dir.join("io.github.alice.evil");
    fs::create_dir_all(&alice_ext).unwrap();
    fs::write(
        alice_ext.join("extension.toml"),
        r#"
id = "io.github.alice.evil"
name = "Evil"
version = "0.1.0"
schema_version = 1
api_version = "0.1.0"
min_shilpo_version = "0.1.0"
authors = ["Attacker <attacker@example.com>"]
description = "Unauthorized"

[library]
path = "extension.wasm"
"#,
    )
    .unwrap();

    let result = scan_and_validate(&ext_dir, &owners_file, Some("bob"), None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("PR author is 'bob'"));
}

#[test]
fn test_invalid_author_format_rejected() {
    let temp = tempdir().unwrap();
    let ext_dir = temp.path().join("extensions");
    let owners_file = temp.path().join("owners.toml");
    fs::create_dir_all(&ext_dir).unwrap();

    fs::write(
        &owners_file,
        "[namespaces]\n\"org.shilpo\" = [\"sayeed205\"]\n",
    )
    .unwrap();

    let bad_ext = ext_dir.join("org.shilpo.bad");
    fs::create_dir_all(&bad_ext).unwrap();
    fs::write(
        bad_ext.join("extension.toml"),
        r#"
id = "org.shilpo.bad"
name = "Bad"
version = "0.1.0"
schema_version = 1
api_version = "0.1.0"
min_shilpo_version = "0.1.0"
authors = ["Anonymous"]

[library]
path = "extension.wasm"
"#,
    )
    .unwrap();

    let result = scan_and_validate(&ext_dir, &owners_file, Some("sayeed205"), None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("invalid author"));
}

#[test]
fn test_immutability_and_idempotency() {
    let temp = tempdir().unwrap();
    let ext_dir = temp.path().join("extensions");
    let dist_dir = temp.path().join("dist");
    let owners_file = temp.path().join("owners.toml");
    let index_file = temp.path().join("index.json");
    fs::create_dir_all(&ext_dir).unwrap();
    fs::create_dir_all(&dist_dir).unwrap();

    fs::write(
        &owners_file,
        "[namespaces]\n\"org.shilpo\" = [\"sayeed205\"]\n",
    )
    .unwrap();

    let ext = ext_dir.join("org.shilpo.wallpaper");
    fs::create_dir_all(&ext).unwrap();
    fs::write(
        ext.join("extension.toml"),
        r#"
id = "org.shilpo.wallpaper"
name = "Wallpaper"
version = "0.1.0"
schema_version = 1
api_version = "0.1.0"
min_shilpo_version = "0.1.0"
authors = ["Sayeed Ahmed <sayeed205@gmail.com>"]
description = "Wallpaper"

[library]
path = "extension.wasm"
"#,
    )
    .unwrap();

    // Package bytes 1
    let pkg_path = dist_dir.join("org.shilpo.wallpaper-0.1.0.shilpo-ext");
    fs::write(&pkg_path, b"package bytes v1").unwrap();

    let options = GeneratorOptions {
        extensions_dir: ext_dir.clone(),
        dist_dir: Some(dist_dir.clone()),
        owners_path: owners_file.clone(),
        previous_index_path: Some(index_file.clone()),
        ..Default::default()
    };

    let index1 = generate_index(&options).unwrap();
    fs::write(&index_file, serde_json::to_string_pretty(&index1).unwrap()).unwrap();

    // 1. Re-run with identical package -> Idempotent success
    let index2 = generate_index(&options).unwrap();
    assert_eq!(index1.releases, index2.releases);

    // 2. Modify package bytes -> Immutability failure
    fs::write(&pkg_path, b"tampered bytes v2").unwrap();
    let result = generate_index(&options);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("cannot overwrite published release"));
}

#[test]
fn test_schema_drift_check() {
    let temp = tempdir().unwrap();
    let schema_file = temp.path().join("registry-index-v1.schema.json");

    write_schema_file(&schema_file).unwrap();
    assert!(check_schema_drift(&schema_file).is_ok());
}
