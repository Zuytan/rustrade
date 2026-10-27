use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use std::time::Duration;

pub struct HttpClientFactory;

impl HttpClientFactory {
    /// Creates a new HTTP client with retry middleware.
    ///
    /// Only for idempotent requests (GET, DELETE). Never use it to submit orders:
    /// a retried POST after a timeout can place the same order twice.
    pub fn create_client() -> ClientWithMiddleware {
        // Retry policy:
        // - Exponential backoff
        // - Max 3 retries
        // - Base delay 500ms
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

        ClientBuilder::new(Self::base_client())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build()
    }

    /// Creates an HTTP client without automatic retries, for order submission.
    ///
    /// Ambiguous failures (timeouts, 5xx) must be resolved by looking the order up,
    /// never by resending it.
    pub fn create_write_client() -> ClientWithMiddleware {
        ClientBuilder::new(Self::base_client()).build()
    }

    fn base_client() -> Client {
        // This configuration only fails if the TLS backend cannot be initialized. The former
        // `Client::new()` fallback panicked in that same case (and would have dropped the
        // timeouts otherwise), so fail loudly with an explicit message instead.
        Client::builder()
            .pool_max_idle_per_host(5)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client with timeouts (TLS backend unavailable?)")
    }
}

/// Helper function to build a URL with query parameters.
/// Since reqwest-middleware 0.5.0 doesn't expose the .query() method,
/// we build the query string manually and append it to the URL.
pub fn build_url_with_query<K, V>(base_url: &str, params: &[(K, V)]) -> String
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    if params.is_empty() {
        return base_url.to_string();
    }

    let query_string: String = params
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                urlencoding_encode(k.as_ref()),
                urlencoding_encode(v.as_ref())
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    if base_url.contains('?') {
        format!("{}&{}", base_url, query_string)
    } else {
        format!("{}?{}", base_url, query_string)
    }
}

/// Simple URL encoding function for query parameter values.
fn urlencoding_encode(s: &str) -> String {
    let mut encoded = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                encoded.push(c);
            }
            _ => {
                for byte in c.to_string().as_bytes() {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    encoded
}
