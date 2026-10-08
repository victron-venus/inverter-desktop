# Local Windows certificate-chain policy

The native application retains `rustls-platform-verifier` 0.7.1 and the separate
Home Assistant worker retains 0.7.0 through their Cargo path patches. Each copy
comes from the exact published crate recorded in
[`platform-verifier-provenance.json`](platform-verifier-provenance.json), including
its upstream revision and per-file hashes. MIT and Apache-2.0 license texts are
retained in both directories. All upstream files except `src/verification/windows.rs`
are byte-identical; the upstream development `Cargo.lock` is not included.

The Windows delta supplies an explicit serialized strong-sign policy to the
existing `CertGetCertificateChain` call: RSA keys must be at least 2048 bits,
ECDSA keys at least 256 bits, and certificate signatures use SHA-256, SHA-384 or
SHA-512 with RSA/ECDSA. The explicit RSA limit avoids the 2047-bit threshold in
Windows' predefined strong-sign OID. ECDSA-P224 and legacy SHA-1 certificate
chains are intentionally outside this policy. Applications using private CAs
must replace undersized keys and legacy signatures rather than disable checking.

Both the normal system-store path and the in-memory extra-root fallback call the
same chain builder. It evaluates the chain Windows selects, including a root
omitted by the server. The subsequent SSL hostname, trust, usage and revocation
checks remain in place. `dwStrongSignFlags` stays zero, retaining the separate
end-certificate key check; serialized `dwFlags` stays zero, preserving existing
CRL/OCSP validation behavior. No trust stores, registry settings, global policy,
or certificate-error bypasses are changed.

The shipped Windows build targets Windows 10 or later, consistent with the
[supported Rust Windows targets](https://blog.rust-lang.org/2024/02/26/Windows-7/).
The strong-sign API is available starting with Windows 8. An explicit `win7`
cross-compilation target is rejected rather than silently compiling a weaker
policy. Linux, Apple and Android verification code is unchanged.

The three actual client builders are tested in Windows CI with disposable
certificates; see [the probe instructions](../../../tests/tls-policy/README.md).
This source patch is not proof that every platform, proxy or separately
implemented application TLS path enforces the same bounds. In particular, the
observed Apple RSA-2047 acceptance is a separate unresolved minimum-strength gap.

When updating either upstream crate, retain the local Windows delta, update the
provenance hashes, and rerun all three Windows clients. Review the complete
upstream changes; do not remove the local patch based only on a version bump.

API references:

- [CERT_CHAIN_PARA](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_chain_para)
- [CERT_STRONG_SIGN_SERIALIZED_INFO](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_strong_sign_serialized_info)
- [CERT_STRONG_SIGN_PARA](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_strong_sign_para)
