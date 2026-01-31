use crate::domain::ml::feature_registry::{self, FEATURE_NAMES};
use crate::domain::trading::types::FeatureSet;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing::error;

pub struct DataCollector {
    buffer: VecDeque<PendingDataPoint>,
    output_path: PathBuf,
    _history_size: usize,
}

struct PendingDataPoint {
    timestamp: i64,
    symbol: String,
    price: f64,
    // Store raw features vector directly from registry to ensure consistency
    features: Vec<f64>,
    return_1m: Option<f64>,
    return_5m: Option<f64>,
    return_15m: Option<f64>,
}

impl DataCollector {
    pub fn new(output_path: PathBuf) -> Self {
        Self {
            buffer: VecDeque::new(),
            output_path,
            _history_size: 20,
        }
    }

    pub fn process_update(
        &mut self,
        symbol: &str,
        price: f64,
        timestamp: i64,
        feature_set: &FeatureSet,
    ) {
        // Use Registry to extract features in guaranteed order
        let features = feature_registry::features_to_f64_vector(feature_set);

        // Add new point to buffer
        self.buffer.push_back(PendingDataPoint {
            timestamp,
            symbol: symbol.to_string(),
            price,
            features,
            return_1m: None,
            return_5m: None,
            return_15m: None,
        });

        // Update labels for older points and flush
        self.update_labels_and_flush(price, timestamp);
    }

    fn update_labels_and_flush(&mut self, current_price: f64, current_ts: i64) {
        // 1. Calculate labels
        for point in self.buffer.iter_mut() {
            let elapsed = current_ts - point.timestamp;

            if elapsed >= 60 && point.return_1m.is_none() {
                point.return_1m = Some((current_price - point.price) / point.price);
            }

            if elapsed >= 300 && point.return_5m.is_none() {
                point.return_5m = Some((current_price - point.price) / point.price);
            }

            if elapsed >= 900 && point.return_15m.is_none() {
                point.return_15m = Some((current_price - point.price) / point.price);
            }
        }

        // 2. Flush fully labeled points (older than 15m)
        while let Some(front) = self.buffer.front() {
            if current_ts - front.timestamp > 900 {
                if let Some(mut point) = self.buffer.pop_front() {
                    // One last check on returns
                    if point.return_15m.is_none() {
                        point.return_15m = Some((current_price - point.price) / point.price);
                    }
                    self.write_to_csv(&point);
                }
            } else {
                break; // Buffer is sorted by time
            }
        }
    }

    fn write_to_csv(&self, data: &PendingDataPoint) {
        let file_exists = self.output_path.exists();

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.output_path);

        match file {
            Ok(f) => {
                let mut wtr = csv::WriterBuilder::new()
                    .has_headers(false) // We maintain headers manually to ensure ordering
                    .from_writer(f);

                // Write Header if new file
                if !file_exists {
                    let mut headers = vec!["timestamp".to_string(), "symbol".to_string()];
                    headers.extend(FEATURE_NAMES.iter().map(|s| s.to_string()));
                    headers.push("return_1m".to_string());
                    headers.push("return_5m".to_string());
                    headers.push("return_15m".to_string());

                    if let Err(e) = wtr.write_record(&headers) {
                        error!("Failed to write CSV headers: {}", e);
                    }
                }

                // Write Data Row
                // We need to manually construct the record strings because we have mixed types
                // efficient enough for logging
                let mut record = Vec::with_capacity(FEATURE_NAMES.len() + 5);
                record.push(data.timestamp.to_string());
                record.push(data.symbol.clone());

                // Add features
                for val in &data.features {
                    record.push(val.to_string());
                }

                // Add Labels
                record.push(data.return_1m.unwrap_or(0.0).to_string());
                record.push(data.return_5m.unwrap_or(0.0).to_string());
                record.push(data.return_15m.unwrap_or(0.0).to_string());

                if let Err(e) = wtr.write_record(record) {
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
