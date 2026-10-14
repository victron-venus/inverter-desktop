# Build and quality evidence

## Reproduce the checks

Use current stable Rust with Cargo, rustfmt and Clippy; Node.js 24 with pnpm 10;
and Python 3.12 or newer for build contracts. The locked dependency graph is in
`pnpm-lock.yaml`, `src-tauri/Cargo.lock` and each worker's Cargo.lock. Linux native
prerequisites are listed by the [CI workflow](../.github/workflows/ci.yml); Apple
and mobile builds additionally need their platform SDKs. The obsolete Rust 1.70
README badge was not a tested minimum supported Rust version.

From a clean checkout run `bash scripts/ci.sh`. This performs locked dependency
installation, frontend/mobile tests and builds, Rust formatting, Clippy and Rust
tests. CI additionally runs `pnpm lint`, independent worker tests/builds on Linux,
macOS and Windows, package acceptance scenarios, native TLS probes, mobile
packaging and release/workflow contracts. Read the workflow before installing
system prerequisites. Do not run a release/deployment command to test a change.

Tests belong next to frontend modules as `*.test.ts`, in Rust `#[cfg(test)]`
modules, or under `tests` and `.github/release-tests` for Python contracts. Add
rejected-input and failure-path tests as well as successful examples. Major
features require automated tests; bug fixes require a regression test or a
specific explanation of a practical alternative. This applies to maintainers.

## Assessment baseline, 2026-10-13

At source `bc3deddc5efc3e5e9a87a442a5edcbc6ae223889`, a fresh macOS checkout passed:

- Frontend: 716 tests in 60 files; mobile: 18 tests in four files; build profiles:
  eight tests. Both desktop and mobile production frontend builds succeeded,
  including Vue/TypeScript checks. Prettier passed.
- The Gradle bootstrap verifier and all five tamper/missing/symlink regression
  tests passed. CI verifies the wrapper before and after Android initialization.
- Native Rust tests passed: 654 library tests and two package-example tests;
  15 opt-in tests were ignored in that ordinary invocation. Those include
  platform/package acceptance scenarios separately invoked by CI.
- The unfiltered host Cargo audit reported zero entries in its vulnerability
  list. Informational advisory findings are discussed in the
  [security assurance](security-assurance.md); a zero exit code does not erase them.

These are source-checkout results, not installed-device or full cross-platform
acceptance. The final pull request's complete required CI provides its own
revision-specific evidence. No whole-project statement/branch coverage,
byte-for-byte reproducible native release, or sanitizer coverage of Apple
Objective-C/unsafe FFI is claimed.

## Warning review

Biome reported 113 warning-level diagnostics across 27,240 lines of frontend
TypeScript, Vue and CSS (165 files), plus 67 informational style suggestions.
The warning classes were 61 `noImportantStyles`, 50 `noNonNullAssertion`, and two
`noPrototypeBuiltins`. This baseline is accepted for the assessment, not hidden or
reclassified by changing the linter. CSS priorities implement existing theme and
mobile overrides; removing them requires visual regression review. Of the 50
assertions, 49 are fixture/test assertions and one is the notification timestamp
inside a `v-if` that validates it. The two prototype calls already use
`Object.prototype.hasOwnProperty.call`, avoiding a payload-owned method.

The Vite build also reports dependency `use client` directives in the bundled
React-based spreadsheet component; this application renders entirely on the
client and does not use React server components. These upstream directives are
not an application trust boundary. Rust Clippy uses `-D warnings`; the warning
baseline does not relax that gate. The Silver maximum-warning-strictness
criterion remains unmet while this frontend warning debt and native-platform
coverage remain. Changes must not silently expand accepted warning classes.

## Dependencies and release evidence

Review Dependabot/Renovate proposals, cargo-audit, cargo-deny, CodeQL and secret
scanning. The host and six worker lockfiles are separate; updating one does not
update the others. CodeQL covers Rust/JavaScript/TypeScript, Python, GitHub Actions
and Java/Kotlin extracted during the Android build. Compiler/linter passes alone
are not security analysis. Known scanner findings need an actual applicability
review rather than blanket suppression.

The [release policy](../RELEASING.md) requires reviewed versioned notes, exact
candidate acceptance and normal release gates. [FUZZING.md](../FUZZING.md) documents
three bounded RC-only libFuzzer targets against production validators. A policy
or seed replay is not evidence that a mutation campaign ran; inspect the exact
candidate's retained report. Apple/mobile SDK requirements, `strip = true` in the
release profile, no measured reproducibility, and no audited complete environment
variable/install-prefix contract remain Silver limitations.

The GLib compatibility fix adds a separate locked three-test release-mode
regression crate under `tests/glib-variant-regression`. It calls the vendored
`VariantStrIter` implementation with compiler optimization enabled; ordinary
application tests alone do not establish this dependency fix.

The same optimized GLib regression against the original registry crate terminated
with SIGSEGV; the patched crate passed all three tests. This control confirms that
the regression exercises the old invalid pointer behavior.

The focused GLib test build also emits 34 warnings from unchanged upstream
0.18.5 source (deprecated interfaces and lifetime syntax). They are retained
visibly; the backport changes only the unsafe out-pointer. These third-party
warnings are another reason not to claim maximal warning strictness.
