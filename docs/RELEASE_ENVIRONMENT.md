# Release build environment

The desktop and mobile release adapters use Node.js **24.21.0**, pnpm
**10.34.7** and Rust **1.99.1**. Python is fixed to **3.12.15** on Linux
and **3.12.10** on macOS/Windows, matching the available official installers and
the versions already used by the accepted platform builds. The macOS jobs select
`macos-26` explicitly. The reviewed workflow fixes these versions for every
channel; updating them requires a new candidate from the changed source.

A major-version selector such as `node-version: 24` can select a different
locally cached patch version on different hosted images. The exact Node pin
removes that variation. See the [setup-node version selection documentation](https://github.com/actions/setup-node/blob/main/docs/advanced-usage.md#check-latest-version).

## What is checked

1. The preparation job validates the accepted RC, source, policy and payloads,
   then reserves the final version plan with its exact parent manifest identity.
2. After setting up the tools, each final package job runs
   `scripts/check-release-toolchain.py`. It verifies the selected RC receipt
   against its published manifest and immutable Actions evidence, checks the
   reserved parent, source and current RC attempt, then compares the **entire**
   recorded toolchain object with the current environment. A mismatch stops the
   job before frontend installation and native package compilation.
3. The final publisher repeats the complete RC and asset checks and compares
   every platform receipt again. The early check does not accept a package or
   replace the protected release environment and actual package acceptance.

The recorded toolchain includes the actual runner image revision (`ImageVersion`),
OS and architecture, Python, and the detected Node, Rust, Cargo, Go and Xcode
versions. No fields are removed or normalized for comparison. pnpm is fixed in
the workflow; it is not currently a separate field in the shared toolchain receipt.

## Hosted image limits

`macos-26` fixes the OS family, **not** an immutable machine image. Standard
GitHub-hosted images still receive updates. GitHub's custom-image feature can pin
an image version, but its [supported platforms](https://docs.github.com/en/actions/how-tos/manage-runners/larger-runners/use-custom-images#setting-up-an-image-generation-runner)
are Linux and Windows; it does not provide a pinned macOS image for this workflow.
An exact final/RC match on standard hosted macOS is therefore checked, not promised
by the runner label. The early check makes a rollout mismatch cheaper to detect.

For example, a failed final build used macOS image `20261013.0424.1` and Node
`24.21.0`, while its accepted RC used image `20260907.0351.1` and Node `24.20.0`.
The release correctly stopped despite passing its other checks. Fixing only the
Node selector would not make the two image revisions equal.

## Recovery from a mismatch

- Preserve the failed run, both platform receipts and the exact differing fields.
  Do not approve or publish its staged packages.
- Check which image revisions and tool versions are available. A repeated job or
  a new RC by itself does not guarantee that a hosted rollout has finished.
- Review any required workflow/toolchain change, merge normally, then build and
  accept a fresh RC from that source. A changed recipe invalidates an older RC.
- Build the final packages from that same source. Require the early check, full
  final verification and acceptance of those new package bytes before approval.

A future move to a maintained, immutable macOS builder would be an infrastructure
change requiring its own review and validation. This workflow does not silently
fall back to another image or relax the equality check.
