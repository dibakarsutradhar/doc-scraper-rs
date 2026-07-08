use crate::cli::Cli;

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub url: url::Url,
    pub output_dir: std::path::PathBuf,
    pub flat: bool,
    pub toc: bool,
    pub llms_txt_path: Option<std::path::PathBuf>,
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

impl From<Cli> for ResolvedConfig {
    fn from(c: Cli) -> Self {
        Self {
            url: c.url,
            output_dir: c.output.unwrap_or_else(|| std::path::PathBuf::from("./<site-slug>/")),
            flat: c.flat,
            toc: c.toc,
            llms_txt_path: if c.no_llms_txt { None } else { c.llms_txt },
            filters: c.filter,
            delay_secs: c.delay,
            retries: c.retries,
            timeout_secs: c.timeout,
            concurrency: c.concurrency,
            legacy: c.legacy,
            overwrite: c.overwrite,
            verbose: c.verbose,
            quiet: c.quiet,
            user_agent: c.user_agent,
        }
    }
}