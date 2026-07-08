use crate::error::Result;
use crate::page::{url_to_relative_path, Page};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use url::Url;

/// Result of a [`write_page`] call: the file was newly written, or a
/// pre-existing file was left untouched (because `--overwrite` was not
/// passed). The wrapped `PathBuf` is the on-disk target path in both
/// cases.
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
    base: &Url,
    slug_counts: &mut HashMap<String, usize>,
) -> Result<PathBuf> {
    let base_name = if flat {
        crate::page::url_to_flat_slug(&page.url)?
    } else {
        // Mirror mode: reuse the canonical url_to_relative_path helper so we
        // don't drift from page.rs. Caller passes the site `base` URL.
        url_to_relative_path(&page.url, base)?
            .to_string_lossy()
            .replace('\\', "/")
    };

    // Mirror mode is 1:1 (same URL → same file), so no collision counter is needed.
    // Collision suffixing only applies in flat mode.
    let filename = if !flat {
        base_name
    } else {
        let entry = slug_counts.entry(base_name.clone()).or_insert(0);
        *entry += 1;
        if *entry == 1 {
            base_name
        } else {
            // Numeric suffix: foo.md → foo-2.md, foo-3.md, ...
            let stem = base_name.trim_end_matches(".md");
            format!("{stem}-{}.md", *entry)
        }
    };
    let full = dir.join(filename);
    Ok(full)
}

/// Write a single page's body to disk. Honours `--flat` and the
/// `slug_counts` collision counter; with `overwrite = false`, pre-existing
/// files at the planned target are returned as [`WriteOutcome::Skipped`]
/// rather than overwritten, making re-runs idempotent.
pub fn write_page(
    page: Page,
    body: String,
    dir: &Path,
    flat: bool,
    overwrite: bool,
    base: &Url,
    slug_counts: &mut HashMap<String, usize>,
) -> Result<WriteOutcome> {
    let path = plan_path(&page, dir, flat, base, slug_counts)?;
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

    fn base() -> Url {
        Url::parse("https://docs.strata.markets/").unwrap()
    }

    #[test]
    fn mirror_mode_creates_subdirs_and_writes_file() {
        let dir = tempdir().unwrap();
        let p = page("https://docs.strata.markets/markets/ethena-usde");
        let mut counts = HashMap::new();
        let outcome = write_page(
            p,
            "# body".into(),
            dir.path(),
            false,
            true,
            &base(),
            &mut counts,
        )
        .unwrap();
        let written = match outcome {
            WriteOutcome::Written(p) => p,
            _ => panic!("expected Written"),
        };
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
        let outcome =
            write_page(p, "x".into(), dir.path(), true, true, &base(), &mut counts).unwrap();
        let written = match outcome {
            WriteOutcome::Written(p) => p,
            _ => panic!(),
        };
        assert_eq!(
            written.file_name().unwrap(),
            "markets.ethena-usde.srusde.md"
        );
    }

    #[test]
    fn collision_uses_numeric_suffix_in_flat_mode() {
        let dir = tempdir().unwrap();
        let p1 = page("https://docs.strata.markets/a/b");
        let p2 = page("https://docs.strata.markets/a/b"); // duplicate URL
        let mut counts = HashMap::new();
        let _ = write_page(
            p1,
            "first".into(),
            dir.path(),
            true,
            true,
            &base(),
            &mut counts,
        )
        .unwrap();
        let outcome = write_page(
            p2,
            "second".into(),
            dir.path(),
            true,
            true,
            &base(),
            &mut counts,
        )
        .unwrap();
        let written = match outcome {
            WriteOutcome::Written(p) => p,
            _ => panic!(),
        };
        assert_eq!(written.file_name().unwrap(), "a.b-2.md");
    }

    #[test]
    fn no_overwrite_skips_existing() {
        let dir = tempdir().unwrap();
        let p1 = page("https://docs.strata.markets/foo");
        let p2 = page("https://docs.strata.markets/foo");
        let mut counts = HashMap::new();
        let _ = write_page(
            p1,
            "first".into(),
            dir.path(),
            false,
            false,
            &base(),
            &mut counts,
        )
        .unwrap();
        let outcome = write_page(
            p2,
            "second".into(),
            dir.path(),
            false,
            false,
            &base(),
            &mut counts,
        )
        .unwrap();
        assert!(matches!(outcome, WriteOutcome::Skipped(_)));
    }

    #[test]
    fn overwrite_flag_replaces_existing() {
        let dir = tempdir().unwrap();
        let p1 = page("https://docs.strata.markets/foo");
        let p2 = page("https://docs.strata.markets/foo");
        let mut counts = HashMap::new();
        let _ = write_page(
            p1,
            "first".into(),
            dir.path(),
            false,
            true,
            &base(),
            &mut counts,
        )
        .unwrap();
        let outcome = write_page(
            p2,
            "second".into(),
            dir.path(),
            false,
            true,
            &base(),
            &mut counts,
        )
        .unwrap();
        assert!(matches!(outcome, WriteOutcome::Written(_)));
    }
}
