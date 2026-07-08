use thiserror::Error;

/// All errors surfaced by the scraper. Variants are derived from the
/// underlying cause (`reqwest`, `std::io`, `url::ParseError`) where
/// possible, with stringly-typed fallbacks for sitemap / XML parse failures
/// that don't have a natural typed source.
#[derive(Debug, Error)]
pub enum ScraperError {
    #[error("sitemap error: {0}")]
    Sitemap(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("xml parse error: {0}")]
    Xml(String),

    #[error("{0}")]
    Other(String),
}

/// Crate-wide `Result` alias. All fallible APIs in `doc-scraper-rs` return
/// `Result<T, ScraperError>` so callers don't have to spell out the error
/// type at every call site.
pub type Result<T> = std::result::Result<T, ScraperError>;
