//! `doc-scraper-rs` — CLI to export GitBook documentation as a clean markdown
//! mirror tree, plus auto-generated `llms.txt`, `llms-full.txt`, `AGENTS.md`,
//! and per-section `skills/` sidecars ready for direct LLM ingestion and
//! AI coding-agent context.
//!
//! See the [README](https://github.com/dibakarsutradhar/doc-scraper-rs) for
//! usage, the [CHANGELOG](../CHANGELOG.md) for what's changed, and the
//! rendered rustdoc on [docs.rs](https://docs.rs/doc-scraper-rs) for
//! module-level details.
//!
//! ## Quickstart
//!
//! ```text
//! doc-scraper https://docs.strata.markets
//! ```
//!
//! ## Module map
//!
//! - [`cli`] — clap derive for the `Cli` struct.
//! - [`config`] — `ResolvedConfig` (merges CLI + env).
//! - [`error`] — `ScraperError` + the crate's `Result` alias.
//! - [`fetch`] — `.md` endpoint fetcher with soft-404 fallback + fan-out.
//! - [`http`] — reqwest client construction, retry/backoff.
//! - [`legacy`] — pre–Next.js HTML scraper (currently a stub).
//! - [`markdown`] — `index.md`, `llms.txt`, `llms-full.txt`, `AGENTS.md`,
//!   and `skills/*.md` generation.
//! - [`page`] — URL ↔ filesystem path and site-slug helpers.
//! - [`sitemap`] — `sitemap.xml` / `sitemap-pages.xml` parsing.
//! - [`writer`] — on-disk path planning; collision counter for `--flat`.

use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;
use tracing_subscriber::EnvFilter;

mod cli;
mod config;
mod error;
mod fetch;
mod http;
mod legacy;
mod markdown;
mod page;
mod sitemap;
mod writer;

