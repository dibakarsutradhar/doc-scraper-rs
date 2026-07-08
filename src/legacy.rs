use crate::config::ResolvedConfig;
use crate::error::Result;

/// Legacy HTML-scraping mode for old GitBook sites (pre-sitemap).
/// Not implemented yet; this stub ensures `mod legacy;` resolves and the
/// pipeline compiles. Selected via `--legacy`; main.rs routes there early.
pub async fn scrape(_cfg: &ResolvedConfig) -> Result<()> {
    unimplemented!("legacy::scrape will be implemented in a later task");
}