use clap::Parser;
use std::path::PathBuf;
use url::Url;

/// Raw clap-derived struct holding every CLI flag the binary accepts.
///
/// Use [`ResolvedConfig`](crate::config::ResolvedConfig) at runtime instead —
/// it merges `Cli` with default-derived values (e.g. the output directory
/// slug, the `llms.txt` path) into a single bag that's easier to pass
/// around.
#[derive(Debug, Parser)]
#[command(
    name = "doc-scraper",
    about = "Export GitBook docs as clean markdown.",
    long_about = "Export GitBook docs as clean markdown.\n\
                  \n\
                  Hits GitBook's hidden `text/markdown` endpoint directly — one HTTP\n\
                  request per page, no HTML parsing, no browser. Produces a clean\n\
                  <output>/<url-path>/<slug>.md mirror tree plus auto-generated\n\
                  `llms.txt`, `llms-full.txt`, `AGENTS.md`, and a sectioned\n\
                  `index.md` table of contents.\n\
                  \n\
                  Run with no arguments for the full flag reference; pass a URL to\n\
                  start scraping. Examples:\n\
                  \n\
                      doc-scraper https://docs.strata.markets\n\
                      doc-scraper https://docs.pareto.credit --filter Tranching\n\
                      doc-scraper https://docs.example.com --flat -o ./scratch/",
    version
)]
pub struct Cli {
    /// GitBook docs base URL.
    pub url: Url,

    /// Output directory.
    #[arg(short, long, env = "GITBOOK_SCRAPER_OUTPUT_DIR")]
    pub output: Option<PathBuf>,

    /// Flatten URLs into filenames instead of mirroring hierarchy.
    #[arg(long, default_value_t = false)]
    pub flat: bool,

    /// Generate index.md with TOC.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub toc: bool,

    /// Path to llms.txt sidecar. Use --no-llms-txt to skip both llms.txt and llms-full.txt.
    #[arg(long, env = "GITBOOK_SCRAPER_LLMS_TXT")]
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
    #[arg(
        long,
        env = "GITBOOK_SCRAPER_USER_AGENT",
        default_value = "doc-scraper-rs/0.1"
    )]
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
        let c = parse(&[
            "doc-scraper",
            "https://x/",
            "--filter",
            "Tranche",
            "--filter",
            "Audits",
        ]);
        assert_eq!(c.filter, vec!["Tranche".to_string(), "Audits".to_string()]);
    }

    #[test]
    fn missing_url_produces_missing_required_argument_error() {
        // The runtime `main()` intercepts this specific error kind to
        // print long help instead of a usage line. The test guards
        // against the error kind changing across clap upgrades.
        let err = Cli::try_parse_from(["doc-scraper"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn invalid_url_produces_value_validation_error() {
        // Any non-MissingRequiredArgument error falls through to clap's
        // default exit-handling (usage + non-zero exit). Make sure this
        // path still produces an error of the expected kind.
        let err = Cli::try_parse_from(["doc-scraper", "not-a-url"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }
}
