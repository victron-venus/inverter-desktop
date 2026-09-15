# Public TLS test material

These disposable certificates and the deliberately public server private key
exist only for loopback subprocess tests. Never install this CA in a system
trust store, deploy the key, or use it for package signing. The CA private key
was discarded after creating this fixture.

The ECDSA P-256 CA signs one server certificate with `serverAuth` and SANs for
`localhost` and `127.0.0.1`. Both certificates are valid from 2020-01-01 through
2040-01-01 to avoid depending on the fixture generation date in CI. The server
key is unencrypted PKCS#8 solely so the test can instantiate its local server.

Tests copy the CA to a uniquely created temporary directory and set
`SSL_CERT_FILE` only in an environment-cleared child process. No user or system
certificate store is modified. WSS uses rustls-native-certs on all desktop
platforms. Reqwest's platform verifier honors this override on Linux; macOS and
Windows retain OS certificate verification, which must reject this fixture CA.

The secret scanner permits only this public fixture at its exact repository path
and with matching key material. Other paths, other key material and all other
secret-detection rules remain checked.
