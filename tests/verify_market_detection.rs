use rust_decimal_macros::dec;

use rust_decimal::Decimal;

use rustrade::application::agents::analyst_config::AnalystConfig;
use rustrade::application::monitoring::feature_engineering_service::TechnicalFeatureEngineeringService;
use rustrade::domain::ports::FeatureEngineeringService;
use rustrade::domain::trading::types::Candle;

fn create_candle(price: f64, volatility: f64) -> Candle {
    Candle {
        symbol: "TEST".to_string(),
        open: Decimal::from_f64_retain(price).unwrap(),
        high: Decimal::from_f64_retain(price + volatility).unwrap(),
        low: Decimal::from_f64_retain(price - volatility).unwrap(),
        close: Decimal::from_f64_retain(price).unwrap(),
        volume: Decimal::new(1000, 0),
        timestamp: 1000,
    }
}

#[test]
fn test_atr_reacts_to_volatility_without_price_change() {
    let config = AnalystConfig::default();
    let mut service = TechnicalFeatureEngineeringService::new(&config);

    // Warm up
    for _ in 0..20 {
        service.update(&create_candle(100.0, 1.0));
    }

    // Now insert a VERY volatile candle but with SAME close price
    // High=110, Low=90, Close=100. TR should be 20.
    // If bug exists (using close only), TR will be close-prev_close = 0.
    let volatile_candle = create_candle(100.0, 10.0);

    let features = service.update(&volatile_candle);

    let atr = features.atr.unwrap();
    println!("ATR after volatile event: {}", atr);

    // Previous ATR was approx 2.0 (High-Low = 2.0 for volatility 1.0)
    // New TR is 20.0.
    // ATR should jump significantly.
    // With 14 period, (2.0 * 13 + 20) / 14 = (26+20)/14 = 3.28

    assert!(
        atr > dec!(2.5),
        "ATR should reflect the increased volatility (Expected > 2.5, Got {})",
        atr
    );
}
