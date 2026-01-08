use reqwest::Client;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use std::time::Duration;

pub struct HttpClientFactory;

impl HttpClientFactory {
    /// Creates a new HTTP client with retry middleware
    pub fn create_client() -> ClientWithMiddleware {
        // Retry policy:
        // - Exponential backoff
        // - Max 3 retries
        // - Base delay 500ms
        let retry_policy = ExponentialBackoff::builder()
            .build_with_max_retries(3);

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
