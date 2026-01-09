use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::domain::ports::SectorProvider;
use crate::domain::risk::filters::validator_trait::{RiskValidator, ValidationContext, ValidationResult};
use crate::domain::trading::types::OrderSide;

/// Configuration for sector exposure validator
#[derive(Clone)]
pub struct SectorExposureConfig {
    /// Maximum exposure per sector as percentage of equity (e.g., 0.30 = 30%)
    pub max_sector_exposure_pct: f64,
    
    /// Optional provider for sector data
    pub sector_provider: Option<Arc<dyn SectorProvider>>,
}

impl Default for SectorExposureConfig {
    fn default() -> Self {
        Self {
            max_sector_exposure_pct: 0.30,
            sector_provider: None,
        }
    }
}

/// Validates that portfolio exposure to a single sector doesn't exceed limits
/// 
/// This validator prevents over-concentration in specific market sectors (e.g., "Technology", "Energy").
/// It maintains a local cache of symbol->sector mappings to minimize API calls.
pub struct SectorExposureValidator {
    config: SectorExposureConfig,
    /// Cache for symbol -> sector lookups to avoid repeated API calls
    sector_cache: Arc<Mutex<HashMap<String, String>>>,
}

impl SectorExposureValidator {
    pub fn new(config: SectorExposureConfig) -> Self {
        Self {
            config,
            sector_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Helper to get sector for a symbol (using cache or provider)
    async fn get_sector(&self, symbol: &str) -> String {
        // 1. Try cache first
        {
            let cache = self.sector_cache.lock().unwrap();
            if let Some(sector) = cache.get(symbol) {
                return sector.clone();
            }
        }

        // 2. Use provider if available
        if let Some(provider) = &self.config.sector_provider {
            let sector = provider
                .get_sector(symbol)
                .await
                .unwrap_or_else(|_| "Unknown".to_string());
            
            // Update cache
            let mut cache = self.sector_cache.lock().unwrap();
            cache.insert(symbol.to_string(), sector.clone());
            return sector;
        }

        "Unknown".to_string()
    }
}

#[async_trait]
impl RiskValidator for SectorExposureValidator {
    fn name(&self) -> &str {
        "SectorExposureValidator"
    }

    async fn validate(&self, ctx: &ValidationContext<'_>) -> ValidationResult {
        // Only Buy orders increase exposure
        if !matches!(ctx.proposal.side, OrderSide::Buy) {
            return ValidationResult::Approve;
        }

        if ctx.current_equity <= Decimal::ZERO {
            return ValidationResult::Approve;
        }

        // 1. Identify Target Sector
        let target_sector = self.get_sector(&ctx.proposal.symbol).await;
        if target_sector == "Unknown" {
            // Cannot validate unknown sectors, defaulting to Approve
            // (or could be strict and Reject, but usually we allow unknowns)
            return ValidationResult::Approve;
        }

        // 2. Calculate Current Sector Exposure
        let mut current_sector_value = Decimal::ZERO;

        for (sym, position) in &ctx.portfolio.positions {
            // Optimization: if symbol is same as proposal, we already know the sector
            let pos_sector = if sym == &ctx.proposal.symbol {
                target_sector.clone()
            } else {
                self.get_sector(sym).await
            };

            if pos_sector == target_sector {
                // Use current market price if available, otherwise cost basis
                let price = ctx.current_prices
                    .get(sym)
                    .cloned()
                    .unwrap_or(position.average_price);
                current_sector_value += price * position.quantity;
            }
        }

        // 3. Add Proposed Trade Value
        let trade_value = ctx.calculate_proposal_exposure();
        let new_sector_value = current_sector_value + trade_value;

        // 4. Calculate Percentage and Validate
        let new_sector_pct = (new_sector_value / ctx.current_equity)
            .to_f64()
            .unwrap_or(0.0);

        if new_sector_pct > self.config.max_sector_exposure_pct {
            return ValidationResult::Reject(format!(
                "Sector exposure limit exceeded for {}. Sector: {}, New Exposure: {:.2}% (Limit: {:.2}%)",
                ctx.proposal.symbol,
                target_sector,
                new_sector_pct * 100.0,
                self.config.max_sector_exposure_pct * 100.0
            ));
        }

        ValidationResult::Approve
    }

    fn priority(&self) -> u8 {
        30 // Medium priority (after Pdt, before Sentiment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::risk::state::RiskState;
    use crate::domain::trading::portfolio::{Portfolio, Position};
    use crate::domain::trading::types::{OrderType, TradeProposal};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    // Mock provider
    struct MockSectorProvider {
        sectors: HashMap<String, String>,
    }

    #[async_trait]
    impl SectorProvider for MockSectorProvider {
        async fn get_sector(&self, symbol: &str) -> std::result::Result<String, anyhow::Error> {
            Ok(self.sectors.get(symbol).cloned().unwrap_or("Unknown".to_string()))
        }
    }

    fn create_test_proposal(symbol: &str) -> TradeProposal {
        TradeProposal {
            symbol: symbol.to_string(),
            side: OrderSide::Buy,
            price: dec!(100),
            quantity: dec!(10), // Value = $1000
            order_type: OrderType::Market,
            reason: "test".to_string(),
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_approve_when_no_provider() {
        let validator = SectorExposureValidator::new(SectorExposureConfig::default());
        let proposal = create_test_proposal("AAPL");
        let portfolio = Portfolio::new();
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(&proposal,  &portfolio, dec!(10000), &prices, &risk_state, None, None, None, Decimal::ZERO, dec!(10000));

        // Without provider, sector is "Unknown", so approves
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_approve_low_exposure() {
        let mut sectors = HashMap::new();
        sectors.insert("AAPL".to_string(), "Tech".to_string());
        let provider = Arc::new(MockSectorProvider { sectors });

        let validator = SectorExposureValidator::new(SectorExposureConfig {
            max_sector_exposure_pct: 0.20, // 20% limit
            sector_provider: Some(provider),
        });

        let proposal = create_test_proposal("AAPL"); // $1000 value
        let portfolio = Portfolio::new(); // Empty portfolio
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(&proposal, &portfolio, dec!(10000), &prices, &risk_state, None, None, None, Decimal::ZERO, dec!(10000));

        // Exposure: $1000/$10000 = 10% (Limit 20%) -> Approve
        let result = validator.validate(&ctx).await;
        assert!(result.is_approved());
    }

    #[tokio::test]
    async fn test_reject_high_exposure() {
        let mut sectors = HashMap::new();
        sectors.insert("AAPL".to_string(), "Tech".to_string());
        let provider = Arc::new(MockSectorProvider { sectors });

        let validator = SectorExposureValidator::new(SectorExposureConfig {
            max_sector_exposure_pct: 0.10, // 10% limit
            sector_provider: Some(provider),
        });

        let proposal = create_test_proposal("AAPL"); // $1000 value
        let portfolio = Portfolio::new();
        // Assume equity $5000. 10% limit = $500.
        // Proposal $1000 > $500 -> Reject
        // Note: Equity isn't derived from portfolio in context, it's passed explicitly
        
        let prices = HashMap::new();
        let risk_state = RiskState::default();

        let ctx = ValidationContext::new(&proposal, &portfolio, dec!(5000), &prices, &risk_state, None, None, None, Decimal::ZERO, dec!(5000));

        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("Sector exposure limit exceeded"));
    }

    #[tokio::test]
    async fn test_accumulate_sector_exposure() {
        let mut sectors = HashMap::new();
        sectors.insert("AAPL".to_string(), "Tech".to_string());
        sectors.insert("MSFT".to_string(), "Tech".to_string());
        let provider = Arc::new(MockSectorProvider { sectors });

        let validator = SectorExposureValidator::new(SectorExposureConfig {
            max_sector_exposure_pct: 0.30, // 30% limit
            sector_provider: Some(provider),
        });

        let proposal = create_test_proposal("AAPL"); // $1000 value
        let mut portfolio = Portfolio::new();
        
        // Already hold MSFT ($2500)
        portfolio.positions.insert("MSFT".to_string(), Position {
            symbol: "MSFT".to_string(),
            quantity: dec!(25),
            average_price: dec!(100),
        });

        let mut prices = HashMap::new();
        prices.insert("MSFT".to_string(), dec!(100)); // Current price matches cost

        let risk_state = RiskState::default();
        let ctx = ValidationContext::new(&proposal, &portfolio, dec!(10000), &prices, &risk_state, None, None, None, Decimal::ZERO, dec!(10000));

        // Current Sector Exp: $2500 (MSFT)
        // New Trade: $1000 (AAPL)
        // Total Tech: $3500
        // Equity: $10000 -> 35% exposure
        // Limit: 30% -> Reject
        let result = validator.validate(&ctx).await;
        assert!(result.is_rejected());
        assert!(result.rejection_reason().unwrap().contains("35.00%"));
    }
}