use cli::Cli;
use config::ResolvedConfig;
use error::{Result, ScraperError};
use fetch::fetch_all;
use http::{build_client, fetch_with_retry};
use markdown::{
    build_agents_md, generate_index, generate_llms_full_txt, generate_llms_txt, generate_skill_md,
    group_into_sections, slugify_section, AgentSection, LlmsEntry,
};
use page::Page;
use writer::{write_page, WriteOutcome};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut cfg = ResolvedConfig::from_cli(cli);

    if cfg.verbose {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("debug"))
            .with_writer(std::io::stderr)
            .init();
    }

    // NOTE: create_dir_all is intentionally deferred until after the sitemap
    // succeeds — see the "do not create on zero-page" check below (C6).

    if cfg.legacy {
        std::fs::create_dir_all(&cfg.output_dir)
            .with_context(|| format!("create output dir {:?}", cfg.output_dir))?;
        return legacy::scrape(&cfg)
            .await
            .map_err(|e| anyhow::anyhow!("legacy scrape failed: {e}"));
    }

    let client = build_client(&cfg.user_agent, cfg.timeout_secs)?;

    // 1. Sitemap
    eprintln!("Fetching sitemap for {}...", cfg.url);
    let pages = sitemap::fetch_sitemap(&client, &cfg.url).await?;
    if pages.is_empty() {
        // C6: do NOT create the output dir before we know we have work to do.
        anyhow::bail!("sitemap returned zero pages");
    }

    // Now that we have ≥1 page we know we'll need the output directory.
    std::fs::create_dir_all(&cfg.output_dir)
        .with_context(|| format!("create output dir {:?}", cfg.output_dir))?;
    // Canonicalize so subsequent `path.strip_prefix(&cfg.output_dir)` calls
    // work even when the user passed a relative path like `./pareto-docs/`.
    // `write_page` returns absolute paths because `dir.join(...).exists()` etc.
    // resolve against the cwd. Without this, the strip_prefix silently fails
    // and the homepage fallback's written file is never recorded in
    // `written_files` — making the "wrote N pages" count under-report.
    if let Ok(canon) = std::fs::canonicalize(&cfg.output_dir) {
        cfg.output_dir = canon;
    }

    // 2. Homepage fallback via llms.txt
    // I7: surface errors in --verbose without aborting the run.
    let homepage_fallback = match fetch_llms_txt(&client, &cfg.url).await {
        Ok(body) => Some(body),
        Err(e) => {
            if cfg.verbose {
                eprintln!("homepage llms.txt fallback failed: {e}");
            }
            None
        }
    };

    // 3. Fetch
    let pb = if cfg.quiet {
        ProgressBar::hidden()
    } else {
        mk_progress(pages.len())
    };
    pb.set_message("scraping");

    let results = fetch_all(
        client.clone(),
        pages.clone(),
        cfg.concurrency,
        cfg.retries,
        cfg.delay_secs,
    )
    .await;
    pb.finish_and_clear();

    // 4. Process results
    let mut written_files: Vec<(String, String, String)> = Vec::new(); // (title, url, rel-path)
                                                                       // (title, url, body, first_paragraph) — the body is kept in memory so we
                                                                       // can later emit `llms-full.txt` without re-reading files from disk.
    let mut llms_entries: Vec<LlmsEntry> = Vec::new();
    let mut slug_counts: HashMap<String, usize> = HashMap::new();
    let mut errors: u32 = 0;

    // I8: count fetch errors so `exit 2` correctly reflects partial failures.
    // Iterate without `.flatten()` and handle Ok / Err explicitly.
    for item in results.into_iter() {
        let (page_ref, body) = match item {
            Ok(pair) => pair,
            Err((page_ref, e)) => {
                // Homepage URLs in sitemaps have no `.md` form — the
                // `homepage_fallback` path below will fetch `/llms.txt` for
                // them. Don't surface that as a user-visible error or
                // `exit 2` will fire on otherwise-successful runs.
                let is_home = page_ref.loc.host_str() == cfg.url.host_str()
                    && (page_ref.loc.path() == "/" || page_ref.loc.path().is_empty());
                if !is_home {
                    errors += 1;
                    eprintln!("fetch error: {e}");
                }
                continue;
            }
        };
        let page = Page {
            url: page_ref.loc.clone(),
            title: extract_title(&body, page_ref.loc.as_str()),
            body: body.clone(),
        };
        // filter
        if !cfg.filters.is_empty()
            && !cfg
                .filters
                .iter()
                .any(|f| page.title.as_deref().unwrap_or("").contains(f))
        {
            continue;
        }
        // Write
        match write_page(
            page.clone(),
            body.clone(),
            &cfg.output_dir,
            cfg.flat,
            cfg.overwrite,
            &cfg.url,
            &mut slug_counts,
        ) {
            Ok(WriteOutcome::Written(path)) => {
                if let Ok(rel) = path.strip_prefix(&cfg.output_dir) {
                    let url = rel
                        .to_string_lossy()
                        .replace('\\', "/")
                        .trim_end_matches(".md")
                        .to_string();
                    let title = page
                        .title
                        .clone()
                        .unwrap_or_else(|| rel.to_string_lossy().into_owned());
                    written_files.push((title.clone(), url, rel.to_string_lossy().into_owned()));
                    let first_para = first_paragraph(&page.body);
                    llms_entries.push((title, page_ref.loc.to_string(), body.clone(), first_para));
                }
            }
            Ok(WriteOutcome::Skipped(_)) => {}
            Err(e) => {
                errors += 1;
                eprintln!("write error: {e}");
            }
        }
    }

    // Apply homepage fallback if no page was produced for the homepage (rare 404 case).
    // C1: compare against the actual homepage URL (not ""), and when the fallback
    // writes a new file, record it so llms.txt stays consistent with the filesystem.
    if let Some(body) = homepage_fallback {
        let homepage_url_str = cfg.url.as_str().trim_end_matches('/').to_string();
        let already = written_files.iter().any(|(_, _, rel)| {
            let trimmed = rel.trim_end_matches(".md");
            trimmed == "index" || trimmed.is_empty() || trimmed == homepage_url_str
        });
        if !already {
            let page = Page {
                url: cfg.url.clone(),
                title: extract_title(&body, cfg.url.as_str()),
                body: body.clone(),
            };
            match write_page(
                page.clone(),
                body,
                &cfg.output_dir,
                cfg.flat,
                cfg.overwrite,
                &cfg.url,
                &mut slug_counts,
            ) {
                Ok(WriteOutcome::Written(path)) => {
                    if let Ok(rel) = path.strip_prefix(&cfg.output_dir) {
                        let rel_str = rel.to_string_lossy().replace('\\', "/");
                        let url = rel_str.trim_end_matches(".md").to_string();
                        let title = page.title.clone().unwrap_or_else(|| rel_str.clone());
                        written_files.push((title.clone(), url, rel_str.clone()));
                        llms_entries.push((
                            title,
                            cfg.url.to_string(),
                            page.body.clone(),
                            first_paragraph(&page.body),
                        ));
                    }
                }
                Ok(WriteOutcome::Skipped(_)) => {}
                Err(e) => {
                    errors += 1;
                    eprintln!("homepage fallback write error: {e}");
                }
            }
        }
    }

    // 5. index.md (C5: do not propagate io::Error mid-pipeline).
    if cfg.toc {
        let sections = group_into_sections(written_files.clone());
        let out = generate_index(&sections);
        match std::fs::write(cfg.output_dir.join("index.md"), out) {
            Ok(()) => {}
            Err(e) => {
                errors += 1;
                eprintln!("write index.md: {e}");
            }
        }
    }

    // 6. llms.txt (C5: same handling as index.md).
    if let Some(path) = cfg.llms_txt_path.as_ref() {
        // Ensure parent dir exists (mainly for the index.md write path).
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let out = generate_llms_txt(&llms_entries);
        match std::fs::write(path, out) {
            Ok(()) => {}
            Err(e) => {
                errors += 1;
                eprintln!("write llms.txt: {e}");
            }
        }

        // 6b. llms-full.txt — the entire site as one file, suitable for direct
        // upload to LLM provider file APIs. Sits next to llms.txt; uses
        // `with_file_name` so a custom `--llms-txt <path>` directory still
        // produces `<dir>/llms-full.txt`.
        let full_path = path.with_file_name("llms-full.txt");
        let full_pages: Vec<(String, String, String)> = llms_entries
            .iter()
            .map(|(t, u, b, _)| (t.clone(), u.clone(), b.clone()))
            .collect();
        let site_title = cfg.url.host_str().unwrap_or("site");
        let summary = format!(
            "Concatenated markdown for {} — {} pages.",
            cfg.url,
            full_pages.len()
        );
        let full = generate_llms_full_txt(site_title, &summary, &full_pages);
        match std::fs::write(&full_path, full) {
            Ok(()) => {}
            Err(e) => {
                errors += 1;
                eprintln!("write llms-full.txt: {e}");
            }
        }

        // 6c. AGENTS.md — synthesized project context, auto-loaded by Claude
        // Code / Cursor / Copilot / OpenAI Codex / aider.
        let sections_for_agents = build_sections_for_agents(&llms_entries);
        let overview_first_para = pick_overview_first_para(&llms_entries);
        let agents = build_agents_md(
            site_title,
            cfg.url.as_str(),
            &today_ymd(),
            &sections_for_agents,
            overview_first_para.as_deref(),
        );
        let agents_path = cfg.output_dir.join("AGENTS.md");
        match std::fs::write(&agents_path, agents) {
            Ok(()) => {}
            Err(e) => {
                errors += 1;
                eprintln!("write AGENTS.md: {e}");
            }
        }

        // 6d. skills/ — per-topic deep dives, one file per top-level section.
        let skills_dir = cfg.output_dir.join("skills");
        if let Err(e) = std::fs::create_dir_all(&skills_dir) {
            errors += 1;
            eprintln!("create skills dir: {e}");
        } else {
            // Build a body lookup by URL so we can hand each section the raw
            // markdown bodies of its pages (the same data the full-txt pass
            // holds in memory, just bucketed by section).
            let bodies_by_url: HashMap<&str, &str> = llms_entries
                .iter()
                .map(|(_, u, b, _)| (u.as_str(), b.as_str()))
                .collect();
            for (idx, (section_title, entries)) in sections_for_agents.iter().enumerate() {
                let pages: Vec<(String, String, String)> = entries
                    .iter()
                    .filter_map(|entry: &(String, String, String)| {
                        let (title, url, _relpath) = entry;
                        bodies_by_url
                            .get(url.as_str())
                            .map(|body: &&str| (title.clone(), url.clone(), (*body).to_string()))
                    })
                    .collect();
                let body = generate_skill_md(section_title, site_title, &pages);
                let slug = slugify_section(section_title);
                let fname = format!("{:02}-{slug}.md", idx);
                match std::fs::write(skills_dir.join(&fname), body) {
                    Ok(()) => {}
                    Err(e) => {
                        errors += 1;
                        eprintln!("write skills/{fname}: {e}");
                    }
                }
            }
        }
    }

    if errors > 0 {
        eprintln!("done with {errors} errors");
        std::process::exit(2);
    }
    eprintln!(
        "wrote {} pages to {:?}",
        written_files.len(),
        cfg.output_dir
    );
    Ok(())
}

