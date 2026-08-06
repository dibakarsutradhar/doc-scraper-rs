use crate::error::{Result, ScraperError};
use url::Url;

/// One entry from `sitemap-pages.xml`: the page URL plus the optional
/// `lastmod` timestamp. The fetch pipeline takes a `Vec<PageRef>` and
/// returns one result per entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRef {
    pub loc: Url,
    pub lastmod: Option<String>,
}

/// The XML shape that `/sitemap.xml` happens to use. Drives which parser the
/// sitemap fetch pipeline calls and which post-processing the fetch pipeline
/// applies to each page body. Auto-detected on every run; not user-visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SitemapShape {
    /// `<sitemapindex>` referencing `sitemap-pages.xml` (which holds the
    /// `<urlset>`). Today's GitBook Next.js sites.
    GitBookIndex,
    /// Flat `<urlset>` directly inside `/sitemap.xml`. Mintlify and similar.
    MintlifyUrlset,
}

/// Fetches `https://<host>/sitemap.xml` and parses the page list, auto-detecting
/// the sitemap shape. Returns a [`SitemapShape`] tag alongside the page list so
/// downstream code can apply platform-specific post-processing (banner stripping
/// and stub detection for Mintlify; soft-404 fallback for GitBook).
pub async fn fetch_sitemap(
    client: &reqwest::Client,
    base_url: &Url,
) -> Result<(SitemapShape, Vec<PageRef>)> {
    let sitemap_url = base_url.join("/sitemap.xml")?;
    let resp = client.get(sitemap_url).send().await?.error_for_status()?;
    let body = resp.text().await?;

    // Parse once to inspect the root element. Both branches re-parse the body
    // (cheap, ~KB) rather than threading the tree around — keeps each branch's
    // happy-path a single `parse_*` call.
    let probe = roxmltree::Document::parse(&body).map_err(|e| ScraperError::Xml(e.to_string()))?;
    match probe.root_element().tag_name().name() {
        "sitemapindex" => {
            // Existing GitBook path: find the inner <loc>, fetch sitemap-pages.xml,
            // parse it as a urlset.
            let pages_url = extract_sitemap_pages_url(&body, base_url)?;
            let pages_resp = client.get(pages_url).send().await?.error_for_status()?;
            let pages_body = pages_resp.text().await?;
            let pages = parse_sitemap_pages(&pages_body)?;
            Ok((SitemapShape::GitBookIndex, pages))
        }
        "urlset" => {
            // Mintlify (and similar): the urlset lives directly in /sitemap.xml.
            let pages = parse_sitemap_urlset(&body)?;
            Ok((SitemapShape::MintlifyUrlset, pages))
        }
        other => Err(ScraperError::Sitemap(format!(
            "unknown sitemap shape: expected <sitemapindex> or <urlset>, got <{other}>"
        ))),
    }
}

/// Extracts the `<sitemap><loc>...</loc></sitemap>` URL pointing at the pages sitemap.
pub fn extract_sitemap_pages_url(body: &str, base_url: &Url) -> Result<Url> {
    let doc = roxmltree::Document::parse(body).map_err(|e| ScraperError::Xml(e.to_string()))?;
    let root = doc.root_element();
    // <sitemapindex><sitemap><loc>...</loc></sitemap>...</sitemapindex>
    for node in root.descendants() {
        if node.tag_name().name() == "loc" {
            if let Some(text) = node.text() {
                let url = base_url.join(text.trim())?;
                return Ok(url);
            }
        }
    }
    Err(ScraperError::Sitemap("no <loc> in sitemap index".into()))
}

/// Parses sitemap-pages.xml (a `urlset` of `url` / `loc` records) into PageRef records.
pub fn parse_sitemap_pages(body: &str) -> Result<Vec<PageRef>> {
    let doc = roxmltree::Document::parse(body).map_err(|e| ScraperError::Xml(e.to_string()))?;
    let mut pages = Vec::new();
    for url_node in doc.descendants().filter(|n| n.tag_name().name() == "url") {
        let loc_text = url_node
            .children()
            .find(|c| c.tag_name().name() == "loc")
            .and_then(|c| c.text())
            .ok_or_else(|| ScraperError::Sitemap("<url> missing <loc>".into()))?;
        let loc = Url::parse(loc_text.trim())?;
        let lastmod = url_node
            .children()
            .find(|c| c.tag_name().name() == "lastmod")
            .and_then(|c| c.text())
            .map(str::to_owned);
        pages.push(PageRef { loc, lastmod });
    }
    Ok(pages)
}

