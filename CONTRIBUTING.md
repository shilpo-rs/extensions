# Contributing to the Shilpo Extension Registry

Welcome! The `shilpo-rs/extensions` repository is the official extension registry for the [Shilpo Desktop Environment](https://github.com/shilpo-rs/shilpo).

---

## 1. Extension Publication Model

The Shilpo extension registry operates as a **centralized build authority**:

- **Submit Source Only**: Extensions are submitted as source code under `extensions/<extension-id>/`. Pre-compiled binaries (`.wasm` or `.shilpo-ext` files) are never accepted.
- **CI Build Authority**: Merging to `main` never publishes anything by itself. A release is a separate, deliberate action — pushing a version tag (see §7) — and CI then compiles only that one extension's WebAssembly component (`wasm32-wasip2`), packages the `.shilpo-ext` bundle, generates checksums, signs the package and registry index using protected signing keys, and publishes release assets. Other extensions are untouched by that release.
- **No Author-Held Keys**: Authors do not need to manage publisher keys. Authenticity is guaranteed by CI compilation and Ed25519 cryptographic index signatures.

---

## 2. Extension Directory Layout

Each extension must be located in its own directory named after its extension ID:

```text
extensions/
└── <extension-id>/
    ├── extension.toml          # Required: extension manifest
    ├── Cargo.toml              # Required for Rust extensions
    ├── src/                    # Rust source files
    │   └── lib.rs
    ├── settings.schema.json    # Optional: settings schema if settings page contributed
    ├── assets/                 # Optional: icons, images, static assets
    ├── README.md               # Recommended: documentation
    └── LICENSE                 # Required: open-source license
```

---

## 3. Namespace Policy

Extension IDs must adhere to reverse-domain naming conventions:

- **Official Namespaces (`org.shilpo.*`)**: Reserved for core Shilpo maintainers.
- **Community Namespaces (`io.github.<login>.*`)**: Open to contributors, where `<login>` is your GitHub login username.
- **Ownership (`owners.toml`)**: Maps namespace prefixes to authorized GitHub login usernames. When your initial submission PR is approved and merged, your namespace is registered in `owners.toml`.

---

## 4. Manifest Requirements

Every `extension.toml` must:
- Specify `schema_version = 1` and `api_version = "0.1.0"` (or supported API version).
- Contain a valid `id` matching your assigned namespace.
- Provide author identities in strict mailbox format: `"Display Name <email@domain>"`.
- Declare only the minimal capabilities required for declared features (no wildcard or unused capabilities).
- Comply with the `ExtensionManifest` schema (no `[runtime]` tables).

---

## 5. Trusted Local Scripts

Trusted local scripts are unsandboxed local-only scripts that run directly under the user's login session. Reference scripts live under `local-scripts/` and **have no registry distribution path**. Do not submit script bundles to `extensions/`.

---

## 6. Submission Workflow

1. Fork `shilpo-rs/extensions` and create a feature branch.
2. Add your extension under `extensions/<extension-id>/`.
3. Add your crate to the root `Cargo.toml` `[workspace] members` if written in Rust.
4. Verify your extension builds cleanly:
   ```bash
   cargo fmt --all -- --check
   cargo build --workspace --target wasm32-wasip2 --release
   ```
5. Open a Pull Request to `main`. CI will validate formatting, manifests, namespace rules, capability diffs, and WASM compilation.

---

## 7. Releasing a New Version

Merging a PR to `main` does not publish anything. A release happens only when a maintainer
pushes a version tag matching the pattern `<extension-id>-v<version>` — for example:

```bash
git tag org.shilpo.weather-v1.0.1
git push origin org.shilpo.weather-v1.0.1
```

That tag triggers exactly one extension's release workflow (`.github/workflows/release-<extension-id>.yml`,
each calling the shared `release-extension.yml`). It builds, packs, signs, and publishes only
that extension — every other extension's package, index entry, and CI run are completely
untouched, whether the tag push succeeds or fails.

Each extension needs its own trigger file the first time it's onboarded (copy an existing
`release-<extension-id>.yml`, matching the pattern to the new extension's id) — the shared
`release-extension.yml` reusable workflow does not need to change.

Non-Rust extensions (currently `pending-ci-support/org.shilpo.example`) have no automated
build path yet and are excluded from `extensions/` until one exists — see that directory's
README.
