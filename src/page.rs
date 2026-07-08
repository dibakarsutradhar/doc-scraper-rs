use crate::error::{Result, ScraperError};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone)]
pub struct Page {
    pub url: Url,
    pub title: Option<String>,
    pub body: String,
}

/// Returns the URL path relative to the site's base, suitable for the mirror tree.
/// `/introduction/why-strata` -> `introduction/why-strata.md`. Trailing `/` -> `index.md`.
pub fn url_to_relative_path(url: &Url, base: &Url) -> Result<PathBuf> {
    let mut path = url.path().trim_start_matches('/').to_string();
    if path.is_empty() {
        path = "index".into();
    }
    if path.ends_with('/') {
        path.push_str("index");
    }
    if !path.ends_with(".md") {
        path.push_str(".md");
    }
    // Sanity: make sure base host matches.
    if url.host_str() != base.host_str() {
        return Err(ScraperError::Other(format!(
            "url host {} does not match base {}",
            url.host_str().unwrap_or(""), base.host_str().unwrap_or("")
        )));
    }
    Ok(PathBuf::from(path))
}

/// Flattens URL path into a single slug. `/markets/ethena-usde/srusde` -> `markets.ethena-usde.srusde.md`.
pub fn url_to_flat_slug(url: &Url) -> Result<String> {
    let segs: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return Ok("index.md".into());
    }
    let slug = segs.join(".") + ".md";
    Ok(slug)
}

/// Reverse of url_to_relative_path (without the `.md` suffix) for index.md / llms.txt links.
pub fn path_to_relative_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut s = s.trim_end_matches(".md").to_string();
    if s == "index" {
        s.clear();
    } else {
        s = s.trim_end_matches("/index").to_string();
    }
    if s.is_empty() {
        return String::new();
    }
    s
}

/// `docs.strata.markets` -> `strata-docs`, `docs.example.com` -> `example-docs`.
pub fn site_slug_from_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or("site");
    let stripped = host.trim_start_matches("www.").to_string();
    let labels: Vec<&str> = stripped.split('.').collect();
    let second_label = if labels.len() >= 2 {
        labels[labels.len() - 2]
    } else {
        labels.first().copied().unwrap_or(&stripped)
    };
    format!("{second_label}-docs")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url { Url::parse(s).unwrap() }

    #[test]
    fn url_to_relative_path_simple() {
        let base = url("https://docs.strata.markets/");
        let u = url("https://docs.strata.markets/introduction/why-strata");
        let p = url_to_relative_path(&u, &base).unwrap();
        assert_eq!(p, PathBuf::from("introduction/why-strata.md"));
    }

    #[test]
    fn url_to_relative_path_root() {
        let base = url("https://docs.strata.markets/");
        let u = url("https://docs.strata.markets/");
        let p = url_to_relative_path(&u, &base).unwrap();
        assert_eq!(p, PathBuf::from("index.md"));
    }

    #[test]
    fn url_to_relative_path_trailing_slash() {
        let base = url("https://docs.strata.markets/");
        let u = url("https://docs.strata.markets/markets/ethena-usde/");
        let p = url_to_relative_path(&u, &base).unwrap();
        assert_eq!(p, PathBuf::from("markets/ethena-usde/index.md"));
    }

    #[test]
    fn url_to_relative_path_already_md() {
        let base = url("https://docs.strata.markets/");
        let u = url("https://docs.strata.markets/introduction/why-strata.md");
        let p = url_to_relative_path(&u, &base).unwrap();
        assert_eq!(p, PathBuf::from("introduction/why-strata.md"));
    }

    #[test]
    fn url_to_relative_path_rejects_foreign_host() {
        let base = url("https://docs.strata.markets/");
        let u = url("https://example.com/x");
        assert!(url_to_relative_path(&u, &base).is_err());
    }

    #[test]
    fn url_to_flat_slug_nested() {
        let u = url("https://docs.strata.markets/markets/ethena-usde/srusde");
        assert_eq!(url_to_flat_slug(&u).unwrap(), "markets.ethena-usde.srusde.md");
    }

    #[test]
    fn url_to_flat_slug_root() {
        let u = url("https://docs.strata.markets/");
        assert_eq!(url_to_flat_slug(&u).unwrap(), "index.md");
    }

    #[test]
    fn path_to_relative_url_strips_md_and_index() {
        assert_eq!(path_to_relative_url(&PathBuf::from("introduction/why-strata.md")), "introduction/why-strata");
        assert_eq!(path_to_relative_url(&PathBuf::from("markets/ethena-usde/index.md")), "markets/ethena-usde");
        assert_eq!(path_to_relative_url(&PathBuf::from("index.md")), "");
    }

    #[test]
    fn site_slug_strata() {
        let u = url("https://docs.strata.markets/");
        assert_eq!(site_slug_from_url(&u), "strata-docs");
    }

    #[test]
    fn site_slug_strips_www() {
        let u = url("https://www.example.com/");
        assert_eq!(site_slug_from_url(&u), "example-docs");
    }
}
