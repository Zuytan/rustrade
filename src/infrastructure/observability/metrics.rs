//! Prometheus metrics definitions for Rustrade
//!
//! All metrics use the `rustrade_` prefix and are read-only.

use prometheus::{
    CounterVec, Gauge, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
    core::{AtomicF64, GenericGauge, GenericGaugeVec},
};
use std::sync::Arc;

/// Prometheus metrics for the trading system
#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,
    /// Total portfolio value in USD
    pub portfolio_value_usd: GenericGauge<AtomicF64>,
    /// Available cash in USD
    pub portfolio_cash_usd: GenericGauge<AtomicF64>,
    /// Number of open positions
    pub positions_count: GenericGauge<AtomicF64>,
    /// Position value per symbol
    pub position_value_usd: GenericGaugeVec<AtomicF64>,
    /// Daily P&L in USD
    pub daily_pnl_usd: GenericGauge<AtomicF64>,
    /// Total orders counter by side and status
    pub orders_total: CounterVec,
    /// Circuit breaker status (0=open, 1=tripped)
    pub circuit_breaker_status: GenericGauge<AtomicF64>,
    /// Sentiment score (Fear & Greed index)
    pub sentiment_score: GenericGauge<AtomicF64>,
    /// Uptime in seconds
    pub uptime_seconds: GenericGauge<AtomicF64>,
    /// API latency in seconds
    pub api_latency_seconds: HistogramVec,
    /// WebSocket reconnection attempts
    pub websocket_reconnects_total: CounterVec,
    /// Strategy signals generated
    pub trade_signals_total: CounterVec,
    /// Current win rate (0-1)
    pub win_rate_current: GenericGauge<AtomicF64>,
    /// Current drawdown (0-1)
    pub drawdown_current: GenericGauge<AtomicF64>,
    /// Trades today
    pub trades_today: CounterVec,
}

impl Metrics {
    /// Create a new Metrics instance with all gauges and counters registered
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        let portfolio_value_usd = Gauge::with_opts(Opts::new(
            "rustrade_portfolio_value_usd",
            "Total portfolio value in USD",
        ))?;
        registry.register(Box::new(portfolio_value_usd.clone()))?;

        let portfolio_cash_usd = Gauge::with_opts(Opts::new(
            "rustrade_portfolio_cash_usd",
            "Available cash in USD",
        ))?;
        registry.register(Box::new(portfolio_cash_usd.clone()))?;

        let positions_count = Gauge::with_opts(Opts::new(
            "rustrade_positions_count",
            "Number of open positions",
        ))?;
        registry.register(Box::new(positions_count.clone()))?;

        let position_value_usd = GaugeVec::new(
            Opts::new(
                "rustrade_position_value_usd",
                "Position value per symbol in USD",
            ),
            &["symbol"],
        )?;
        registry.register(Box::new(position_value_usd.clone()))?;

        let daily_pnl_usd =
            Gauge::with_opts(Opts::new("rustrade_daily_pnl_usd", "Daily P&L in USD"))?;
        registry.register(Box::new(daily_pnl_usd.clone()))?;

        let orders_total = CounterVec::new(
            Opts::new("rustrade_orders_total", "Total orders by side and status"),
            &["side", "status"],
        )?;
        registry.register(Box::new(orders_total.clone()))?;

        let circuit_breaker_status = Gauge::with_opts(Opts::new(
            "rustrade_circuit_breaker_status",
            "Circuit breaker status (0=open, 1=tripped)",
        ))?;
        registry.register(Box::new(circuit_breaker_status.clone()))?;

        let sentiment_score = Gauge::with_opts(Opts::new(
            "rustrade_sentiment_score",
            "Fear & Greed sentiment index (0-100)",
        ))?;
        registry.register(Box::new(sentiment_score.clone()))?;

        let uptime_seconds = Gauge::with_opts(Opts::new(
            "rustrade_uptime_seconds",
            "Server uptime in seconds",
        ))?;
        registry.register(Box::new(uptime_seconds.clone()))?;

