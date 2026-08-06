# Changelog

All notable changes to `doc-scraper-rs` are recorded here. Versions follow
[Semantic Versioning](https://semver.org/). The format is loosely based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Bumped `reqwest` to 0.13 (was 0.12). The `rustls-tls` feature was renamed
  to `rustls`; no API changes affect our call sites. The new
  `rustls-platform-verifier` default picks native CA roots automatically,
  so users behind corporate TLS interceptors no longer need to ship a
  custom CA bundle.
- Bumped `roxmltree` to 0.21 (was 0.20) and `indicatif` to 0.18 (was
  0.17). No API changes affect our call sites.

### Removed

- `criterion` dev-dependency (declared but unused — no `benches/` directory
  existed). Re-add when a benchmark suite is introduced.

### Added

- `llms-full.txt` corpus sidecar — every page's markdown concatenated into a
  single file in [llmstxt.org](https://llmstxt.org/) extended format
  (`# Title` + `URL:` + body, separated by `---`). Ready for direct upload to
  OpenAI's Files API, Anthropic prompt-cache attachments, or any RAG
  pipeline. Lands next to `llms.txt` and is skipped under `--no-llms-txt`.
- `AGENTS.md` synthesized project context ([agents.md](https://agents.md/))
  plus per-section deep-dive files under `skills/00-…md` / `01-…md` / etc.
  `AGENTS.md` is auto-loaded by Claude Code, OpenAI Codex, GitHub Copilot,
  and Cursor. The numeric prefix on each `skills/` file matches the order
  of AGENTS.md's `## Sections` block, so agents can `cat` the right file
  on demand. Both are skipped under `--no-llms-txt`.
- `.github/workflows/release.yml` — matrix build of pre-built binaries for
  Linux (x86_64), macOS (Intel + Apple Silicon), and Windows (x86_64),
  packaged as `.tar.xz` / `.zip` with sibling `.sha256` files, uploaded
  to the GitHub Release on `v*` tag push. Lets users without a Rust
  toolchain download a binary directly from the Releases page.
  **Linux ARM (`aarch64-unknown-linux-gnu`) is intentionally not in the
  prebuilt matrix** — `reqwest`'s `aws-lc-sys` cross-compile path needs a
  CMake toolchain file + cross binutils + sysroot alignment that is too
  fragile for an automated release. Linux ARM users can
  `cargo install doc-scraper-rs` instead.
- npm wrapper published as five packages — `doc-scraper` (umbrella) plus
  four platform packages `doc-scraper-linux-x64-gnu`,
  `doc-scraper-darwin-x64`, `doc-scraper-darwin-arm64`,
  `doc-scraper-win32-x64-msvc` — under `npm/doc-scraper/` +
  `npm/platforms/`. `npm install -g doc-scraper` resolves the platform
  via `optionalDependencies`, downloads the matching binary from the
  GitHub Release at install time (SHA-256 verified), and exposes the
  same `doc-scraper` command on `$PATH`. New `.github/workflows/npm.yml`
  runs after `release.yml` to publish the packages on `v*` tag push.
  **Additionally builds a `.tar.gz` of every target** alongside the
  existing `.tar.xz`/`.zip` so the npm install pipeline can extract
  without depending on platform `xz`/`tar -J` — `.tar.gz` is the only
  format universally extractable on every OS that has Node installed.

### Fixed

- `--llms-txt` was silently reading the `GITBOOK_SCRAPER_USER_AGENT` env var
  (copy-paste from the user-agent flag). Now correctly reads
  `GITBOOK_SCRAPER_LLMS_TXT`.

## [0.1.0] — 2026-07-08

Initial crates.io publish.

### Added

- Mirror-tree output (`<output>/<url-path>/<slug>.md`) preserving the site
  hierarchy, plus `--flat` for collapsed filenames.
- `llms.txt` sidecar with title, URL, and first paragraph per page.
- Auto-generated `index.md` TOC grouped by top-level section, with `--toc
  false` to disable.
- `--filter <TITLE>` (repeatable) for title-substring filtering. Matches
  against the markdown H1 of the page.
- `.md` endpoint fetcher with a content-based soft-404 fallback to the bare
  page URL. Detection keys off GitBook's `# Page Not Found` heading.
- Politeness delay (`--delay`, env `GITBOOK_SCRAPER_DELAY`), bounded
  concurrency (`--concurrency 20`), per-request retry with exponential
  backoff (`--retries 3`), and per-request timeout (`--timeout 20`).
- `--overwrite` opt-in for re-running into an existing output dir without
  skipping.
- `--legacy` stub for pre–Next.js GitBook sites.
- Site-host → output-slug derivation as the default output directory
  (`docs.pareto.credit` → `./pareto-docs/`).
- Environment overrides for output dir, user agent, and delay.

[Unreleased]: https://github.com/dibakarsutradhar/doc-scraper-rs/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/dibakarsutradhar/doc-scraper-rs/releases/tag/v0.1.0
