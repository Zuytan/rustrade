use rustrade::domain::market::{Candle, MarketEvent};
use rustrade::domain::types::Decimal;
use rustrade::application::agents::risk_manager::RiskManager;

#[cfg(test)]
mod stress_tests {
    use super::*;

    /// Simulates a -10% flash crash in 1 minute
    #[tokio::test]
    async fn test_flash_crash_resilience() {
        // Setup System
        let (mut risk_manager, mut rx) = create_test_system();
        
        // Feed crash data
        let start_price = Decimal::from(50000);
        let mut price = start_price;
        
        for _ in 0..60 {
            // Drop 0.2% per second (~12% total)
            price = price * Decimal::from_f64_retain(0.998).unwrap();
            risk_manager.handle_tick(price).await;
            
            // Assert Circuit Breaker trips at -5%
            if price < start_price * Decimal::from_f64_retain(0.95).unwrap() {
                assert!(risk_manager.circuit_breaker_active(), "Circuit breaker should toggle ON");
            }
        }
    }
}
