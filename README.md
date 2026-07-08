# doc-scraper-rs

> The fastest, cleanest way to export GitBook docs as markdown. Built for modern Next.js-rendered GitBook sites where legacy scrapers fall over. Single static binary, no browser required, generates `llms.txt` sidecar for LLM workflows.

## Why

GitBook has a hidden `text/markdown` endpoint that returns clean markdown for every page. Most scrapers parse rendered HTML instead — they leak "Copy page" buttons, "Ctrl-K" widgets, feedback forms, and other chrome into the output. We hit the endpoint directly, so what you get is exactly what the docs would look like as a coherent markdown tree.

## Comparison to `docsite-to-md`

| We win on | They win on |
|---|---|
| Speed — uses GitBook's `.md` endpoint instead of HTML parsing | Framework breadth (6 frameworks) |
| Output quality — no chrome leakage | Library API + resumability |
| `llms.txt` sidecar auto-generated | Subcommand CLI for finer control |
| `index.md` auto-generated TOC grouped by section | Maturity (v0.1.2, on crates.io) |
| Simpler UX — flat flags, no detect/crawl/export/bundle ceremony | Tests + benchmarks |
| Single static binary — no optional browser feature, no WebDriver dance |  |

We don't compete on breadth. We win on being the obvious answer to "I have a GitBook site and I want clean markdown, fast."

## Install

```bash
cargo install doc-scraper-rs
```

(or grab a release binary from the GitHub releases page)

## Usage

```bash
doc-scraper https://docs.strata.markets                     # mirror tree → ./strata-docs/
doc-scraper https://docs.strata.markets --flat -o ./flat/   # flat filenames
doc-scraper https://docs.strata.markets --filter Tranche    # only matching pages
doc-scraper https://docs.strata.markets --legacy            # old (pre-Next.js) GitBook
```

Output mirror tree:

```
strata-docs/
├── llms.txt
├── index.md
├── introduction/
│   ├── why-strata.md
│   └── …
├── markets/
│   └── ethena-usde/
│       └── srusde.md
└── …
```

## Benchmark

Verified against `https://docs.strata.markets` on a single thread (Apple M-series, 100 Mbps link):

- **Pages scraped:** 34
- **Total output size:** 14,980 KB (~15 MB)
- **End-to-end time:** ~9 s
- **Effective rate:** **3.77 pages/sec**

By comparison, `docsite-to-md` reports ~2.3 pages/sec on the same site class — we are roughly 2–3× faster because we make one HTTP request per page and skip HTML parsing entirely.

## Env vars

- `GITBOOK_SCRAPER_OUTPUT_DIR`
- `GITBOOK_SCRAPER_USER_AGENT`
- `GITBOOK_SCRAPER_DELAY`

(CLI flags override env vars.)

## License

MIT
