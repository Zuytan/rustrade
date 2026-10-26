//! REST/WebSocket API server configuration parsing from environment variables.
//!
//! The API exposes portfolio data and the live log stream, so it is bound to
//! loopback by default and refuses to listen on a non-loopback address
//! unless an API key is configured.

use anyhow::{Context, bail};
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

const DEFAULT_PORT: u16 = 8080;
const MIN_API_KEY_LEN: usize = 16;

/// API server environment configuration
#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    pub bind_address: IpAddr,
    pub port: u16,
    /// Required in the `X-API-Key` header when set
    pub api_key: Option<String>,
}

impl ApiServerConfig {
    /// Load from environment variables:
    /// - `API_BIND_ADDRESS` (default `127.0.0.1`)
    /// - `PORT`, then `API_PORT` (default `8080`)
    /// - `API_KEY` (mandatory when the bind address is not loopback)
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// Same as `from_env`, with an injectable variable lookup (used by tests).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let bind_address = match get("API_BIND_ADDRESS") {
            Some(raw) => raw
                .trim()
                .parse::<IpAddr>()
                .with_context(|| format!("Invalid API_BIND_ADDRESS: '{raw}'"))?,
            None => IpAddr::V4(Ipv4Addr::LOCALHOST),
        };

        let port = match get("PORT").or_else(|| get("API_PORT")) {
            Some(raw) => raw
                .trim()
                .parse::<u16>()
                .with_context(|| format!("Invalid API port (PORT/API_PORT): '{raw}'"))?,
            None => DEFAULT_PORT,
        };

        let api_key = get("API_KEY")
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());

        if let Some(key) = &api_key
            && key.chars().count() < MIN_API_KEY_LEN
        {
            bail!("API_KEY is too short: at least {MIN_API_KEY_LEN} characters are required");
        }

        if !bind_address.is_loopback() && api_key.is_none() {
            bail!(
                "Refusing to expose the API on {bind_address} without API_KEY. \
                 Set API_KEY or bind to 127.0.0.1"
            );
        }

        Ok(Self {
            bind_address,
            port,
            api_key,
        })
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_address, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const STRONG_KEY: &str = "0123456789abcdef0123";

    fn parse(vars: &[(&str, &str)]) -> anyhow::Result<ApiServerConfig> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ApiServerConfig::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn test_defaults_bind_loopback_without_key() {
        let config = parse(&[]).expect("defaults must be valid");
        assert_eq!(config.bind_address, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(config.port, 8080);
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_port_from_port_then_api_port() {
        assert_eq!(parse(&[("API_PORT", "9000")]).unwrap().port, 9000);
        assert_eq!(
            parse(&[("PORT", "7000"), ("API_PORT", "9000")])
                .unwrap()
                .port,
            7000
        );
    }

    #[test]
    fn test_invalid_port_is_an_error_not_a_silent_fallback() {
        assert!(parse(&[("API_PORT", "80800")]).is_err());
        assert!(parse(&[("API_PORT", "abc")]).is_err());
    }

    #[test]
    fn test_invalid_bind_address_is_an_error() {
        assert!(parse(&[("API_BIND_ADDRESS", "not-an-ip")]).is_err());
    }

    #[test]
    fn test_public_bind_without_key_is_refused() {
        let err = parse(&[("API_BIND_ADDRESS", "0.0.0.0")]).unwrap_err();
        assert!(err.to_string().contains("API_KEY"), "got: {err}");
    }

    #[test]
    fn test_public_bind_with_key_is_allowed() {
        let config = parse(&[("API_BIND_ADDRESS", "0.0.0.0"), ("API_KEY", STRONG_KEY)])
            .expect("public bind with a key must be accepted");
        assert_eq!(config.api_key.as_deref(), Some(STRONG_KEY));
    }

    #[test]
    fn test_blank_key_counts_as_absent() {
        assert!(parse(&[("API_KEY", "   ")]).unwrap().api_key.is_none());
        assert!(parse(&[("API_BIND_ADDRESS", "0.0.0.0"), ("API_KEY", "  ")]).is_err());
    }

    #[test]
    fn test_short_key_is_refused() {
        assert!(parse(&[("API_KEY", "short")]).is_err());
    }
}
