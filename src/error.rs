use thiserror::Error;

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

pub type Result<T> = std::result::Result<T, ScraperError>;