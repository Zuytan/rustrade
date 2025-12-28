use crate::application::analyst::{Analyst, AnalystConfig};
use crate::application::strategies::{AdvancedTripleFilterStrategy, TradingStrategy};
use crate::domain::ports::ExecutionService;
use crate::domain::types::MarketEvent;
use crate::infrastructure::alpaca::AlpacaMarketDataService;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::domain::types::{Candle, Order};

pub struct BacktestResult {
    pub trades: Vec<Order>,
    pub initial_equity: Decimal,
    pub final_equity: Decimal,
    pub total_return_pct: Decimal,
    pub buy_and_hold_return_pct: Decimal,
    pub daily_closes: Vec<(i64, Decimal)>, // (Timestamp seconds, Close Price)
    pub alpha: f64,
    pub beta: f64,
    pub benchmark_correlation: f64,
}

pub struct Simulator {
    market_data: Arc<AlpacaMarketDataService>,
    execution_service: Arc<dyn ExecutionService>,
    config: AnalystConfig,
}

impl Simulator {
    /// Calculate alpha and beta using linear regression
    /// Returns (alpha, beta, correlation)
    /// Formula: strategy_return = alpha + beta * benchmark_return + error
    fn calculate_alpha_beta(
        strategy_returns: &[f64],
        benchmark_returns: &[f64],
    ) -> (f64, f64, f64) {
        if strategy_returns.len() != benchmark_returns.len() || strategy_returns.is_empty() {
            return (0.0, 0.0, 0.0);
        }

        let n = strategy_returns.len() as f64;

        // Calculate means
        let mean_strategy: f64 = strategy_returns.iter().sum::<f64>() / n;
        let mean_benchmark: f64 = benchmark_returns.iter().sum::<f64>() / n;

        // Calculate covariance and variance
        let mut covariance = 0.0;
        let mut variance_benchmark = 0.0;
        let mut variance_strategy = 0.0;

        for i in 0..strategy_returns.len() {
            let diff_strategy = strategy_returns[i] - mean_strategy;
            let diff_benchmark = benchmark_returns[i] - mean_benchmark;
            covariance += diff_strategy * diff_benchmark;
            variance_benchmark += diff_benchmark * diff_benchmark;
            variance_strategy += diff_strategy * diff_strategy;
        }

        covariance /= n;
        variance_benchmark /= n;
        variance_strategy /= n;

        // Beta = Cov(strategy, benchmark) / Var(benchmark)
        let beta = if variance_benchmark > 0.0 {
            covariance / variance_benchmark
        } else {
            0.0
        };

        // Alpha = mean_strategy - beta * mean_benchmark
        let alpha = mean_strategy - beta * mean_benchmark;

        // Correlation = Cov / (StdDev_strategy * StdDev_benchmark)
        let correlation = if variance_benchmark > 0.0 && variance_strategy > 0.0 {
            covariance / (variance_benchmark.sqrt() * variance_strategy.sqrt())
        } else {
            0.0
        };

        (alpha, beta, correlation)
    }
    pub fn new(
        market_data: Arc<AlpacaMarketDataService>,
        execution_service: Arc<dyn ExecutionService>,
        config: AnalystConfig,
    ) -> Self {
        Self {
            market_data,
            execution_service,
            config,
        }
    }

