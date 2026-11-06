#!/usr/bin/env bash
set -e
# Run from the repository root wherever it is called from.
cd "$(dirname "$0")/.."

# Build default example modules in release mode and copy to CLI module_defaults folder

MODULES=("graphics" "audio" "input" "module_manager" "network")
TARGET_DIR="target/wasm32-unknown-unknown/release"
DEST_DIR="crates/interstice-cli/module_defaults"

echo "Building default modules..."

for module in "${MODULES[@]}"; do
    echo "Building $module..."
    cargo build -p "$module" --target wasm32-unknown-unknown --release
done

echo ""
echo "Copying WASM files to $DEST_DIR..."

# Create destination directory if it doesn't exist
mkdir -p "$DEST_DIR"

for module in "${MODULES[@]}"; do
    # Convert hyphens to underscores for the wasm filename
    wasm_name="${module//-/_}"
    WASM_FILE="$TARGET_DIR/${wasm_name}.wasm"
    if [ -f "$WASM_FILE" ]; then
        cp "$WASM_FILE" "$DEST_DIR/"
        echo "  ✓ $wasm_name.wasm"
    else
        echo "  ✗ Warning: $WASM_FILE not found"
    fi
done

echo ""
echo "Done! Example modules built and copied to $DEST_DIR"

# ── Build example modules ─────────────────────────────────────────────────────

#!/usr/bin/env bash
set -e

# Build all example modules in release mode and copy to CLI module_examples folder

# desktop-example bakes in the app wasms via include_bytes!, so the apps it
# bundles must be built AND copied before it: the loop below copies each wasm
# right after building it.
MODULES=("audio-example" "caller-example" "graphics-example" "hello-example" "agar-server" "agar-client" "http-get-example" "browser-example" "ui-example" "notes" "desktop-example")
# Respect CARGO_TARGET_DIR so WASM is copied from the same tree `cargo build` wrote to.
: "${CARGO_TARGET_DIR:=target}"
TARGET_DIR="${CARGO_TARGET_DIR}/wasm32-unknown-unknown/release"
DEST_DIR="crates/interstice-cli/module_examples"

echo "Building example modules..."

mkdir -p "$DEST_DIR"
for module in "${MODULES[@]}"; do
    echo "Building $module..."
    cargo build -p "$module" --target wasm32-unknown-unknown --release
    # Copy now: desktop-example, built last, bakes the others in.
    cp "$TARGET_DIR/${module//-/_}.wasm" "$DEST_DIR/"
done

echo ""
echo "Copying WASM files to $DEST_DIR..."

# Create destination directory if it doesn't exist
mkdir -p "$DEST_DIR"

for module in "${MODULES[@]}"; do
    # Convert hyphens to underscores for the wasm filename
    wasm_name="${module//-/_}"
    WASM_FILE="$TARGET_DIR/${wasm_name}.wasm"
    if [ -f "$WASM_FILE" ]; then
        cp "$WASM_FILE" "$DEST_DIR/"
        echo "  ✓ $wasm_name.wasm"
    else
        echo "  ✗ Warning: $WASM_FILE not found"
    fi
done

echo ""
echo "Done! Example modules built and copied to $DEST_DIR"

echo ""
echo "Building the benchmark workload (used in place by interstice-bench)..."
cargo build -p bench --target wasm32-unknown-unknown --release
