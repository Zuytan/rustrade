use rust_decimal::Decimal;

use std::sync::Arc;
use tracing::info;

use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::monitoring::cost_evaluator::CostEvaluator;
use crate::application::risk_management::circuit_breaker_service::HaltLevel;
use crate::application::risk_management::volatility::calculate_realized_volatility;
use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
use crate::domain::trading::types::{OrderSide, OrderType, TradeProposal};

#[derive(Debug, Clone)]
pub struct SizingConfig {
    pub risk_per_trade_percent: Decimal,
    pub max_positions: usize,
    pub max_position_size_pct: Decimal,
    pub static_trade_quantity: Decimal,
    // NEW: Volatility Targeting
    pub enable_vol_targeting: bool,
    pub target_volatility: Decimal, // Target annualized volatility (e.g., 0.15 = 15%)
}

/// Trade statistics for Kelly Criterion position sizing. Use when n_trades >= 30.
#[derive(Debug, Clone)]
pub struct KellyStats {
    pub win_rate: f64,
    pub avg_win: Decimal,
    pub avg_loss: Decimal,
    pub n_trades: usize,
}

impl KellyStats {
    /// Quarter-Kelly fraction: f* = (p*b - (1-p)*a) / b, then 0.25 * f*.
    /// Returns None if avg_win <= 0 or result is non-positive.
    pub fn quarter_kelly_fraction(&self) -> Option<Decimal> {
        if self.n_trades < 30 || self.avg_win <= Decimal::ZERO {
            return None;
        }
        let p = Decimal::from_f64_retain(self.win_rate).unwrap_or(Decimal::ZERO);
        let one_p = Decimal::ONE - p;
        let loss_as_positive = self.avg_loss.abs();
        let numerator = p * self.avg_win - one_p * loss_as_positive;
        let f_star = numerator.checked_div(self.avg_win).unwrap_or(Decimal::ZERO);
        if f_star <= Decimal::ZERO {
            return None;
        }
        let quarter = dec!(0.25);
        Some((f_star * quarter).min(Decimal::ONE).max(Decimal::ZERO))
    }
}

pub struct SizingEngine {
    spread_cache: Arc<SpreadCache>,
    cost_evaluator: Option<CostEvaluator>,
}

use rust_decimal_macros::dec;

impl SizingEngine {
    pub fn new(spread_cache: Arc<SpreadCache>) -> Self {
        Self {
            spread_cache,
            cost_evaluator: None,
        }
    }

    /// Create a SizingEngine that deducts estimated transaction costs from position size.
    pub fn with_cost_evaluator(
        spread_cache: Arc<SpreadCache>,
        cost_evaluator: CostEvaluator,
    ) -> Self {
        Self {
            spread_cache,
            cost_evaluator: Some(cost_evaluator),
        }
    }

