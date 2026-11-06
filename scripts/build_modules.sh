#!/usr/bin/env bash
# Build the WASM modules the CLI uses (DESIGN §10, "Built modules are not in git"):
#   - the default modules, embedded in the CLI: crates/interstice-cli/module_defaults/
#   - the examples, fetched by `interstice example`: dist/examples/
#   - the benchmark workload, used in place by interstice-bench.
# `--defaults` builds only the default modules (enough to build the CLI).
set -euo pipefail
cd "$(dirname "$0")/.."

: "${CARGO_TARGET_DIR:=target}"
OUT="${CARGO_TARGET_DIR}/wasm32-unknown-unknown/release"
DEFAULTS_DIR="crates/interstice-cli/module_defaults"
EXAMPLES_DIR="dist/examples"

build() { cargo build --target wasm32-unknown-unknown --release "$@"; }

DEFAULTS=(graphics audio input module_manager network)
echo "Default modules → $DEFAULTS_DIR"
build $(printf -- '-p %s ' "${DEFAULTS[@]}")
mkdir -p "$DEFAULTS_DIR"
for module in "${DEFAULTS[@]}"; do
    cp "$OUT/${module//-/_}.wasm" "$DEFAULTS_DIR/"
done

if [ "${1:-}" = "--defaults" ]; then
    exit 0
fi

# desktop-example bakes in the apps it launches (include_bytes! from
# dist/examples), so they are built and copied before it.
APPS=(audio-example caller-example graphics-example hello-example agar-server agar-client http-get-example browser-example ui-example notes)
echo "Examples → $EXAMPLES_DIR"
build $(printf -- '-p %s ' "${APPS[@]}")
mkdir -p "$EXAMPLES_DIR"
for module in "${APPS[@]}"; do
    cp "$OUT/${module//-/_}.wasm" "$EXAMPLES_DIR/"
done
build -p desktop-example
cp "$OUT/desktop_example.wasm" "$EXAMPLES_DIR/"

echo "Benchmark workload"
build -p bench
echo "Done."
