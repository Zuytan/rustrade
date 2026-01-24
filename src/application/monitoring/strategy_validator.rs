use crate::application::monitoring::empirical_win_rate_provider::EmpiricalWinRateProvider;
use crate::domain::performance::metrics::PerformanceMetrics;
use std::sync::Arc;
use tracing::{info, warn};

/// Validation thresholds for strategy deployment
///
/// These thresholds ensure that only strategies with proven
/// statistical edge are deployed to live trading.
#[derive(Debug, Clone)]
pub struct ValidationThresholds {
    /// Minimum Sharpe ratio (annualized risk-adjusted return)
    /// Recommended: 1.0+ for live trading
    pub min_sharpe_ratio: f64,

    /// Minimum win rate as percentage (e.g., 0.50 = 50%)
    /// Recommended: 0.40+ (40%) for directional strategies
    pub min_win_rate: f64,

    /// Minimum profit factor (gross profit / gross loss)
    /// Recommended: 1.5+ (earn $1.50 for every $1 lost)
    pub min_profit_factor: f64,

    /// Maximum drawdown as percentage (e.g., 0.20 = 20%)
    /// Recommended: < 0.25 (25%) for retail strategies
    pub max_drawdown_pct: f64,

    /// Minimum number of trades for statistical significance
    /// Recommended: 30+ trades for reliable statistics
    pub min_trades: usize,
}

impl Default for ValidationThresholds {
    fn default() -> Self {
        Self {
            min_sharpe_ratio: 1.0,
            min_win_rate: 0.40,
            min_profit_factor: 1.5,
            max_drawdown_pct: 0.25,
            min_trades: 30,
        }
    }
}

/// Result of strategy validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub passed_checks: Vec<String>,
    pub failed_checks: Vec<String>,
    pub metrics: StrategyMetrics,
}

/// Metrics used for validation
#[derive(Debug, Clone)]
pub struct StrategyMetrics {
    pub sharpe_ratio: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub max_drawdown_pct: f64,
    pub total_trades: usize,
}

/// Service for validating trading strategies
///
/// Enforces minimum performance thresholds to ensure strategies
/// have proven statistical edge before live deployment.
///
/// # Example
/// ```
/// use crate::application::monitoring::strategy_validator::{StrategyValidator, ValidationThresholds};
/// use crate::application::monitoring::empirical_win_rate_provider::EmpiricalWinRateProvider;
/// use crate::domain::performance::metrics::PerformanceMetrics;
/// use std::sync::Arc;
///
/// # async fn example() -> anyhow::Result<()> {
/// // Create mock repository and win rate provider
/// # use async_trait::async_trait;
/// # use crate::domain::repositories::TradeRepository;
/// # struct MockRepo;
/// # #[async_trait]
/// # impl TradeRepository for MockRepo {
/// #     async fn save(&self, _: &crate::domain::trading::types::Order) -> anyhow::Result<()> { Ok(()) }
/// #     async fn find_by_symbol(&self, _: &str) -> anyhow::Result<Vec<crate::domain::trading::types::Order>> { Ok(vec![]) }
/// #     async fn find_recent(&self, _: usize) -> anyhow::Result<Vec<crate::domain::trading::types::Order>> { Ok(vec![]) }
/// #     async fn get_all(&self) -> anyhow::Result<Vec<crate::domain::trading::types::Order>> { Ok(vec![]) }
/// #     async fn count(&self) -> anyhow::Result<usize> { Ok(0) }
/// # }
/// let repo = Arc::new(MockRepo);
/// let win_rate_provider = Arc::new(EmpiricalWinRateProvider::new(repo, 0.5, 10));
/// let validator = StrategyValidator::new(
///     win_rate_provider,
///     ValidationThresholds::default()
/// );
///
/// let performance_metrics = PerformanceMetrics::default();
/// let result = validator.validate("AAPL", &performance_metrics).await;
/// if !result.is_valid {
///     println!("Strategy failed validation: {:?}", result.failed_checks);
/// }
/// # Ok(())
/// # }
/// ```
pub struct StrategyValidator {
    win_rate_provider: Arc<EmpiricalWinRateProvider>,
    thresholds: ValidationThresholds,
}

