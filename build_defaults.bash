#!/usr/bin/env bash
set -e

# Build all example modules in release mode and copy to CLI module_examples folder

MODULES=("graphics" "audio" "input" "module_manager" "ui")
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
