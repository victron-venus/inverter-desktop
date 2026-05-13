#!/bin/bash
# Setup script for local fuzzing development

set -e

echo "Setting up fuzzing environment for inverter-desktop..."

# Check if rustup is installed
if ! command -v rustup &> /dev/null; then
    echo "Error: rustup not found. Please install Rust from https://rustup.rs/"
    exit 1
fi

# Install nightly Rust
echo "Installing Rust nightly toolchain..."
rustup install nightly
rustup default nightly

# Install cargo-fuzz
echo "Installing cargo-fuzz..."
cargo install cargo-fuzz

# Build fuzz targets
echo "Building fuzz targets..."
cd src-tauri
cargo fuzz build --release fuzz_json_parsing
cargo fuzz build --release fuzz_mqtt_handling
cargo fuzz build --release fuzz_command_parsing

echo ""
echo "✅ Fuzzing environment setup complete!"
echo ""
echo "To run fuzz targets:"
echo "  cd src-tauri"
echo "  cargo fuzz run fuzz_json_parsing"
echo "  cargo fuzz run fuzz_mqtt_handling"
echo "  cargo fuzz run fuzz_command_parsing"
echo ""
echo "For more information, see FUZZING.md"
