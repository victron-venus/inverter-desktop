# Native HTTP TLS key-policy probes

These opt-in tests measure the actual HTTP client builders used by desktop
plugin downloads, media transfers and the separate Home Assistant worker.
The production builder settings are unchanged; extracting them makes the same
builders accessible to their private unit tests.

Requirements: Python 3.11 or newer with OpenSSL 3, the `openssl` command, and the
normal native Rust build prerequisites. No Python packages are installed.

```sh
export DESKTOP_TLS_PYTHON="$(command -v python3)"
export DESKTOP_TLS_OPENSSL="$(command -v openssl)"
export DESKTOP_TLS_PROBE_OUTPUT_DIR="$PWD/tls-policy-results"
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib tls_key_policy -- --ignored --nocapture
cargo test --locked --manifest-path desktop-plugins/home-assistant/Cargo.toml tls_key_policy -- --ignored --nocapture
```

Each builder is tested with a strong RSA-2048 chain, a RSA-1024 leaf, a RSA-1024
intermediate, a RSA-1024 root, an unknown root and a wrong hostname. Certificates
are freshly generated, valid for two days, signed with SHA-256, and contain the
appropriate CA, key-usage and server-name extensions. Private keys are disposable
test data created in temporary directories and removed after the test. No
certificate is installed in a system or user trust store.

The TLS server binds only to loopback. Before exposing its port to Rust, it
completes a real TLS/HTTP exchange using a separate oracle client. This oracle
still verifies the hostname and certificate chain, but permits the intentionally
weak test keys. A failed oracle aborts the test rather than counting as a tested
client rejection. The server records application bytes separately from handshake
errors, and a rejected case must receive no HTTP bytes or test bearer token.

Test-only changes to the real client builder are one additional fixture root,
loopback DNS resolution, and disabling proxies for the fixture connection. The
unknown-root case omits the extra root. These seams preserve the actual selected
TLS provider and certificate verifier. They do not measure production proxy or
DNS policy. No invalid-certificate or invalid-hostname override is used.
On Windows, the pinned platform verifier first attempts the normal chain and
then uses an in-memory exclusive-root chain engine for the extra fixture root.
Both paths use its normal certificate-policy validation. This does not measure
the contents or administrative policies of the system trust store.

The assertions deliberately require rejection of weak chains, so a provider
acceptance will fail and leave its JSON evidence for review. An operator-supplied
weak root accepting a connection is not, by itself, proof of a violated OpenSSF
criterion: the criterion permits weaker compatibility configurations when strong
defaults and a configuration disabling smaller keys are available. These probes
measure provider behavior; they do not establish universal operating-system
policy, elliptic-curve issuer bounds, or behavior of the separate WebPKI, iOS or
WKWebView clients. Windows results require a Windows run; macOS success cannot
substitute for it.

The tests are ignored in ordinary unit-test runs because they require external
fixture tools. The existing Windows CI jobs explicitly invoke `--ignored`,
require all three expected JSON reports across the native and worker jobs, and
preserve failed-run reports. These steps do not add certificate trust or broaden
job permissions.
