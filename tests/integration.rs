use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn full_pipeline_against_mocked_gitbook() {
    let server = MockServer::start().await;

    // Sitemap index
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

    // Sitemap pages
    Mock::given(method("GET"))
        .and(path("/sitemap-pages.xml"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                r#"<?xml version="1.0"?>
               <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <url><loc>{{base}}/introduction</loc></url>
                 <url><loc>{{base}}/introduction/why-strata</loc></url>
                 <url><loc>{{base}}/markets</loc></url>
                 <url><loc>{{base}}/markets/ethena-usde</loc></url>
                 <url><loc>{{base}}/markets/ethena-usde/srusde</loc></url>
               </urlset>"#
                    .replace("{{base}}", &server.uri()),
            ),
        )
        .mount(&server)
        .await;

    // Each .md endpoint
    for slug in &[
        "introduction",
        "introduction/why-strata",
        "markets",
        "markets/ethena-usde",
        "markets/ethena-usde/srusde",
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/{slug}.md")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/markdown; charset=utf-8")
                    .set_body_string(format!("# {slug}\n\nIntro paragraph for {slug}.\n")),
            )
            .mount(&server)
            .await;
    }

    let out_dir = tempdir().unwrap();
    // Spawn the binary
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("-o")
        .arg(out_dir.path())
        .arg("--delay")
        .arg("0")
        .arg("--retries")
        .arg("1")
        .status()
        .expect("spawn binary");
    assert!(status.success(), "binary failed: {status}");

    // Verify mirror tree
    assert!(out_dir.path().join("index.md").exists(), "index.md missing");
    assert!(out_dir
        .path()
        .join("introduction")
        .join("why-strata.md")
        .exists());
    assert!(out_dir
        .path()
        .join("markets")
        .join("ethena-usde")
        .join("srusde.md")
        .exists());
    assert!(out_dir.path().join("llms.txt").exists(), "llms.txt missing");
    assert!(
        out_dir.path().join("llms-full.txt").exists(),
        "llms-full.txt missing"
    );

    // Sanity-check llms-full.txt shape: site-title header, URL line per page,
    // page separator.
    let full =
        std::fs::read_to_string(out_dir.path().join("llms-full.txt")).expect("read llms-full.txt");
    assert!(
        full.starts_with("# 127.0.0.1"),
        "missing site title header: {full:?}"
    );
    assert!(full.contains("URL: http://127.0.0.1:"));
    assert!(full.contains("\n---\n"), "missing page separator: {full:?}");

    // Sanity-check index.md groups by section
    let index = std::fs::read_to_string(out_dir.path().join("index.md")).unwrap();
    assert!(index.contains("## introduction\n"));
    assert!(index.contains("## markets\n"));

    // AGENTS.md exists with the right header and sections list.
    let agents = std::fs::read_to_string(out_dir.path().join("AGENTS.md")).expect("read AGENTS.md");
    assert!(agents.starts_with("# 127.0.0.1 — Agent Context\n\n"));
    assert!(agents.contains("## Sections\n\n"));
    assert!(agents.contains("### introduction\n"));
    assert!(agents.contains("### markets\n"));
    assert!(agents.contains("- [00-introduction.md](skills/00-introduction.md)"));
    assert!(agents.contains("- [01-markets.md](skills/01-markets.md)"));

    // skills/ has one file per section, sorted by numeric prefix.
    let skills_dir = out_dir.path().join("skills");
    assert!(skills_dir.exists(), "skills/ directory missing");
    let mut names: Vec<String> = std::fs::read_dir(&skills_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(names.len() >= 2, "expected ≥2 skill files, got: {names:?}");
    assert!(
        names[0].starts_with("00-"),
        "first skill should be 00-: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "00-introduction.md"),
        "missing 00-introduction.md: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "01-markets.md"),
        "missing 01-markets.md: {names:?}"
    );

    // Skill file shape: per-page blocks, page separators.
    let skill = std::fs::read_to_string(skills_dir.join("00-introduction.md"))
        .expect("read 00-introduction.md");
    assert!(skill.starts_with("# introduction — 127.0.0.1\n\n"));
    assert!(skill.contains("URL: http://127.0.0.1:"));
    assert!(skill.matches("\n---\n").count() >= 1);
}

