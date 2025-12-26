use crate::application::analyst::{Analyst, AnalystConfig};
use crate::domain::ports::ExecutionService;
use crate::domain::types::MarketEvent;
use crate::infrastructure::alpaca::AlpacaMarketDataService;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::domain::types::Order;

pub struct BacktestResult {
    pub trades: Vec<Order>,
    pub initial_equity: Decimal,
    pub final_equity: Decimal,
    pub total_return_pct: Decimal,
    pub buy_and_hold_return_pct: Decimal,
}

pub struct Simulator {
    market_data: Arc<AlpacaMarketDataService>,
    execution_service: Arc<dyn ExecutionService>,
    config: AnalystConfig,
}

impl Simulator {
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
        let bars = self.market_data
            .get_historical_bars(symbol, start, end, "1Min")
            .await
            .context("Failed to fetch historical bars")?;
            
        info!("Simulator: Fetched {} bars. Starting simulation...", bars.len());

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
            strategy_mode: self.config.strategy_mode.clone(),
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
        };

        let mut analyst = Analyst::new(market_rx, proposal_tx, self.execution_service.clone(), sim_config);

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
        let start_price = bars.first().map(|b| Decimal::from_f64_retain(b.close).unwrap_or(Decimal::ZERO)).unwrap_or(Decimal::ZERO);
        let last_close = bars.last().map(|b| Decimal::from_f64_retain(b.close).unwrap_or(Decimal::ZERO)).unwrap_or(Decimal::ZERO);

        let symbol_clone = symbol.to_string();
        let feeder_handle = tokio::spawn(async move {
            for bar in bars {
                let timestamp = chrono::DateTime::parse_from_rfc3339(&bar.timestamp)
                    .unwrap_or_default()
                    .timestamp_millis();
                
                let price = Decimal::from_f64_retain(bar.close).unwrap_or(Decimal::ZERO);

                let event = MarketEvent::Quote {
                    symbol: symbol_clone.clone(),
                    price,
                    timestamp,
                };
                
                // artificial delay to allow Analyst to catch up / generate proposal before we feed next 100 bars?
                // tokio::time::sleep(std::time::Duration::from_micros(10)).await;
                
                if let Err(_) = market_tx.send(event).await {
                    break;
                }
            }
        });

        let mut executed_trades = Vec::new();


        while let Some(prop) = proposal_rx.recv().await {
            // Execute Immediately to update Portfolio State for next Analyst check
            let order = crate::domain::types::Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: prop.symbol.clone(),
                side: prop.side,
                price: prop.price,
                quantity: prop.quantity,
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

        Ok(BacktestResult {
            trades: executed_trades,
            initial_equity,
            final_equity,
            total_return_pct,
            buy_and_hold_return_pct,
        })
    }
}
