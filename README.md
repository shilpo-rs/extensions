# Shilpo Extensions

First-party sandboxed WebAssembly extensions for [Shilpo](https://github.com/shilpo-rs/shilpo):
`extensions/org.shilpo.wallpaper`, `extensions/org.shilpo.weather`, and `extensions/io.github.sayeed205.world-clock`.

Reference trusted local scripts (unsandboxed, local-only) live in `local-scripts/` and have no registry distribution path. The TypeScript showcase extension lives in `pending-ci-support/org.shilpo.example/` — see that directory's README for why it's excluded from the automated registry pipeline for now. See [CONTRIBUTING.md](CONTRIBUTING.md) for extension submission and authoring guidelines.

## Building

```bash
rustup target add wasm32-wasip2
cargo build --workspace --target wasm32-wasip2 --release
```

## Relationship to the main repository

These extensions consume the canonical `shilpo:extension` WIT contract through
the `shilpo-ext-sdk` crate, which is pulled from `shilpo-rs/shilpo` as a git
dependency **pinned to an exact revision** (never a branch) so builds stay
reproducible. To adopt a newer contract, bump the `rev` in the workspace
`Cargo.toml`.
