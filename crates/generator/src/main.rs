use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod generator;
mod manifest;
mod owners;
mod schema;

use generator::{GeneratorOptions, generate_index, scan_and_validate};
use schema::{check_schema_drift, write_schema_file};

#[derive(Parser, Debug)]
#[command(name = "shilpo-registry-generator")]
#[command(about = "Official index generator and validator for the Shilpo Extension Registry")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Validates extensions, manifests, namespace ownership, and schema drift
    Validate {
        #[arg(long, default_value = "extensions")]
        extensions_dir: PathBuf,

        #[arg(long, default_value = "owners.toml")]
        owners_file: PathBuf,

        #[arg(long)]
        pr_author: Option<String>,

        #[arg(long)]
        base_index: Option<PathBuf>,

        #[arg(long, default_value = "schema/registry-index-v1.schema.json")]
        schema_file: PathBuf,
    },

    /// Generates canonical unsigned index.json from scanned extensions
    BuildIndex {
        #[arg(long, default_value = "extensions")]
        extensions_dir: PathBuf,

        #[arg(long)]
        dist_dir: Option<PathBuf>,

        #[arg(long, default_value = "owners.toml")]
        owners_file: PathBuf,

        #[arg(long)]
        previous_index: Option<PathBuf>,

        #[arg(
            long,
            default_value = "https://github.com/shilpo-rs/extensions/releases/download"
        )]
        base_url: String,

        #[arg(long)]
        output: Option<PathBuf>,

        #[arg(long)]
        commit_timestamp: Option<String>,
    },

    /// Emits JSON Schema from canonical contract types
    EmitSchema {
        #[arg(long, default_value = "schema/registry-index-v1.schema.json")]
        output: PathBuf,
    },

    /// Checks if checked-in JSON Schema matches canonical contract types
    CheckSchema {
        #[arg(long, default_value = "schema/registry-index-v1.schema.json")]
        schema_file: PathBuf,
    },

    /// Packs extensions into .shilpo-ext archives
    Pack {
        #[arg(long, default_value = "extensions")]
        extensions_dir: PathBuf,

        #[arg(long, default_value = "target/wasm32-wasip2/release")]
        target_dir: PathBuf,

        #[arg(long, default_value = "dist")]
        output_dir: PathBuf,
    },

    /// Signs index and packages using Ed25519 private keys
    Sign {
        #[arg(long)]
        unsigned_index: PathBuf,

        #[arg(long)]
        dist_dir: Option<PathBuf>,

        #[arg(long, env = "INDEX_SIGNING_KEY")]
        index_signing_key: String,

        #[arg(long, env = "PACKAGE_SIGNING_KEY")]
        package_signing_key: Option<String>,

        #[arg(long, default_value = "index.json")]
        output: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Validate {
            extensions_dir,
            owners_file,
            pr_author,
            base_index,
            schema_file,
        } => {
            println!(
                "🔍 Validating extensions in '{}'...",
                extensions_dir.display()
            );
            match scan_and_validate(
                &extensions_dir,
                &owners_file,
                pr_author.as_deref(),
                base_index.as_deref(),
            ) {
                Ok(report) => {
                    println!(
                        "✅ Successfully validated {} extensions.",
                        report.extensions_count
                    );
                    for warning in report.warnings {
                        println!("⚠️  {warning}");
                    }
                    for addition in report.capability_additions {
                        println!("ℹ️  [CAPABILITY_ADDITION] {addition}");
                    }
                }
                Err(err) => {
                    eprintln!("❌ Validation error: {err}");
                    return ExitCode::FAILURE;
                }
            }

            if schema_file.exists() {
                println!(
                    "🔍 Checking schema drift against '{}'...",
                    schema_file.display()
                );
                if let Err(err) = check_schema_drift(&schema_file) {
                    eprintln!("❌ Schema drift error: {err}");
                    return ExitCode::FAILURE;
                }
                println!("✅ Schema is in sync.");
            }

            ExitCode::SUCCESS
        }

        Commands::BuildIndex {
            extensions_dir,
            dist_dir,
            owners_file,
            previous_index,
            base_url,
            output,
            commit_timestamp,
        } => {
            let options = GeneratorOptions {
                extensions_dir,
                dist_dir,
                owners_path: owners_file,
                previous_index_path: previous_index,
                base_url,
                commit_timestamp,
                ..Default::default()
            };

            match generate_index(&options) {
                Ok(index) => {
                    let json = match serde_json::to_string_pretty(&index) {
                        Ok(json) => json,
                        Err(err) => {
                            eprintln!("❌ JSON serialization error: {err}");
                            return ExitCode::FAILURE;
                        }
                    };

                    if let Some(out_path) = output {
                        if let Some(parent) = out_path.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        if let Err(err) = fs::write(&out_path, format!("{json}\n")) {
                            eprintln!(
                                "❌ Failed to write output file '{}': {err}",
                                out_path.display()
                            );
                            return ExitCode::FAILURE;
                        }
                        println!(
                            "✅ Successfully generated index with {} releases to '{}'.",
                            index.releases.len(),
                            out_path.display()
                        );
                    } else {
                        println!("{json}");
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("❌ Index generation error: {err}");
                    return ExitCode::FAILURE;
                }
            }
        }

        Commands::EmitSchema { output } => {
            if let Err(err) = write_schema_file(&output) {
                eprintln!("❌ Failed to emit schema: {err}");
                return ExitCode::FAILURE;
            }
            println!("✅ Emitted JSON schema to '{}'.", output.display());
            ExitCode::SUCCESS
        }

        Commands::CheckSchema { schema_file } => {
            if let Err(err) = check_schema_drift(&schema_file) {
                eprintln!("❌ Schema drift error: {err}");
                return ExitCode::FAILURE;
            }
            println!(
                "✅ Schema '{}' is in sync with contract types.",
                schema_file.display()
            );
            ExitCode::SUCCESS
        }

        Commands::Pack {
            extensions_dir,
            target_dir,
            output_dir,
        } => {
            match generator::pack_extensions(&extensions_dir, &target_dir, &output_dir) {
                Ok(packed) => {
                    println!("✅ Successfully packed {} extension(s):", packed.len());
                    for p in packed {
                        println!("   📦 {}", p.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("❌ Packaging error: {err}");
                    return ExitCode::FAILURE;
                }
            }
        }

        Commands::Sign {
            unsigned_index,
            dist_dir,
            index_signing_key,
            package_signing_key,
            output,
        } => {
            let index = match generator::load_index_file(&unsigned_index) {
                Ok(idx) => idx,
                Err(err) => {
                    eprintln!("❌ Failed to load unsigned index '{}': {err}", unsigned_index.display());
                    return ExitCode::FAILURE;
                }
            };

            match generator::sign_index_and_packages(
                index,
                dist_dir.as_deref(),
                package_signing_key.as_deref(),
                &index_signing_key,
            ) {
                Ok(signed) => {
                    let json = match serde_json::to_string_pretty(&signed) {
                        Ok(json) => json,
                        Err(err) => {
                            eprintln!("❌ JSON serialization error: {err}");
                            return ExitCode::FAILURE;
                        }
                    };

                    if let Some(parent) = output.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Err(err) = fs::write(&output, format!("{json}\n")) {
                        eprintln!("❌ Failed to write signed index '{}': {err}", output.display());
                        return ExitCode::FAILURE;
                    }
                    println!("✅ Successfully signed index and written to '{}'.", output.display());
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("❌ Signing error: {err}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
}
