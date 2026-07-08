use std::path::PathBuf;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn full_pipeline_against_mocked_gitbook() {
    let server = MockServer::start().await;

    // Sitemap index
    Mock::given(method("GET")).and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<?xml version="1.0"?>
               <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <sitemap><loc>/sitemap-pages.xml</loc></sitemap>
               </sitemapindex>"#))
        .mount(&server).await;

    // Sitemap pages
    Mock::given(method("GET")).and(path("/sitemap-pages.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<?xml version="1.0"?>
               <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <url><loc>{{base}}/introduction</loc></url>
                 <url><loc>{{base}}/introduction/why-strata</loc></url>
                 <url><loc>{{base}}/markets</loc></url>
                 <url><loc>{{base}}/markets/ethena-usde</loc></url>
                 <url><loc>{{base}}/markets/ethena-usde/srusde</loc></url>
               </urlset>"#.replace("{{base}}", &server.uri())))
        .mount(&server).await;

    // Each .md endpoint
    for slug in &["introduction", "introduction/why-strata", "markets", "markets/ethena-usde", "markets/ethena-usde/srusde"] {
        Mock::given(method("GET")).and(path(&format!("/{slug}")))
            .respond_with(ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string(format!("# {slug}\n\nIntro paragraph for {slug}.\n")))
            .mount(&server).await;
    }

    let out_dir = tempdir().unwrap();
    // Spawn the binary
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("-o").arg(out_dir.path())
        .arg("--delay").arg("0")
        .arg("--retries").arg("1")
        .status()
        .expect("spawn binary");
    assert!(status.success(), "binary failed: {status}");

    // Verify mirror tree
    assert!(out_dir.path().join("index.md").exists(), "index.md missing");
    assert!(out_dir.path().join("introduction").join("why-strata.md").exists());
    assert!(out_dir.path().join("markets").join("ethena-usde").join("srusde.md").exists());
    assert!(out_dir.path().join("llms.txt").exists(), "llms.txt missing");

    // Sanity-check index.md groups by section
    let index = std::fs::read_to_string(out_dir.path().join("index.md")).unwrap();
    assert!(index.contains("## introduction\n"));
    assert!(index.contains("## markets\n"));
}

#[tokio::test]
async fn flat_mode_writes_everything_at_root() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<?xml version="1.0"?>
               <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <sitemap><loc>/sitemap-pages.xml</loc></sitemap>
               </sitemapindex>"#))
        .mount(&server).await;
    Mock::given(method("GET")).and(path("/sitemap-pages.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<?xml version="1.0"?>
               <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <url><loc>{{base}}/introduction</loc></url>
                 <url><loc>{{base}}/markets/foo</loc></url>
               </urlset>"#.replace("{{base}}", &server.uri())))
        .mount(&server).await;
    for slug in &["introduction", "markets/foo"] {
        Mock::given(method("GET")).and(path(&format!("/{slug}")))
            .respond_with(ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string(format!("# {slug}\n\nx\n")))
            .mount(&server).await;
    }

    let out = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("--flat")
        .arg("-o").arg(out.path())
        .arg("--delay").arg("0")
        .arg("--retries").arg("1")
        .status().unwrap();
    assert!(status.success());
    // All pages at top level
    let entries: Vec<_> = std::fs::read_dir(out.path()).unwrap()
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
    Mock::given(method("GET")).and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<?xml version="1.0"?>
               <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <sitemap><loc>/sitemap-pages.xml</loc></sitemap>
               </sitemapindex>"#))
        .mount(&server).await;
    Mock::given(method("GET")).and(path("/sitemap-pages.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<?xml version="1.0"?>
               <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                 <url><loc>{{base}}/audit1</loc></url>
                 <url><loc>{{base}}/audit2</loc></url>
                 <url><loc>{{base}}/guide1</loc></url>
               </urlset>"#.replace("{{base}}", &server.uri())))
        .mount(&server).await;
    for slug in &["audit1", "audit2", "guide1"] {
        Mock::given(method("GET")).and(path(&format!("/{slug}")))
            .respond_with(ResponseTemplate::new(200)
                .insert_header("content-type", "text/markdown; charset=utf-8")
                .set_body_string(format!("# {slug}\n\nbody\n")))
            .mount(&server).await;
    }

    let out = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_doc-scraper");
    let status = std::process::Command::new(bin)
        .arg(server.uri())
        .arg("-o").arg(out.path())
        .arg("--filter").arg("audit")
        .arg("--delay").arg("0")
        .arg("--retries").arg("1")
        .status().unwrap();
    assert!(status.success());
    assert!(out.path().join("audit1.md").exists());
    assert!(out.path().join("audit2.md").exists());
    assert!(!out.path().join("guide1.md").exists());
}
