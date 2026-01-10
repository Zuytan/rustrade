use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use std::time::Duration;

pub struct HttpClientFactory;

impl HttpClientFactory {
    /// Creates a new HTTP client with retry middleware
    pub fn create_client() -> ClientWithMiddleware {
        // Retry policy:
        // - Exponential backoff
        // - Max 3 retries
        // - Base delay 500ms
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

        let client = Client::builder()
            .pool_max_idle_per_host(5)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        ClientBuilder::new(client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build()
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
