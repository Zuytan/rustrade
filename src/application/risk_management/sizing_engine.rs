use rust_decimal::Decimal;

use tracing::info;

#[derive(Debug, Clone)]
pub struct SizingConfig {
    pub risk_per_trade_percent: f64,
    pub max_positions: usize,
    pub max_position_size_pct: f64,
    pub static_trade_quantity: Decimal,
}

pub struct SizingEngine;

impl SizingEngine {
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
