use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use crate::domain::repositories::CandleRepository;
use crate::domain::trading::types::Candle;
use rust_decimal::prelude::ToPrimitive;

pub struct CorrelationService {
    candle_repository: Arc<dyn CandleRepository>,
}

impl CorrelationService {
    pub fn new(candle_repository: Arc<dyn CandleRepository>) -> Self {
        Self { candle_repository }
    }

    /// Calculate Pearson correlation matrix for a list of symbols
    /// uses 30 days of historical data
    pub async fn calculate_correlation_matrix(&self, symbols: &[String]) -> Result<HashMap<(String, String), f64>> {
        let end_ts = chrono::Utc::now().timestamp();
        let start_ts = end_ts - (30 * 24 * 60 * 60); // 30 days
        
        let mut returns = HashMap::new();
        
        for symbol in symbols {
            let candles = self.candle_repository.get_range(symbol, start_ts, end_ts).await
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
                
                let corr = self.calculate_pearson_correlation(&returns[s1], &returns[s2]);
                matrix.insert((s1.clone(), s2.clone()), corr);
                if s1 != s2 {
                    matrix.insert((s2.clone(), s1.clone()), corr);
                }
            }
        }

        Ok(matrix)
    }

    fn calculate_returns(&self, candles: &[Candle]) -> Vec<f64> {
        if candles.len() < 2 {
            return Vec::new();
        }

        let mut returns = Vec::with_capacity(candles.len() - 1);
        for i in 1..candles.len() {
            let prev = candles[i-1].close.to_f64().unwrap_or(0.0);
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
