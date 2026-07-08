use crate::error::Result;
use crate::http::fetch_with_retry;
use crate::sitemap::PageRef;
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;
use url::Url;

pub async fn fetch_all(
    client: reqwest::Client,
    pages: Vec<PageRef>,
    concurrency: usize,
    retries: u32,
    delay_secs: f64,
) -> Vec<Result<(PageRef, String)>> {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
    let client = Arc::new(client);
    let mut tasks = FuturesUnordered::new();

    for page in pages {
        let permit_sem = semaphore.clone();
        let client = client.clone();
        tasks.push(async move {
            let _permit = permit_sem.acquire_owned().await.expect("semaphore closed");
            let result = fetch_one(&client, &page.loc, retries, delay_secs).await;
            match result {
                Ok(body) => Ok((page, body)),
                Err(e) => Err(e),
            }
        });
    }

    let mut out = Vec::new();
    while let Some(item) = tasks.next().await {
        out.push(item);
    }
    out
}

async fn fetch_one(
    client: &reqwest::Client,
    url: &Url,
    retries: u32,
    delay_secs: f64,
) -> Result<String> {
    // Delay first to be polite even on success path.
    if delay_secs > 0.0 {
        tokio::time::sleep(std::time::Duration::from_secs_f64(delay_secs)).await;
    }
    let resp = fetch_with_retry(client, url, retries, delay_secs).await?;
    let body = resp.text().await?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_all_returns_one_result_per_input() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/a"))
            .respond_with(ResponseTemplate::new(200).set_body_string("# A"))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/b"))
            .respond_with(ResponseTemplate::new(200).set_body_string("# B"))
            .mount(&server).await;

        let client = reqwest::Client::new();
        let pages = vec![
            PageRef { loc: Url::parse(&format!("{}/a", server.uri())).unwrap(), lastmod: None },
            PageRef { loc: Url::parse(&format!("{}/b", server.uri())).unwrap(), lastmod: None },
        ];
        let results = fetch_all(client, pages, 2, 0, 0.0).await;
        assert_eq!(results.len(), 2);
        for r in &results {
            let (_page, body) = r.as_ref().unwrap();
            assert!(body.starts_with("# "));
        }
    }

    #[tokio::test]
    async fn fetch_all_respects_concurrency_limit() {
        let server = MockServer::start().await;
        for i in 0..4 {
            Mock::given(method("GET")).and(path(&format!("/p{i}")))
                .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(100)).set_body_string(format!("# p{i}")))
                .mount(&server).await;
        }
        let client = reqwest::Client::new();
        let pages = (0..4).map(|i| PageRef {
            loc: Url::parse(&format!("{}/p{i}", server.uri())).unwrap(),
            lastmod: None,
        }).collect();
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
        Mock::given(method("GET")).and(path("/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("# ok"))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/bad"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server).await;

        let client = reqwest::Client::new();
        let pages = vec![
            PageRef { loc: Url::parse(&format!("{}/ok", server.uri())).unwrap(), lastmod: None },
            PageRef { loc: Url::parse(&format!("{}/bad", server.uri())).unwrap(), lastmod: None },
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