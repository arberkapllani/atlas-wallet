# Security Policy

## Supported Versions

Atlas is in active pre-1.0 development. Only the latest `main` branch and the
most recent tagged release are supported. **Do not store significant funds**
until Atlas has received a third-party security audit.

## Reporting a Vulnerability

**Please do not file public GitHub issues for security vulnerabilities.**

Instead, report privately by one of:

1. GitHub's private vulnerability reporting:
   <https://github.com/arberkapllani/atlas-wallet/security/advisories/new>
2. Email: `security@atlas-wallet.example` (replace with real address before launch)

Include:
- A description of the issue and its impact.
- Steps to reproduce or a proof of concept.
- Affected version / commit hash.
- Your name / handle for credit (optional).

We will acknowledge your report within **72 hours** and aim to provide a fix
or mitigation within **30 days** for critical issues.

## Scope

In scope:
- Cryptographic flaws (vault encryption, KDF, key derivation, signing).
- IPC boundary issues (`tauri-specta` commands exposing private state).
- Memory-safety issues in Rust crates that could leak seed material.
- Frontend XSS / CSP bypass leading to private-key exfiltration.
- Bundled dependency vulnerabilities affecting the shipped binary.

Out of scope:
- Social engineering of end users.
- Issues requiring an already-compromised host machine.
- Phishing of arbitrary websites that pretend to be Atlas (we ship a
  signed updater; only verify binaries from official channels).

## Disclosure

We follow coordinated disclosure. After a fix ships in a signed release,
we will credit reporters who wish to be named.