impl StrategyValidator {
    /// Create a new StrategyValidator
    ///
    /// # Arguments
    /// * `win_rate_provider` - Provider for calculating empirical win rates
    /// * `thresholds` - Performance thresholds for validation
    pub fn new(
        win_rate_provider: Arc<EmpiricalWinRateProvider>,
        thresholds: ValidationThresholds,
    ) -> Self {
        Self {
            win_rate_provider,
            thresholds,
        }
    }

    /// Validate a strategy based on historical performance
    ///
    /// # Arguments
    /// * `symbol` - Symbol to validate strategy for
    /// * `metrics` - Performance metrics from backtesting
    ///
    /// # Returns
    /// ValidationResult with pass/fail status and detailed checks
    pub async fn validate(&self, symbol: &str, metrics: &PerformanceMetrics) -> ValidationResult {
        let mut passed_checks = Vec::new();
        let mut failed_checks = Vec::new();

        // Get empirical statistics
        let stats = self.win_rate_provider.get_statistics(symbol).await;

        let strategy_metrics = StrategyMetrics {
            sharpe_ratio: metrics.sharpe_ratio,
            win_rate: stats.win_rate,
            profit_factor: stats.profit_factor,
            max_drawdown_pct: metrics.max_drawdown_pct,
            total_trades: stats.total_trades,
        };

        // Check 1: Minimum number of trades
        if stats.total_trades >= self.thresholds.min_trades {
            passed_checks.push(format!(
                "Trade count: {} >= {} minimum",
                stats.total_trades, self.thresholds.min_trades
            ));
        } else {
            failed_checks.push(format!(
                "INSUFFICIENT DATA: {} trades < {} minimum required",
                stats.total_trades, self.thresholds.min_trades
            ));
        }

        // Check 2: Sharpe ratio
        if metrics.sharpe_ratio >= self.thresholds.min_sharpe_ratio {
            passed_checks.push(format!(
                "Sharpe ratio: {:.2} >= {:.2} minimum",
                metrics.sharpe_ratio, self.thresholds.min_sharpe_ratio
            ));
        } else {
            failed_checks.push(format!(
                "LOW SHARPE: {:.2} < {:.2} minimum",
                metrics.sharpe_ratio, self.thresholds.min_sharpe_ratio
            ));
        }

        // Check 3: Win rate
        if stats.win_rate >= self.thresholds.min_win_rate {
            passed_checks.push(format!(
                "Win rate: {:.1}% >= {:.1}% minimum",
                stats.win_rate * 100.0,
                self.thresholds.min_win_rate * 100.0
            ));
        } else {
            failed_checks.push(format!(
                "LOW WIN RATE: {:.1}% < {:.1}% minimum",
                stats.win_rate * 100.0,
                self.thresholds.min_win_rate * 100.0
            ));
        }

        // Check 4: Profit factor
        if stats.profit_factor >= self.thresholds.min_profit_factor {
            passed_checks.push(format!(
                "Profit factor: {:.2} >= {:.2} minimum",
                stats.profit_factor, self.thresholds.min_profit_factor
            ));
        } else {
            failed_checks.push(format!(
                "LOW PROFIT FACTOR: {:.2} < {:.2} minimum",
                stats.profit_factor, self.thresholds.min_profit_factor
            ));
        }

        // Check 5: Maximum drawdown
        if metrics.max_drawdown_pct <= self.thresholds.max_drawdown_pct {
            passed_checks.push(format!(
                "Max drawdown: {:.1}% <= {:.1}% maximum",
                metrics.max_drawdown_pct * 100.0,
                self.thresholds.max_drawdown_pct * 100.0
            ));
        } else {
            failed_checks.push(format!(
                "EXCESSIVE DRAWDOWN: {:.1}% > {:.1}% maximum",
                metrics.max_drawdown_pct * 100.0,
                self.thresholds.max_drawdown_pct * 100.0
            ));
        }

        let is_valid = failed_checks.is_empty();

        if is_valid {
            info!(
                "StrategyValidator [{}]: ✅ PASSED validation ({} checks)",
                symbol,
                passed_checks.len()
            );
        } else {
            warn!(
                "StrategyValidator [{}]: ❌ FAILED validation - {} failures: {:?}",
                symbol,
                failed_checks.len(),
                failed_checks
            );
        }

        ValidationResult {
            is_valid,
            passed_checks,
            failed_checks,
            metrics: strategy_metrics,
        }
    }

