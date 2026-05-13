FROM gcr.io/oss-fuzz-base/base-builder

# Install Rust
RUN git clone --depth 1 https://github.com/rust-lang/rust.git /tmp/rust && \
    cd /tmp/rust && \
    ./x.py build --stage 2 && \
    ./x.py install --stage 2 && \
    cd / && \
    rm -rf /tmp/rust

# Copy project files
COPY . /src/inverter-desktop
WORKDIR /src/inverter-desktop

# Build fuzz targets
RUN cargo install cargo-fuzz
RUN cd src-tauri && \
    cargo fuzz build --release fuzz_json_parsing && \
    cargo fuzz build --release fuzz_mqtt_handling && \
    cargo fuzz build --release fuzz_command_parsing

# Copy fuzz targets to output
RUN cp src-tauri/target/x86_64-unknown-linux-gnu/release/fuzz_json_parsing $OUT/ && \
    cp src-tauri/target/x86_64-unknown-linux-gnu/release/fuzz_mqtt_handling $OUT/ && \
    cp src-tauri/target/x86_64-unknown-linux-gnu/release/fuzz_command_parsing $OUT/

# Copy corpus and dictionaries
RUN cp -r src-tauri/fuzz/corpus $OUT/ && \
    cp -r src-tauri/fuzz/dictionaries $OUT/
