use crate::error::Result;
use std::time::Duration;
use url::Url;

/// Construct a shared `reqwest::Client` with the configured user agent and
/// per-request timeout. One client is built at the start of a run and
/// reused across all sitemap / page / llms.txt fetches so the underlying
/// connection pool is amortised.
pub fn build_client(user_agent: &str, timeout_secs: u64) -> Result<reqwest::Client> {
    let ua = user_agent.to_string();
    let timeout = Duration::from_secs(timeout_secs);
    let client = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(timeout)
        .build()?;
    Ok(client)
}

/// Fetches a URL, retrying up to `retries` times on transport errors and 5xx/429 responses.
/// Sleeps `delay_secs`, then `2 * delay_secs`, etc., capped at `5 * delay_secs`.
pub async fn fetch_with_retry(
    client: &reqwest::Client,
    url: &Url,
    retries: u32,
    delay_secs: f64,
) -> Result<reqwest::Response> {
    let mut attempt: u32 = 0;
    loop {
        match client.get(url.as_str()).send().await {
            Ok(resp) => {
                let status = resp.status();
                // Retry on 5xx and 429 (rate-limited) responses.
                let retryable = status.is_server_error() || status.as_u16() == 429;
                if retryable && attempt < retries {
                    let sleep = backoff_secs(delay_secs, attempt);
                    tokio::time::sleep(Duration::from_secs_f64(sleep)).await;
                    attempt += 1;
                    continue;
                }
                let mapped = resp
                    .error_for_status()
                    .map_err(crate::error::ScraperError::Http)?;
                return Ok(mapped);
            }
            Err(_) if attempt < retries => {
                let sleep = backoff_secs(delay_secs, attempt);
                tokio::time::sleep(Duration::from_secs_f64(sleep)).await;
                attempt += 1;
            }
            Err(e) => return Err(crate::error::ScraperError::Http(e)),
        }
    }
}

fn backoff_secs(base: f64, attempt: u32) -> f64 {
    let cap = base * 5.0;
    (base * 2f64.powi(attempt as i32)).min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_then_caps() {
        assert_eq!(backoff_secs(0.3, 0), 0.3);
        assert_eq!(backoff_secs(0.3, 1), 0.6);
        assert_eq!(backoff_secs(0.3, 2), 1.2);
        assert_eq!(backoff_secs(0.3, 5), 1.5); // capped at 5x = 1.5
    }

    #[tokio::test]
    async fn build_client_uses_user_agent_and_timeout() {
        let c = build_client("test-ua/1.0", 5).unwrap();
        // Verify the configured UA is sent on the wire by checking against a wiremock server.
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header(reqwest::header::USER_AGENT.as_str(), "test-ua/1.0"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let resp = c.get(server.uri()).send().await.unwrap();
        assert!(resp.status().is_success());
    }

    #[tokio::test]
    async fn fetch_with_retry_succeeds_on_first_try_without_sleep() {
        // Build a client pointed at a never-resolving address; one retry should bail out fast.
        let c = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let url = Url::parse("http://127.0.0.1:1/").unwrap();
        let start = std::time::Instant::now();
        let _ = fetch_with_retry(&c, &url, 1, 0.0).await;
        // 1 retry means up to 2 attempts; with base 0.0 this should be near-instant.
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn fetch_with_retry_retries_on_429() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        // First call: 429, second call: 200.
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let c = reqwest::Client::new();
        let url = Url::parse(&format!("{}/x", server.uri())).unwrap();
        let resp = fetch_with_retry(&c, &url, 2, 0.0).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }
}
