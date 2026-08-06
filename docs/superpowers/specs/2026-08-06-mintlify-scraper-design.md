# Mintlify support in `doc-scraper-rs`

Status: approved
Date: 2026-08-06

## Goal

`doc-scraper-rs` today scrapes GitBook sites by hitting their hidden `.md` endpoint and producing a clean markdown mirror tree plus `llms.txt` / `llms-full.txt` / `AGENTS.md` / `skills/` sidecars. We want the same one-command experience for Mintlify-hosted docs (e.g. `https://docs.livepeer.org`) with **no new user-facing flag**.

After this change:

```bash
doc-scraper https://docs.livepeer.org
# Just works. Produces ./livepeer-docs/ the same way it does for GitBook.
```

## Background — what Mintlify and GitBook have in common

Both platforms expose, for every public page, a clean markdown view at the same URL with `.md` appended. Both ship a `sitemap.xml`. Both serve a `/llms.txt` summary. The only structural difference is in the sitemap's XML shape:

| Platform | `/sitemap.xml` shape |
|---|---|
| GitBook (Next.js) | `<sitemapindex>` pointing at `<sitemap-pages.xml>` (which holds the actual `<urlset>`) |
| Mintlify | flat `<urlset>` directly in `/sitemap.xml` |

Mintlify also prepends a small banner to every `.md` response:

```
> ## Documentation Index
> Fetch the complete documentation index at: https://docs.livepeer.org/llms.txt
> Use this file to discover all available pages before exploring further.

# <Real Page Title>
…
```

And Mintlify's `.md` endpoint may return a short stub for missing pages (similar to GitBook's soft-404), but with a different signature.

## Non-goals

- No new CLI flag. Auto-detection only.
- No HTML-to-markdown conversion. If the `.md` endpoint is missing for a page, we skip it (counted as a fetch error) rather than fetching the HTML and parsing it.
- No use of Mintlify's `/llms-full.txt` as a fetch source. We always fetch per-page so the mirror tree matches the sitemap shape.
- No Mintlify-specific branding in the output (no Mintlify metadata in headers, no different file layout). Output structure stays identical to the GitBook path.
- No upstream Mintlify-specific soft-404 retry queue. Stub pages are dropped; if the user sees them in the error count, they can re-run with `--verbose`.

## Design

### High-level approach

1. Fetch `/sitemap.xml` once.
2. Detect the shape by inspecting the root element:
   - `<sitemapindex>` → `SitemapShape::GitBookIndex` (existing path, unchanged).
   - `<urlset>` → `SitemapShape::MintlifyUrlset` (new path).
3. Parse the page list accordingly.
4. Fetch each page via `.md` (same code path either way).
5. Apply Mintlify-specific post-processing **only when shape == MintlifyUrlset**: strip the doc-index banner, drop stub bodies.
6. Everything downstream (writing, `llms.txt`, `llms-full.txt`, `AGENTS.md`, `skills/`, `index.md`) is shape-unaware and unchanged.

### Module changes

#### `src/sitemap.rs` — add shape detection and a flat-urlset parser

New public enum:

```rust
pub enum SitemapShape {
    GitBookIndex,
    MintlifyUrlset,
}
```

Changed signature:

```rust
pub async fn fetch_sitemap(
    client: &reqwest::Client,
    base_url: &Url,
) -> Result<(SitemapShape, Vec<PageRef>)>
```

Behavior:

1. `GET /sitemap.xml`, parse with `roxmltree`.
2. Inspect root element tag name:
   - `sitemapindex` → existing path: extract `<loc>`, fetch and parse `sitemap-pages.xml`, return `(GitBookIndex, pages)`.
   - `urlset` → call new `parse_sitemap_urlset(body)` directly, return `(MintlifyUrlset, pages)`.
   - Anything else → `ScraperError::Sitemap("unknown sitemap shape; expected <sitemapindex> or <urlset>")`.

New pure parser:

```rust
pub fn parse_sitemap_urlset(body: &str) -> Result<Vec<PageRef>>
```

Walks the `<urlset>` exactly like `parse_sitemap_pages` does today (looking for `<url>` → `<loc>` plus optional `<lastmod>`). Keeps the two parsers separate so each one stays focused and easy to unit-test.

#### `src/fetch.rs` — banner stripping and stub detection

Two new pure helpers:

