use crate::infrastructure::alpaca::common::AlpacaBar;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct Mover {
    pub symbol: String,
}

pub fn parse_movers(json: Value) -> Result<Vec<Mover>> {
    let movers: Vec<Mover> = if let Some(gainers) = json.get("gainers") {
        if gainers.is_null() {
            vec![]
        } else {
            serde_json::from_value(gainers.clone()).context("Failed to parse gainers array")?
        }
    } else if let Some(movers_array) = json.as_array() {
        serde_json::from_value(Value::Array(movers_array.clone()))
            .context("Failed to parse movers array")?
    } else {
        vec![]
    };

    Ok(movers)
}

#[derive(Debug, Deserialize)]
pub struct SnapshotTrade {
    #[serde(rename = "p")]
    pub price: f64,
}

#[derive(Debug, Deserialize)]
pub struct SnapshotDay {
    #[serde(rename = "v")]
    pub volume: f64,
}

#[derive(Debug, Deserialize)]
pub struct Snapshot {
    #[serde(rename = "latestTrade")]
    pub latest_trade: Option<SnapshotTrade>,
    #[serde(rename = "dailyBar")]
    pub daily_bar: Option<SnapshotDay>,
    #[serde(rename = "prevDailyBar")]
    pub prev_daily_bar: Option<SnapshotDay>,
}

pub fn parse_snapshots(json: Value) -> Result<std::collections::HashMap<String, Snapshot>> {
    serde_json::from_value(json).context("Failed to parse snapshots response")
}

#[derive(Debug, Deserialize)]
pub struct CryptoBarsResponse {
    pub bars: std::collections::HashMap<String, Vec<AlpacaBar>>,
}

pub fn parse_crypto_bars(json: Value) -> Result<std::collections::HashMap<String, Vec<AlpacaBar>>> {
    let response: CryptoBarsResponse =
        serde_json::from_value(json).context("Failed to parse crypto bars response")?;
    Ok(response.bars)
}
