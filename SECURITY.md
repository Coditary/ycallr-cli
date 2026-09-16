# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability in the ycallr CLI, please report it responsibly:

1. **Do not** open a public GitHub issue for security-sensitive findings.
2. Email the maintainer or open a private security advisory on GitHub if available.
3. Include a clear description, steps to reproduce, and potential impact.

We aim to acknowledge reports within 72 hours and provide a fix or mitigation plan as soon as possible.

## Security Notes

- The CLI loads only compiled protobuf profiles from `~/.config/ycallr/apis/`.
- YAML profiles are parsed only during `ycallr install`, not at call time.
- HTTP behavior and SSRF protections are enforced by [ycallr-core](https://github.com/Coditary/ycallr-core).
