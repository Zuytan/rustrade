use crate::application::agents::analyst::{Analyst, AnalystConfig, AnalystDependencies};
use crate::domain::ports::{ExecutionService, MarketDataService};
use crate::domain::trading::types::MarketEvent;
use crate::domain::trading::types::{Candle, Order, OrderSide, Trade};
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
        timeframe: &str,
    ) -> Result<BacktestResult> {
        let bars = self
            .market_data
            .get_historical_bars(symbol, start, end, timeframe)
            .await
            .context("Failed to fetch historical bars")?;
        self.run_with_bars(symbol, &bars, start, end, None).await
    }

    pub async fn run_multi_asset(
        &self,
        symbols: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
    ) -> Result<BacktestResult> {
        if symbols.is_empty() {
            anyhow::bail!("Simulator: no symbols provided for run_multi_asset");
        }

        // Fetch historical bars for all symbols
        let mut futures = Vec::new();
        for symbol in symbols {
            let market = self.market_data.clone();
            let symbol_clone = symbol.clone();
            let tf = timeframe.to_string();
            futures.push(tokio::spawn(async move {
                market
                    .get_historical_bars(&symbol_clone, start, end, &tf)
                    .await
            }));
        }

        let results = futures::future::join_all(futures).await;
        let mut all_candles = Vec::new();
        for (idx, res) in results.into_iter().enumerate() {
            let symbol = &symbols[idx];
            let bars = res
                .context(format!("Failed to join thread for {}", symbol))?
                .context(format!("Failed to fetch historical bars for {}", symbol))?;
            all_candles.extend(bars);
        }

        if all_candles.is_empty() {
            anyhow::bail!("Simulator: no bars fetched for any symbol");
        }

        // Sort chronologically by timestamp
        all_candles.sort_by_key(|c| c.timestamp);

        self.run_with_multi_bars(symbols, &all_candles, start, end, None)
            .await
    }

    pub async fn run_with_multi_bars(
        &self,
        symbols: &[String],
        bars: &[Candle],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        spy_bars: Option<Vec<Candle>>,
    ) -> Result<BacktestResult> {
        if bars.is_empty() {
            anyhow::bail!("Simulator: no bars provided in run_with_multi_bars");
        }
        let bars_owned = bars.to_vec();

        // 1. Pre-process daily prices for each symbol
        let mut daily_prices: std::collections::HashMap<String, Vec<(i64, Decimal)>> =
            std::collections::HashMap::new();

        let mut grouped_bars: std::collections::HashMap<String, Vec<Candle>> =
            std::collections::HashMap::new();
        for bar in bars {
            grouped_bars
                .entry(bar.symbol.clone())
                .or_default()
                .push(bar.clone());
        }

        for (symbol, symbol_bars) in &grouped_bars {
            let mut daily_map: std::collections::BTreeMap<String, (i64, Decimal)> =
                std::collections::BTreeMap::new();
            for bar in symbol_bars {
                let dt = chrono::DateTime::from_timestamp(bar.timestamp, 0)
                    .unwrap_or_default()
                    .with_timezone(&Utc);
                let date_key = dt.format("%Y-%m-%d").to_string();
                daily_map.insert(date_key, (bar.timestamp, bar.close));
            }
            let closes: Vec<(i64, Decimal)> = daily_map.values().cloned().collect();
            daily_prices.insert(symbol.clone(), closes);
        }

        let initial_portfolio = self.execution_service.get_portfolio().await?;
        let initial_equity = initial_portfolio.cash;

        let (market_tx, market_rx) = mpsc::channel(1000);
        let (proposal_tx, mut proposal_rx) = mpsc::channel(100);

        let sim_config = self.config.clone();

        let strategy = crate::application::strategies::StrategyFactory::create(
            sim_config.strategy.strategy_mode,
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
                    bars_owned.clone(),
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

        // Spawn Feeder
        let feeder_handle = tokio::spawn(async move {
            for bar in bars_owned {
                let event = MarketEvent::Candle(bar);
                if market_tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        let mut executed_trades = Vec::new();
        let max_drawdown_pct = Decimal::new(-50, 0); // -50% max loss

        while let Some(prop) = proposal_rx.recv().await {
            // Circuit Breaker
            if let Ok(portfolio) = self.execution_service.get_portfolio().await {
                // For multi-asset, we calculate current equity across all active positions
                let mut current_equity = portfolio.cash;
                for pos in portfolio.positions.values() {
                    let last_price = pos.average_price; // Fallback to average_price
                    current_equity += pos.quantity * last_price;
                }

                let drawdown_pct = if !initial_equity.is_zero() {
                    (current_equity - initial_equity)
                        .checked_div(initial_equity)
                        .map(|r| r * Decimal::from(100))
                        .unwrap_or(Decimal::ZERO)
                } else {
                    Decimal::ZERO
                };

                if drawdown_pct < max_drawdown_pct {
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

            let order = crate::domain::trading::types::Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: prop.symbol.clone(),
                side: prop.side,
                price: execution_price,
                quantity: prop.quantity,
                order_type: crate::domain::trading::types::OrderType::Market,
                status: crate::domain::trading::types::OrderStatus::Filled,
                timestamp: prop.timestamp,
                correlation_id: prop.correlation_id.clone(),
                stop_loss: prop.stop_loss,
            };

            if let Err(e) = self.execution_service.execute(&order).await {
                tracing::warn!(
                    "Simulator: Failed to execute order (id={}): {}",
                    order.id,
                    e
                );
            } else {
                executed_trades.push(order);
            }
        }

        feeder_handle.await?;
        analyst_handle.await?;

        // Calculate Final Portfolio Equity
        let final_portfolio = self.execution_service.get_portfolio().await?;
        let mut final_equity = final_portfolio.cash;
        for pos in final_portfolio.positions.values() {
            // Find latest close for this symbol
            let last_close = grouped_bars
                .get(&pos.symbol)
                .and_then(|v| v.last())
                .map(|b| b.close)
                .unwrap_or(pos.average_price);
            final_equity += pos.quantity * last_close;
        }

        let mut total_return_pct = if !initial_equity.is_zero() {
            (final_equity - initial_equity)
                .checked_div(initial_equity)
                .map(|r| r * Decimal::from(100))
                .unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        let min_return = Decimal::new(-100, 0);
        if total_return_pct < min_return {
            total_return_pct = min_return;
        }

        // Buy & hold return of the primary/first symbol
        let first_symbol = symbols.first().map(|s| s.as_str()).unwrap_or("");
        let start_price = grouped_bars
            .get(first_symbol)
            .and_then(|v| v.first())
            .map(|b| b.close)
            .unwrap_or(Decimal::ZERO);
        let last_close = grouped_bars
            .get(first_symbol)
            .and_then(|v| v.last())
            .map(|b| b.close)
            .unwrap_or(Decimal::ZERO);

        let buy_and_hold_return_pct = if !start_price.is_zero() {
            (last_close - start_price)
                .checked_div(start_price)
                .map(|r| r * Decimal::from(100))
                .unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        // SPY benchmark for alpha/beta
        let spy_bars_resolved: Vec<Candle> = if let Some(s) = spy_bars {
            s
        } else {
            self.market_data
                .get_historical_bars("SPY", start, end, "1Day")
                .await
                .unwrap_or_default()
        };

        // First convert daily prices to the format expected by calculate_multi_asset
        let trades_realized = local_orders_to_trades(&executed_trades);

        // Alpha/Beta estimation using SPY
        let mut daily_closes_flat = Vec::new();
        let mut benchmark_returns = Vec::new();
        if !daily_prices.is_empty() {
            // Construct simulated equity curve day-by-day
            // We use the metrics calculation's daily equity curve timestamps
            let mut unique_timestamps: std::collections::BTreeSet<i64> =
                std::collections::BTreeSet::new();
            for prices in daily_prices.values() {
                for &(ts, _) in prices {
                    unique_timestamps.insert(ts);
                }
            }

            // Build lookup maps for faster price lookup
            let mut price_lookups: std::collections::HashMap<
                String,
                std::collections::BTreeMap<i64, Decimal>,
            > = std::collections::HashMap::new();
            for (symbol, prices) in &daily_prices {
                let mut entry = std::collections::BTreeMap::new();
                for &(ts, price) in prices {
                    entry.insert(ts, price);
                }
                price_lookups.insert(symbol.clone(), entry);
            }

            for ts in unique_timestamps {
                let mut realized_pnl = Decimal::ZERO;
                let mut unrealized_pnl = Decimal::ZERO;

                for trade in &trades_realized {
                    let entry_ts = trade.entry_timestamp / 1000;
                    let exit_ts = trade.exit_timestamp.map(|t| t / 1000).unwrap_or(i64::MAX);

                    if exit_ts <= ts {
                        realized_pnl += trade.pnl;
                    } else if entry_ts <= ts {
                        let close_price = price_lookups
                            .get(&trade.symbol)
                            .and_then(|lookup| lookup.range(..=ts).next_back().map(|(_, &p)| p))
                            .unwrap_or(trade.entry_price);

                        unrealized_pnl += (close_price - trade.entry_price) * trade.quantity;
                    }
                }
                daily_closes_flat.push((ts * 1000, initial_equity + realized_pnl + unrealized_pnl));
            }

            // Calculate SPY returns if available
            if !spy_bars_resolved.is_empty() && daily_closes_flat.len() > 1 {
                let mut spy_daily_map: std::collections::BTreeMap<String, Decimal> =
                    std::collections::BTreeMap::new();
                for bar in &spy_bars_resolved {
                    let dt = chrono::DateTime::from_timestamp(bar.timestamp, 0)
                        .unwrap_or_default()
                        .with_timezone(&Utc);
                    let date_key = dt.format("%Y-%m-%d").to_string();
                    spy_daily_map.insert(date_key, bar.close);
                }

                for i in 1..daily_closes_flat.len() {
                    let prev_ts = daily_closes_flat[i - 1].0;
                    let curr_ts = daily_closes_flat[i].0;
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
            }
        }

        let metrics =
            crate::domain::performance::metrics::PerformanceMetrics::calculate_multi_asset(
                &trades_realized,
                initial_equity,
                &daily_prices,
                if benchmark_returns.is_empty() {
                    None
                } else {
                    Some(&benchmark_returns)
                },
            );

        let alpha = metrics.alpha;
        let beta = metrics.beta;

        Ok(BacktestResult {
            trades: executed_trades,
            initial_equity,
            final_equity,
            total_return_pct,
            buy_and_hold_return_pct,
            daily_closes: daily_closes_flat,
            alpha,
            beta,
            benchmark_correlation: 0.0,
        })
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
            sim_config.strategy.strategy_mode,
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
            tracing::info!(
                "Simulator received proposal: {:?} {} {} @ {} for {}",
                prop.side,
                prop.quantity,
                prop.symbol,
                prop.price,
                prop.reason
            );

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
                correlation_id: prop.correlation_id.clone(),
                stop_loss: prop.stop_loss,
            };

            if let Err(e) = self.execution_service.execute(&order).await {
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

fn local_orders_to_trades(orders: &[Order]) -> Vec<Trade> {
    let mut trades = Vec::new();
    let mut open_positions: std::collections::HashMap<String, &Order> =
        std::collections::HashMap::new();
    for order in orders {
        match order.side {
            OrderSide::Buy => {
                open_positions.insert(order.symbol.clone(), order);
            }
            OrderSide::Sell => {
                if let Some(buy) = open_positions.remove(&order.symbol) {
                    let pnl = (order.price - buy.price) * order.quantity;
                    trades.push(Trade {
                        id: order.id.clone(),
                        symbol: order.symbol.clone(),
                        side: OrderSide::Buy,
                        entry_price: buy.price,
                        exit_price: Some(order.price),
                        quantity: order.quantity,
                        pnl,
                        entry_timestamp: buy.timestamp,
                        exit_timestamp: Some(order.timestamp),
                        strategy_used: None,
                        regime_detected: None,
                        entry_reason: None,
                        exit_reason: None,
                        slippage: None,
                        fees: rust_decimal::Decimal::ZERO,
                    });
                }
            }
        }
    }
    trades
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

    async fn get_range(&self, symbol: &str, start_ts: i64, end_ts: i64) -> Result<Vec<Candle>> {
        let candles = self
            .candles
            .lock()
            .expect("InMemoryCandleRepository mutex poisoned - concurrent panic");
        Ok(candles
            .iter()
            .filter(|c| c.symbol == symbol && c.timestamp >= start_ts && c.timestamp <= end_ts)
            .cloned()
            .collect())
    }

    async fn get_latest_timestamp(&self, symbol: &str) -> Result<Option<i64>> {
        let candles = self
            .candles
            .lock()
            .expect("InMemoryCandleRepository mutex poisoned - concurrent panic");
        Ok(candles
            .iter()
            .rfind(|c| c.symbol == symbol)
            .map(|c| c.timestamp))
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
