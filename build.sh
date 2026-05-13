#!/bin/bash -eu
# Copyright 2024 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Build script for inverter-desktop fuzz targets

# Install Rust if not already installed
if ! command -v cargo &> /dev/null; then
    echo "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly
    source $HOME/.cargo/env
fi

# Install cargo-fuzz
echo "Installing cargo-fuzz..."
cargo install cargo-fuzz

# Build fuzz targets
echo "Building fuzz targets..."
cd src-tauri

# Build each fuzz target
cargo fuzz build --release fuzz_json_parsing
cargo fuzz build --release fuzz_mqtt_handling
cargo fuzz build --release fuzz_command_parsing

# Copy binaries to output directory
echo "Copying fuzz targets to $OUT..."
cp target/x86_64-unknown-linux-gnu/release/fuzz_json_parsing $OUT/
cp target/x86_64-unknown-linux-gnu/release/fuzz_mqtt_handling $OUT/
cp target/x86_64-unknown-linux-gnu/release/fuzz_command_parsing $OUT/

# Copy corpus and dictionaries
echo "Copying corpus and dictionaries..."
cp -r fuzz/corpus $OUT/
cp -r fuzz/dictionaries $OUT/

echo "Build complete!"
