//! Smoke test for Mintlify auto-detection. Mirrors `soft_404_smoke.rs` in
//! shape: spawns a `wiremock` server that emulates a Mintlify site (flat
//! `<urlset>` sitemap + `.md` endpoint with the doc-index banner), runs the
//! `doc-scraper` binary against it, and asserts that:
//!
//! 1. The sitemap was auto-detected as Mintlify (no `--source` flag needed).
//! 2. The doc-index banner is stripped from every on-disk page.
//! 3. The standard sidecars (`llms.txt`, `llms-full.txt`, `AGENTS.md`,
//!    `skills/`, `index.md`) are written.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example mintlify_smoke
//! ```
//!
//! The example does not require network access; the mock server binds to
//! `127.0.0.1`. Re-runs are safe — the output directory is recreated from
//! scratch each invocation.

use std::path::PathBuf;
use std::process::Command;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let base = server.uri();

    // Flat `<urlset>` sitemap (Mintlify shape).
    let url_intro = format!("{base}/introduction");
    let url_market = format!("{base}/markets/ethena");
    let url_guide = format!("{base}/guides/start");
    Mock::given(method("GET"))
        .and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
               <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <url><loc>{url_intro}</loc></url>
                 <url><loc>{url_market}</loc></url>
                 <url><loc>{url_guide}</loc></url>
               </urlset>"#
        )))
        .mount(&server)
        .await;

    // Each page's `.md` URL returns the Mintlify doc-index banner + content.
    for (slug, title) in &[
        ("introduction", "Introduction"),
        ("markets/ethena", "Ethena"),
        ("guides/start", "Start"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/{slug}.md")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/markdown; charset=utf-8")
                    .set_body_string(format!(
                        "> ## Documentation Index\n\
                         > Fetch the complete documentation index at: {base}/llms.txt\n\
                         > Use this file to discover all available pages before exploring further.\n\
                         \n\
                         # {title}\n\nBody for {title}.\n"
                    )),
            )
            .mount(&server)
            .await;
    }

    // Homepage-fallback path. Mintlify serves a real /llms.txt; mock it so the
    // homepage fallback path is exercised end-to-end.
    Mock::given(method("GET"))
        .and(path("/llms.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string("# Docs\n\n- [Introduction](introduction.md)\n"),
        )
        .mount(&server)
        .await;

    let out = tempdir_in_cwd("doc-scraper-mintlify-smoke-out")?;
    let bin = locate_binary()?;

    let status = Command::new(bin)
        .arg(&base)
        .arg("-o")
        .arg(&out)
        .arg("--delay")
        .arg("0")
        .arg("--retries")
        .arg("1")
        .arg("--quiet")
        .status()?;

    assert!(status.success(), "binary exited non-zero: {status}");

    // Per-page files exist.
    let intro = out.join("introduction.md");
    let market = out.join("markets").join("ethena.md");
    let guide = out.join("guides").join("start.md");
    assert!(intro.exists(), "expected {intro:?}");
    assert!(market.exists(), "expected {market:?}");
    assert!(guide.exists(), "expected {guide:?}");

    // Banner was stripped from every on-disk page.
    for f in [&intro, &market, &guide] {
        let body = std::fs::read_to_string(f)?;
        assert!(
            !body.contains("Documentation Index"),
            "banner leaked into {}: {body:?}",
            f.display()
        );
        assert!(
            body.starts_with("# "),
            "{} should start with an H1, got: {body:?}",
            f.display()
        );
    }

    // Sidecars exist.
    assert!(out.join("llms.txt").exists(), "llms.txt missing");
    assert!(out.join("llms-full.txt").exists(), "llms-full.txt missing");
    assert!(out.join("AGENTS.md").exists(), "AGENTS.md missing");
    assert!(out.join("index.md").exists(), "index.md missing");
    assert!(out.join("skills").is_dir(), "skills/ directory missing");

    println!("ok — mintlify_smoke passed");
    println!("  base    = {base}");
    println!("  out     = {}", out.display());
    Ok(())
}

/// Creates a fresh temp directory under the current working directory.
/// Mirrors the helper in `soft_404_smoke.rs` — we don't use `tempfile`'s
/// auto-cleanup because the user may want to inspect the output after the
/// example exits.
fn tempdir_in_cwd(prefix: &str) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = std::env::current_dir()?.join(format!(".{prefix}-{pid}-{stamp}-{n}"));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Locate the `doc-scraper` binary for the current profile. Same precedence
/// as `soft_404_smoke.rs` — see that file for the rationale.
fn locate_binary() -> std::io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("CARGO_BIN_EXE_doc-scraper") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Ok(p);
        }
    }
    if let Some(p) = std::env::var_os("DOC_SCRAPER_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Ok(p);
        }
    }
    let profile = if std::env::var_os("PROFILE").as_deref() == Some(std::ffi::OsStr::new("release"))
    {
        "release"
    } else {
        "debug"
    };
    let p = PathBuf::from(format!("target/{profile}/doc-scraper"));
    if p.exists() {
        return Ok(p);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "could not locate doc-scraper binary. Tried $CARGO_BIN_EXE_doc-scraper, \
             $DOC_SCRAPER_BIN, and {}; did you run `cargo build` first?",
            p.display()
        ),
    ))
}
