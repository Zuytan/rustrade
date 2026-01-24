use rust_decimal::Decimal;

use std::sync::Arc;
use tracing::info;

use crate::application::market_data::spread_cache::SpreadCache;
use crate::application::risk_management::volatility::calculate_realized_volatility;

#[derive(Debug, Clone)]
pub struct SizingConfig {
    pub risk_per_trade_percent: f64,
    pub max_positions: usize,
    pub max_position_size_pct: f64,
    pub static_trade_quantity: Decimal,
    // NEW: Volatility Targeting
    pub enable_vol_targeting: bool,
    pub target_volatility: f64, // Target annualized volatility (e.g., 0.15 = 15%)
}

pub struct SizingEngine {
    spread_cache: Arc<SpreadCache>,
}

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
        recent_prices: Option<&[f64]>, // NEW: for volatility calculation
    ) -> Decimal {
        let mut base_qty = Self::calculate_quantity(config, total_equity, price, symbol);

        // NEW: Apply volatility targeting if enabled
        if config.enable_vol_targeting {
            #[allow(clippy::collapsible_if)]
            if let Some(prices) = recent_prices {
                if let Some(realized_vol) = calculate_realized_volatility(prices, 252.0) {
                    if realized_vol > 0.0 {
                        // Vol targeting: scale position inversely to volatility
                        // If realized_vol = 20% and target = 15%, multiplier = 15/20 = 0.75
                        let vol_multiplier = config.target_volatility / realized_vol;
                        let vol_multiplier = vol_multiplier.clamp(0.25, 2.0); // Limit to 25%-200%

                        info!(
                            "SizingEngine: Vol targeting for {} - Realized: {:.2}%, Target: {:.2}%, Multiplier: {:.2}x",
                            symbol,
                            realized_vol * 100.0,
                            config.target_volatility * 100.0,
                            vol_multiplier
                        );

                        base_qty *=
                            Decimal::from_f64_retain(vol_multiplier).unwrap_or(Decimal::ONE);

                        base_qty = base_qty.round_dp(4);
                    }
                }
            }
        }

        // Apply slippage adjustment
        if let Some(spread_pct) = self.spread_cache.get_spread_pct(symbol) {
            // Reduce size if spread is wide (>20 bps = 0.002 = 0.2%)
            // If spread is 0.5% (0.005), multiplier = 0.002 / 0.005 = 0.4 (40% of size)
            // Cap reduction to avoid tiny sizes? No, cap multiplier at 1.0.
            let slippage_multiplier = if spread_pct > 0.002 {
                let m = (0.002 / spread_pct).min(1.0);
                info!(
                    "SizingEngine: High spread detected for {} ({:.4}%), reducing size by factor {:.2}",
                    symbol,
                    spread_pct * 100.0,
                    m
                );
                m
            } else {
                1.0 // No adjustment for tight spreads
            };

            if slippage_multiplier < 1.0 {
                let adjusted_qty = base_qty
                    * Decimal::from_f64_retain(slippage_multiplier).unwrap_or(Decimal::ONE);
                return adjusted_qty.round_dp(4);
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
        if config.risk_per_trade_percent <= 0.0 {
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
        let mut target_amt = total_equity
            * Decimal::from_f64_retain(config.risk_per_trade_percent).unwrap_or(Decimal::ZERO);

        info!(
            "SizingEngine: Initial target amount for {} ({}% of equity): ${}",
            symbol,
            config.risk_per_trade_percent * 100.0,
            target_amt
        );

        // 2. Apply Caps for diversification

        // Cap 1: Max Positions bucket (if max_positions > 0)
        // Ensure we don't use more than 1/Nth of the portfolio
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
        if config.max_position_size_pct > 0.0 {
            let max_pos_val = total_equity
                * Decimal::from_f64_retain(config.max_position_size_pct).unwrap_or(Decimal::ZERO);
            let before = target_amt;
            target_amt = target_amt.min(max_pos_val);
            if target_amt < before {
                info!(
                    "SizingEngine: Capped {} by max_position_size_pct ({}%): ${} -> ${}",
                    symbol,
                    config.max_position_size_pct * 100.0,
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
            risk_per_trade_percent: 0.01,
            max_positions: 5,
            max_position_size_pct: 0.20,
            static_trade_quantity: dec!(1.0),
            enable_vol_targeting: false,
            target_volatility: 0.15,
        }
    }

    #[test]
    fn test_calculate_quantity_normal_spread() {
        let spread_cache = Arc::new(SpreadCache::new());
        // Update cache with normal spread (5 bps = 0.05%)
        // Bid: 100.00, Ask: 100.05
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
        // Use Bid 99.75, Ask 100.25 => Mid 100.00 => Spread 0.50 => 0.50/100 = 0.005 (0.5%)
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
    fn test_calculate_quantity_no_spread_data() {
        let spread_cache = Arc::new(SpreadCache::new());
        // No data in cache

        let engine = SizingEngine::new(spread_cache);
        let config = create_test_config();

        let qty = engine.calculate_quantity_with_slippage(
            &config,
            dec!(100000),
            dec!(100),
            "BTC/USD",
            None,
        );

        // Should be base quantity (10)
        assert_eq!(qty, dec!(10));
    }

    #[test]
    fn test_calculate_quantity_static_fallback() {
        let spread_cache = Arc::new(SpreadCache::new());
        let engine = SizingEngine::new(spread_cache);

        let mut config = create_test_config();
        config.risk_per_trade_percent = 0.0; // Disable risk sizing
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