    /// Calculate quantity with slippage adjustment based on bid-ask spread,
    /// optionally volatility targeting, Kelly Criterion cap, circuit breaker level, and market regime.
    /// `available_cash` caps the target amount to prevent orders exceeding available funds.
    #[allow(clippy::too_many_arguments)]
    pub fn calculate_quantity_with_slippage(
        &self,
        config: &SizingConfig,
        total_equity: Decimal,
        price: Decimal,
        symbol: &str,
        recent_prices: Option<&[f64]>,
        kelly_stats: Option<&KellyStats>,
        halt_level: Option<HaltLevel>,
        regime: Option<&MarketRegime>,
        available_cash: Option<Decimal>,
    ) -> Decimal {
        let mut base_qty = self.calculate_quantity(
            config,
            total_equity,
            price,
            symbol,
            kelly_stats,
            halt_level,
            regime,
            available_cash,
        );

        // NEW: Apply volatility targeting if enabled
        if config.enable_vol_targeting {
            #[allow(clippy::collapsible_if)]
            if let Some(prices) = recent_prices {
                if let Some(realized_vol) = calculate_realized_volatility(prices, 252.0) {
                    if realized_vol > 0.0 {
                        // Vol targeting: scale position inversely to volatility
                        let realized_vol_dec =
                            Decimal::from_f64_retain(realized_vol).unwrap_or(Decimal::ONE);
                        if realized_vol_dec > Decimal::ZERO {
                            let vol_multiplier = config.target_volatility / realized_vol_dec;
                            let vol_multiplier = vol_multiplier.clamp(dec!(0.25), dec!(2.0));

                            info!(
                                "SizingEngine: Vol targeting for {} - Realized: {}%, Target: {}%, Multiplier: {}x",
                                symbol,
                                realized_vol_dec * dec!(100),
                                config.target_volatility * dec!(100),
                                vol_multiplier
                            );

                            base_qty *= vol_multiplier;
                            base_qty = base_qty.round_dp(4);
                        }
                    }
                }
            }
        }

        // Apply slippage adjustment
        if let Some(spread_pct_f64) = self.spread_cache.get_spread_pct(symbol) {
            let spread_pct = Decimal::from_f64_retain(spread_pct_f64).unwrap_or(Decimal::ZERO);
            // Reduce size if spread is wide (>20 bps = 0.002 = 0.2%)
            if spread_pct > dec!(0.002) {
                let slippage_multiplier = dec!(0.002)
                    .checked_div(spread_pct)
                    .map(|v| v.min(Decimal::ONE))
                    .unwrap_or(Decimal::ONE);
                info!(
                    "SizingEngine: High spread detected for {} ({}%), reducing size by factor {}",
                    symbol,
                    spread_pct * dec!(100),
                    slippage_multiplier
                );
                base_qty *= slippage_multiplier;
                return base_qty.round_dp(4);
            }
        }

        base_qty
    }

