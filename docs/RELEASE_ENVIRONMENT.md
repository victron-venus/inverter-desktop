# Release build environment

The desktop and mobile release adapters use Node.js **24.21.0**, pnpm
**10.34.7** and Rust **1.99.1**. Python is fixed to **3.12.15** on Linux
and **3.12.10** on macOS/Windows. Both macOS targets use `macos-26`.
The reviewed workflow fixes these versions for candidate builds; updating the
source or build recipe requires a new candidate.

A major-version selector such as `node-version: 24` can select a different
locally cached patch version on different hosted images. The exact Node pin
removes that variation. See the
[setup-node version selection documentation](https://github.com/actions/setup-node/blob/main/docs/advanced-usage.md#check-latest-version).

## Candidate builds and stable promotion

The current `.release-policy.json` selects **`promote-bytes`**. Nightly, beta and
RC requests build the required platform matrix from one frozen version plan.
Every required validation workflow, platform package and Release gate must pass
before candidate publication. RC requests also require the configured fuzzing
campaigns. Platform receipts record the source, version inputs, actual toolchain
and package hashes; immutable Actions evidence binds them to the candidate.

RC packages already contain the final package version. Their embedded descriptor
retains the RC tag, source SHA and reserved native build counters. After the exact
RC assets have passed acceptance, stable promotion copies those assets byte for
byte. It does not run a final package matrix, compile again or re-sign the app.
The original RC descriptor remains visible after stable promotion.

The protected `release` environment still requires approval. Promotion verifies
the accepted RC's original successful run and attempt, source and policy,
default-branch ancestry, Release gate, retained immutable evidence and payload
checksums. Neither successful CI nor a beta publication proves RC acceptance.
See the [release runbook](release-workflow.md) for commands and ownership.

## Recorded environment and hosted image limits

The toolchain receipt records the actual runner image revision (`ImageVersion`),
OS and architecture, Python, and detected Node, Rust, Cargo, Go and Xcode versions.
pnpm is fixed in the workflow; it is not currently a separate field in the shared
toolchain receipt.

`macos-26` selects an OS family, not an immutable machine image. Standard hosted
images receive updates. A new candidate can therefore record a different image
revision even with unchanged tool version pins. The current promotion profile
avoids an RC/final runner-image comparison because stable uses the accepted RC's
existing bytes. This does not claim that independent rebuilds are reproducible.

The shared release tooling also supports a separate **`final-build`** profile.
For that profile only, `scripts/check-release-toolchain.py` verifies the accepted
RC and compares the entire recorded toolchain object before final compilation;
the final publisher checks the receipts again. A historical final build stopped
because its macOS image and Node version differed from its RC. That historical
failure explains the retained helper, but it is not the current stable workflow.
An old `final-build` RC cannot be reinterpreted as a `promote-bytes` RC.

## Recovery

- Preserve a failed candidate run, its receipts and the observed errors. Correct
  source or recipe defects through review and create a fresh candidate.
- If source, dependencies or packaging change after RC acceptance, build and
  accept a new RC. Do not replace assets, re-sign published binaries, rewrite
  tags or alter version-allocation history.
- If promotion evidence has expired or is missing or invalid, create and accept
  a new RC. Do not use a local rebuild as substitute promotion evidence.
- For a transient publication failure, inspect the existing run and any draft
  before recovery. Prefer the already queued automatic beta at the intended
  source; do not dispatch duplicate candidates without checking run state.

A move to an immutable macOS builder would be a separate infrastructure change
requiring review and validation.
