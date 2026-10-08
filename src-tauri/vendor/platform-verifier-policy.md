# Local native certificate-chain policies

The native application retains `rustls-platform-verifier` 0.7.1 and the separate
Home Assistant worker retains 0.7.0 through their Cargo path patches. Each copy
comes from the exact published crate recorded in
[`platform-verifier-provenance.json`](platform-verifier-provenance.json), including
its upstream revision and per-file hashes. MIT and Apache-2.0 license texts are
retained in both directories. All upstream files except `src/verification/windows.rs` and
`src/verification/apple.rs` and `src/verification/others.rs` are byte-identical; the upstream development `Cargo.lock` is not included.

The Windows delta supplies an explicit serialized strong-sign policy to the
existing `CertGetCertificateChain` call: RSA keys must be at least 2048 bits,
ECDSA keys at least 256 bits, and certificate signatures use SHA-256, SHA-384 or
SHA-512 with RSA/ECDSA. The explicit RSA limit avoids selecting Windows' predefined 2047-bit
strong-sign OID, but Windows can still accept a 2047-bit modulus under this
configuration. After successful native SSL policy verification, an additional
check walks every element in all selected simple chains, including the trusted
root. It uses native CNG import/export for RSA and RSA-PSS public keys and counts
the actual significant bits of the exported modulus. It does not rely on a
rounded key-width property. Missing or malformed key data, import/export
failures, and RSA moduli below 2048 bits fail closed. The strong-sign policy
continues to enforce ECDSA and signature requirements. ECDSA-P224 and legacy SHA-1 certificate
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
policy. Linux and Android certificate verification is unchanged. On Unix,
root-loading warnings retain their static error context but omit detailed error
values, because malformed PEM errors can contain raw input lines.

The three actual client builders are tested in Windows CI with disposable
certificates; see [the probe instructions](../../../tests/tls-policy/README.md).
This source patch is not proof that every platform, proxy or separately
implemented application TLS path enforces the same bounds. The Apple policy below addresses the separately observed RSA-2047 acceptance.

The Apple delta runs only after native `SecTrust` evaluation succeeds. It checks
all certificates in the evaluated chain, including the selected trust anchor,
using native public-key type and exact bit-count metadata. RSA keys require at
least 2048 bits and ECDSA keys at least 256 bits. Missing, invalid or unsupported
key metadata fails closed; other key algorithms are outside this profile.
Hostname, trust anchors, validity, usage and revocation still come from the
original native evaluation. No extra trust evaluation, trust-store mutation or
certificate parser is introduced. The added key API has the same minimum OS
availability as the verifier's existing `SecTrustEvaluateWithError` call.

Apple's supported certificate profile is RSA with PKCS#1 certificate signatures
and ECDSA. The existing native evaluator rejects the two tested RSA-PSS
certificate-signature/public-key fixtures before reaching the new key check.
Those compatibility observations are separate from the ten-case Apple policy
gate. This does not disable RSA-PSS TLS handshake signatures: the successful
RSA certificate probe negotiates TLS 1.3. Windows retains all twelve cases,
including both RSA-PSS certificate fixtures. Local macOS results do not prove
actual iOS device behavior, despite the shared Apple implementation.

When updating either upstream crate, retain the local Windows and Apple deltas, update the
provenance hashes, and rerun all three Windows and macOS clients. Review the complete
upstream changes; do not remove the local patch based only on a version bump.

API references:

- [CERT_CHAIN_PARA](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_chain_para)
- [CERT_STRONG_SIGN_SERIALIZED_INFO](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_strong_sign_serialized_info)
- [CERT_STRONG_SIGN_PARA](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_strong_sign_para)

- [CERT_CHAIN_CONTEXT](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_chain_context)
- [CryptImportPublicKeyInfoEx2](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/nf-wincrypt-cryptimportpublickeyinfoex2)
- [BCryptExportKey](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptexportkey)
- [BCRYPT_RSAKEY_BLOB](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/ns-bcrypt-bcrypt_rsakey_blob)

- [SecTrustGetCertificateAtIndex](https://developer.apple.com/documentation/security/sectrustgetcertificateatindex(_:_:))
- [SecCertificateCopyKey](https://developer.apple.com/documentation/security/seccertificatecopykey(_:))
- [kSecAttrKeySizeInBits](https://developer.apple.com/documentation/security/ksecattrkeysizeinbits)
