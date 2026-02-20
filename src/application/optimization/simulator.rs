use crate::application::agents::analyst::{Analyst, AnalystConfig, AnalystDependencies};
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::trading::types::MarketEvent;
use crate::domain::trading::types::{Candle, Order};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::domain::performance::stats::Stats;
use crate::domain::repositories::CandleRepository;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tracing::info;

#[derive(Debug, Clone)]
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
    market_data: Arc<dyn MarketDataService>,
    execution_service: Arc<dyn ExecutionService>,
    config: AnalystConfig,
}

impl Simulator {
    /// Calculate alpha and beta using linear regression
    /// Returns (alpha, beta, correlation)
    /// Formula: strategy_return = alpha + beta * benchmark_return + error
    fn calculate_alpha_beta(
        strategy_returns: &[Decimal],
        benchmark_returns: &[Decimal],
    ) -> (f64, f64, f64) {
        let (a, b, c) = Stats::alpha_beta(strategy_returns, benchmark_returns);
        (
            a.to_f64().unwrap_or(0.0),
            b.to_f64().unwrap_or(0.0),
            c.to_f64().unwrap_or(0.0),
        )
    }
    pub fn new(
        market_data: Arc<dyn MarketDataService>,
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
        let bars = self
            .market_data
            .get_historical_bars(symbol, start, end, "1Min")
            .await
            .context("Failed to fetch historical bars")?;
        self.run_with_bars(symbol, &bars, start, end, None).await
    }