fn mk_progress(total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(ProgressStyle::with_template("{bar:40} {pos}/{len} {msg}").unwrap());
    pb
}

/// Bucket `llms_entries` by URL path's top-level segment ("markets/ethena-usde"
/// → "markets"). Returns `AgentSection` tuples in sorted-key order so AGENTS.md
/// and the `skills/` directory have a stable, alphabetic layout.
///
/// The third tuple element per page is the relpath used for AGENTS.md links —
/// derived from the URL path with `.md` appended, matching the mirror tree that
/// `url_to_relative_path` produces. The homepage URL (`/` or empty) becomes
/// `index.md`, matching the on-disk filename the writer produces for it.
fn build_sections_for_agents(entries: &[LlmsEntry]) -> Vec<AgentSection> {
    let mut map: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for (title, url, _body, _descr) in entries {
        let path = url_path_segment(url);
        let key = if path.is_empty() || path == "/" {
            "index".into()
        } else {
            path.trim_start_matches('/')
                .split('/')
                .next()
                .unwrap_or("")
                .to_string()
        };
        let key = if key.is_empty() { "index".into() } else { key };
        // Relpath: URL path + ".md". For the homepage (`/`), this is "index.md".
        // Drop query/fragment first — sitemap URLs are clean but be safe.
        let clean_url = url.split(['?', '#']).next().unwrap_or(url).to_string();
        let path_for_link = if path.is_empty() || path == "/" {
            "index".to_string()
        } else {
            path.trim_start_matches('/').to_string()
        };
        let relpath = format!("{path_for_link}.md");
        map.entry(key)
            .or_default()
            .push((title.clone(), clean_url, relpath));
    }
    map.into_iter().collect()
}