    pub async fn run(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<BacktestResult> {
        info!("Simulator: Fetching historical bars for {}...", symbol);
        let bars = self
            .market_data
            .get_historical_bars(symbol, start, end, "1Min")
            .await
            .context("Failed to fetch historical bars")?;

        info!(
            "Simulator: Fetched {} bars. Starting simulation...",
            bars.len()
        );

        // Pre-process bars to extract daily closes
        // Map: Date (String YYYY-MM-DD) -> (Timestamp, ClosePrice)
        // We want the LAST bar of each day
        let mut daily_map: std::collections::BTreeMap<String, (i64, Decimal)> =
            std::collections::BTreeMap::new();

        for bar in &bars {
            let dt = chrono::DateTime::parse_from_rfc3339(&bar.timestamp)
                .unwrap_or_default()
                .with_timezone(&Utc);
            let date_key = dt.format("%Y-%m-%d").to_string();
            let close = Decimal::from_f64_retain(bar.close).unwrap_or(Decimal::ZERO);
            daily_map.insert(date_key, (dt.timestamp_millis(), close));
        }

        // Convert to Vec sorted by date (BTreeMap ensures sort)
        let daily_closes: Vec<(i64, Decimal)> = daily_map.values().cloned().collect();

        let initial_portfolio = self.execution_service.get_portfolio().await?;
        let initial_equity = initial_portfolio.cash; // simplify: assume cash only start

        let (market_tx, market_rx) = mpsc::channel(1000);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(100);

        let sim_config = AnalystConfig {
            fast_sma_period: self.config.fast_sma_period,
            slow_sma_period: self.config.slow_sma_period,
            max_positions: self.config.max_positions,
            trade_quantity: self.config.trade_quantity,
            sma_threshold: self.config.sma_threshold,
            order_cooldown_seconds: self.config.order_cooldown_seconds,
            risk_per_trade_percent: self.config.risk_per_trade_percent,
            strategy_mode: self.config.strategy_mode,
            trend_sma_period: self.config.trend_sma_period,
            rsi_period: self.config.rsi_period,
            macd_fast_period: self.config.macd_fast_period,
            macd_slow_period: self.config.macd_slow_period,
            macd_signal_period: self.config.macd_signal_period,
            trend_divergence_threshold: self.config.trend_divergence_threshold,
            trailing_stop_atr_multiplier: self.config.trailing_stop_atr_multiplier,
            atr_period: self.config.atr_period,
            rsi_threshold: self.config.rsi_threshold,
            trend_riding_exit_buffer_pct: self.config.trend_riding_exit_buffer_pct,
            mean_reversion_rsi_exit: self.config.mean_reversion_rsi_exit,
            mean_reversion_bb_period: self.config.mean_reversion_bb_period,
            slippage_pct: self.config.slippage_pct,
            commission_per_share: self.config.commission_per_share, // Added
            max_position_size_pct: self.config.max_position_size_pct,
        };

        // Use Advanced strategy for simulations
        let strategy: Arc<dyn TradingStrategy> = Arc::new(AdvancedTripleFilterStrategy::new(
            sim_config.fast_sma_period,
            sim_config.slow_sma_period,
            sim_config.sma_threshold,
            sim_config.trend_sma_period,
            sim_config.rsi_threshold,
        ));

        let mut analyst = Analyst::new(
            market_rx,
            proposal_tx,
            self.execution_service.clone(),
            strategy,
            sim_config,
            None,
            None,
        );

        let analyst_handle = tokio::spawn(async move {
            analyst.run().await;
        });

        // Loop: Feed Market -> Wait a bit -> Process Proposals
        // This is tricky because Analyst is async and decoupled.
        // For a true backtest, we must process events sequentially.
        // BUT, our Analyst is designed for streaming.
        // So we can feed all bars?
        // If we feed all bars, Analyst will generate proposals with timestamps.
        // We can just collect them all and "simulate" execution afterwards?
        // NO, because Analyst decides quantity based on Portfolio state (Risk Management).
        // So we MUST execute trades as they come to update Portfolio.

        // Solution: Run Feeder in background, but slower?
        // Or better: Analyst processes events one by one. But it's decoupled via channel.
        // If we flood the channel, Analyst might process faster than we read proposals.
        // But for risk management, the Analyst READS the portfolio.
        // If we haven't executed the previous proposal, Analyst sees old portfolio.
        // So we have a race condition in simulation vs real-time.

        // Correct approach for Simulator with State:
        // Feeder sends 1 bar.
        // We wait for checking proposals.
        // But Analyst might not emit proposal for that bar.
        // How do we know "Analyst finished processing bar X"? We don't.
        // Compromise for this Architectuure:
        // 1. Config Analyst order_cooldown enough that we don't have overlapping trades in short time.
        // 2. Just run it. Analyst will be slightly behind Feeder.
        //    It will emit a Proposal. We read it, execute it (update MockPortfolio).
        //    Next time Analyst checks Portfolio, it sees updated one.
        //    Ideally, channel size = 1? But Analyst reads batch from market.

        // Let's stick to the streaming approach.
        // We spawn feeder.
        // We process proposals as they arrive.
        // Execute them immediately.

        // Pre-calculate prices for metrics before moving bars
        let start_price = bars
            .first()
            .map(|b| Decimal::from_f64_retain(b.close).unwrap_or(Decimal::ZERO))
            .unwrap_or(Decimal::ZERO);
        let last_close = bars
            .last()
            .map(|b| Decimal::from_f64_retain(b.close).unwrap_or(Decimal::ZERO))
            .unwrap_or(Decimal::ZERO);

        let symbol_clone = symbol.to_string();
        let feeder_handle = tokio::spawn(async move {
            for bar in bars {
                let timestamp = chrono::DateTime::parse_from_rfc3339(&bar.timestamp)
                    .unwrap_or_default()
                    .timestamp(); // Seconds

                let candle = Candle {
                    symbol: symbol_clone.clone(),
                    open: Decimal::from_f64_retain(bar.open).unwrap_or(Decimal::ZERO),
                    high: Decimal::from_f64_retain(bar.high).unwrap_or(Decimal::ZERO),
                    low: Decimal::from_f64_retain(bar.low).unwrap_or(Decimal::ZERO),
                    close: Decimal::from_f64_retain(bar.close).unwrap_or(Decimal::ZERO),
                    volume: bar.volume,
                    timestamp,
                };

                let event = MarketEvent::Candle(candle);

                // artificial delay to allow Analyst to catch up / generate proposal before we feed next 100 bars?
                // tokio::time::sleep(std::time::Duration::from_micros(10)).await;

                if market_tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        let mut executed_trades = Vec::new();

        while let Some(prop) = proposal_rx.recv().await {
            let slippage =
                Decimal::from_f64_retain(self.config.slippage_pct).unwrap_or(Decimal::ZERO);
            let execution_price = match prop.side {
                crate::domain::types::OrderSide::Buy => prop.price * (Decimal::ONE + slippage),
                crate::domain::types::OrderSide::Sell => prop.price * (Decimal::ONE - slippage),
            };

            // Execute Immediately to update Portfolio State for next Analyst check
            let order = crate::domain::types::Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: prop.symbol.clone(),
                side: prop.side,
                price: execution_price,
                quantity: prop.quantity,
                order_type: crate::domain::types::OrderType::Market,
                timestamp: prop.timestamp,
            };

            self.execution_service.execute(order.clone()).await?;
            executed_trades.push(order);
        }

        // Wait for components to finish
        feeder_handle.await?;
        analyst_handle.await?;

        // Calculate Final Metrics
        let final_portfolio = self.execution_service.get_portfolio().await?;

        let mut final_equity = final_portfolio.cash;

        // Recalculate Final Equity with positions valued at `last_close`
        for pos in final_portfolio.positions.values() {
            if pos.symbol == symbol {
                final_equity += pos.quantity * last_close;
            }
        }

        let total_return_pct = if !initial_equity.is_zero() {
            (final_equity - initial_equity) / initial_equity * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Buy & Hold Return: (LastPrice - StartPrice) / StartPrice
        let buy_and_hold_return_pct = if !start_price.is_zero() {
            (last_close - start_price) / start_price * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Fetch SPY (S&P 500) benchmark data for alpha/beta calculation
        info!("Simulator: Fetching SPY benchmark data...");
        let (alpha, beta, benchmark_correlation) = match self
            .market_data
            .get_historical_bars("SPY", start, end, "1Day")
            .await
        {
            Ok(spy_bars) if !spy_bars.is_empty() && daily_closes.len() > 1 => {
                // Calculate daily returns for strategy
                let mut strategy_returns = Vec::new();
                for i in 1..daily_closes.len() {
                    let prev_price = daily_closes[i - 1].1.to_f64().unwrap_or(1.0);
                    let curr_price = daily_closes[i].1.to_f64().unwrap_or(1.0);
                    if prev_price > 0.0 {
                        strategy_returns.push((curr_price - prev_price) / prev_price);
                    }
                }

                // Build SPY daily close map
                let mut spy_daily_map: std::collections::BTreeMap<String, f64> =
                    std::collections::BTreeMap::new();
                for bar in &spy_bars {
                    let dt = chrono::DateTime::parse_from_rfc3339(&bar.timestamp)
                        .unwrap_or_default()
                        .with_timezone(&Utc);
                    let date_key = dt.format("%Y-%m-%d").to_string();
                    spy_daily_map.insert(date_key, bar.close);
                }

                // Calculate SPY daily returns aligned with strategy dates
                let mut benchmark_returns = Vec::new();
                for i in 1..daily_closes.len() {
                    let prev_ts = daily_closes[i - 1].0;
                    let curr_ts = daily_closes[i].0;
                    let prev_dt = chrono::DateTime::from_timestamp(prev_ts / 1000, 0)
                        .unwrap_or_default()
                        .format("%Y-%m-%d")
                        .to_string();
                    let curr_dt = chrono::DateTime::from_timestamp(curr_ts / 1000, 0)
                        .unwrap_or_default()
                        .format("%Y-%m-%d")
                        .to_string();

                    if let (Some(&prev_spy), Some(&curr_spy)) =
                        (spy_daily_map.get(&prev_dt), spy_daily_map.get(&curr_dt))
                    {
                        if prev_spy > 0.0 {
                            benchmark_returns.push((curr_spy - prev_spy) / prev_spy);
                        }
                    }
                }

                // Only calculate if we have matching returns
                if strategy_returns.len() == benchmark_returns.len() && !strategy_returns.is_empty()
                {
                    Self::calculate_alpha_beta(&strategy_returns, &benchmark_returns)
                } else {
                    info!(
                        "Simulator: Mismatched return lengths (strategy: {}, benchmark: {})",
                        strategy_returns.len(),
                        benchmark_returns.len()
                    );
                    (0.0, 0.0, 0.0)
                }
            }
            Ok(_) => {
                info!("Simulator: Insufficient SPY data for alpha/beta calculation");
                (0.0, 0.0, 0.0)
            }
            Err(e) => {
                info!("Simulator: Failed to fetch SPY data: {}", e);
                (0.0, 0.0, 0.0)
            }
        };

        Ok(BacktestResult {
            trades: executed_trades,
            initial_equity,
            final_equity,
            total_return_pct,
            buy_and_hold_return_pct,
            daily_closes,
            alpha,
            beta,
            benchmark_correlation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alpha_beta_calculation() {
        // Strategy returns: 1%, 2%, -1%, 3%
        // Benchmark returns: 0.5%, 1%, -0.5%, 1.5%
        // Expected: Beta ~= 2.0 (strategy is twice as volatile as benchmark)
        let strategy_returns = vec![0.01, 0.02, -0.01, 0.03];
        let benchmark_returns = vec![0.005, 0.01, -0.005, 0.015];

        let (alpha, beta, correlation) =
            Simulator::calculate_alpha_beta(&strategy_returns, &benchmark_returns);

        // Beta should be around 2.0 (strategy moves 2x benchmark)
        assert!(
            beta > 1.5 && beta < 2.5,
            "Beta should be ~2.0, got {}",
            beta
        );

        // Alpha should be close to 0 (strategy follows benchmark proportionally)
        assert!(alpha.abs() < 0.01, "Alpha should be near 0, got {}", alpha);

        // Correlation should be positive and high
        assert!(
            correlation > 0.8,
            "Correlation should be high, got {}",
            correlation
        );
    }

    #[test]
    fn test_alpha_beta_with_excess_return() {
        // Strategy consistently beats benchmark
        let strategy_returns = vec![0.02, 0.03, 0.01, 0.04];
        let benchmark_returns = vec![0.01, 0.01, 0.01, 0.01];

        let (alpha, _beta, _correlation) =
            Simulator::calculate_alpha_beta(&strategy_returns, &benchmark_returns);

        // Positive alpha (strategy outperforms)
        assert!(
            alpha > 0.0,
            "Alpha should be positive for outperformance, got {}",
            alpha
        );
    }

    #[test]
    fn test_alpha_beta_negative_correlation() {
        // Strategy moves opposite to benchmark
        let strategy_returns = vec![0.01, -0.01, 0.02, -0.02];
        let benchmark_returns = vec![-0.01, 0.01, -0.02, 0.02];

        let (_alpha, _beta, correlation) =
            Simulator::calculate_alpha_beta(&strategy_returns, &benchmark_returns);

        // Negative correlation
        assert!(
            correlation < 0.0,
            "Correlation should be negative, got {}",
            correlation
        );
    }

    #[test]
    fn test_alpha_beta_empty_returns() {
        let (alpha, beta, correlation) = Simulator::calculate_alpha_beta(&[], &[]);

        assert_eq!(alpha, 0.0);
        assert_eq!(beta, 0.0);
        assert_eq!(correlation, 0.0);
    }
}