    #[allow(clippy::too_many_arguments)]
    pub fn calculate_quantity(
        &self,
        config: &SizingConfig,
        total_equity: Decimal,
        price: Decimal,
        symbol: &str,
        kelly_stats: Option<&KellyStats>,
        halt_level: Option<HaltLevel>,
        regime: Option<&MarketRegime>,
        available_cash: Option<Decimal>,
    ) -> Decimal {
        // Fallback to static quantity if risk sizing is disabled
        if config.risk_per_trade_percent <= Decimal::ZERO {
            info!(
                "SizingEngine: Using static quantity for {}: {}",
                symbol, config.static_trade_quantity
            );
            let q = config.static_trade_quantity;
            let q = apply_halt_multiplier(q, halt_level);
            return apply_regime_multiplier(q, regime);
        }

        let min_price = dec!(0.0001);
        if total_equity <= Decimal::ZERO || price <= Decimal::ZERO || price < min_price {
            info!(
                "SizingEngine: Cannot calculate quantity for {} - TotalEquity={}, Price={}",
                symbol, total_equity, price
            );
            return Decimal::ZERO;
        }

        // 1. Calculate the target amount to allocate based on risk_per_trade_percent
        let mut target_amt = total_equity * config.risk_per_trade_percent;

        // 1a. Cap by Quarter-Kelly when we have enough trade history
        if let Some(stats) = kelly_stats
            && let Some(kelly_frac) = stats.quarter_kelly_fraction()
        {
            let kelly_amt = total_equity * kelly_frac;
            if kelly_amt < target_amt {
                info!(
                    "SizingEngine: Kelly cap for {} - ${} (risk) capped to ${} (quarter-Kelly)",
                    symbol, target_amt, kelly_amt
                );
                target_amt = kelly_amt;
            }
        }

        // 1b. Deduct estimated transaction costs when CostEvaluator is available
        if let Some(ref evaluator) = self.cost_evaluator {
            let qty_est = target_amt.checked_div(price).unwrap_or(Decimal::ZERO);
            let proposal = TradeProposal {
                symbol: symbol.to_string(),
                side: OrderSide::Buy,
                price,
                quantity: qty_est,
                order_type: OrderType::Market,
                reason: String::new(),
                timestamp: 0,
                stop_loss: None,
                take_profit: None,
            };
            let costs = evaluator.evaluate(&proposal);
            target_amt = (target_amt - costs.total_cost).max(Decimal::ZERO);
        }

        info!(
            "SizingEngine: Initial target amount for {} ({}% of equity): ${}",
            symbol,
            config.risk_per_trade_percent * dec!(100),
            target_amt
        );

        // 2. Apply Caps for diversification

        // Cap 1: Max Positions bucket (if max_positions > 0)
        if config.max_positions > 0 {
            let max_bucket = total_equity / Decimal::from(config.max_positions);
            let before = target_amt;
            target_amt = target_amt.min(max_bucket);
            if target_amt < before {
                info!(
                    "SizingEngine: Capped {} by max_positions bucket: ${} -> ${}",
                    symbol, before, target_amt
                );
            }
        }

        // Cap 2: Max Position Size % (Hard Cap)
        if config.max_position_size_pct > Decimal::ZERO {
            let max_pos_val = total_equity * config.max_position_size_pct;
            let before = target_amt;
            target_amt = target_amt.min(max_pos_val);
            if target_amt < before {
                info!(
                    "SizingEngine: Capped {} by max_position_size_pct ({}%): ${} -> ${}",
                    symbol,
                    config.max_position_size_pct * dec!(100),
                    before,
                    target_amt
                );
            }
        }

        // 3. Cap by available cash (prevents buying more than we can pay for)
        if let Some(cash) = available_cash
            && cash > Decimal::ZERO
            && target_amt > cash
        {
            info!(
                "SizingEngine: Capped {} by available cash: ${} -> ${}",
                symbol, target_amt, cash
            );
            target_amt = cash;
        }

        // 4. Convert to Shares (checked_div avoids overflow for tiny price)
        let quantity = target_amt
            .checked_div(price)
            .map(|q| q.round_dp(4))
            .unwrap_or(Decimal::ZERO);
        let quantity = apply_halt_multiplier(quantity, halt_level);
        let quantity = apply_regime_multiplier(quantity, regime);

        info!(
            "SizingEngine: Final quantity for {}: {} shares (${} / ${} per share)",
            symbol, quantity, target_amt, price
        );

        quantity
    }
}

fn apply_halt_multiplier(qty: Decimal, halt_level: Option<HaltLevel>) -> Decimal {
    halt_level
        .map(|l| {
            let mult = Decimal::from_f64_retain(l.size_multiplier()).unwrap_or(Decimal::ONE);
            (qty * mult).round_dp(4)
        })
        .unwrap_or(qty)
}

/// Position size multiplier by market regime (adjusted for crypto: Volatile 0.5, Unknown 0.3, Ranging 0.7).
fn regime_size_multiplier(regime_type: MarketRegimeType) -> f64 {
    match regime_type {
        MarketRegimeType::TrendingUp => 1.0,
        MarketRegimeType::TrendingDown => 0.5,
        MarketRegimeType::Ranging => 0.7,
        MarketRegimeType::Volatile => 0.5,
        MarketRegimeType::Unknown => 0.3,
    }
}

