# doc-scraper-rs

[![CI](https://github.com/dibakarsutradhar/doc-scraper-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/dibakarsutradhar/doc-scraper-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/doc-scraper-rs.svg)](https://crates.io/crates/doc-scraper-rs)
[![docs.rs](https://img.shields.io/crates/v/doc-scraper-rs?color=blue&label=docs.rs)](https://docs.rs/doc-scraper-rs)
[![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](#msrv)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

Export GitBook docs as a clean `markdown/` mirror tree, with auto-generated `llms.txt`, `llms-full.txt`, and `AGENTS.md` sidecars ready for direct LLM ingestion and AI coding-agent context.

`doc-scraper-rs` hits GitBook's hidden `text/markdown` endpoint directly — one HTTP request per page, no HTML parsing, no browser, no chrome ("Copy page", "Ctrl-K", feedback widgets) leaking into the output. It produces the docs as they would look if you'd written them as a coherent markdown tree, plus a structured `index.md` grouped by section.

```bash
cargo install doc-scraper-rs

# Mirror docs.strata.markets → ./strata-docs/
doc-scraper https://docs.strata.markets

# Only the pages about tranches
doc-scraper https://docs.strata.markets --filter Tranching

# One-shot flat filenames
doc-scraper https://docs.pareto.credit --flat -o ./scratch/
```

A runnable smoke test that exercises the GitBook soft-404 fallback lives in
[`examples/soft_404_smoke.rs`](examples/soft_404_smoke.rs).

## Why

GitBook Next.js-rendered sites expose every page twice: once as a fully-rendered React app (380–770 KB of HTML) and once as a clean `text/markdown` payload at the same URL with `.md` appended. Most scrapers hit the HTML and try to parse the result; we hit the markdown endpoint. The output is what the author wrote, not what GitBook's React shell wrapped around it.

## Features

| Capability                                 | Default    | What it does                                                                                   |
| ------------------------------------------ | :--------: | ---------------------------------------------------------------------------------------------- |
| Mirror-tree output                         | ✅         | Writes `<output>/<url-path>/<slug>.md` so the on-disk shape matches the site tree.             |
| `llms.txt` sidecar                         | ✅         | Auto-generates an `llms.txt` listing every page (title + first paragraph + URL).                |
| `llms-full.txt` corpus                     | ✅         | Concatenates every page's markdown into one file, ready for direct upload to LLM provider file APIs. |
| `AGENTS.md` + `skills/` for AI agents      | ✅         | Synthesized project context + per-section deep-dive files. Auto-loaded by Claude Code / Cursor / Copilot / OpenAI Codex. |
| `index.md` TOC grouped by section          | ✅         | Generates a sectioned `index.md` table of contents. Disable with `--toc false`.                |
| Title-substring filter                     | `--filter` | Repeats; keeps pages whose title contains any of the supplied substrings.                       |
| `.md` endpoint with HTML fallback          | ✅         | Asks for `…/foo.md`; if GitBook returns its soft-404 stub, falls back to `…/foo` automatically. |
| Politeness delay between requests          | `--delay`  | `--delay 0.3` by default; set `--delay 0` for benchmark / trusted local mirrors.                |
| Concurrent fan-out                         | `--concurrency` | Tokio semaphore; `--concurrency 20` by default. Bounded retry loop on 5xx and 429.        |
| Legacy HTML-scraping mode (pre–Next.js)    | `--legacy` | Fallback for old GitBook sites that don't expose the `.md` endpoint.                           |

## Installation

You need a recent stable Rust toolchain (1.85 or newer). If you don't have one:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then in the same shell, pick up the new `PATH`:

```bash
source "$HOME/.cargo/env"
```

Verify with `rustc --version` and `cargo --version`. The installer doesn't add `~/.cargo/bin` to your currently-running shell's `PATH` — open a new terminal or `source` it manually as above.

With the toolchain in place:

```bash
cargo install doc-scraper-rs
```

The binary installs as `doc-scraper`. Verify with `doc-scraper --version`.

If you prefer `cargo add`:

```bash
cargo add doc-scraper-rs
```

If you don't want to install the Rust toolchain at all, download a pre-built
binary from the [GitHub Releases page](https://github.com/dibakarsutradhar/doc-scraper-rs/releases/latest).
Pick the archive that matches your OS and architecture, extract it, and put
`doc-scraper` (or `doc-scraper.exe` on Windows) somewhere on your `PATH`:

| Archive                                                  | Platform                          |
| -------------------------------------------------------- | --------------------------------- |
| `doc-scraper-<version>-x86_64-unknown-linux-gnu.tar.xz`  | Linux (Intel / AMD)               |
| `doc-scraper-<version>-x86_64-apple-darwin.tar.xz`       | macOS (Intel)                     |
| `doc-scraper-<version>-aarch64-apple-darwin.tar.xz`      | macOS (Apple Silicon)             |
| `doc-scraper-<version>-x86_64-pc-windows-msvc.zip`       | Windows                           |

Each archive also includes `README.md` and `CHANGELOG.md`, plus a sibling
`.sha256` file so you can verify integrity with `sha256sum -c`.

**Linux on ARM?** Not in the prebuilt matrix yet — `reqwest`'s `aws-lc-sys`
cross-compile path is currently flaky enough that we don't want to ship a
broken archive. ARM Linux users can install via `cargo install doc-scraper-rs`
from the Rust toolchain they almost certainly already have.

## Usage

The base URL is the only positional argument. Everything else has a sensible default:

```bash
doc-scraper <URL> [OPTIONS]
```

### Flag reference

| Flag                     | Env var                          | Default          | Purpose                                                                                                                       |
| ------------------------ | -------------------------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `-o, --output <PATH>`    | `GITBOOK_SCRAPER_OUTPUT_DIR`     | `./{slug}/`      | Output directory. The slug is derived from the site host (`docs.pareto.credit` → `pareto-docs`).                              |
| `--llms-txt <PATH>`      | `GITBOOK_SCRAPER_LLMS_TXT`         | `<output>/llms.txt` | Override the location of the `llms.txt` sidecar only; `llms-full.txt` lands next to it.                                     |
| `--no-llms-txt`          | —                                | `false`          | Skip writing both `llms.txt` and `llms-full.txt`.                                                                            |
| `--toc <BOOL>`           | —                                | `true`           | Toggle the auto-generated `index.md` TOC. Use `--toc false` to disable.                                                      |
| `--filter <TITLE>`       | —                                | —                | Repeatable; keep pages whose title contains the substring. Case-sensitive. Matches against the markdown H1.                  |
| `--delay <SECONDS>`      | `GITBOOK_SCRAPER_DELAY`          | `0.3`            | Sleep before each request. Reduce for trusted upstreams or benchmarks.                                                        |
| `--retries <N>`          | —                                | `3`              | Retries per failed request on 5xx / 429.                                                                                       |
| `--concurrency <N>`      | —                                | `20`             | Max in-flight HTTP requests. Bounded by `--retries`.                                                                          |
| `--timeout <SECONDS>`    | —                                | `20`             | Per-request timeout.                                                                                                          |
| `--flat`                 | —                                | `false`          | Don't mirror the URL tree — flatten paths into single filenames (`a/b/c` → `a.b.c.md`).                                       |
| `--overwrite`            | —                                | `false`          | Overwrite existing files. By default, pre-existing files at the target path are **skipped**, so re-runs are idempotent.        |
| `--legacy`               | —                                | `false`          | Force legacy HTML-scraping mode for old (pre–Next.js) GitBook sites.                                                          |
| `--user-agent <UA>`      | `GITBOOK_SCRAPER_USER_AGENT`     | `doc-scraper-rs/0.1` | Override the `User-Agent` header.                                                                                          |
| `-v, --verbose`          | —                                | `false`          | `RUST_LOG=debug` output on stderr.                                                                                            |
| `-q, --quiet`            | —                                | `false`          | Suppress the progress bar.                                                                                                    |

CLI flags override env vars.

### Output structure

For a default run against `https://docs.strata.markets`:

```text
strata-docs/
├── llms.txt                  # one line per page: title, URL, first paragraph
├── llms-full.txt             # entire site concatenated, ready for LLM upload
├── AGENTS.md                 # synthesized project context for AI coding agents
├── index.md                  # sectioned TOC grouped by top-level path
├── skills/
│   ├── 00-introduction.md    # per-section deep dives, sorted alphabetically
│   ├── 01-markets.md
│   └── …
├── introduction/
│   ├── welcome-to-strata.md
│   ├── why-strata.md
│   ├── senior-tranche.md
│   └── junior-tranche.md
├── markets/
│   ├── ethena-usde/
│   │   └── srusde.md
│   └── …
└── …
```

For `--flat`:

```text
flat/
├── llms.txt
├── llms-full.txt
├── AGENTS.md
├── skills/
│   └── …
├── introduction.welcome-to-strata.md
├── introduction.why-strata.md
├── introduction.senior-tranche.md
└── markets.ethena-usde.srusde.md
```

### Filter semantics

`--filter` matches the **page title** extracted from the markdown body (the first H1 heading), with case-sensitive substring matching. Use it to scope a scrape to one section, e.g.:

```bash
doc-scraper https://docs.pareto.credit --filter Tranching
doc-scraper https://docs.strata.markets --filter Senior --filter Junior
```

Re-runs are safe even with filters active: `--overwrite` is not implied. Set `--overwrite` to refresh a previously-scraped output dir.

### Soft-404 fallback

If a sitemap URL's `.md` endpoint returns GitBook's "Page Not Found" stub (HTTP 200 with `# Page Not Found` as the first heading — the typical case when a page was published to the sitemap before the `.md` route was wired up), the scraper automatically falls back to the bare page URL. That fallback body is written to the same target path as the `.md` would have been. The detection is content-based because the soft 200 doesn't look like a 404 on the wire.

### Feeding LLMs

Every scrape produces two LLM-ready artefacts in the output directory:

- **`llms.txt`** — a [llmstxt.org](https://llmstxt.org/) catalog. One line per page: title, URL, optional first paragraph. Good as a navigation index.
- **`llms-full.txt`** — the entire site as one markdown file. Each page is emitted as `# Title`, `URL: ...`, body, then a `---` separator. This is the file you upload to OpenAI's Files API, Anthropic's prompt-cache attachments, or any RAG pipeline that wants the whole corpus at once.

Disable both with `--no-llms-txt`. Override the path with `--llms-txt <PATH>`; `llms-full.txt` lands next to it.

### Feeding agents

Most AI coding agents auto-load a single file as project-level context:

- `AGENTS.md` — Claude Code, OpenAI Codex, Amp, aider (with `--read`)
- `CLAUDE.md` — Claude Code alias
- `.cursorrules` / `.cursor/rules/*.mdc` — Cursor
- `.github/copilot-instructions.md` — GitHub Copilot

`doc-scraper-rs` writes `AGENTS.md` — a synthesized overview of the codebase with a "What this is" paragraph, a per-section page index, and links to per-topic deep-dive files. To convert it to a vendor-specific name:

```bash
cp AGENTS.md CLAUDE.md
cp AGENTS.md .cursorrules   # Cursor reads this as plain text
mkdir -p .github && cp AGENTS.md .github/copilot-instructions.md
```

Per-section deep-dive files land under `skills/00-…md`, `skills/01-…md`, …. The numeric prefix sorts them in the same order as AGENTS.md's **Sections** block, so an agent that reads AGENTS.md can `cat` the matching `skills/NN-*.md` on demand rather than loading the whole corpus into context. Each skill file has the same shape as `llms-full.txt` — one header followed by `# Title` + `URL:` + body + `---` per page — but scoped to a single section.

Disable `AGENTS.md` and `skills/` (along with `llms.txt` / `llms-full.txt`) with `--no-llms-txt`.

## Examples

The `examples/` directory ships one runnable smoke test:

- **`soft_404_smoke.rs`** — pulls a known GitBook site that returns the soft-404 stub on a subset of pages, and asserts the fallback produced a non-empty body.

  ```bash
  cargo run --example soft_404_smoke
  ```

Integration tests under `tests/integration.rs` spin up a `wiremock` server that emulates both a clean `.md` site and the soft-404 case end-to-end:

```bash
cargo test
```

## Configuration

All knobs are CLI flags or env vars listed above; no config file is read. The only env-var-driven behaviour at runtime is:

- `GITBOOK_SCRAPER_OUTPUT_DIR` — equivalent to `-o`.
- `GITBOOK_SCRAPER_LLMS_TXT` — equivalent to `--llms-txt`.
- `GITBOOK_SCRAPER_USER_AGENT` — equivalent to `--user-agent`.
- `GITBOOK_SCRAPER_DELAY` — equivalent to `--delay`.

## Project layout

```text
src/
├── main.rs          # CLI entry point + sitemap/llms.txt/llms-full.txt/AGENTS.md/skills/index orchestration
├── cli.rs           # clap derive for Cli struct
├── config.rs        # ResolvedConfig (merges CLI + env)
├── http.rs          # reqwest::Client builder + retry/backoff
├── sitemap.rs       # sitemap.xml + sitemap-pages.xml parsing
├── fetch.rs         # .md endpoint with soft-404 fallback + fan-out
├── writer.rs        # path planning + filesystem writes
├── markdown.rs      # generate_index / generate_llms_txt / generate_llms_full_txt / build_agents_md / generate_skill_md / group_into_sections
├── page.rs          # url ↔ filesystem path helpers + site slug derivation
├── error.rs         # ScraperError + Result alias
└── legacy.rs        # pre–Next.js HTML-scraper stub

tests/               # wiremock integration tests
examples/            # soft_404_smoke
```

## Benchmark

Measured against `https://docs.strata.markets` on an Intel MacBook Pro i5-8257U with default settings (20-way concurrency, 0.3 s per-request delay):

- **Pages scraped:** 34
- **Total output size:** ~172 KB of clean markdown
- **End-to-end time:** ~1.3 s
- **Effective rate:** **~26 pages/sec**

With `--delay 0` (no politeness), the median drops to ~0.95 s — **~36 pages/sec**, peaking near 48 pages/sec on warm runs.

Re-run on your own hardware to verify — network latency and the target site's HTML payload size will shift both numbers.

## MSRV

The minimum supported Rust version is **1.85** (`std::sync::LazyLock` from the standard library). Bumping the MSRV is a breaking change and will require a minor version bump.

## Roadmap

`doc-scraper-rs` is on the `0.1.x` track. The `0.1.0` release cut the first **crates.io publish**. Semver is meaningful from here: breaking changes require a `0.2.0` bump; additive changes go into `0.1.x`.

See [`CHANGELOG.md`](CHANGELOG.md) for the current state, including any breaking-change notes between releases.

## Contributing

Contributions welcome — please read [`CONTRIBUTING.md`](CONTRIBUTING.md) first. It covers building from source, the test layout, and the rules for adding new site-specific behaviour behind a CLI flag rather than as a special case in the main pipeline.

## Security

Report security issues privately — see [`SECURITY.md`](SECURITY.md). **Do not** file public GitHub issues for vulnerabilities.

## License

Licensed under [MIT](LICENSE).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be licensed under MIT, without any additional terms or conditions. (Adapted boilerplate — this crate is MIT-only; the Apache-2.0 reference is for contributors who dual-license their code upstream.)