    /// Validate multiple symbols and return aggregated results
    ///
    /// Useful for portfolio-level validation.
    pub async fn validate_portfolio(
        &self,
        symbols: &[String],
        metrics_by_symbol: &std::collections::HashMap<String, PerformanceMetrics>,
    ) -> Vec<(String, ValidationResult)> {
        let mut results = Vec::new();

        for symbol in symbols {
            if let Some(metrics) = metrics_by_symbol.get(symbol) {
                let result = self.validate(symbol, metrics).await;
                results.push((symbol.clone(), result));
            }
        }

        results
    }

    /// Check if the overall portfolio meets validation criteria
    ///
    /// Returns true only if ALL symbols pass validation.
    pub async fn validate_all_pass(
        &self,
        symbols: &[String],
        metrics_by_symbol: &std::collections::HashMap<String, PerformanceMetrics>,
    ) -> bool {
        let results = self.validate_portfolio(symbols, metrics_by_symbol).await;
        results.iter().all(|(_, result)| result.is_valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::TradeRepository;
    use crate::domain::trading::types::{Order, OrderSide, OrderType};
    use async_trait::async_trait;
    use rust_decimal_macros::dec;

    struct MockTradeRepository {
        orders: Vec<Order>,
    }

    #[async_trait]
    impl TradeRepository for MockTradeRepository {
        async fn save(&self, _trade: &Order) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_symbol(&self, symbol: &str) -> anyhow::Result<Vec<Order>> {
            Ok(self
                .orders
                .iter()
                .filter(|o| o.symbol == symbol)
                .cloned()
                .collect())
        }
        async fn find_by_status(
            &self,
            _status: crate::domain::trading::types::OrderStatus,
        ) -> anyhow::Result<Vec<Order>> {
            Ok(vec![])
        }

        async fn find_recent(&self, _limit: usize) -> anyhow::Result<Vec<Order>> {
            Ok(self.orders.clone())
        }

        async fn get_all(&self) -> anyhow::Result<Vec<Order>> {
            Ok(self.orders.clone())
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(self.orders.len())
        }
    }

    fn create_winning_trade_pair(symbol: &str) -> Vec<Order> {
        vec![
            Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: symbol.to_string(),
                side: OrderSide::Buy,
                price: dec!(100.0),
                quantity: dec!(10.0),
                order_type: OrderType::Market,
                status: crate::domain::trading::types::OrderStatus::Filled,
                timestamp: 0,
            },
            Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: symbol.to_string(),
                side: OrderSide::Sell,
                price: dec!(110.0),
                quantity: dec!(10.0),
                order_type: OrderType::Market,
                status: crate::domain::trading::types::OrderStatus::Filled,
                timestamp: 1000,
            },
        ]
    }

