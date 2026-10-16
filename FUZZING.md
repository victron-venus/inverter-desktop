# Release candidate fuzzing

See the [illustrated RC fuzzing process and verified rollout results](https://github.com/victron-venus/venus-os-ci-toolkit/blob/main/docs/rc-fuzzing-results.md)
for the shared release flow, evidence requirements and a dated cross-project
snapshot. The project-specific scope and reproduction guidance below still apply.

Fuzzing runs the production input validators against mutated, malformed, and
boundary inputs. This repository uses `cargo-fuzz`/libFuzzer through the shared
`victron-venus/venus-os-ci-toolkit` release pipeline. It is **not enrolled in
Google OSS-Fuzz**, and this is not a continuous or exhaustive fuzzing service.

## When it runs and what a badge means

The release policy opts into `.github/fuzz.json`. The shared workflow checks both
repository visibility and the resolved release channel before allocating a
fuzzing runner. Campaigns run only for a **public repository's release candidate**.
They do not run for a pull request, ordinary push, nightly, beta, schedule, or
stable promotion. The ordinary CI may replay the small checked-in corpus as a
deterministic unit test; it does not generate new inputs or run libFuzzer.

The README's **Fuzzing: RC only** badge links here and describes this policy. It
is not a green test result. The RC release workflow, its fuzzing job, and its
retained source-bound report are the evidence for a particular candidate.

All configured targets must pass before an RC can be published. A crash, panic,
assertion failure, timeout, or resource-limit failure is a failed campaign.
Fix the cause, add its smallest safe reproducer to tests, then build and test a
new RC. A stable promotion reuses the accepted RC result only when the release
pipeline verifies the matching source and artifacts; it does not rerun fuzzing.

## Target scope and assertions

The standalone `fuzz` crate depends on the same `inverter-worker-protocol` and
`inverter-camera-common` libraries that the installed desktop workers use. It
does not duplicate the parsers or link the Tauri GUI.

- `worker-frames` feeds arbitrary byte streams to the real bounded stdio frame
  reader with several buffer sizes. Pipe segmentation must not change commands
  or errors. Accepted actions must preserve token, deadline, parameter-size,
  nesting, and node-count restrictions. Structured action envelopes also drive
  the semantic validator. Seeds cover CRLF, multiple frames, reserved internal
  JSON number keys, and escaped keys.
- `camera-topics` exercises provider-scoped MQTT filters, labels, and exclusion
  lists. Accepted subscriptions cannot become global `#` subscriptions, exceed
  configured limits, or match extra topic segments. Accepted settings must keep
  identity, cardinality, text-size, and control-character limits.
- `live-urls` exercises exact Home Assistant camera destinations, URL parsing,
  and token resolution. An accepted event must retain the configured HTTPS
  origin and camera path, contain one bounded token parameter, and never gain
  credentials, a fragment, or access to an unconfigured camera. Seeds include
  origin confusion, encoded query separators, and valid configuration.

These targets do not test GUI rendering, broker behavior, TLS negotiation,
codec decoding, arbitrary plugin binaries, hardware, or the whole application's
state machine. No broker, camera, Home Assistant, Cerbo, credential store, or
physical control is contacted. The shared workflow isolates target execution
from the network; dependency preparation may download public build inputs.

## Budgets, seeds, and reports

The manifest assigns **60 seconds to each of three targets**, run sequentially
(180 seconds of mutation budget). The shared runner additionally enforces a
per-input timeout, memory bound, and overall job timeout. Compilation time is
separate. The toolchain is pinned to `nightly-2026-10-12`, and the fuzz crate has
a committed lockfile. The shared workflow pins its tooling and action revisions.

Checked-in files under `fuzz/corpus/<target>/` are synthetic, public seeds. The
runner copies them to a fresh writable diagnostics directory before mutating;
it does not modify the checkout. Logs, report, evolving corpus, and failure
inputs are retained with that run. A saved corpus is not automatically trusted
as release evidence for another source revision. Do not add user credentials,
captured production payloads, or private URLs to public fixtures or artifacts.

## Reproduce locally

See the [official cargo-fuzz guide](https://rust-fuzz.github.io/book/cargo-fuzz.html)
for supported hosts. Install the same nightly and the cargo-fuzz version pinned
by the shared workflow. From the repository root:

```sh
rustup toolchain install nightly-2026-10-12 --profile minimal --component rust-src
cargo fetch --locked --manifest-path fuzz/Cargo.toml
cargo test --locked --manifest-path fuzz/Cargo.toml --lib
cargo +nightly-2026-10-12 fuzz build --fuzz-dir fuzz
```

Use a disposable corpus directory because libFuzzer expands its first corpus:

```sh
mkdir -p /tmp/inverter-worker-corpus
cp fuzz/corpus/worker_frames/* /tmp/inverter-worker-corpus/
cargo +nightly-2026-10-12 fuzz run worker_frames /tmp/inverter-worker-corpus -- \
  -max_total_time=60 -timeout=10 -rss_limit_mb=2048
```

The other executable names are `camera_topics` and `live_urls`. The shared
Linux runner builds for `x86_64-unknown-linux-gnu` and executes the instrumented
binaries directly with its own artifact and corpus paths. Local macOS testing
is useful but does not replace that RC gate.

To reproduce and minimize a retained failure:

```sh
cargo +nightly-2026-10-12 fuzz run worker_frames /path/to/crash-input
cargo +nightly-2026-10-12 fuzz tmin worker_frames /path/to/crash-input
```

First reproduce on the report's source revision and toolchain. Determine whether
the failure is in the production code, the oracle, or the environment; do not
silence the assertion or exclude the input merely to make a release pass.
Add a focused ordinary regression test for a production bug and preserve a
synthetic minimized seed when safe. Security-sensitive findings follow
[SECURITY.md](SECURITY.md) before any public issue or reproducer is posted.
