# Contributing to `doc-scraper-rs`

Thanks for your interest in `doc-scraper-rs`. This document covers how to
build from source, run the tests, and add new site-specific behaviour
without growing the CLI surface by accident.

## Building from source

You need a stable Rust toolchain (1.85 or newer — see `rust-version` in
`Cargo.toml`). If you don't have one yet:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"     # pick up the new PATH in this shell
```

Then:

```bash
git clone https://github.com/dibakarsutradhar/doc-scraper-rs
cd doc-scraper-rs
cargo build --release
```

The release binary lands at `target/release/doc-scraper`.

## Running the tests

```bash
cargo test                    # all unit + integration tests
cargo test --release          # closer to bench numbers, slower compile
```

Integration tests under `tests/integration.rs` spin up `wiremock` servers
that emulate GitBook's `.md` endpoint, the soft-404 stub, and the HTML
fallback. They run offline — no network is required.

## Lints

The crate uses [`clippy.toml`](clippy.toml) and `rustfmt.toml` if they
exist. Run `cargo clippy --all-targets --all-features -- -D warnings`
and `cargo fmt --check` before opening a PR.

## Adding new site-specific behaviour

The fastest way to regress this crate is to grow the CLI surface with
per-site flags ("`--strata-mode`", "`--pareto-tweak`"). Don't.

When you need to handle a behaviour specific to one site:

1. If the existing `.md`-endpoint-and-soft-404-fallback pipeline produces
   reasonable output without your change, prefer that.
2. If a small heuristic solves it (e.g. detecting another GitBook
   soft-404 signature), put it in `fetch.rs` next to the existing one.
3. If neither applies, open an issue first and explain the concrete
   input → desired output case before adding code.

## Adding a CLI flag

Backwards compatibility matters on the `0.1.x` track. Before introducing
a new flag:

- Is it actually expressible as `--filter <X>` or a `--delay` /
  `--concurrency` tweak?
- Does the existing default produce acceptable output for the common
  case, or is this flag always going to be needed?
- Will the flag accept the value users would naturally type (e.g.
  relative paths vs absolute, plain strings vs globs)?

Flag additions need a unit test where reasonable, and a one-line mention
in `README.md`'s flag table.

## Project layout

See `README.md` § "Project layout" for the module map. The high-level
shape is:

- `src/main.rs` — orchestration: sitemap → fetch → write → index/llms.
- `src/fetch.rs` — `.md` endpoint, soft-404 fallback, fan-out.
- `src/sitemap.rs` — sitemap.xml / sitemap-pages.xml parsing.
- `src/writer.rs` — on-disk path planning; collision counter for flat mode.
- `src/markdown.rs` — `index.md` and `llms.txt` generation.
- `src/page.rs` — URL ↔ filesystem path and site-slug helpers.
- `src/http.rs` — reqwest client construction, retry/backoff.
- `src/cli.rs` — clap derive for the `Cli` struct.
- `src/config.rs` — `ResolvedConfig`, merges CLI + env.
- `src/error.rs` — `ScraperError` and the crate's `Result` alias.
- `src/legacy.rs` — pre–Next.js HTML scraper (currently a stub).

## Commit messages

Use the imperative mood ("add soft-404 fallback", not "added…"). Reference
any related issue in the body (`Fixes #12`).

## Releases

`doc-scraper-rs` follows semver. Bumping the MSRV is a breaking change
and requires a minor version bump. The MSRV is documented in both
`Cargo.toml` (`rust-version`) and the README badge.
