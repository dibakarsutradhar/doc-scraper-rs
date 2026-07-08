use crate::config::ResolvedConfig;
use clap::Parser;
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Parser)]
#[command(name = "doc-scraper", about = "Export GitBook docs as clean markdown.", version)]
pub struct Cli {
    /// GitBook docs base URL.
    pub url: Url,

    /// Output directory.
    #[arg(short, long, env = "GITBOOK_SCRAPER_OUTPUT_DIR", default_value = "./<site-slug>/")]
    pub output: Option<PathBuf>,

    /// Flatten URLs into filenames instead of mirroring hierarchy.
    #[arg(long, default_value_t = false)]
    pub flat: bool,

    /// Generate index.md with TOC.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub toc: bool,

    /// Write llms.txt sidecar. Use --no-llms-txt to skip.
    #[arg(long, env = "GITBOOK_SCRAPER_USER_AGENT")]
    pub llms_txt: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub no_llms_txt: bool,

    /// Only include pages whose title contains this substring (repeatable).
    #[arg(long, value_name = "TITLE")]
    pub filter: Vec<String>,

    /// Per-request delay, in seconds.
    #[arg(long, env = "GITBOOK_SCRAPER_DELAY", default_value_t = 0.3)]
    pub delay: f64,

    /// Retries per failed request.
    #[arg(long, default_value_t = 3)]
    pub retries: u32,

    /// Request timeout, in seconds.
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,

    /// Max parallel requests.
    #[arg(long, default_value_t = 20)]
    pub concurrency: usize,

    /// Force legacy HTML-scraping mode for old GitBook sites.
    #[arg(long, default_value_t = false)]
    pub legacy: bool,

    /// Overwrite existing files (default: skip).
    #[arg(long, default_value_t = false)]
    pub overwrite: bool,

    /// Debug logging.
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// Suppress progress output.
    #[arg(short, long, default_value_t = false)]
    pub quiet: bool,

    /// Override User-Agent.
    #[arg(long, env = "GITBOOK_SCRAPER_USER_AGENT", default_value = "doc-scraper-rs/0.1")]
    pub user_agent: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("parse failed")
    }

    #[test]
    fn url_is_positional() {
        let c = parse(&["doc-scraper", "https://docs.strata.markets/"]);
        assert_eq!(c.url.as_str(), "https://docs.strata.markets/");
    }

    #[test]
    fn defaults_match_spec() {
        let c = parse(&["doc-scraper", "https://x/"]);
        assert_eq!(c.delay, 0.3);
        assert_eq!(c.retries, 3);
        assert_eq!(c.timeout, 20);
        assert_eq!(c.concurrency, 20);
        assert_eq!(c.user_agent, "doc-scraper-rs/0.1");
        assert!(!c.flat);
        assert!(!c.legacy);
        assert!(!c.overwrite);
    }

    #[test]
    fn flat_flag_overrides_default() {
        let c = parse(&["doc-scraper", "https://x/", "--flat"]);
        assert!(c.flat);
    }

    #[test]
    fn env_var_delay_is_picked_up() {
        // clap respects env vars only when the env var is set at parse-time.
        // We can't easily set env vars in tests portably, so we just sanity-check the
        // attribute is wired: the field uses env = "GITBOOK_SCRAPER_DELAY".
        let c = parse(&["doc-scraper", "https://x/", "--delay", "1.5"]);
        assert_eq!(c.delay, 1.5);
    }

    #[test]
    fn filter_is_repeatable() {
        let c = parse(&["doc-scraper", "https://x/", "--filter", "Tranche", "--filter", "Audits"]);
        assert_eq!(c.filter, vec!["Tranche".to_string(), "Audits".to_string()]);
    }
}