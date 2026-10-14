# Security architecture and assurance

This is a reasoned account of the current controls and limits, not a guarantee
that the program is free of vulnerabilities. See [desktop hardening](desktop-hardening.md)
for the detailed storage, authentication and migration contracts.

## Assets and trust boundaries

Protect outbound service credentials, local authentication verifiers, encryption
keys, signed/pinned plugin contents, camera media, and the authority to issue
inverter/home-automation commands. Untrusted input arrives from brokers,
configured services, imported settings, downloaded packages and the webview.
A compromised process running as the same operating-system user, a hostile
administrator, compromised trusted plugin binary or compromised configured
controller is outside the local application's isolation guarantee.

The Vue frontend calls an explicit Rust command interface. Native session guards
protect configuration changes, export and controls when authentication is enabled.
The [worker protocol](plugin-worker-protocol.md) bounds frames and actions and
checks capabilities, generation and deadlines. A revoked generation cannot keep
control authority. [Plugin packages](plugin-packages.md) require configured HTTPS
archive pins and hashes, or a trusted signature where a publisher is configured;
unsafe archive paths and arbitrary executable selection are rejected. The default
publisher set is empty. These worker processes are not a general hostile-code
sandbox.

Media grants constrain origin, path and credential scope; native transfers do not
follow redirects. Imports validate the destination and omit inbound credentials;
exports omit secrets. Configuration writes use private temporary files and atomic
replacement; unavailable keys and malformed ciphertext fail closed. Tests cover
bad archives, stale authority, broken storage, unknown trust, wrong hostnames and
secret redaction, with isolated fixtures rather than real devices.

## Cryptography

The project uses RustCrypto AES-256-GCM with fresh 96-bit nonces for local encrypted
configuration/plugin settings; Argon2id v19 stores the local login verifier using
19 MiB, two iterations, one lane, a 32-byte output and a fresh random salt of at
least 16 bytes. `rand`'s OS-seeded cryptographic thread RNG and library OS randomness
supply new keys/nonces/salts; plugin signatures use Ed25519 and archive integrity
uses SHA-256. These are published algorithms implemented by maintained FLOSS
libraries, not application-designed ciphers. Remote service credentials remain
reversibly encrypted because the application must send them to those services.
They are distinct from local login password verification.

Rustls/AWS-LC and reqwest enforce peer trust and supported TLS versions. The
[native platform verifier policy](../src-tauri/vendor/platform-verifier-policy.md)
and [executable TLS probes](../tests/tls-policy/README.md) document exact RSA key
bounds, EC restrictions and platform differences, including rejection of the
RSA-2047 boundary. Rustls defaults use TLS 1.2/1.3, authenticated ephemeral key
exchange and modern AEAD suites. HTTPS gateway and plugin download paths require
secure transport. Operating-system trust stores remain an administrative boundary.
Application code never disables certificate verification to accept a failed peer.

Legacy mobile shared-key ciphertext is **read-only migration input**. New writes
use a random installation key. Protect old backups, migrate them, and remove old
copies when no longer required; the old format is not secure confidential storage.
Local authentication migration similarly commits a verifier before granting a
session and prevents a silent downgrade to a blank old password.

Anonymous local MQTT and explicitly configured HTTP/camera endpoints still exist
for device compatibility. Keep them on a trusted isolated network; core MQTT
rejects credentials without TLS. This is why the Silver all-network-secure-default
criterion is not claimed. Secrets currently share encrypted configuration files
with other settings; runtime replacement is supported, but the Silver separate
credential-file criterion is not claimed. Crypto algorithm negotiation in TLS does
not imply application-level agility for every stored format.

## Advisory and delivery review

The 2026-10-13 review found no open Dependabot or secret-scanning alerts, no published
repository security advisories and enabled GitHub private vulnerability reporting.
The only open code-scanning item was Scorecard's generic binary-artifact finding
for `gradle-wrapper.jar`. Its exact SHA-256 and distribution ZIP are pinned, and
[the verifier](../scripts/android/verify_gradle_wrapper.py) runs before Java executes
the bootstrap. Five tests demonstrate missing, modified and linked inputs fail.
Binary presence alone is not a confirmed exploitable vulnerability.

Unfiltered Cargo audit on the baseline returned no vulnerability-list entries but
reported the unmaintained `proc-macro-error` and `ttf-parser` components and the
`glib::VariantStrIter` unsoundness advisory. The `glib` issue is fixed by a minimal, provenance-recorded backport of upstream
PR 1343 in [the vendored crate](../src-tauri/vendor/glib-0.18.5/INVERTER-PATCH.md).
The 0.18 line ends at 0.18.5, so a fixed compatible patch release is unavailable.
The three optimized regression tests call the actual patched iterator, and Linux
CI runs them. Version-based tools may still list the advisory against the
unchanged 0.18.5 version; no advisory ignore was added. The two unmaintained
components remain tracked update risks, not proven exploitable vulnerabilities. The six obsolete webpki and
quick-xml ignores are removed: the locked fixed versions are webpki 0.103.15 and
quick-xml 0.41.0. Do not restore old suppressions when a future advisory appears.

Source and releases are distributed over GitHub HTTPS. Published checksums and
source-bound manifests improve integrity, but are not independent signatures.
Android signing does not make every desktop/mobile release cryptographically
signed. Whole-release signatures, signed Git tags, sanitizer testing of native
FFI and an independent recovery maintainer remain unclaimed Silver requirements.

Report suspected issues through [SECURITY.md](../SECURITY.md). The maintainer
triages impact, reproduces safely, coordinates a fix/mitigation and disclosure,
credits reporters who consent, and adds regression evidence. New findings can
invalidate an assessment and should update this document and the public form.
