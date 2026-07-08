use crate::error::{Result, ScraperError};
use crate::http::fetch_with_retry;
use crate::sitemap::PageRef;
use futures::stream::{FuturesUnordered, StreamExt};
use regex::Regex;
use std::sync::{Arc, LazyLock};
use url::Url;

/// GitBook's "Page Not Found" stub for unknown `.md` paths. We use this as a
/// content-based signal to fall back to the bare HTML URL, since the response
/// is HTTP 200 (soft 404) — we cannot rely on the status code.
static PAGE_NOT_FOUND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^\s*#\s+page not found\b").expect("page-not-found regex"));

/// Returns true if `body` looks like GitBook's soft-404 markdown stub for an
/// unknown page. Must NOT trigger on body text that mentions "page not found"
/// in a non-heading line (e.g. documentation copy).
pub fn is_gitbook_not_found(body: &str) -> bool {
    PAGE_NOT_FOUND.is_match(body)
}

/// Convert a page URL into its `.md` endpoint form.
///
/// Returns `None` when:
/// - the scheme is not `http`/`https` (defensive: mailto, data, etc.), or
/// - the URL is the site root (no path or just `/`) — the homepage is served
///   as a SPA shell and has no useful `.md` representation; the caller should
///   rely on the `/llms.txt` fallback path instead.
pub fn to_md_url(url: &Url) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let path = url.path();
    // Strip a trailing `/` so `/markets/ethena/` and `/markets/ethena`
    // produce the same `.md` endpoint. We always start with a path that has
    // at least one non-`/` character (the root case is rejected above).
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let mut new_url = url.clone();
    new_url.set_path(&format!("{trimmed}.md"));
    Some(new_url)
}

pub async fn fetch_all(
    client: reqwest::Client,
    pages: Vec<PageRef>,
    concurrency: usize,
    retries: u32,
    mut delay_secs: f64,
) -> Vec<std::result::Result<(PageRef, String), (PageRef, ScraperError)>> {
    // Clamp negative user-supplied delay to zero. The pre-fetch sleep in
    // `fetch_one` and the exponential backoff inside `fetch_with_retry` both
    // consume `delay_secs`; clamping here ensures neither path produces a
    // negative sleep duration. Effective per-request rate is bounded below by
    // the larger of (delay_secs) and the backoff cap (5 * delay_secs) on the
    // final retry.
    if delay_secs < 0.0 {
        delay_secs = 0.0;
    }
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
    let client = Arc::new(client);
    let mut tasks = FuturesUnordered::new();

    for page in pages {
        let permit_sem = semaphore.clone();
        let client = client.clone();
        tasks.push(async move {
            // We own the semaphore (it is constructed above and never closed),
            // so `acquire_owned` cannot fail. Use `unwrap_or_else` instead of
            // `expect` so a future refactor that drops the semaphore does not
            // panic the whole pipeline.
            let _permit = permit_sem.acquire_owned().await.unwrap_or_else(|_| {
                unreachable!("semaphore is owned by this future and never closed")
            });
            let result = fetch_one(&client, &page.loc, retries, delay_secs).await;
            match result {
                Ok(body) => Ok((page, body)),
                Err(e) => Err((page, e)),
            }
        });
    }

    let mut out: Vec<std::result::Result<(PageRef, String), (PageRef, ScraperError)>> = Vec::new();
    while let Some(item) = tasks.next().await {
        out.push(item);
    }
    out
}

