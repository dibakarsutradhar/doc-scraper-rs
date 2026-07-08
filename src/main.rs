use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use std::collections::HashMap;
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
use markdown::{generate_index, generate_llms_txt, group_into_sections};
use page::Page;
use writer::{write_page, WriteOutcome};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = ResolvedConfig::from_cli(cli);

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
        return legacy::scrape(&cfg).await
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
    let pb = if cfg.quiet { ProgressBar::hidden() } else { mk_progress(pages.len()) };
    pb.set_message("scraping");

    let results = fetch_all(client.clone(), pages.clone(), cfg.concurrency, cfg.retries, cfg.delay_secs).await;
    pb.finish_and_clear();

    // 4. Process results
    let mut written_files: Vec<(String, String, String)> = Vec::new(); // (title, url, rel-path)
    let mut llms_entries: Vec<(String, String, Option<String>)> = Vec::new();
    let mut slug_counts: HashMap<String, usize> = HashMap::new();
    let mut errors: u32 = 0;

    // I8: count fetch errors so `exit 2` correctly reflects partial failures.
    // Iterate without `.flatten()` and handle Ok / Err explicitly.
    for item in results.into_iter() {
        let (page_ref, body) = match item {
            Ok(pair) => pair,
            Err(e) => {
                errors += 1;
                eprintln!("fetch error: {e}");
                continue;
            }
        };
        let page = Page { url: page_ref.loc.clone(), title: extract_title(&body, page_ref.loc.as_str()), body: body.clone() };
        // filter
        if !cfg.filters.is_empty()
            && !cfg.filters.iter().any(|f| page.title.as_deref().unwrap_or("").contains(f))
        {
            continue;
        }
        // Write
        match write_page(page.clone(), body.clone(), &cfg.output_dir, cfg.flat, cfg.overwrite, &cfg.url, &mut slug_counts) {
            Ok(WriteOutcome::Written(path)) => {
                if let Ok(rel) = path.strip_prefix(&cfg.output_dir) {
                    let url = rel.to_string_lossy().replace('\\', "/").trim_end_matches(".md").to_string();
                    let title = page.title.clone().unwrap_or_else(|| rel.to_string_lossy().into_owned());
                    written_files.push((title.clone(), url, rel.to_string_lossy().into_owned()));
                    let first_para = first_paragraph(&page.body);
                    llms_entries.push((title, page_ref.loc.to_string(), first_para));
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
            let page = Page { url: cfg.url.clone(), title: extract_title(&body, cfg.url.as_str()), body: body.clone() };
            match write_page(page.clone(), body, &cfg.output_dir, cfg.flat, cfg.overwrite, &cfg.url, &mut slug_counts) {
                Ok(WriteOutcome::Written(path)) => {
                    if let Ok(rel) = path.strip_prefix(&cfg.output_dir) {
                        let rel_str = rel.to_string_lossy().replace('\\', "/");
                        let url = rel_str.trim_end_matches(".md").to_string();
                        let title = page.title.clone().unwrap_or_else(|| rel_str.clone());
                        written_files.push((title.clone(), url, rel_str.clone()));
                        llms_entries.push((title, cfg.url.to_string(), first_paragraph(&page.body)));
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
    }

    if errors > 0 {
        eprintln!("done with {errors} errors");
        std::process::exit(2);
    }
    eprintln!("wrote {} pages to {:?}", written_files.len(), cfg.output_dir);
    Ok(())
}

fn mk_progress(total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(ProgressStyle::with_template("{bar:40} {pos}/{len} {msg}").unwrap());
    pb
}

fn extract_title(body: &str, url: &str) -> Option<String> {
    // Strategy 1: try <title>...</title> from HTML body (GitBook returns HTML at .md endpoint).
    if let Some(caps) = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").ok().and_then(|re| re.captures(body)) {
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
        if let Some(seg) = parsed.path_segments().and_then(|s| s.filter(|x| !x.is_empty()).last()) {
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
    body.lines()
        .skip_while(|l| l.trim().is_empty() || l.starts_with('#'))
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

async fn fetch_llms_txt(client: &reqwest::Client, base: &url::Url) -> Result<String> {
    let url = base.join("/llms.txt")?;
    let resp = fetch_with_retry(client, &url, 1, 0.0).await?;
    let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    if !ct.contains("text/markdown") && !ct.contains("text/plain") {
        return Err(ScraperError::Other(format!("unexpected content-type {ct}")));
    }
    resp.text().await.map_err(ScraperError::Http)
}

#[cfg(test)]
mod tests {
    use super::extract_title;

    #[test]
    fn extract_title_from_html_title_tag() {
        let body = r#"<html><head><title>Foo</title></head><body>x</body></html>"#;
        assert_eq!(extract_title(body, "https://example.com/x"), Some("Foo".to_string()));
    }

    #[test]
    fn extract_title_trims_whitespace() {
        let body = r#"<html><head><title>   Bar   </title></head><body>x</body></html>"#;
        assert_eq!(extract_title(body, "https://example.com/x"), Some("Bar".to_string()));
    }

    #[test]
    fn extract_title_falls_back_to_url_slug() {
        let body = "<html><head></head><body>no title tag here</body></html>";
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
        let body = "<html><head></head><body>x</body></html>";
        assert_eq!(
            extract_title(body, "https://docs.strata.markets/markets/ethena-usde/srusde"),
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
}