    /// Run backtest with pre-fetched bars (avoids repeated API calls when optimizing).
    /// If spy_bars is None, SPY is fetched for alpha/beta; pass Some(...) to reuse.
    pub async fn run_with_bars(
        &self,
        symbol: &str,
        bars: &[Candle],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        spy_bars: Option<Vec<Candle>>,
    ) -> Result<BacktestResult> {
        if bars.is_empty() {
            anyhow::bail!(
                "Simulator: no bars provided for {} in run_with_bars",
                symbol
            );
        }
        let bars_owned: Vec<Candle> = bars.to_vec();

        // Pre-process bars to extract daily closes
        // Map: Date (String YYYY-MM-DD) -> (Timestamp, ClosePrice)
        // We want the LAST bar of each day
        let mut daily_map: std::collections::BTreeMap<String, (i64, Decimal)> =
            std::collections::BTreeMap::new();

        for bar in bars {
            let dt = chrono::DateTime::from_timestamp(bar.timestamp, 0)
                .unwrap_or_default()
                .with_timezone(&Utc);
            let date_key = dt.format("%Y-%m-%d").to_string();
            let close = bar.close;
            daily_map.insert(date_key, (dt.timestamp_millis(), close));
        }

        // Convert to Vec sorted by date (BTreeMap ensures sort)
        let daily_closes: Vec<(i64, Decimal)> = daily_map.values().cloned().collect();

        let initial_portfolio = self.execution_service.get_portfolio().await?;
        let initial_equity = initial_portfolio.cash; // simplify: assume cash only start

        let (market_tx, market_rx) = mpsc::channel(1000);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(100);

        let sim_config = self.config.clone();

        // Use StrategyFactory to create the correct strategy for simulations
        let strategy = crate::application::strategies::StrategyFactory::create(
            sim_config.strategy_mode,
            &sim_config,
        );

        let (_analyst_cmd_tx, analyst_cmd_rx) = mpsc::channel(1);

        let mut analyst = Analyst::new(
            market_rx,
            analyst_cmd_rx,
            proposal_tx,
            sim_config,
            strategy,
            AnalystDependencies {
                execution_service: self.execution_service.clone(),
                market_service: self.market_data.clone(),
                candle_repository: Some(Arc::new(InMemoryCandleRepository::new(
                    bars.iter()
                        .map(|b| Candle {
                            symbol: symbol.to_string(),
                            open: b.open,
                            high: b.high,
                            low: b.low,
                            close: b.close,
                            volume: b.volume,
                            timestamp: b.timestamp,
                        })
                        .collect(),
                ))),
                strategy_repository: None,
                win_rate_provider: None,
                ui_candle_tx: None,
                spread_cache: Arc::new(
                    crate::application::market_data::spread_cache::SpreadCache::new(),
                ),
                connection_health_service: {
                    let health = Arc::new(crate::application::monitoring::connection_health_service::ConnectionHealthService::new());
                    health.set_market_data_status(
                        crate::application::monitoring::connection_health_service::ConnectionStatus::Online,
                        Some("Simulation Started".to_string())
                    ).await;
                    health
                },
                agent_registry: Arc::new(
                    crate::application::monitoring::agent_status::AgentStatusRegistry::new(
                        crate::infrastructure::observability::Metrics::new().unwrap(),
                    ),
                ),
            },
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
        let start_price = bars.first().map(|b| b.close).unwrap_or(Decimal::ZERO);
        let last_close = bars.last().map(|b| b.close).unwrap_or(Decimal::ZERO);

        let symbol_clone = symbol.to_string();
        let feeder_handle = tokio::spawn(async move {
            for bar in bars_owned {
                let candle = Candle {
                    symbol: symbol_clone.clone(),
                    open: bar.open,
                    high: bar.high,
                    low: bar.low,
                    close: bar.close,
                    volume: bar.volume,
                    timestamp: bar.timestamp,
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
        let max_drawdown_pct = Decimal::new(-50, 0); // -50% max loss

        while let Some(prop) = proposal_rx.recv().await {
            // Circuit Breaker: Check equity before executing
            if let Ok(portfolio) = self.execution_service.get_portfolio().await {
                let current_equity = portfolio.cash
                    + portfolio
                        .positions
                        .values()
                        .filter(|p| p.symbol == prop.symbol)
                        .map(|p| p.quantity * prop.price)
                        .sum::<Decimal>();

                let drawdown_pct = if !initial_equity.is_zero() {
                    (current_equity - initial_equity)
                        .checked_div(initial_equity)
                        .map(|r| r * Decimal::from(100))
                        .unwrap_or(Decimal::ZERO)
                } else {
                    Decimal::ZERO
                };

                if drawdown_pct < max_drawdown_pct {
                    info!(
                        "Simulator: CIRCUIT BREAKER TRIGGERED! Drawdown {:.2}% < {:.2}%. Halting trading.",
                        drawdown_pct, max_drawdown_pct
                    );
                    break;
                }
            }

            let costs = self
                .config
                .fee_model
                .calculate_cost(prop.quantity, prop.price, prop.side);
            let slippage_amount = costs.slippage_cost;
            let slippage_per_unit = if prop.quantity.is_zero() {
                Decimal::ZERO
            } else {
                slippage_amount
                    .checked_div(prop.quantity)
                    .unwrap_or(Decimal::ZERO)
            };
            let execution_price = match prop.side {
                crate::domain::trading::types::OrderSide::Buy => prop.price + slippage_per_unit,
                crate::domain::trading::types::OrderSide::Sell => prop.price - slippage_per_unit,
            };

            // Execute Immediately to update Portfolio State for next Analyst check
            let order = crate::domain::trading::types::Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: prop.symbol.clone(),
                side: prop.side,
                price: execution_price,
                quantity: prop.quantity,
                order_type: crate::domain::trading::types::OrderType::Market,
                status: crate::domain::trading::types::OrderStatus::Filled,
                timestamp: prop.timestamp,
            };

            if let Err(e) = self.execution_service.execute(order.clone()).await {
                tracing::warn!(
                    "Simulator: Failed to execute order (id={}): {}",
                    order.id,
                    e
                );
            } else {
                executed_trades.push(order);
            }
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

        let mut total_return_pct = if !initial_equity.is_zero() {
            (final_equity - initial_equity)
                .checked_div(initial_equity)
                .map(|r| r * Decimal::from(100))
                .unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        // Floor: Can't lose more than 100% of capital (prevents stock split artifacts)
        let min_return = Decimal::new(-100, 0);
        if total_return_pct < min_return {
            info!(
                "Simulator: Return {:.2}% capped to -100% (stock split or data issue)",
                total_return_pct
            );
            total_return_pct = min_return;
        }

        // Buy & Hold Return: (LastPrice - StartPrice) / StartPrice
        let buy_and_hold_return_pct = if !start_price.is_zero() {
            (last_close - start_price)
                .checked_div(start_price)
                .map(|r| r * Decimal::from(100))
                .unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        // SPY benchmark for alpha/beta: use provided bars or fetch once
        let spy_bars_resolved: Vec<Candle> = if let Some(s) = spy_bars {
            s
        } else {
            self.market_data
                .get_historical_bars("SPY", start, end, "1Day")
                .await
                .unwrap_or_default()
        };
        let (alpha, beta, benchmark_correlation) = if !spy_bars_resolved.is_empty()
            && daily_closes.len() > 1
        {
            let spy_bars_ref = &spy_bars_resolved;
            // Calculate daily returns for strategy
            let mut strategy_returns = Vec::new();
            for i in 1..daily_closes.len() {
                let prev_price = daily_closes[i - 1].1;
                let curr_price = daily_closes[i].1;
                if prev_price > Decimal::ZERO {
                    strategy_returns.push((curr_price - prev_price) / prev_price);
                }
            }

            // Build SPY daily close map
            let mut spy_daily_map: std::collections::BTreeMap<String, Decimal> =
                std::collections::BTreeMap::new();
            for bar in spy_bars_ref {
                let dt = chrono::DateTime::from_timestamp(bar.timestamp, 0)
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
                    && prev_spy > Decimal::ZERO
                {
                    benchmark_returns.push((curr_spy - prev_spy) / prev_spy);
                }
            }

            // Only calculate if we have matching returns
            if strategy_returns.len() == benchmark_returns.len() && !strategy_returns.is_empty() {
                Self::calculate_alpha_beta(&strategy_returns, &benchmark_returns)
            } else {
                (0.0, 0.0, 0.0)
            }
        } else {
            (0.0, 0.0, 0.0)
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

// Helper Repository for Simulator
struct InMemoryCandleRepository {
    candles: Mutex<Vec<Candle>>,
}

impl InMemoryCandleRepository {
    fn new(candles: Vec<Candle>) -> Self {
        Self {
            candles: Mutex::new(candles),
        }
    }
}

#[async_trait]
impl CandleRepository for InMemoryCandleRepository {
    async fn save(&self, _candle: &Candle) -> Result<()> {
        Ok(())
    }

    async fn get_range(&self, _symbol: &str, start_ts: i64, end_ts: i64) -> Result<Vec<Candle>> {
        let candles = self
            .candles
            .lock()
            .expect("InMemoryCandleRepository mutex poisoned - concurrent panic");
        Ok(candles
            .iter()
            .filter(|c| c.timestamp >= start_ts && c.timestamp <= end_ts)
            .cloned()
            .collect())
    }

    async fn get_latest_timestamp(&self, _symbol: &str) -> Result<Option<i64>> {
        let candles = self
            .candles
            .lock()
            .expect("InMemoryCandleRepository mutex poisoned - concurrent panic");
        Ok(candles.last().map(|c| c.timestamp))
    }

    async fn count_candles(&self, _symbol: &str, _start_ts: i64, _end_ts: i64) -> Result<usize> {
        Ok(0)
    }

    async fn prune(&self, _days_retention: i64) -> Result<u64> {
        Ok(0)
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
        let strategy_returns = vec![
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.02).unwrap_or_default(),
            Decimal::from_f64_retain(-0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.03).unwrap_or_default(),
        ];
        let benchmark_returns = vec![
            Decimal::from_f64_retain(0.005).unwrap_or_default(),
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(-0.005).unwrap_or_default(),
            Decimal::from_f64_retain(0.015).unwrap_or_default(),
        ];

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
        let strategy_returns = vec![
            Decimal::from_f64_retain(0.02).unwrap_or_default(),
            Decimal::from_f64_retain(0.03).unwrap_or_default(),
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.04).unwrap_or_default(),
        ];
        let benchmark_returns = vec![
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
        ];

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
        let strategy_returns = vec![
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(-0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.02).unwrap_or_default(),
            Decimal::from_f64_retain(-0.02).unwrap_or_default(),
        ];
        let benchmark_returns = vec![
            Decimal::from_f64_retain(-0.01).unwrap_or_default(),
            Decimal::from_f64_retain(0.01).unwrap_or_default(),
            Decimal::from_f64_retain(-0.02).unwrap_or_default(),
            Decimal::from_f64_retain(0.02).unwrap_or_default(),
        ];

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
