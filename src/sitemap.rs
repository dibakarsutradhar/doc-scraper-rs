use crate::error::{Result, ScraperError};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRef {
    pub loc: Url,
    pub lastmod: Option<String>,
}

/// Fetches `https://<host>/sitemap.xml`, follows to `sitemap-pages.xml`, parses it.
pub async fn fetch_sitemap(client: &reqwest::Client, base_url: &Url) -> Result<Vec<PageRef>> {
    let sitemap_url = base_url.join("/sitemap.xml")?;
    let resp = client.get(sitemap_url).send().await?
        .error_for_status()?;
    let body = resp.text().await?;
    let pages_url = extract_sitemap_pages_url(&body, base_url)?;
    let pages_resp = client.get(pages_url).send().await?.error_for_status()?;
    let pages_body = pages_resp.text().await?;
    parse_sitemap_pages(&pages_body)
}

/// Extracts the `<sitemap><loc>...</loc></sitemap>` URL pointing at the pages sitemap.
pub fn extract_sitemap_pages_url(body: &str, base_url: &Url) -> Result<Url> {
    let doc = roxmltree::Document::parse(body)
        .map_err(|e| ScraperError::Xml(e.to_string()))?;
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

/// Parses sitemap-pages.xml (a <urlset> of <url><loc>...</loc></url>) into PageRef records.
pub fn parse_sitemap_pages(body: &str) -> Result<Vec<PageRef>> {
    let doc = roxmltree::Document::parse(body)
        .map_err(|e| ScraperError::Xml(e.to_string()))?;
    let mut pages = Vec::new();
    for url_node in doc.descendants().filter(|n| n.tag_name().name() == "url") {
        let loc_text = url_node.children()
            .find(|c| c.tag_name().name() == "loc")
            .and_then(|c| c.text())
            .ok_or_else(|| ScraperError::Sitemap("<url> missing <loc>".into()))?;
        let loc = Url::parse(loc_text.trim())?;
        let lastmod = url_node.children()
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
        assert_eq!(pages[0].loc.as_str(), "https://docs.example.com/introduction");
        assert_eq!(pages[1].loc.as_str(), "https://docs.example.com/markets");
        assert_eq!(pages[1].lastmod.as_deref(), Some("2026-01-15"));
        assert_eq!(pages[2].loc.as_str(), "https://docs.example.com/technical/security");
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
}