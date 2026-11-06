#!/usr/bin/env bash
# Print `-p <package>` for every WASM module crate (defaults, examples,
# benchmarks), for `cargo build --target wasm32-unknown-unknown`.
cd "$(dirname "$0")/.."
for manifest in modules/*/*/Cargo.toml; do
    name=$(sed -n 's/^name *= *"\(.*\)"/\1/p' "$manifest" | head -1)
    [ -n "$name" ] && printf -- '-p %s ' "$name"
done
echo
