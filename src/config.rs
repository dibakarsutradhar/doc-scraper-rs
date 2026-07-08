use crate::cli::Cli;
use crate::page::site_slug_from_url;
use std::path::PathBuf;

/// Fully-resolved runtime configuration: the raw clap [`Cli`] struct merged
/// with defaults derived from the input (e.g. the output directory slugged
/// from the site host, the `llms.txt` path defaulting into the output dir).
///
/// `ResolvedConfig::from_cli` is the one entry point that produces this.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub url: url::Url,
    pub output_dir: PathBuf,
    pub flat: bool,
    pub toc: bool,
    pub llms_txt_path: Option<PathBuf>,
    pub filters: Vec<String>,
    pub delay_secs: f64,
    pub retries: u32,
    pub timeout_secs: u64,
    pub concurrency: usize,
    pub legacy: bool,
    pub overwrite: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub user_agent: String,
}

impl ResolvedConfig {
    pub fn from_cli(cli: Cli) -> Self {
        let slug = site_slug_from_url(&cli.url);
        let output_dir = cli
            .output
            .unwrap_or_else(|| PathBuf::from(format!("./{slug}/")));
        let llms_txt_path = if cli.no_llms_txt {
            None
        } else {
            Some(cli.llms_txt.unwrap_or_else(|| output_dir.join("llms.txt")))
        };
        Self {
            url: cli.url,
            output_dir,
            flat: cli.flat,
            toc: cli.toc,
            llms_txt_path,
            filters: cli.filter,
            delay_secs: cli.delay,
            retries: cli.retries,
            timeout_secs: cli.timeout,
            concurrency: cli.concurrency,
            legacy: cli.legacy,
            overwrite: cli.overwrite,
            verbose: cli.verbose,
            quiet: cli.quiet,
            user_agent: cli.user_agent,
        }
    }
}
