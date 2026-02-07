//! Crypto cluster definitions for cluster-based optimization.
//!
//! Cryptos are grouped by market profile (e.g. large-cap, mid-cap) so that
//! one set of parameters is optimized per cluster and can be applied to all
//! symbols in that cluster (similar behavior, lower overfitting than per-symbol).

/// A named group of crypto symbols assumed to share similar market behavior.
#[derive(Debug, Clone)]
pub struct CryptoCluster {
    /// Identifier used in CLI (e.g. "large_cap").
    pub id: &'static str,
    /// Human-readable label.
    pub label: &'static str,
    /// Symbols in this cluster (normalized, e.g. "BTC/USD").
    pub symbols: &'static [&'static str],
}

impl CryptoCluster {
    /// First symbol in the cluster, used as representative for optimization.
    pub fn representative_symbol(&self) -> &'static str {
        self.symbols.first().copied().unwrap_or("BTC/USD")
    }

    /// All symbols as owned strings.
    pub fn symbol_list(&self) -> Vec<String> {
        self.symbols.iter().map(|s| (*s).to_string()).collect()
    }
}

/// Default crypto clusters: large-cap (blue chips), mid-cap (L1s), small-cap (alts).
pub fn default_clusters() -> Vec<CryptoCluster> {
    vec![
        CryptoCluster {
            id: "large_cap",
            label: "Large cap (BTC, ETH)",
            symbols: &["BTC/USD", "ETH/USD"],
        },
        CryptoCluster {
            id: "mid_cap",
            label: "Mid cap (SOL, AVAX, LINK, etc.)",
            symbols: &["SOL/USD", "AVAX/USD", "LINK/USD", "DOT/USD", "MATIC/USD"],
        },
        CryptoCluster {
            id: "small_cap",
            label: "Small cap / alts",
            symbols: &["DOGE/USD", "ATOM/USD", "UNI/USD", "LTC/USD", "XRP/USD"],
        },
    ]
}

/// Returns the cluster whose `id` matches (case-insensitive), or None.
pub fn cluster_by_id(id: &str) -> Option<CryptoCluster> {
    let id_lower = id.to_lowercase();
    default_clusters()
        .into_iter()
        .find(|c| c.id.to_lowercase() == id_lower)
}

/// Returns clusters whose ids are in the given list (or all if list is empty).
pub fn resolve_clusters(ids: &[String]) -> Vec<CryptoCluster> {
    if ids.is_empty() {
        return default_clusters();
    }
    ids.iter().filter_map(|id| cluster_by_id(id)).collect()
}
