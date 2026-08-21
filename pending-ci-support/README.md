# Pending CI support

Extensions here are real, working extensions whose toolchain the registry pipeline cannot
build yet. They are excluded from `extensions/` — the only directory the generator scans —
so a gap in one toolchain cannot fail the whole registry build.

## `org.shilpo.example`

TypeScript, built via `shilpo ext build` (a `tsc` typecheck followed by `@bytecodealliance/jco`
componentization — see `desktop/ext-runtime/src/build.rs::build_typescript` in
`shilpo-rs/shilpo`). `main-build.yml` only runs `cargo build --workspace`; it has no Node/jco
step. Move this back into `extensions/` once the pipeline gains one.

Until then, `crates/generator`'s `find_wasm_binary` will not resolve a WASM binary for an
extension with no `Cargo.toml`, and correctly fails the build rather than substituting another
extension's binary — which is what previously happened here, undetected, and shipped
`shilpo_weather_extension.wasm` signed and published as `org.shilpo.example`.
