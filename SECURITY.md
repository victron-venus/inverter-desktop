# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| Current main branch | ✅ |
| Previous releases | ❌ |

## Reporting a Vulnerability

If you discover a security vulnerability in this project, please report it privately before disclosing it publicly.

### How to Report

**Email:** security@victron-venus.org

Please include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fixes (if known)

### What to Expect

- **Response time:** We will acknowledge your report within 48 hours
- **Disclosure timeline:** We aim to fix and release patches within 30-90 days depending on severity
- **Coordination:** We will work with you to coordinate disclosure timing
- **Credit:** With your permission, we will credit you in the security advisory

### Severity Levels

- **Critical:** Immediate risk to system integrity or data (fix within 7 days)
- **High:** Significant impact but limited exposure (fix within 14 days)
- **Medium:** Moderate impact (fix within 30 days)
- **Low:** Minor issues (fix within 90 days)

## Security Best Practices

- Keep dependencies updated
- Review pull requests for security implications
- Use strong authentication for MQTT connections
- Enable HTTPS/TLS for production deployments
- Regularly audit access controls

## Coordinated Disclosure

We follow the [OSS Vulnerability Guide](https://github.com/ossf/oss-vulnerability-guide/blob/main/maintainer-guide.md) for coordinated vulnerability disclosure.

## Security Advisories

Security advisories will be published in the [GitHub Security Advisories](https://github.com/victron-venus/inverter-desktop/security/advisories) section.

## Encryption

For sensitive vulnerability reports, you can encrypt using our public key:

```
-----BEGIN PGP PUBLIC KEY BLOCK-----
[Public key would be here]
-----END PGP PUBLIC KEY BLOCK-----
```

*Note: PGP key not yet configured. Contact via email for secure communication setup.*

## Related Resources

- [GitHub Security Documentation](https://docs.github.com/en/code-security)
- [OSSF Vulnerability Guide](https://github.com/ossf/oss-vulnerability-guide)
- [CVE Assignment](https://cveform.mitre.org/)

## License

This security policy is part of the project and follows the same MIT license.
