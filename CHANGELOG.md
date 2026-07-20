# Changelog

All notable changes to this project will be documented in this file.

## [2.2.1] - 2026-07-20

### Fixed

- HA entities disappearing and auth bypass ([#134](https://github.com/victron-venus/inverter-desktop/issues/134))
- Clear stale HA entity map to avoid mutex panic
- Remove dead shutdown flag
- Close missing parenthesis in `compute_filtered_data` call

### Security

- Use `cargo build --locked` for dependency verification
- Add `cargo fetch --locked` for reproducible builds
- Add `--ignore-scripts` to npm/pnpm install

### Chores

- Remove redundant explicit dereferences flagged by clippy