    fn create_losing_trade_pair(symbol: &str) -> Vec<Order> {
        vec![
            Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: symbol.to_string(),
                side: OrderSide::Buy,
                price: dec!(100.0),
                quantity: dec!(10.0),
                order_type: OrderType::Market,
                status: crate::domain::trading::types::OrderStatus::Filled,
                timestamp: 0,
            },
            Order {
                id: uuid::Uuid::new_v4().to_string(),
                symbol: symbol.to_string(),
                side: OrderSide::Sell,
                price: dec!(90.0),
                quantity: dec!(10.0),
                order_type: OrderType::Market,
                status: crate::domain::trading::types::OrderStatus::Filled,
                timestamp: 1000,
            },
        ]
    }

    #[tokio::test]
    async fn test_validation_passes_with_good_metrics() {
        // Create 40 trades: 25 wins (62.5%), 15 losses
        let mut orders = Vec::new();
        for _ in 0..25 {
            orders.extend(create_winning_trade_pair("AAPL"));
        }
        for _ in 0..15 {
            orders.extend(create_losing_trade_pair("AAPL"));
        }

        let repo = Arc::new(MockTradeRepository { orders });
        let win_rate_provider = Arc::new(EmpiricalWinRateProvider::new(repo, 0.50, 10));
        let validator = StrategyValidator::new(win_rate_provider, ValidationThresholds::default());

        let metrics = PerformanceMetrics {
            sharpe_ratio: 1.5,
            max_drawdown_pct: 0.15,
            ..Default::default()
        };

        let result = validator.validate("AAPL", &metrics).await;

        assert!(result.is_valid, "Should pass validation");
        assert_eq!(result.failed_checks.len(), 0);
        assert!(result.passed_checks.len() >= 4); // At least 4 checks should pass
    }

    #[tokio::test]
    async fn test_validation_fails_with_low_sharpe() {
        let mut orders = Vec::new();
        for _ in 0..40 {
            orders.extend(create_winning_trade_pair("AAPL"));
        }

        let repo = Arc::new(MockTradeRepository { orders });
        let win_rate_provider = Arc::new(EmpiricalWinRateProvider::new(repo, 0.50, 10));
        let validator = StrategyValidator::new(win_rate_provider, ValidationThresholds::default());

        let metrics = PerformanceMetrics {
            sharpe_ratio: 0.5, // Below threshold
            max_drawdown_pct: 0.10,
            ..Default::default()
        };

        let result = validator.validate("AAPL", &metrics).await;

        assert!(!result.is_valid, "Should fail validation");
        assert!(
            result
                .failed_checks
                .iter()
                .any(|c| c.contains("LOW SHARPE")),
            "Should have Sharpe ratio failure"
        );
    }

    #[tokio::test]
    async fn test_validation_fails_with_insufficient_trades() {
        // Only 5 trades, below 30 threshold
        let mut orders = Vec::new();
        for _ in 0..5 {
            orders.extend(create_winning_trade_pair("AAPL"));
        }

        let repo = Arc::new(MockTradeRepository { orders });
        let win_rate_provider = Arc::new(EmpiricalWinRateProvider::new(repo, 0.50, 10));
        let validator = StrategyValidator::new(win_rate_provider, ValidationThresholds::default());

        let metrics = PerformanceMetrics {
            sharpe_ratio: 2.0, // Good Sharpe
            max_drawdown_pct: 0.10,
            ..Default::default()
        };

        let result = validator.validate("AAPL", &metrics).await;

        assert!(!result.is_valid, "Should fail validation");
        assert!(
            result
                .failed_checks
                .iter()
                .any(|c| c.contains("INSUFFICIENT DATA")),
            "Should have insufficient data failure"
        );
    }

    #[tokio::test]
    async fn test_custom_thresholds() {
        // Create 15 wins + 5 losses = 20 trades (75% win rate, profit factor 3.0)
        let mut orders = Vec::new();
        for _ in 0..15 {
            orders.extend(create_winning_trade_pair("AAPL"));
        }
        for _ in 0..5 {
            orders.extend(create_losing_trade_pair("AAPL"));
        }

        let repo = Arc::new(MockTradeRepository { orders });
        let win_rate_provider = Arc::new(EmpiricalWinRateProvider::new(repo, 0.50, 5));

        let custom_thresholds = ValidationThresholds {
            min_sharpe_ratio: 0.5,
            min_win_rate: 0.30,
            min_profit_factor: 1.0,
            max_drawdown_pct: 0.30,
            min_trades: 10, // Lower threshold
        };

        let validator = StrategyValidator::new(win_rate_provider, custom_thresholds);

        let metrics = PerformanceMetrics {
            sharpe_ratio: 0.7,
            max_drawdown_pct: 0.25,
            ..Default::default()
        };

        let result = validator.validate("AAPL", &metrics).await;

        if !result.is_valid {
            eprintln!("Failed checks: {:?}", result.failed_checks);
            eprintln!("Metrics: {:?}", result.metrics);
        }

        assert!(
            result.is_valid,
            "Should pass with custom thresholds. Failed checks: {:?}",
            result.failed_checks
        );
    }
}
