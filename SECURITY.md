# Security

## Reporting a vulnerability

Report vulnerabilities privately through
[GitHub security advisories](https://github.com/manaforged/artificer/security/advisories/new).
Do not open a public issue for a vulnerability.

Include:

- the Artificer version (`artificer --version`) or commit
- the operating system and Rust version
- a minimal reproduction
- the expected and observed behavior
- the security impact you believe is possible

You will get an acknowledgement within 7 days of your report.

For a high or critical issue, the target is a fix or a mitigation within 30
days of the acknowledgement. The fix ships as a patch release. Its
[CHANGELOG.md](CHANGELOG.md) entry has a `### Security` line that describes
the issue and credits the reporter, unless you ask otherwise.

One maintainer reviews reports. If a fix will miss the 30-day target, the
maintainer tells you the new date in the advisory thread.

## Supported versions

Only the latest release receives security fixes.

## Scope

- Serving a cached artifact for the wrong build input (cache poisoning).
- Reading or writing another user's cache.
- Command injection or unsafe path handling in the shim, the installer, or
  `artificer import`.
- Disclosure of the optional daemon's token. The daemon binds `127.0.0.1`.
  On Unix its store directory is private to your user. The daemon is not
  available on Windows.

## Out of scope

- A build that falls back to real Cargo. Fallback is the designed behavior
  for anything Artificer does not support.
- Code that a build script or proc-macro runs. Artificer runs the same code
  Cargo would.
- An export that you import from an untrusted source. Cached files are
  executable code.
