use crate::error::{Result, ScraperError};
use crate::page::Page;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Written(PathBuf),
    Skipped(PathBuf),
}

/// Decides the on-disk path for a page, honoring `--flat` and de-duping collisions.
pub fn plan_path(
    page: &Page,
    dir: &Path,
    flat: bool,
    slug_counts: &mut HashMap<String, usize>,
) -> Result<PathBuf> {
    let base = if flat {
        let slug = crate::page::url_to_flat_slug(&page.url)?;
        slug
    } else {
        // Mirror mode: derive from base URL; we store base alongside body in caller.
        // For tests, mirror mode uses the Page's stored derived path; main.rs computes it.
        // Here, the convention is the page URL's full path under dir.
        let mut p = page.url.path().trim_start_matches('/').to_string();
        if p.is_empty() { p = "index".into(); }
        if p.ends_with('/') { p.push_str("index"); }
        if !p.ends_with(".md") { p.push_str(".md"); }
        p
    };

    // Mirror mode is 1:1 (same URL → same file), so no collision counter is needed.
    // Collision suffixing only applies in flat mode.
    let filename = if !flat {
        base
    } else {
        let entry = slug_counts.entry(base.clone()).or_insert(0);
        *entry += 1;
        if *entry == 1 {
            base
        } else {
            // Numeric suffix: foo.md → foo-2.md, foo-3.md, ...
            let stem = base.trim_end_matches(".md");
            format!("{stem}-{}.md", *entry)
        }
    };
    let full = dir.join(filename);
    Ok(full)
}

pub fn write_page(
    page: Page,
    body: String,
    dir: &Path,
    flat: bool,
    overwrite: bool,
    slug_counts: &mut HashMap<String, usize>,
) -> Result<WriteOutcome> {
    let path = plan_path_for_page(&page, dir, flat, slug_counts)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if path.exists() && !overwrite {
        return Ok(WriteOutcome::Skipped(path));
    }
    std::fs::write(&path, body.as_bytes())?;
    Ok(WriteOutcome::Written(path))
}

// Helper that captures the mode-aware plan without needing caller to construct a "Page"
fn plan_path_for_page(
    page: &Page,
    dir: &Path,
    flat: bool,
    slug_counts: &mut HashMap<String, usize>,
) -> Result<PathBuf> {
    plan_path(page, dir, flat, slug_counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;
    use url::Url;

    fn page(url: &str) -> Page {
        Page {
            url: Url::parse(url).unwrap(),
            title: None,
            body: String::new(),
        }
    }

    #[test]
    fn mirror_mode_creates_subdirs_and_writes_file() {
        let dir = tempdir().unwrap();
        let p = page("https://docs.strata.markets/markets/ethena-usde");
        let mut counts = HashMap::new();
        let outcome = write_page(p, "# body".into(), dir.path(), false, true, &mut counts).unwrap();
        let written = match outcome { WriteOutcome::Written(p) => p, _ => panic!("expected Written") };
        assert!(written.exists());
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "# body");
        // Subdir should exist
        assert!(dir.path().join("markets").join("ethena-usde.md").exists());
    }

    #[test]
    fn flat_mode_writes_to_root() {
        let dir = tempdir().unwrap();
        let p = page("https://docs.strata.markets/markets/ethena-usde/srusde");
        let mut counts = HashMap::new();
        let outcome = write_page(p, "x".into(), dir.path(), true, true, &mut counts).unwrap();
        let written = match outcome { WriteOutcome::Written(p) => p, _ => panic!() };
        assert_eq!(written.file_name().unwrap(), "markets.ethena-usde.srusde.md");
    }

    #[test]
    fn collision_uses_numeric_suffix_in_flat_mode() {
        let dir = tempdir().unwrap();
        let p1 = page("https://docs.strata.markets/a/b");
        let p2 = page("https://docs.strata.markets/a/b"); // duplicate URL
        let mut counts = HashMap::new();
        let _ = write_page(p1, "first".into(), dir.path(), true, true, &mut counts).unwrap();
        let outcome = write_page(p2, "second".into(), dir.path(), true, true, &mut counts).unwrap();
        let written = match outcome { WriteOutcome::Written(p) => p, _ => panic!() };
        assert_eq!(written.file_name().unwrap(), "a.b-2.md");
    }

    #[test]
    fn no_overwrite_skips_existing() {
        let dir = tempdir().unwrap();
        let p1 = page("https://docs.strata.markets/foo");
        let p2 = page("https://docs.strata.markets/foo");
        let mut counts = HashMap::new();
        let _ = write_page(p1, "first".into(), dir.path(), false, false, &mut counts).unwrap();
        let outcome = write_page(p2, "second".into(), dir.path(), false, false, &mut counts).unwrap();
        assert!(matches!(outcome, WriteOutcome::Skipped(_)));
    }

    #[test]
    fn overwrite_flag_replaces_existing() {
        let dir = tempdir().unwrap();
        let p1 = page("https://docs.strata.markets/foo");
        let p2 = page("https://docs.strata.markets/foo");
        let mut counts = HashMap::new();
        let _ = write_page(p1, "first".into(), dir.path(), false, true, &mut counts).unwrap();
        let outcome = write_page(p2, "second".into(), dir.path(), false, true, &mut counts).unwrap();
        assert!(matches!(outcome, WriteOutcome::Written(_)));
    }
}