//! Common types and constants for Binance infrastructure

#[cfg(test)]
mod tests {
    use crate::domain::trading::types::{denormalize_crypto_symbol, normalize_crypto_symbol};

    #[test]
    fn test_binance_symbol_denormalization() {
        assert_eq!(denormalize_crypto_symbol("BTC/USDT"), "BTCUSDT");
        assert_eq!(denormalize_crypto_symbol("ETH/USDT"), "ETHUSDT");
        assert_eq!(denormalize_crypto_symbol("AVAX/USDT"), "AVAXUSDT");
    }

    #[test]
    fn test_binance_symbol_normalization() {
        assert_eq!(normalize_crypto_symbol("BTCUSDT").unwrap(), "BTC/USDT");
        assert_eq!(normalize_crypto_symbol("ETHUSDT").unwrap(), "ETH/USDT");
        assert_eq!(normalize_crypto_symbol("BNBUSDT").unwrap(), "BNB/USDT");
    }
}
