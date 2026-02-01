use rust_decimal::Decimal;

use std::sync::Arc;
use tracing::info;

use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::risk_management::volatility::calculate_realized_volatility;

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

pub struct SizingEngine {
    spread_cache: Arc<SpreadCache>,
}

use rust_decimal_macros::dec;

impl SizingEngine {
    pub fn new(spread_cache: Arc<SpreadCache>) -> Self {
        Self { spread_cache }
    }

    /// Calculate quantity with slippage adjustment based on bid-ask spread
    /// and optionally volatility targeting
    pub fn calculate_quantity_with_slippage(
        &self,
        config: &SizingConfig,
        total_equity: Decimal,
        price: Decimal,
        symbol: &str,
        recent_prices: Option<&[f64]>, // Keep f64 for volatility calc for now as it uses ta crate
    ) -> Decimal {
        let mut base_qty = Self::calculate_quantity(config, total_equity, price, symbol);

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
                let slippage_multiplier = (dec!(0.002) / spread_pct).min(Decimal::ONE);
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

    pub fn calculate_quantity(
        config: &SizingConfig,
        total_equity: Decimal,
        price: Decimal,
        symbol: &str,
    ) -> Decimal {
        // Fallback to static quantity if risk sizing is disabled
        if config.risk_per_trade_percent <= Decimal::ZERO {
            info!(
                "SizingEngine: Using static quantity for {}: {}",
                symbol, config.static_trade_quantity
            );
            return config.static_trade_quantity;
        }

        if total_equity <= Decimal::ZERO || price <= Decimal::ZERO {
            info!(
                "SizingEngine: Cannot calculate quantity for {} - TotalEquity={}, Price={}",
                symbol, total_equity, price
            );
            return Decimal::ZERO;
        }

        // 1. Calculate the target amount to allocate based on risk_per_trade_percent
        let mut target_amt = total_equity * config.risk_per_trade_percent;

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

        // 3. Convert to Shares
        let quantity = (target_amt / price).round_dp(4);

        info!(
            "SizingEngine: Final quantity for {}: {} shares (${} / ${} per share)",
            symbol, quantity, target_amt, price
        );

        quantity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::market_data::spread_cache::SpreadCache;
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
        );

        assert_eq!(qty, dec!(5));
    }
}
