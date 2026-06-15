#!/usr/bin/env bash
set -e

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

# desktop-example bakes in hello_example.wasm via include_bytes!, so hello-example
# must appear before it so the wasm exists in module_examples when desktop builds.
MODULES=("audio-example" "caller-example" "graphics-example" "hello-example" "agar-server" "agar-client" "benchmark-workload" "http-get-example" "ui-example" "desktop-example")
# Respect CARGO_TARGET_DIR so WASM is copied from the same tree `cargo build` wrote to.
: "${CARGO_TARGET_DIR:=target}"
TARGET_DIR="${CARGO_TARGET_DIR}/wasm32-unknown-unknown/release"
DEST_DIR="crates/interstice-cli/module_examples"

echo "Building example modules..."

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