/// Fetches a page by first requesting its `.md` endpoint. If GitBook returns a
/// soft-404 markdown stub, fall back to the bare HTML URL with no extra delay
/// (we already paid the politeness cost on the first request).
async fn fetch_one(
    client: &reqwest::Client,
    url: &Url,
    retries: u32,
    delay_secs: f64,
) -> Result<String> {
    let primary = to_md_url(url);
    let primary = match primary {
        Some(u) => u,
        None => return Err(ScraperError::Other(format!("no .md form for {url}"))),
    };

    // Per-request politeness delay is applied *before every primary attempt*,
    // including each retry inside `fetch_with_retry` (which adds its own
    // exponential backoff on top). Effective per-request floor = max(delay_secs,
    // exponential backoff sequence capped at 5*delay_secs).
    if delay_secs > 0.0 {
        tokio::time::sleep(std::time::Duration::from_secs_f64(delay_secs)).await;
    }

    let resp = fetch_with_retry(client, &primary, retries, delay_secs).await?;
    let body = resp.text().await?;

    if is_gitbook_not_found(&body) {
        // Fall back to the bare URL (HTML). Reuse the same retry budget so we
        // don't silently give up on transient outages here. No additional delay:
        // we've already waited on the first try.
        let resp = fetch_with_retry(client, url, retries, 0.0).await?;
        return resp.text().await.map_err(ScraperError::Http);
    }

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn to_md_url_appends_suffix() {
        let u = Url::parse("https://docs.example.com/markets/ethena").unwrap();
        assert_eq!(
            to_md_url(&u).unwrap().as_str(),
            "https://docs.example.com/markets/ethena.md"
        );
    }

    #[test]
    fn to_md_url_handles_trailing_slash() {
        // Trailing slashes are stripped so `/a/` and `/a` map to the same
        // `.md` endpoint — GitBook treats them as equivalent.
        let u = Url::parse("https://docs.example.com/markets/ethena/").unwrap();
        assert_eq!(
            to_md_url(&u).unwrap().as_str(),
            "https://docs.example.com/markets/ethena.md"
        );
    }

    #[test]
    fn to_md_url_returns_none_for_root() {
        let u = Url::parse("https://docs.example.com/").unwrap();
        assert!(to_md_url(&u).is_none());
        let u = Url::parse("https://docs.example.com").unwrap();
        assert!(to_md_url(&u).is_none());
    }

    #[test]
    fn to_md_url_rejects_non_http_schemes() {
        let u = Url::parse("mailto:hello@example.com").unwrap();
        assert!(to_md_url(&u).is_none());
    }

    #[test]
    fn is_gitbook_not_found_matches_heading_stub() {
        let body = "# Page Not Found\n\nThe URL `foo` does not exist.\n";
        assert!(is_gitbook_not_found(body));
        // Case-insensitive
        let body = "# page not found\n\n…\n";
        assert!(is_gitbook_not_found(body));
    }

    #[test]
    fn is_gitbook_not_found_ignores_prose_mention() {
        let body = "# Foo\n\nThe page not found message is shown when…\n";
        assert!(!is_gitbook_not_found(body));
    }

    #[test]
    fn is_gitbook_not_found_rejects_empty() {
        assert!(!is_gitbook_not_found(""));
    }

    #[tokio::test]
    async fn fetch_all_returns_one_result_per_input() {
        let server = MockServer::start().await;
        for slug in &["a", "b"] {
            Mock::given(method("GET"))
                .and(path(format!("/{slug}.md")))
                .respond_with(ResponseTemplate::new(200).set_body_string(format!("# {slug}")))
                .mount(&server)
                .await;
        }

        let client = reqwest::Client::new();
        let pages = vec![
            PageRef {
                loc: Url::parse(&format!("{}/a", server.uri())).unwrap(),
                lastmod: None,
            },
            PageRef {
                loc: Url::parse(&format!("{}/b", server.uri())).unwrap(),
                lastmod: None,
            },
        ];
        let results = fetch_all(client, pages, 2, 0, 0.0).await;
        assert_eq!(results.len(), 2);
        for r in &results {
            let (_page, body) = r.as_ref().unwrap();
            assert!(body.starts_with("# "));
        }
    }

    #[tokio::test]
    async fn fetch_all_falls_back_to_html_on_soft_404() {
        let server = MockServer::start().await;
        // Primary `.md` returns GitBook's soft-404 stub.
        Mock::given(method("GET"))
            .and(path("/missing.md"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/markdown")
                    .set_body_string("# Page Not Found\n\nThe URL `missing` does not exist.\n"),
            )
            .mount(&server)
            .await;
        // Fallback bare URL returns real HTML.
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_string("<html><title>Missing | Docs</title></html>"),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let pages = vec![PageRef {
            loc: Url::parse(&format!("{}/missing", server.uri())).unwrap(),
            lastmod: None,
        }];
        let results = fetch_all(client, pages, 1, 0, 0.0).await;
        assert_eq!(results.len(), 1);
        let (_, body) = results.into_iter().next().unwrap().unwrap();
        assert!(
            body.contains("<title>Missing"),
            "should fall back to HTML body"
        );
    }

    #[tokio::test]
    async fn fetch_all_skips_root_url_without_md_form() {
        let server = MockServer::start().await;
        let client = reqwest::Client::new();
        let pages = vec![PageRef {
            loc: Url::parse(&server.uri()).unwrap(), // root: no .md form
            lastmod: None,
        }];
        let results = fetch_all(client, pages, 1, 0, 0.0).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err(), "root URL must surface an error");
    }

    #[tokio::test]
    async fn fetch_all_respects_concurrency_limit() {
        let server = MockServer::start().await;
        for i in 0..4 {
            Mock::given(method("GET"))
                .and(path(format!("/p{i}.md")))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_delay(std::time::Duration::from_millis(100))
                        .set_body_string(format!("# p{i}")),
                )
                .mount(&server)
                .await;
        }
        let client = reqwest::Client::new();
        let pages = (0..4)
            .map(|i| PageRef {
                loc: Url::parse(&format!("{}/p{i}", server.uri())).unwrap(),
                lastmod: None,
            })
            .collect();
        // 4 pages, each 100ms, concurrency=2 → at least ~200ms total.
        let start = std::time::Instant::now();
        let results = fetch_all(client, pages, 2, 0, 0.0).await;
        let elapsed = start.elapsed();
        assert_eq!(results.len(), 4);
        assert!(elapsed >= std::time::Duration::from_millis(180));
    }

    #[tokio::test]
    async fn fetch_all_keeps_going_on_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ok.md"))
            .respond_with(ResponseTemplate::new(200).set_body_string("# ok"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/bad.md"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let pages = vec![
            PageRef {
                loc: Url::parse(&format!("{}/ok", server.uri())).unwrap(),
                lastmod: None,
            },
            PageRef {
                loc: Url::parse(&format!("{}/bad", server.uri())).unwrap(),
                lastmod: None,
            },
        ];
        // 0 retries → /bad surfaces as error, /ok returns ok.
        let results = fetch_all(client, pages, 2, 0, 0.0).await;
        assert_eq!(results.len(), 2);
        let oks = results.iter().filter(|r| r.is_ok()).count();
        let errs = results.iter().filter(|r| r.is_err()).count();
        assert_eq!(oks, 1);
        assert_eq!(errs, 1);
    }
}
