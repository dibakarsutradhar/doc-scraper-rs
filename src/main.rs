use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::io::Write;
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
use page::{url_to_relative_path, Page};
use sitemap::PageRef;
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

    std::fs::create_dir_all(&cfg.output_dir)
        .with_context(|| format!("create output dir {:?}", cfg.output_dir))?;

    if cfg.legacy {
        return legacy::scrape(&cfg).await
            .map_err(|e| anyhow::anyhow!("legacy scrape failed: {e}"));
    }

    let client = build_client(&cfg.user_agent, cfg.timeout_secs)?;

    // 1. Sitemap
    eprintln!("Fetching sitemap for {}...", cfg.url);
    let pages = sitemap::fetch_sitemap(&client, &cfg.url).await?;
    if pages.is_empty() {
        anyhow::bail!("sitemap returned zero pages");
    }

    // 2. Homepage fallback via llms.txt
    let homepage_fallback = fetch_llms_txt(&client, &cfg.url).await.ok();

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

    for (page_ref, body) in results.into_iter().flatten() {
        let page = Page { url: page_ref.loc.clone(), title: extract_title(&body), body: body.clone() };
        // filter
        if !cfg.filters.is_empty()
            && !cfg.filters.iter().any(|f| page.title.as_deref().unwrap_or("").contains(f))
        {
            continue;
        }
        // Write
        match write_page(page.clone(), body.clone(), &cfg.output_dir, cfg.flat, cfg.overwrite, &mut slug_counts) {
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
    if let Some(body) = homepage_fallback {
        let already = written_files.iter().any(|(_, u, _)| u.is_empty());
        if !already {
            let page = Page { url: cfg.url.clone(), title: extract_title(&body), body: body.clone() };
            let _ = write_page(page, body, &cfg.output_dir, cfg.flat, cfg.overwrite, &mut slug_counts);
        }
    }

    // 5. index.md
    if cfg.toc {
        let sections = group_into_sections(written_files.clone());
        let out = generate_index(&sections);
        std::fs::write(cfg.output_dir.join("index.md"), out)?;
    }

    // 6. llms.txt
    if let Some(path) = cfg.llms_txt_path.as_ref() {
        let out = generate_llms_txt(&llms_entries);
        std::fs::write(path, out)?;
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

fn extract_title(body: &str) -> Option<String> {
    body.lines().find(|l| l.starts_with("# ")).map(|l| l.trim_start_matches("# ").trim().to_string())
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