/// Extract the path portion of a URL string, tolerating non-URL input. We
/// don't use `url::Url::parse` here because the call site is hot (per page,
/// per pass) and the input is already a known-good URL we just stored.
fn url_path_segment(url: &str) -> String {
    // Find the path component: after scheme://host. We use a simple `find` for
    // `://` rather than full URL parsing because we only need the path.
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        if let Some(path_start) = after.find('/') {
            return after[path_start..].to_string();
        }
        return String::new();
    }
    // Already a path-only string (e.g. "/introduction/why-strata").
    url.to_string()
}

/// Detect GitBook's "this page is available as Markdown" redirect stub. The
/// stub is a blockquote that appears as the first paragraph of nearly every
/// scraped page and adds nothing to the page's actual content. Signature:
/// starts with `> For the complete documentation index, see [llms.txt]` or
/// `> Markdown versions of documentation pages are available…`.
fn is_gitbook_redirect_stub(paragraph: &str) -> bool {
    let p = paragraph.trim_start();
    p.starts_with("> For the complete documentation index, see [llms.txt]")
        || p.starts_with("> Markdown versions of documentation pages are available")
}

/// Pick the "What this is" paragraph for AGENTS.md. Heuristic:
/// 1. Among pages in the `introduction` section (or whose title contains
///    "Overview" / "Protocol"), prefer the **longest** non-empty
///    `first_paragraph` that is *not* a GitBook redirect stub. Stubs are
///    short and boilerplate; the real protocol summary is usually the
///    longest paragraph in the intro section.
/// 2. Otherwise, the longest non-stub `first_paragraph` of any entry.
/// 3. Otherwise `None`.
fn pick_overview_first_para(entries: &[LlmsEntry]) -> Option<String> {
    let is_candidate = |t: &str, url: &str| -> bool {
        let path = url_path_segment(url);
        let top = path.trim_start_matches('/').split('/').next().unwrap_or("");
        top.eq_ignore_ascii_case("introduction")
            || t.to_lowercase().contains("overview")
            || t.to_lowercase().contains("protocol")
    };
    let cleaned = |d: &Option<String>| -> Option<String> {
        d.as_ref()
            .filter(|s| !s.trim().is_empty())
            .filter(|s| !is_gitbook_redirect_stub(s))
            .cloned()
    };
    let mut best: Option<String> = None;
    for (t, url, _, d) in entries {
        let Some(p) = cleaned(d) else { continue };
        if !is_candidate(t, url) {
            continue;
        }
        if best.as_ref().is_none_or(|b| p.len() > b.len()) {
            best = Some(p);
        }
    }
    if best.is_some() {
        return best;
    }
    // Fall back to the longest non-stub paragraph of any entry.
    entries
        .iter()
        .filter_map(|(_, _, _, d)| cleaned(d))
        .max_by_key(|s| s.len())
}

