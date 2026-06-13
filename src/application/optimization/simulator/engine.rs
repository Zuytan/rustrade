use crate::application::agents::analyst::{Analyst, AnalystDependencies};
use crate::application::optimization::simulator::result::local_orders_to_trades;
use crate::application::optimization::simulator::{
    BacktestResult, InMemoryCandleRepository, Simulator,
};
use crate::domain::trading::portfolio::Portfolio;
use crate::domain::trading::types::{Candle, MarketEvent, Order};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

impl Simulator {
    /// Common core simulation runner that feeds candles, executes proposals, and handles risk/drawdowns.
    async fn run_simulation_core(
        &self,
        bars_owned: Vec<Candle>,
        candle_repo_candles: Vec<Candle>,
    ) -> Result<(Decimal, Vec<Order>, Portfolio)> {
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
                    candle_repo_candles,
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
                        crate::infrastructure::observability::Metrics::new()?,
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
            // Circuit Breaker: Check equity before executing
            if let Ok(portfolio) = self.execution_service.get_portfolio().await {
                let mut current_equity = portfolio.cash;
                for pos in portfolio.positions.values() {
                    let price = if pos.symbol == prop.symbol {
                        prop.price
                    } else {
                        pos.average_price
                    };
                    current_equity += pos.quantity * price;
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

        let final_portfolio = self.execution_service.get_portfolio().await?;
        Ok((initial_equity, executed_trades, final_portfolio))
    }

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

        let daily_closes: Vec<(i64, Decimal)> = daily_map.values().cloned().collect();

        let candle_repo_candles: Vec<Candle> = bars
            .iter()
            .map(|b| Candle {
                symbol: symbol.to_string(),
                open: b.open,
                high: b.high,
                low: b.low,
                close: b.close,
                volume: b.volume,
                timestamp: b.timestamp,
            })
            .collect();

        let start_price = bars.first().map(|b| b.close).unwrap_or(Decimal::ZERO);
        let last_close = bars.last().map(|b| b.close).unwrap_or(Decimal::ZERO);

        let (initial_equity, executed_trades, final_portfolio) = self
            .run_simulation_core(bars_owned, candle_repo_candles)
            .await?;

        let mut final_equity = final_portfolio.cash;
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

        let min_return = Decimal::new(-100, 0);
        if total_return_pct < min_return {
            total_return_pct = min_return;
        }

        let buy_and_hold_return_pct = if !start_price.is_zero() {
            (last_close - start_price)
                .checked_div(start_price)
                .map(|r| r * Decimal::from(100))
                .unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

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
            let mut strategy_returns = Vec::new();
            for i in 1..daily_closes.len() {
                let prev_price = daily_closes[i - 1].1;
                let curr_price = daily_closes[i].1;
                if prev_price > Decimal::ZERO {
                    strategy_returns.push((curr_price - prev_price) / prev_price);
                }
            }

            let mut spy_daily_map: std::collections::BTreeMap<String, Decimal> =
                std::collections::BTreeMap::new();
            for bar in spy_bars_ref {
                let dt = chrono::DateTime::from_timestamp(bar.timestamp, 0)
                    .unwrap_or_default()
                    .with_timezone(&Utc);
                let date_key = dt.format("%Y-%m-%d").to_string();
                spy_daily_map.insert(date_key, bar.close);
            }

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

        let (initial_equity, executed_trades, final_portfolio) =
            self.run_simulation_core(bars_owned, bars.to_vec()).await?;

        let mut final_equity = final_portfolio.cash;
        for pos in final_portfolio.positions.values() {
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

        let spy_bars_resolved: Vec<Candle> = if let Some(s) = spy_bars {
            s
        } else {
            self.market_data
                .get_historical_bars("SPY", start, end, "1Day")
                .await
                .unwrap_or_default()
        };

        let trades_realized = local_orders_to_trades(&executed_trades);

        let mut daily_closes_flat = Vec::new();
        let mut benchmark_returns = Vec::new();
        if !daily_prices.is_empty() {
            let mut unique_timestamps: std::collections::BTreeSet<i64> =
                std::collections::BTreeSet::new();
            for prices in daily_prices.values() {
                for &(ts, _) in prices {
                    unique_timestamps.insert(ts);
                }
            }

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
}
