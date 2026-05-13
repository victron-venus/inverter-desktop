# Fuzzing Integration

This project uses fuzzing to detect security vulnerabilities and bugs through automated testing with unexpected or random data.

## Overview

Fuzzing is integrated using:
- **cargo-fuzz**: Rust fuzzing framework
- **ClusterFuzzLite**: Google's fuzzing platform for CI/CD
- **GitHub Actions**: Automated fuzzing on push/PR/schedule

## Fuzz Targets

### 1. JSON Parsing Fuzzing (`fuzz_json_parsing`)
Tests JSON parsing for the `InverterState` structure to detect:
- Malformed JSON handling
- Buffer overflows in parsing
- Type confusion vulnerabilities
- Memory safety issues

### 2. MQTT Handling Fuzzing (`fuzz_mqtt_handling`)
Tests MQTT topic and payload processing:
- Topic validation and parsing
- Payload handling and processing
- Message routing logic
- Protocol compliance

### 3. Command Parsing Fuzzing (`fuzz_command_parsing`)
Tests command validation and execution:
- Command structure validation
- Action parameter parsing
- Payload validation
- Security boundary enforcement

## Running Fuzzing Locally

### Prerequisites
- Rust nightly toolchain (required for fuzzing)
- cargo-fuzz

### Install Rust nightly
```bash
rustup install nightly
rustup default nightly
```

### Install cargo-fuzz
```bash
cargo install cargo-fuzz
```

### Run specific fuzz target
```bash
cd src-tauri
cargo fuzz run fuzz_json_parsing
cargo fuzz run fuzz_mqtt_handling
cargo fuzz run fuzz_command_parsing
```

### Run with custom options
```bash
cargo fuzz run fuzz_json_parsing -- -max_total_time=300 -dict=fuzz/dictionaries/json.dict
```

### Build fuzz targets
```bash
cargo fuzz build --release fuzz_json_parsing
```

## Corpus and Dictionaries

### Corpus Files
Located in `src-tauri/fuzz/corpus/`:
- `json_parsing/`: Valid and invalid JSON examples
- `mqtt_handling/`: MQTT topics and payloads
- `command_parsing/`: Command structures

### Dictionaries
Located in `src-tauri/fuzz/dictionaries/`:
- `json.dict`: Common JSON keywords and field names

## GitHub Actions Integration

### Triggers
- **Push to main**: Runs full fuzzing suite
- **Pull requests**: Runs fuzzing on changes
- **Weekly schedule**: Runs comprehensive fuzzing every Sunday

### Jobs
1. **Fuzz Testing**: Runs each fuzz target for 60 seconds
2. **ClusterFuzzLite**: Integrates with Google's fuzzing platform

### Artifacts
Failed fuzzing runs upload artifacts containing:
- Crash inputs
- Reproduction cases
- Debug information

## OSS-Fuzz Integration

This project is designed to integrate with [OSS-Fuzz](https://google.github.io/oss-fuzz/):

### Current Status
- ✅ Fuzz targets implemented
- ✅ GitHub Actions integration
- ✅ Corpus and dictionaries
- 🔄 OSS-Fuzz submission pending

### Next Steps for OSS-Fuzz
1. Submit project to OSS-Fuzz
2. Configure continuous fuzzing
3. Set up crash reporting
4. Enable sanitizer builds

## Security Benefits

Fuzzing helps detect:
- **Memory safety issues**: Buffer overflows, use-after-free
- **Input validation bugs**: Malformed input handling
- **Logic errors**: Edge cases and boundary conditions
- **Protocol vulnerabilities**: MQTT and JSON parsing issues
- **Denial of service**: Crash and hang conditions

## Best Practices

### Adding New Fuzz Targets
1. Create target in `src-tauri/fuzz/fuzz_targets/`
2. Add to `src-tauri/fuzz/Cargo.toml`
3. Create corpus files
4. Update GitHub Actions matrix
5. Test locally before committing

### Corpus Management
- Add interesting inputs to corpus
- Include edge cases and boundary conditions
- Use real-world examples when possible
- Regular corpus updates improve coverage

### Performance Considerations
- Use `--release` builds for production fuzzing
- Limit fuzzing time in CI (60 seconds per target)
- Run longer fuzzing sessions locally
- Monitor memory usage and timeouts

## Troubleshooting

### Build Errors
```bash
# Clean and rebuild
cargo clean
cargo fuzz build --release <target>
```

### Runtime Issues
```bash
# Run with debug output
cargo fuzz run <target> -- -max_total_time=10 -verbosity=2
```

### Corpus Issues
```bash
# Minimize corpus
cargo fuzz tmin <target> <input_file>
```

## Resources

- [cargo-fuzz documentation](https://github.com/rust-fuzz/cargo-fuzz)
- [OSS-Fuzz documentation](https://google.github.io/oss-fuzz/)
- [ClusterFuzzLite documentation](https://google.github.io/clusterfuzzlite/)
- [Fuzzing best practices](https://github.com/google/oss-fuzz/blob/master/docs/continuous_integration.md)

## Contributing

When adding new features:
1. Add corresponding fuzz targets
2. Update corpus with relevant inputs
3. Test fuzz targets locally
4. Monitor GitHub Actions results
5. Address any findings promptly

## License

Fuzzing integration follows the same MIT license as the main project.