/// When `--no-llms-txt` is passed, AGENTS.md and skills/ should NOT be
/// created — they're gated by the same flag.
#[tokio::test]
async fn agents_md_skips_when_no_llms_txt() {
    let server = MockServer::start().await;
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
                 <url><loc>{{base}}/introduction</loc></url>
               </urlset>"#
                    .replace("{{base}}", &server.uri()),
            ),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/introduction.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string("# Introduction\n\nFirst paragraph for the intro page.\n"),
        )
        .mount(&server)
        .await;

    let out_dir = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("-o")
        .arg(out_dir.path())
        .arg("--delay")
        .arg("0")
        .arg("--retries")
        .arg("1")
        .arg("--no-llms-txt")
        .status()
        .expect("spawn binary");
    assert!(status.success(), "binary failed: {status}");

    // llms.txt / llms-full.txt / AGENTS.md / skills/ all suppressed.
    assert!(
        !out_dir.path().join("llms.txt").exists(),
        "llms.txt should be suppressed"
    );
    assert!(
        !out_dir.path().join("llms-full.txt").exists(),
        "llms-full.txt should be suppressed"
    );
    assert!(
        !out_dir.path().join("AGENTS.md").exists(),
        "AGENTS.md should be suppressed"
    );
    assert!(
        !out_dir.path().join("skills").exists(),
        "skills/ should be suppressed"
    );

    // But the per-page markdown file is still written (page-level writes are
    // independent of the --no-llms-txt flag).
    assert!(out_dir.path().join("introduction.md").exists());
}

#[tokio::test]
async fn flat_mode_writes_everything_at_root() {
    let server = MockServer::start().await;
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
                 <url><loc>{{base}}/introduction</loc></url>
                 <url><loc>{{base}}/markets/foo</loc></url>
               </urlset>"#
                    .replace("{{base}}", &server.uri()),
            ),
        )
        .mount(&server)
        .await;
    for slug in &["introduction", "markets/foo"] {
        Mock::given(method("GET"))
            .and(path(format!("/{slug}.md")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/markdown; charset=utf-8")
                    .set_body_string(format!("# {slug}\n\nx\n")),
            )
            .mount(&server)
            .await;
    }

    let out = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("--flat")
        .arg("-o")
        .arg(out.path())
        .arg("--delay")
        .arg("0")
        .arg("--retries")
        .arg("1")
        .status()
        .unwrap();
    assert!(status.success());
    // All pages at top level
    let entries: Vec<_> = std::fs::read_dir(out.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(entries.iter().any(|n| n == "introduction.md"));
    assert!(entries.iter().any(|n| n == "markets.foo.md"));
    // No subdirs in flat mode
    for name in &entries {
        assert!(!name.contains('/'), "flat should not produce subdirs");
    }
}

#[tokio::test]
async fn filter_excludes_nonmatching_pages() {
    let server = MockServer::start().await;
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
                 <url><loc>{{base}}/audit1</loc></url>
                 <url><loc>{{base}}/audit2</loc></url>
                 <url><loc>{{base}}/guide1</loc></url>
               </urlset>"#
                    .replace("{{base}}", &server.uri()),
            ),
        )
        .mount(&server)
        .await;
    for slug in &["audit1", "audit2", "guide1"] {
        Mock::given(method("GET"))
            .and(path(format!("/{slug}.md")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/markdown; charset=utf-8")
                    .set_body_string(format!("# {slug}\n\nbody\n")),
            )
            .mount(&server)
            .await;
    }

    let out = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("-o")
        .arg(out.path())
        .arg("--filter")
        .arg("audit")
        .arg("--delay")
        .arg("0")
        .arg("--retries")
        .arg("1")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(out.path().join("audit1.md").exists());
    assert!(out.path().join("audit2.md").exists());
    assert!(!out.path().join("guide1.md").exists());
}

#[tokio::test]
async fn soft_404_falls_back_to_html() {
    let server = MockServer::start().await;

    // Sitemap index + sitemap-pages
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
                 <url><loc>{{base}}/soft-page</loc></url>
               </urlset>"#
                    .replace("{{base}}", &server.uri()),
            ),
        )
        .mount(&server)
        .await;

    // /soft-page.md → GitBook's soft-404 stub. Detect via the "# Page Not Found"
    // heading that the scraper treats as a signal to fall back to bare HTML.
    Mock::given(method("GET"))
        .and(path("/soft-page.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string("# Page Not Found\n\nThe URL `soft-page` does not exist.\n"),
        )
        .mount(&server)
        .await;

    // /soft-page (bare URL) → real HTML body. The scraper falls back to this
    // when it detects the soft-404 stub above.
    Mock::given(method("GET")).and(path("/soft-page"))
        .respond_with(ResponseTemplate::new(200)
            .insert_header("content-type", "text/html; charset=utf-8")
            .set_body_string("<html><head><title>Soft Page | Docs</title></head><body>real content</body></html>"))
        .mount(&server).await;

    // /llms.txt (the homepage fallback path; runs unconditionally in main).
    Mock::given(method("GET"))
        .and(path("/llms.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string("# Docs\n\n- [Soft Page](soft-page.md)\n"),
        )
        .mount(&server)
        .await;

    let out = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("-o")
        .arg(out.path())
        .arg("--delay")
        .arg("0")
        .arg("--retries")
        .arg("1")
        .status()
        .expect("spawn binary");
    assert!(status.success(), "binary failed: {status}");

    // The fallback should have written the HTML body to soft-page.md.
    let written =
        std::fs::read_to_string(out.path().join("soft-page.md")).expect("soft-page.md missing");
    assert!(
        written.contains("real content"),
        "expected fallback HTML body, got: {written}"
    );
}
