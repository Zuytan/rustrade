use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArbitrageOpportunity {
    pub symbol: String,
    pub buy_exchange: String,
    pub buy_price: Decimal,
    pub sell_exchange: String,
    pub sell_price: Decimal,
    pub gross_spread: Decimal,
    pub net_spread: Decimal,
    pub estimated_profit: Decimal,
}

pub struct ArbitrageEngine {
    pub min_spread_bps: Decimal, // Minimum spread in basis points (1 bps = 0.0001)
}

impl ArbitrageEngine {
    pub fn new(min_spread_bps: Decimal) -> Self {
        Self { min_spread_bps }
    }

    pub fn check_opportunity(
        &self,
        symbol: &str,
        prices: &std::collections::HashMap<String, Decimal>, // exchange -> price
        fees: &std::collections::HashMap<String, Decimal>, // exchange -> transaction fee pct (e.g. 0.001)
    ) -> Option<ArbitrageOpportunity> {
        if prices.len() < 2 {
            return None;
        }

        let mut best_buy: Option<(&String, Decimal)> = None;
        let mut best_sell: Option<(&String, Decimal)> = None;

        for (exchange, &price) in prices {
            if price <= Decimal::ZERO {
                continue;
            }

            if best_buy.is_none() || price < best_buy.unwrap().1 {
                best_buy = Some((exchange, price));
            }
            if best_sell.is_none() || price > best_sell.unwrap().1 {
                best_sell = Some((exchange, price));
            }
        }

        if let (Some((buy_ex, buy_price)), Some((sell_ex, sell_price))) = (best_buy, best_sell) {
            if buy_ex == sell_ex {
                return None;
            }

            let gross_spread = sell_price - buy_price;
            if gross_spread <= Decimal::ZERO {
                return None;
            }

            // Calculate fees
            let buy_fee_pct = fees.get(buy_ex).copied().unwrap_or(Decimal::ZERO);
            let sell_fee_pct = fees.get(sell_ex).copied().unwrap_or(Decimal::ZERO);

            let buy_fee = buy_price * buy_fee_pct;
            let sell_fee = sell_price * sell_fee_pct;
            let total_fees = buy_fee + sell_fee;

            let net_spread = gross_spread - total_fees;

            // Check if net spread meets minimum basis points requirement
            let min_profit = buy_price * (self.min_spread_bps * Decimal::new(1, 4)); // bps to multiplier

            if net_spread >= min_profit && net_spread > Decimal::ZERO {
                return Some(ArbitrageOpportunity {
                    symbol: symbol.to_string(),
                    buy_exchange: buy_ex.clone(),
                    buy_price,
                    sell_exchange: sell_ex.clone(),
                    sell_price,
                    gross_spread,
                    net_spread,
                    estimated_profit: net_spread,
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    #[test]
    fn test_arbitrage_detection() {
        let engine = ArbitrageEngine::new(dec!(10)); // 10 bps minimum

        let mut prices = HashMap::new();
        prices.insert("Alpaca".to_string(), dec!(100));
        prices.insert("Binance".to_string(), dec!(101.5));

        let mut fees = HashMap::new();
        fees.insert("Alpaca".to_string(), dec!(0.001)); // 0.1% fee
        fees.insert("Binance".to_string(), dec!(0.001)); // 0.1% fee

        let opp = engine.check_opportunity("BTC/USD", &prices, &fees).unwrap();

        assert_eq!(opp.buy_exchange, "Alpaca");
        assert_eq!(opp.sell_exchange, "Binance");
        assert_eq!(opp.gross_spread, dec!(1.5));
        // fees = 100 * 0.001 (0.1) + 101.5 * 0.001 (0.1015) = 0.2015
        // net spread = 1.5 - 0.2015 = 1.2985
        assert_eq!(opp.net_spread, dec!(1.2985));
    }

    #[test]
    fn test_arbitrage_insufficient_spread() {
        let engine = ArbitrageEngine::new(dec!(50)); // 50 bps minimum

        let mut prices = HashMap::new();
        prices.insert("Alpaca".to_string(), dec!(100));
        prices.insert("Binance".to_string(), dec!(100.2)); // Very small spread

        let mut fees = HashMap::new();
        fees.insert("Alpaca".to_string(), dec!(0.001));
        fees.insert("Binance".to_string(), dec!(0.001));

        let opp = engine.check_opportunity("BTC/USD", &prices, &fees);
        assert!(opp.is_none());
    }
}
