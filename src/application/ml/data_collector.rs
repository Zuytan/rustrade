use crate::domain::trading::types::FeatureSet;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing::error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingDataPoint {
    pub timestamp: i64,
    pub symbol: String,
    // Flattened features
    pub rsi: f64,
    pub macd: f64,
    pub macd_signal: f64,
    pub macd_hist: f64,
    pub bb_width: f64,
    pub bb_position: f64, // Position within BB (0..1)
    pub atr_pct: f64,     // ATR as % of price
    pub hurst: f64,
    pub skewness: f64,
    pub momentum_norm: f64,
    pub volatility: f64,
    // Labels (Future Returns)
    pub return_1m: Option<f64>,
    pub return_5m: Option<f64>,
    pub return_15m: Option<f64>,
}

pub struct DataCollector {
    buffer: VecDeque<PendingDataPoint>,
    output_path: PathBuf,
    _history_size: usize,
}

struct PendingDataPoint {
    timestamp: i64,
    _symbol: String,
    price: f64,
    features: TrainingDataPoint,
}

impl DataCollector {
    pub fn new(output_path: PathBuf) -> Self {
        Self {
            buffer: VecDeque::new(),
            output_path,
            _history_size: 20, // Keep enough history for 15m returns
        }
    }

    pub fn process_update(
        &mut self,
        symbol: &str,
        price: f64,
        timestamp: i64,
        feature_set: &FeatureSet,
    ) {
        let features = self.extract_features(symbol, price, timestamp, feature_set);

        // Add new point to buffer
        self.buffer.push_back(PendingDataPoint {
            timestamp,
            _symbol: symbol.to_string(),
            price,
            features,
        });

        // Update labels for older points and flush
        self.update_labels_and_flush(price, timestamp);
    }

    fn extract_features(
        &self,
        symbol: &str,
        price: f64,
        timestamp: i64,
        fs: &FeatureSet,
    ) -> TrainingDataPoint {
        let bb_width = if let (Some(u), Some(l), Some(m)) = (fs.bb_upper, fs.bb_lower, fs.bb_middle)
        {
            if m > 0.0 { (u - l) / m } else { 0.0 }
        } else {
            0.0
        };

        let bb_position = if let (Some(u), Some(l)) = (fs.bb_upper, fs.bb_lower) {
            if u - l > 0.0 {
                (price - l) / (u - l)
            } else {
                0.5
            }
        } else {
            0.5
        };

        let atr_pct = if let Some(atr) = fs.atr {
            if price > 0.0 { atr / price } else { 0.0 }
        } else {
            0.0
        };

        TrainingDataPoint {
            timestamp,
            symbol: symbol.to_string(),
            rsi: fs.rsi.unwrap_or(50.0),
            macd: fs.macd_line.unwrap_or(0.0),
            macd_signal: fs.macd_signal.unwrap_or(0.0),
            macd_hist: fs.macd_hist.unwrap_or(0.0),
            bb_width,
            bb_position,
            atr_pct,
            hurst: fs.hurst_exponent.unwrap_or(0.5),
            skewness: fs.skewness.unwrap_or(0.0),
            momentum_norm: fs.momentum_normalized.unwrap_or(0.0),
            volatility: fs.realized_volatility.unwrap_or(0.0),
            return_1m: None,
            return_5m: None,
            return_15m: None,
        }
    }

    fn update_labels_and_flush(&mut self, current_price: f64, current_ts: i64) {
        // Iterate through buffer to populate labels for past points
        // We can only calculate returns if enough time has passed

        // Strategy: Iterate mutable, identify ready points, calculate, extract ready points to flush
        // VecDeque doesn't support easy drain_filter yet stable

        // 1. Calculate labels
        for point in self.buffer.iter_mut() {
            let elapsed = current_ts - point.timestamp;

            // 1 minute return (assuming ~60s)
            if elapsed >= 60 && point.features.return_1m.is_none() {
                point.features.return_1m = Some((current_price - point.price) / point.price);
            }

            // 5 minute return
            if elapsed >= 300 && point.features.return_5m.is_none() {
                point.features.return_5m = Some((current_price - point.price) / point.price);
            }

            // 15 minute return
            if elapsed >= 900 && point.features.return_15m.is_none() {
                point.features.return_15m = Some((current_price - point.price) / point.price);
            }
        }

        // 2. Flush fully labeled points (older than 15m)
        while let Some(front) = self.buffer.front() {
            if current_ts - front.timestamp > 900 {
                if let Some(mut point) = self.buffer.pop_front() {
                    // One last check on returns
                    if point.features.return_15m.is_none() {
                        point.features.return_15m =
                            Some((current_price - point.price) / point.price);
                    }
                    self.write_to_csv(&point.features);
                }
            } else {
                break; // Buffer is sorted by time, so we can stop
            }
        }
    }

    fn write_to_csv(&self, data: &TrainingDataPoint) {
        let file_exists = self.output_path.exists();

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.output_path);

        match file {
            Ok(f) => {
                let mut wtr = csv::WriterBuilder::new()
                    .has_headers(!file_exists)
                    .from_writer(f);

                if let Err(e) = wtr.serialize(data) {
                    error!("Failed to serialize training data: {}", e);
                }
                if let Err(e) = wtr.flush() {
                    error!("Failed to flush CSV writer: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to open training data file: {}", e);
            }
        }
    }
}