/// Parses a flat `<urlset>` sitemap body (as Mintlify serves in `/sitemap.xml`)
/// into `PageRef` records. Equivalent to `parse_sitemap_pages`, kept as a
/// separate function so the call sites document which shape they're parsing.
pub fn parse_sitemap_urlset(body: &str) -> Result<Vec<PageRef>> {
    let doc = roxmltree::Document::parse(body).map_err(|e| ScraperError::Xml(e.to_string()))?;
    let mut pages = Vec::new();
    for url_node in doc.descendants().filter(|n| n.tag_name().name() == "url") {
        let loc_text = url_node
            .children()
            .find(|c| c.tag_name().name() == "loc")
            .and_then(|c| c.text())
            .ok_or_else(|| ScraperError::Sitemap("<url> missing <loc>".into()))?;
        let loc = Url::parse(loc_text.trim())?;
        let lastmod = url_node
            .children()
            .find(|c| c.tag_name().name() == "lastmod")
            .and_then(|c| c.text())
            .map(str::to_owned);
        pages.push(PageRef { loc, lastmod });
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sitemap_pages_returns_urls_in_document_order() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
            <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <url><loc>https://docs.example.com/introduction</loc></url>
              <url><loc>https://docs.example.com/markets</loc><lastmod>2026-01-15</lastmod></url>
              <url><loc>https://docs.example.com/technical/security</loc></url>
            </urlset>"#;
        let pages = parse_sitemap_pages(body).unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(
            pages[0].loc.as_str(),
            "https://docs.example.com/introduction"
        );
        assert_eq!(pages[1].loc.as_str(), "https://docs.example.com/markets");
        assert_eq!(pages[1].lastmod.as_deref(), Some("2026-01-15"));
        assert_eq!(
            pages[2].loc.as_str(),
            "https://docs.example.com/technical/security"
        );
        assert_eq!(pages[2].lastmod, None);
    }

    #[test]
    fn parse_sitemap_pages_errors_on_url_missing_loc() {
        let body = r#"<?xml version="1.0"?>
            <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <url></url>
            </urlset>"#;
        let err = parse_sitemap_pages(body).unwrap_err();
        matches!(err, ScraperError::Sitemap(_));
    }

    #[test]
    fn parse_sitemap_pages_errors_on_invalid_xml() {
        let body = "<<not xml>>";
        let err = parse_sitemap_pages(body).unwrap_err();
        matches!(err, ScraperError::Xml(_));
    }

    #[test]
    fn extract_sitemap_pages_url_resolves_relative_loc() {
        let body = r#"<?xml version="1.0"?>
            <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <sitemap><loc>/sitemap-pages.xml</loc></sitemap>
            </sitemapindex>"#;
        let base = Url::parse("https://docs.example.com/").unwrap();
        let url = extract_sitemap_pages_url(body, &base).unwrap();
        assert_eq!(url.as_str(), "https://docs.example.com/sitemap-pages.xml");
    }

    #[test]
    fn extract_sitemap_pages_url_errors_when_no_loc() {
        let body = r#"<?xml version="1.0"?>
            <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <sitemap></sitemap>
            </sitemapindex>"#;
        let base = Url::parse("https://docs.example.com/").unwrap();
        let err = extract_sitemap_pages_url(body, &base).unwrap_err();
        matches!(err, ScraperError::Sitemap(_));
    }

    #[test]
    fn parse_sitemap_urlset_happy_path() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
            <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <url><loc>https://docs.example.com/introduction</loc></url>
              <url><loc>https://docs.example.com/markets</loc><lastmod>2026-01-15</lastmod></url>
              <url><loc>https://docs.example.com/technical/security</loc></url>
            </urlset>"#;
        let pages = parse_sitemap_urlset(body).unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(
            pages[0].loc.as_str(),
            "https://docs.example.com/introduction"
        );
        assert_eq!(pages[1].loc.as_str(), "https://docs.example.com/markets");
        assert_eq!(pages[1].lastmod.as_deref(), Some("2026-01-15"));
        assert_eq!(
            pages[2].loc.as_str(),
            "https://docs.example.com/technical/security"
        );
        assert_eq!(pages[2].lastmod, None);
    }

    #[test]
    fn parse_sitemap_urlset_errors_on_url_missing_loc() {
        let body = r#"<?xml version="1.0"?>
            <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <url></url>
            </urlset>"#;
        let err = parse_sitemap_urlset(body).unwrap_err();
        matches!(err, ScraperError::Sitemap(_));
    }

    #[test]
    fn parse_sitemap_urlset_errors_on_invalid_xml() {
        let body = "<<not xml>>";
        let err = parse_sitemap_urlset(body).unwrap_err();
        matches!(err, ScraperError::Xml(_));
    }

    #[tokio::test]
    async fn fetch_sitemap_detects_gitbook_index() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
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
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<?xml version="1.0"?>
                   <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                     <url><loc>https://docs.example.com/a</loc></url>
                   </urlset>"#,
            ))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let base = Url::parse(&server.uri()).unwrap();
        let (shape, pages) = fetch_sitemap(&client, &base).await.unwrap();
        assert_eq!(shape, SitemapShape::GitBookIndex);
        assert_eq!(pages.len(), 1);
    }

    #[tokio::test]
    async fn fetch_sitemap_detects_mintlify_urlset() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let url_a = format!("{}/a", server.uri());
        let url_b = format!("{}/b", server.uri());
        Mock::given(method("GET"))
            .and(path("/sitemap.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
                   <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                     <url><loc>{url_a}</loc></url>
                     <url><loc>{url_b}</loc></url>
                   </urlset>"#
            )))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let base = Url::parse(&server.uri()).unwrap();
        let (shape, pages) = fetch_sitemap(&client, &base).await.unwrap();
        assert_eq!(shape, SitemapShape::MintlifyUrlset);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].loc.as_str(), url_a);
        assert_eq!(pages[1].loc.as_str(), url_b);
    }

    #[tokio::test]
    async fn fetch_sitemap_errors_on_unknown_shape() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sitemap.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<?xml version="1.0"?>
                   <not-a-sitemap xmlns="http://example.com/">
                     <whatever/>
                   </not-a-sitemap>"#,
            ))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let base = Url::parse(&server.uri()).unwrap();
        let err = fetch_sitemap(&client, &base).await.unwrap_err();
        matches!(err, ScraperError::Sitemap(_));
    }
}
