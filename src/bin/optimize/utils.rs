use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rustrade::config::StrategyMode;
use rustrade::domain::risk::optimal_parameters::AssetType;
use rustrade::domain::risk::risk_appetite::RiskProfile;

/// Default symbol for run when asset is crypto and user kept stock default.
pub fn resolve_run_symbol(symbol: &str, is_crypto: bool) -> String {
    if is_crypto && (symbol == "TSLA" || symbol == "AAPL") {
        "BTC/USD".to_string()
    } else {
        symbol.to_string()
    }
}

/// Default symbols for batch when asset is crypto.
pub fn resolve_batch_symbols(symbols: &str, is_crypto: bool) -> Vec<String> {
    if is_crypto && symbols.contains("TSLA") {
        vec!["BTC/USD".to_string(), "ETH/USD".to_string()]
    } else {
        symbols.split(',').map(|s| s.trim().to_string()).collect()
    }
}

/// Default symbol for discover-optimal when asset is crypto and user kept stock default.
pub fn resolve_discover_symbol(symbol: &str, asset: AssetType) -> String {
    if asset == AssetType::Crypto && (symbol == "AAPL" || symbol == "TSLA") {
        "BTC/USD".to_string()
    } else {
        symbol.to_string()
    }
}

/// Session times: stock 14:30-21:00, crypto 00:00-23:59 (24/7).
pub fn resolve_session_times(
    start: Option<&str>,
    end: Option<&str>,
    is_crypto: bool,
) -> (String, String) {
    let (s, e) = match (start, end) {
        (Some(s), Some(e)) => (s.to_string(), e.to_string()),
        _ if is_crypto => ("00:00:00".to_string(), "23:59:59".to_string()),
        _ => ("14:30:00".to_string(), "21:00:00".to_string()),
    };
    (s, e)
}

/// Parses start and end date strings into DateTime<Utc>.
pub fn parse_date_range(
    start: &str,
    end: &str,
    start_time: &str,
    end_time: &str,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .context(format!("Invalid start date format: {}", start))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .context(format!("Invalid end date format: {}", end))?;

    let start_time_parsed = chrono::NaiveTime::parse_from_str(start_time, "%H:%M:%S")
        .context(format!("Invalid start time format: {}", start_time))?;
    let end_time_parsed = chrono::NaiveTime::parse_from_str(end_time, "%H:%M:%S")
        .context(format!("Invalid end time format: {}", end_time))?;

    let start_dt = Utc
        .from_local_datetime(&start_date.and_time(start_time_parsed))
        .single()
        .context("Failed to create start datetime")?;
    let end_dt = Utc
        .from_local_datetime(&end_date.and_time(end_time_parsed))
        .single()
        .context("Failed to create end datetime")?;

    Ok((start_dt, end_dt))
}

/// Loads a parameter grid from a TOML file.
pub fn load_grid_from_toml(
    path: &str,
) -> Result<rustrade::application::optimization::optimizer::ParameterGrid> {
    let content = std::fs::read_to_string(path)
        .context(format!("Failed to read grid config file: {}", path))?;
    let grid: rustrade::application::optimization::optimizer::ParameterGrid =
        toml::from_str(&content).context(format!("Failed to parse grid config TOML: {}", path))?;
    Ok(grid)
}

/// Returns the optimal strategy for each risk profile based on benchmark analysis.
pub fn get_strategy_for_profile(profile: RiskProfile) -> StrategyMode {
    match profile {
        RiskProfile::Conservative => StrategyMode::ZScoreMR,
        RiskProfile::Balanced => StrategyMode::RegimeAdaptive,
        RiskProfile::Aggressive => StrategyMode::SMC,
    }
}
