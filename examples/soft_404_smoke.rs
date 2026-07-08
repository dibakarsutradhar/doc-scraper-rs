//! Smoke test for GitBook's soft-404 fallback against the GitBook `.md`
//! endpoint. This example exercises the public `doc-scraper` binary against a
//! `wiremock` server that emulates both:
//!
//! 1. A clean `.md` endpoint returning real markdown.
//! 2. A soft-404 stub (HTTP 200, `Content-Type: text/markdown`, body starts
//!    with `# Page Not Found`) for an unknown path.
//!
//! Expected behaviour: the binary writes a real markdown file for the first
//! URL and falls back to the bare-URL response for the second, producing
//! non-empty files in both cases.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example soft_404_smoke
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

    // 1. /known.md — clean markdown.
    Mock::given(method("GET"))
        .and(path("/known.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string("# Known Page\n\nBody for the known page.\n"),
        )
        .mount(&server)
        .await;

    // 2. /missing.md — GitBook's soft-404 stub.
    Mock::given(method("GET"))
        .and(path("/missing.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string("# Page Not Found\n\nThe URL `missing` does not exist.\n"),
        )
        .mount(&server)
        .await;

    // 3. /missing — bare URL fallback. The scraper hits this only after
    //    detecting the soft-404 stub above.
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(200)
            .insert_header("content-type", "text/html; charset=utf-8")
            .set_body_string("<html><head><title>Missing | Docs</title></head><body>fallback body</body></html>"))
        .mount(&server)
        .await;

    // 4. /llms.txt — homepage-fallback sidecar.
    Mock::given(method("GET"))
        .and(path("/llms.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string("# Docs\n"),
        )
        .mount(&server)
        .await;

    // Sitemap index + sitemap-pages that point at the two pages above.
    Mock::given(method("GET"))
        .and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<?xml version="1.0"?>
               <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <sitemap><loc>/sitemap-pages.xml</loc></sitemap>
               </sitemapindex>"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sitemap-pages.xml"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                r#"<?xml version="1.0"?>
               <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <url><loc>{{base}}/known</loc></url>
                 <url><loc>{{base}}/missing</loc></url>
               </urlset>"#
                    .replace("{{base}}", &base),
            ),
        )
        .mount(&server)
        .await;

    let out = tempdir_in_cwd("doc-scraper-smoke-out")?;
    // Find the freshly-built binary. `cargo run --example` does not set
    // CARGO_BIN_EXE_doc-scraper (that's only set for integration tests), so
    // we fall back to the conventional `target/{profile}/doc-scraper` path.
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

    let known = out.join("known.md");
    let missing = out.join("missing.md");
    assert!(
        known.exists(),
        "expected {known:?} to exist after scraping /known"
    );
    assert!(
        missing.exists(),
        "expected {missing:?} to exist after scraping /missing (fallback)"
    );

    let known_body = std::fs::read_to_string(&known)?;
    assert!(
        known_body.contains("Known Page"),
        "known.md body looked wrong: {known_body:?}"
    );
    let missing_body = std::fs::read_to_string(&missing)?;
    assert!(
        missing_body.contains("fallback body"),
        "expected fallback HTML body in missing.md, got: {missing_body:?}"
    );

    println!("ok — soft_404_smoke passed");
    println!("  base    = {base}");
    println!("  known   = {}", known.display());
    println!("  missing = {}", missing.display());
    Ok(())
}

/// Creates a fresh temp directory under `./target/`, falling back to the
/// current working directory if `./target/` isn't writable (it usually
/// isn't, when the example is run as `cargo run --example` from a foreign
/// checkout). We avoid `tempfile`'s auto-cleanup because the user may want
/// to inspect the output after the example exits.
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

/// Locate the `doc-scraper` binary for the current profile. We try, in
/// order: `CARGO_BIN_EXE_doc-scraper` (set by `cargo test` for integration
/// tests), `DOC_SCRAPER_BIN` (manual override), then `target/{profile}/doc-scraper`
/// (the standard build output for `cargo run --example`).
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