        let api_latency_seconds = HistogramVec::new(
            HistogramOpts::new(
                "rustrade_api_latency_seconds",
                "API request latency in seconds",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
            ]),
            &["broker", "endpoint"],
        )?;
        registry.register(Box::new(api_latency_seconds.clone()))?;

        let websocket_reconnects_total = CounterVec::new(
            Opts::new(
                "rustrade_websocket_reconnects_total",
                "Total WebSocket reconnection attempts",
            ),
            &["broker"],
        )?;
        registry.register(Box::new(websocket_reconnects_total.clone()))?;

        let trade_signals_total = CounterVec::new(
            Opts::new(
                "rustrade_trade_signals_total",
                "Total strategy signals generated",
            ),
            &["strategy", "signal_type"],
        )?;
        registry.register(Box::new(trade_signals_total.clone()))?;

        let win_rate_current = Gauge::with_opts(Opts::new(
            "rustrade_win_rate_current",
            "Current win rate (0-1)",
        ))?;
        registry.register(Box::new(win_rate_current.clone()))?;

        let drawdown_current = Gauge::with_opts(Opts::new(
            "rustrade_drawdown_current",
            "Current drawdown (0-1)",
        ))?;
        registry.register(Box::new(drawdown_current.clone()))?;

        let trades_today = CounterVec::new(
            Opts::new("rustrade_trades_today", "Total trades executed today"),
            &["side", "outcome"],
        )?;
        registry.register(Box::new(trades_today.clone()))?;

        Ok(Self {
            registry: Arc::new(registry),
            portfolio_value_usd,
            portfolio_cash_usd,
            positions_count,
            position_value_usd,
            daily_pnl_usd,
            orders_total,
            circuit_breaker_status,
            sentiment_score,
            uptime_seconds,
            api_latency_seconds,
            websocket_reconnects_total,
            trade_signals_total,
            win_rate_current,
            drawdown_current,
            trades_today,
        })
    }

    /// Render all metrics in Prometheus text format
    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder
            .encode_to_string(&metric_families)
            .unwrap_or_default()
    }

    /// Update position value for a specific symbol
    pub fn set_position_value(&self, symbol: &str, value: f64) {
        self.position_value_usd
            .with_label_values(&[symbol])
            .set(value);
    }

    /// Increment order counter
    pub fn inc_orders(&self, side: &str, status: &str) {
        self.orders_total.with_label_values(&[side, status]).inc();
    }

    /// Observe API latency
    pub fn observe_api_latency(&self, broker: &str, endpoint: &str, latency: f64) {
        self.api_latency_seconds
            .with_label_values(&[broker, endpoint])
            .observe(latency);
    }

    /// Increment WebSocket reconnects
    pub fn inc_reconnects(&self, broker: &str) {
        self.websocket_reconnects_total
            .with_label_values(&[broker])
            .inc();
    }

    /// Increment trade signals
    pub fn inc_signals(&self, strategy: &str, signal_type: &str) {
        self.trade_signals_total
            .with_label_values(&[strategy, signal_type])
            .inc();
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Failed to create default Metrics")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new().expect("Failed to create metrics");
        assert!(metrics.render().contains("rustrade_"));
    }

    #[test]
    fn test_portfolio_value_update() {
        let metrics = Metrics::new().expect("Failed to create metrics");
        metrics.portfolio_value_usd.set(50000.0);
        let output = metrics.render();
        assert!(output.contains("rustrade_portfolio_value_usd 50000"));
    }

    #[test]
    fn test_position_value_per_symbol() {
        let metrics = Metrics::new().expect("Failed to create metrics");
        metrics.set_position_value("AAPL", 10000.0);
        metrics.set_position_value("MSFT", 8000.0);
        let output = metrics.render();
        assert!(output.contains("rustrade_position_value_usd"));
        assert!(output.contains("AAPL"));
        assert!(output.contains("MSFT"));
    }

    #[test]
    fn test_order_counter() {
        let metrics = Metrics::new().expect("Failed to create metrics");
        metrics.inc_orders("buy", "filled");
        metrics.inc_orders("sell", "rejected");
        let output = metrics.render();
        assert!(output.contains("rustrade_orders_total"));
    }
}
