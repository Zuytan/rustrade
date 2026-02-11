use crate::domain::repositories::CandleRepository;
use crate::domain::trading::types::Candle;
use anyhow::{Context, Result};
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

pub struct CorrelationService {
    candle_repository: Arc<dyn CandleRepository>,
    correlation_matrix: Arc<RwLock<HashMap<(String, String), Decimal>>>,
}

use rust_decimal::Decimal;

impl CorrelationService {
    pub fn new(candle_repository: Arc<dyn CandleRepository>) -> Self {
        Self {
            candle_repository,
            correlation_matrix: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start the background refresh task
    pub async fn start_background_refresh(self: Arc<Self>, symbols: Vec<String>) {
        info!("CorrelationService: Starting background refresh task");
        let service = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // Every hour
            loop {
                interval.tick().await;
                if let Err(e) = service.refresh_correlation_matrix(&symbols).await {
                    error!("CorrelationService: Failed to refresh matrix: {}", e);
                }
            }
        });
    }

    /// Refresh the correlation matrix explicitly (can be called by background task or manually)
    pub async fn refresh_correlation_matrix(&self, symbols: &[String]) -> Result<()> {
        info!(
            "CorrelationService: Refreshing correlation matrix for {} symbols",
            symbols.len()
        );
        let end_ts = chrono::Utc::now().timestamp();
        let start_ts = end_ts - (30 * 24 * 60 * 60); // 30 days

        let mut returns = HashMap::new();

        for symbol in symbols {
            // Optimization: We could parallelize this fetch if needed
            let candles = self
                .candle_repository
                .get_range(symbol, start_ts, end_ts)
                .await
                .context(format!("Failed to fetch candles for {}", symbol))?;

            if candles.is_empty() {
                continue;
            }

            let symbol_returns = self.calculate_returns(&candles);
            returns.insert(symbol.clone(), symbol_returns);
        }

        let mut matrix = HashMap::new();
        let active_symbols: Vec<String> = returns.keys().cloned().collect();

        for i in 0..active_symbols.len() {
            for j in i..active_symbols.len() {
                let s1 = &active_symbols[i];
                let s2 = &active_symbols[j];

                let corr_f64 = self.calculate_pearson_correlation(&returns[s1], &returns[s2]);
                let corr = Decimal::from_f64_retain(corr_f64).unwrap_or(Decimal::ZERO);
                matrix.insert((s1.clone(), s2.clone()), corr);
                if s1 != s2 {
                    matrix.insert((s2.clone(), s1.clone()), corr);
                }
            }
        }

        // Update cache
        let mut write_guard = self.correlation_matrix.write().await;
        *write_guard = matrix;
        info!("CorrelationService: Matrix updated successfully");

        Ok(())
    }

    /// Get correlation matrix from cache (Non-blocking / Fast)
    /// If symbols are not in cache, they will return 0.0 correlation (safe default)
    pub async fn get_correlation_matrix(
        &self,
        symbols: &[String],
    ) -> Result<HashMap<(String, String), Decimal>> {
        let cache = self.correlation_matrix.read().await;
        let mut result = HashMap::new();

        for s1 in symbols {
            for s2 in symbols {
                let key = (s1.clone(), s2.clone());
                if let Some(val) = cache.get(&key) {
                    result.insert(key, *val);
                } else if s1 == s2 {
                    result.insert(key, Decimal::ONE);
                } else {
                    // Default to 0 if unknown
                    result.insert(key, Decimal::ZERO);
                }
            }
        }

        Ok(result)
    }

    // Deprecated: kept for compatibility if interface requires it, but redirects to get_correlation_matrix
    pub async fn calculate_correlation_matrix(
        &self,
        symbols: &[String],
    ) -> Result<HashMap<(String, String), Decimal>> {
        self.get_correlation_matrix(symbols).await
    }

    fn calculate_returns(&self, candles: &[Candle]) -> Vec<f64> {
        if candles.len() < 2 {
            return Vec::new();
        }

        let mut returns = Vec::with_capacity(candles.len() - 1);
        for i in 1..candles.len() {
            let prev = candles[i - 1].close.to_f64().unwrap_or(0.0);
            let curr = candles[i].close.to_f64().unwrap_or(0.0);

            if prev != 0.0 {
                let ret = (curr - prev) / prev;
                returns.push(ret);
            }
        }
        returns
    }

    fn calculate_pearson_correlation(&self, v1: &[f64], v2: &[f64]) -> f64 {
        let len = v1.len().min(v2.len());
        if len < 2 {
            return 0.0;
        }

        let v1 = &v1[..len];
        let v2 = &v2[..len];

        let mean1 = v1.iter().sum::<f64>() / len as f64;
        let mean2 = v2.iter().sum::<f64>() / len as f64;

        let mut numer = 0.0;
        let mut denom1 = 0.0;
        let mut denom2 = 0.0;

        for i in 0..len {
            let diff1 = v1[i] - mean1;
            let diff2 = v2[i] - mean2;
            numer += diff1 * diff2;
            denom1 += diff1 * diff1;
            denom2 += diff2 * diff2;
        }

        if denom1 == 0.0 || denom2 == 0.0 {
            return 0.0;
        }

        numer / (denom1.sqrt() * denom2.sqrt())
    }
}