```rust
/// Returns `body` with the Mintlify doc-index banner removed, if present.
/// If the prefix doesn't match, returns `body` unchanged. Never panics.
pub fn strip_mintlify_banner(body: &str) -> &str

/// Returns `true` when `body` looks like a Mintlify stub rather than a real
/// page. Heuristic: fewer than 100 bytes **and** no `# ` heading line.
pub fn is_mintlify_stub(body: &str) -> bool
```

`fetch_one` signature gains a `SitemapShape` parameter:

```rust
async fn fetch_one(
    client: &reqwest::Client,
    url: &Url,
    retries: u32,
    delay_secs: f64,
    shape: SitemapShape,
) -> Result<String>
```

Behavior:

- For `GitBookIndex`: unchanged (existing `.md` fetch + soft-404 fallback).
- For `MintlifyUrlset`:
  - Fetch the `.md` URL.
  - If `is_mintlify_stub(body)` → return `Err(ScraperError::Other("mintlify stub"))`.
  - Otherwise return `strip_mintlify_banner(body)`.

`fetch_all` similarly threads `shape` through to `fetch_one`.

#### `src/main.rs` — single call site change

Only one call site changes:

```rust
// Before:
let pages = sitemap::fetch_sitemap(&client, &cfg.url).await?;

// After:
let (shape, pages) = sitemap::fetch_sitemap(&client, &cfg.url).await?;
```

The shape is passed into `fetch_all(...)`. No changes elsewhere in `main.rs`. The error counter already increments on `Err` from `fetch_all`, so stub pages correctly contribute to `exit 2`.

### Output

Identical to the GitBook path. Per-page files are written to `<output>/<url-path>/<slug>.md`. `llms.txt`, `llms-full.txt`, `AGENTS.md`, `skills/`, and `index.md` are generated the same way. The only observable difference is that Mintlify pages have no banner in them.

### Error handling

| Condition | Behavior |
|---|---|
| Unknown sitemap root element | `ScraperError::Sitemap` — surfaced as fatal error. |
| Empty `<urlset>` (zero pages) | Existing "sitemap returned zero pages" fatal error. |
| Mintlify `.md` returns a stub | Counted as a fetch error; `exit 2` if any other errors. Page is **not** written. |
| Banner prefix doesn't match | Banner left in place (very defensive — also covers the rare case where Mintlify changes the wording). |
| Mintlify stub heuristic false negative | A page that's an unusual short page (no heading, <100 bytes) would be skipped. Acceptable trade-off; the typical Mintlify page is well over 100 bytes. |

### Testing

Unit tests in `src/sitemap.rs`:

- `parse_sitemap_urlset_happy_path` — three `<url>` entries, two with `<lastmod>`.
- `parse_sitemap_urlset_errors_on_url_missing_loc` — `<url></url>` returns `ScraperError::Sitemap`.
- `parse_sitemap_urlset_errors_on_invalid_xml` — `<<not xml>>` returns `ScraperError::Xml`.
- `fetch_sitemap_detects_gitbook_index` — wiremock serves a `<sitemapindex>` body that links to a `sitemap-pages.xml` mock; assert `(GitBookIndex, pages)`.
- `fetch_sitemap_detects_mintlify_urlset` — wiremock serves a flat `<urlset>` body; assert `(MintlifyUrlset, pages)`.
- `fetch_sitemap_errors_on_unknown_shape` — wiremock serves an XML file with an unrelated root element; assert `ScraperError::Sitemap`.

Unit tests in `src/fetch.rs`:

- `strip_mintlify_banner_strips_exact_prefix` — body with the exact banner returns the substring after it.
- `strip_mintlify_banner_unchanged_when_missing` — body without the banner returns the original.
- `strip_mintlify_banner_unchanged_on_partial_match` — body with only the first line of the banner returns the original (conservative).
- `is_mintlify_stub_short_no_heading_true` — 50 bytes, no `# ` → true.
- `is_mintlify_stub_short_with_heading_false` — 50 bytes including `# X` → false.
- `is_mintlify_stub_long_no_heading_false` — 500 bytes, no `# ` → false.
- `fetch_one_mintlify_strips_banner_and_returns_body` — wiremock serves `.md` with banner; assert returned body has no banner prefix.
- `fetch_one_mintlify_stub_returns_error` — wiremock serves a 60-byte `.md` body with no heading; assert `Err`.

Integration test in `tests/integration.rs` (new):

- `mintlify_end_to_end` — wiremock serves:
  - `/sitemap.xml` → flat `<urlset>` with three pages.
  - Each page's `.md` URL → body with the doc-index banner prepended.
  - Asserts: the output directory contains three files, none of them containing the banner substring.

### Docs

`README.md`:

- Add a row to the Features table: "Mintlify site auto-detection" — one-line description.
- Add a short subsection under "Usage" titled "Mintlify sites" with one example: `doc-scraper https://docs.livepeer.org` and a note that no extra flag is needed.
- Add `https://docs.livepeer.org` to the example list near the top.

No CLI flag changes (no new clap field, no new env var).

## Open questions

None.

## Migration / rollout

- Behind an enum in `src/sitemap.rs` — no migration flag needed.
- Pure addition: doesn't change any existing test expectations, doesn't change the on-disk format for GitBook sites.
- `CHANGELOG.md` gets a "Added" entry under the next version.
- A new `examples/mintlify_smoke.rs` will mirror the existing `examples/soft_404_smoke.rs` style for manual verification.