fn apply_regime_multiplier(qty: Decimal, regime: Option<&MarketRegime>) -> Decimal {
    regime
        .map(|r| {
            let mult = Decimal::from_f64_retain(regime_size_multiplier(r.regime_type))
                .unwrap_or(Decimal::ONE);
            (qty * mult).round_dp(4)
        })
        .unwrap_or(qty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::market_data::spread_cache::SpreadCache;
    use crate::domain::market::market_regime::{MarketRegime, MarketRegimeType};
    use rust_decimal_macros::dec;
    use std::sync::Arc;

    fn create_test_config() -> SizingConfig {
        SizingConfig {
            risk_per_trade_percent: dec!(0.01),
            max_positions: 5,
            max_position_size_pct: dec!(0.20),
            static_trade_quantity: dec!(1.0),
            enable_vol_targeting: false,
            target_volatility: dec!(0.15),
        }
    }

    #[test]
    fn test_calculate_quantity_normal_spread() {
        let spread_cache = Arc::new(SpreadCache::new());
        // Update cache with normal spread (5 bps = 0.05%)
        spread_cache.update("BTC/USD".to_string(), 100.00, 100.05);

        let engine = SizingEngine::new(spread_cache);
        let config = create_test_config();

        // Equity: 100,000. Risk 1% = 1,000 target.
        // Price 100. Quantity = 10.
        let qty = engine.calculate_quantity_with_slippage(
            &config,
            dec!(100000),
            dec!(100),
            "BTC/USD",
            None,
            None,
            None,
            None,
            None, // no cash cap
        );

        assert_eq!(qty, dec!(10));
    }

    #[test]
    fn test_calculate_quantity_wide_spread_adjustment() {
        let spread_cache = Arc::new(SpreadCache::new());
        // Update cache with Wide spread (50 bps = 0.5%) > 0.2% threshold
        spread_cache.update("BTC/USD".to_string(), 99.75, 100.25);

        let engine = SizingEngine::new(spread_cache);
        let config = create_test_config();

        // Multiplier logic: 0.002 / 0.005 = 0.4
        // Expected qty: 10 * 0.4 = 4.

        let qty = engine.calculate_quantity_with_slippage(
            &config,
            dec!(100000),
            dec!(100),
            "BTC/USD",
            None,
            None,
            None,
            None,
            None, // no cash cap
        );

        assert_eq!(qty, dec!(4));
    }

    #[test]
    fn test_calculate_quantity_static_fallback() {
        let spread_cache = Arc::new(SpreadCache::new());
        let engine = SizingEngine::new(spread_cache);

        let mut config = create_test_config();
        config.risk_per_trade_percent = Decimal::ZERO; // Disable risk sizing
        config.static_trade_quantity = dec!(5);

        let qty = engine.calculate_quantity_with_slippage(
            &config,
            dec!(100000),
            dec!(100),
            "BTC/USD",
            None,
            None,
            None,
            None,
            None, // no cash cap
        );

        assert_eq!(qty, dec!(5));
    }

    #[test]
    fn test_regime_multiplier_reduces_size() {
        let spread_cache = Arc::new(SpreadCache::new());
        spread_cache.update("BTC/USD".to_string(), 100.00, 100.05);
        let engine = SizingEngine::new(spread_cache);
        let config = create_test_config();
        let regime = MarketRegime::new(MarketRegimeType::Volatile, dec!(0.8), dec!(3.0), dec!(0.0));
        let qty = engine.calculate_quantity_with_slippage(
            &config,
            dec!(100000),
            dec!(100),
            "BTC/USD",
            None,
            None,
            None,
            Some(&regime),
            None, // no cash cap
        );
        // Base qty 10, Volatile multiplier 0.5 -> 5
        assert_eq!(qty, dec!(5));
    }

    #[test]
    fn test_cash_cap_limits_quantity() {
        let spread_cache = Arc::new(SpreadCache::new());
        spread_cache.update("BTC/USD".to_string(), 100.00, 100.05);
        let engine = SizingEngine::new(spread_cache);
        let config = create_test_config();

        // Equity 100k, risk 1% = $1000 target, price $100 -> 10 shares
        // But only $500 cash available -> capped to 5 shares
        let qty = engine.calculate_quantity_with_slippage(
            &config,
            dec!(100000),
            dec!(100),
            "BTC/USD",
            None,
            None,
            None,
            None,
            Some(dec!(500)),
        );
        assert_eq!(qty, dec!(5));
    }
}
