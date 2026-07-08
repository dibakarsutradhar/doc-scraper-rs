# Security Policy

## Reporting a vulnerability

If you discover a security issue in `doc-scraper-rs`, **do not** file a
public GitHub issue. Instead, report it privately using GitHub's
[private vulnerability reporting][gh-private] on the repository.

[gh-private]: https://github.com/dibakarsutradhar/doc-scraper-rs/security/advisories/new

You can expect a response within 7 days. Please include:

- A clear description of the vulnerability and its impact.
- Reproduction steps (URL, CLI flags, expected vs actual output).
- If known, the version of `doc-scraper-rs` you observed the issue on.
- Your handle / email if you'd like to be credited in the advisory.

## Scope

`doc-scraper-rs` is a CLI that makes outbound HTTPS GET requests to a
user-supplied URL and writes the response body to disk. Relevant
vulnerabilities include:

- Path-traversal when writing to a user-supplied `--output` directory.
- Unsafe TLS configuration that ignores certificate errors.
- SSRF patterns (URL schemes, redirects) that let the CLI hit internal
  hosts when the user passed a public URL.
- Excessive resource use (memory, disk, network) reachable from a
  well-formed input.

Vulnerabilities in **third-party dependencies** should be reported
upstream (reqwest, tokio, clap, etc.). Please also open a tracking issue
once the upstream advisory is public so we can pin a fix.

## Out of scope

- Behavioural differences between GitBook sites (output formatting,
  sitemap completeness) — those are bugs, not security issues; please
  file them publicly.
- Performance regressions without a security angle.