/// `YYYY-MM-DD` formatted from `SystemTime::now()`. We don't pull in `chrono`
/// just for this — the helper is a dozen lines and stays accurate until 2100.
fn today_ymd() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Days since 1970-01-01 → Gregorian date. Algorithm from Howard Hinnant's
    // `civil_from_days`, public domain.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146_096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// First-line ATX H1 heading. Used to extract a page title from the markdown
/// body returned by GitBook's `.md` endpoint. Compiled once.
static H1_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*#\s+(.+?)\s*$").expect("H1 regex"));

fn extract_title(body: &str, url: &str) -> Option<String> {
    // Strategy 1: read the first markdown H1 heading. We fetch `.md` endpoints,
    // so the body is markdown — there is no `<title>` tag. This regex is
    // intentionally strict: it must be a line that starts with `#` (not `##`
    // or `#hashtag`), so we don't pick up a `# Heading` inside fenced code or
    // a H2/H3. Anchor-like `#` lines (e.g. `<a id="…">` blocks) don't start
    // with `# ` so they're filtered out by the trailing space requirement.
    if let Some(caps) = H1_RE.captures(body) {
        let t = caps.get(1)?.as_str().trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    // Strategy 2: derive human-readable title from URL path's last non-empty segment.
    // I2: replace '-' and '_' with spaces and trim so the slug is "humanized"
    // (e.g. `srusde` → `srusde`, `why-strata` → `why strata`). Kept lowercase;
    // title-casing is deferred.
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(seg) = parsed
            .path_segments()
            .and_then(|mut s| s.rfind(|x| !x.is_empty()))
        {
            let human: String = seg.replace(['-', '_'], " ");
            let trimmed = human.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn first_paragraph(body: &str) -> Option<String> {
    // Skip blank lines, ATX headings, GitBook redirect-stub blockquotes, and
    // bullet / numbered list items so the description we surface is actual
    // prose. A line that starts with whitespace + `>` / `-` / `*` / `+` / a
    // digit is treated as the kind of metadata we want to skip past.
    fn is_skippable(line: &str) -> bool {
        let t = line.trim_start();
        if t.is_empty() {
            return true;
        }
        if t.starts_with('#') {
            return true;
        }
        if t.starts_with('>') {
            return true;
        }
        if let Some(rest) = t.strip_prefix(|c: char| "-*+".contains(c)) {
            // bullet item: must be followed by space or end-of-line
            return rest.is_empty() || rest.starts_with(char::is_whitespace);
        }
        if t.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            // numbered list: "1. " or "1) "
            if let Some(rest) = t.strip_prefix(|c: char| c.is_ascii_digit()) {
                let rest = rest.trim_start();
                if rest.starts_with('.') || rest.starts_with(')') {
                    return rest.len() <= 1 || rest[1..].starts_with(char::is_whitespace);
                }
            }
        }
        false
    }
    body.lines()
        .skip_while(|l| is_skippable(l))
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

async fn fetch_llms_txt(client: &reqwest::Client, base: &url::Url) -> Result<String> {
    let url = base.join("/llms.txt")?;
    let resp = fetch_with_retry(client, &url, 1, 0.0).await?;
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !ct.contains("text/markdown") && !ct.contains("text/plain") {
        return Err(ScraperError::Other(format!("unexpected content-type {ct}")));
    }
    resp.text().await.map_err(ScraperError::Http)
}

#[cfg(test)]
mod tests {
    use super::{
        build_sections_for_agents, extract_title, is_gitbook_redirect_stub,
        pick_overview_first_para,
    };
    use crate::markdown::LlmsEntry;

    fn entry(title: &str, url: &str, body: &str, descr: Option<&str>) -> LlmsEntry {
        (
            title.into(),
            url.into(),
            body.into(),
            descr.map(String::from),
        )
    }

    #[test]
    fn extract_title_from_markdown_h1() {
        let body = "# Foo\n\nsome body text\n";
        assert_eq!(
            extract_title(body, "https://example.com/x"),
            Some("Foo".to_string())
        );
    }

    #[test]
    fn extract_title_handles_indented_h1() {
        // Up to 3 spaces of indent on an ATX heading is legal CommonMark.
        let body = "   # Foo\n\nbody\n";
        assert_eq!(
            extract_title(body, "https://example.com/x"),
            Some("Foo".to_string())
        );
    }

    #[test]
    fn extract_title_trims_whitespace() {
        let body = "#    Bar   \n\nbody\n";
        assert_eq!(
            extract_title(body, "https://example.com/x"),
            Some("Bar".to_string())
        );
    }

    #[test]
    fn extract_title_picks_first_h1_even_if_h2_appears_first() {
        // GitBook pages sometimes have an H2 above the document H1; we want
        // the first H1 specifically.
        let body = "## Section\n\nbody\n# Real Title\n";
        assert_eq!(
            extract_title(body, "https://example.com/x"),
            Some("Real Title".to_string())
        );
    }

    #[test]
    fn extract_title_ignores_hashtags_in_body_text() {
        // `#hashtag` lines are not headings — must have a space after `#`.
        // Falls through to URL-slug since there's no real heading either.
        let body = "#hashtag-not-a-heading\n\nbody\n";
        assert_eq!(
            extract_title(body, "https://example.com/x"),
            Some("x".to_string())
        );
    }

    #[test]
    fn extract_title_falls_back_to_url_slug() {
        let body = "no heading line at all\n\njust body\n";
        assert_eq!(
            extract_title(body, "https://example.com/path/some-page"),
            Some("some page".to_string())
        );
    }

    #[test]
    fn extract_title_returns_none_when_no_signal() {
        let body = "totally empty body with nothing useful";
        assert_eq!(extract_title(body, "not a url at all"), None);
    }

    #[test]
    fn extract_title_url_slug_humanizes_dashes_and_underscores() {
        // I2: URL slugs with `-` or `_` should become space-separated, lowercase.
        let body = "no heading line\n\nbody\n";
        assert_eq!(
            extract_title(
                body,
                "https://docs.strata.markets/markets/ethena-usde/srusde"
            ),
            Some("srusde".to_string())
        );
        assert_eq!(
            extract_title(body, "https://docs.strata.markets/why_strata"),
            Some("why strata".to_string())
        );
        assert_eq!(
            extract_title(body, "https://docs.strata.markets/senior-tranche"),
            Some("senior tranche".to_string())
        );
    }

    #[test]
    fn build_sections_buckets_by_url_path() {
        let entries = vec![
            entry(
                "Why Strata",
                "https://docs.example.com/introduction/why-strata",
                "body",
                None,
            ),
            entry(
                "Senior Tranche",
                "https://docs.example.com/introduction/senior-tranche",
                "body",
                None,
            ),
            entry(
                "Ethena",
                "https://docs.example.com/markets/ethena-usde",
                "body",
                None,
            ),
            entry("Strata", "https://docs.example.com/", "body", None),
        ];
        let sections = build_sections_for_agents(&entries);
        let keys: Vec<&str> = sections.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["index", "introduction", "markets"]);
        // Homepage → index.md relpath, not "/.md"
        let (_, index_pages) = &sections[0];
        assert_eq!(index_pages[0].2, "index.md");
        // Nested path keeps its full prefix in the relpath
        let (_, intro_pages) = &sections[1];
        assert_eq!(intro_pages[0].2, "introduction/why-strata.md");
    }

    #[test]
    fn pick_overview_prefers_longest_intro_paragraph() {
        let short = "Short stub.";
        let long = "Strata is a structured-yield protocol that splits risk into senior and junior tranches, \
                    with on-chain transparency and composability.";
        let entries = vec![
            // Short stub from a redirect — must be skipped over.
            entry(
                "Why Strata",
                "https://docs.example.com/introduction/why-strata",
                "x",
                Some(short),
            ),
            // Real overview — must win.
            entry(
                "Protocol Overview",
                "https://docs.example.com/introduction/overview",
                "x",
                Some(long),
            ),
        ];
        let picked = pick_overview_first_para(&entries).expect("should pick");
        assert!(picked.contains("structured-yield"), "picked: {picked:?}");
        assert!(!picked.contains("stub"), "should not pick the stub");
    }

    #[test]
    fn pick_overview_falls_back_to_longest_overall() {
        let entries = vec![
            entry("A", "https://x.com/foo/a", "x", Some("short")),
            entry(
                "B",
                "https://x.com/bar/b",
                "x",
                Some("a longer paragraph about things in general."),
            ),
        ];
        let picked = pick_overview_first_para(&entries).expect("should pick");
        assert!(picked.contains("longer paragraph"));
    }

    #[test]
    fn pick_overview_returns_none_when_all_empty() {
        let entries = vec![entry("A", "https://x.com/foo/a", "x", None)];
        assert_eq!(pick_overview_first_para(&entries), None);
    }

    #[test]
    fn is_gitbook_redirect_stub_recognises_both_signatures() {
        assert!(is_gitbook_redirect_stub(
            "> For the complete documentation index, see [llms.txt](https://x/llms.txt). Markdown versions…"
        ));
        assert!(is_gitbook_redirect_stub(
            "> Markdown versions of documentation pages are available by appending `.md` to page URLs."
        ));
        assert!(!is_gitbook_redirect_stub(
            "Strata is a structured-yield protocol that splits risk into senior and junior tranches."
        ));
        assert!(!is_gitbook_redirect_stub(""));
    }

    #[test]
    fn pick_overview_skips_gitbook_stub() {
        let stub = "> For the complete documentation index, see [llms.txt](https://x/llms.txt). Markdown versions of documentation pages are available by appending `.md` to page URLs; this page is available as [Markdown](https://x/introduction/overview.md).";
        let real = "Strata is a structured-yield protocol that splits risk into senior and junior tranches, with on-chain transparency and composability.";
        let entries = vec![
            // Page whose first_paragraph is the GitBook stub — must be skipped
            // even though it's in the introduction section.
            entry(
                "Protocol Overview",
                "https://x.com/introduction/overview",
                "x",
                Some(stub),
            ),
            // Page whose first_paragraph is the real content — must win.
            entry(
                "Why Strata",
                "https://x.com/introduction/why-strata",
                "x",
                Some(real),
            ),
        ];
        let picked = pick_overview_first_para(&entries).expect("should pick");
        assert!(picked.contains("structured-yield"), "picked: {picked:?}");
        assert!(
            !picked.contains("complete documentation index"),
            "stub leaked: {picked:?}"
        );
    }
}
