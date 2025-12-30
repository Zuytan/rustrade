use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct FeeConfig {
    pub maker_fee: Decimal, // e.g., 0.001 (0.1%)
    pub taker_fee: Decimal,
    pub slippage_pct: Decimal,     // e.g., 0.0005 (0.05%)
    pub commission_fixed: Decimal, // e.g., 0.0 (for crypto) or 0.005 (for stocks per share)
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            maker_fee: Decimal::new(1, 3),    // 0.001
            taker_fee: Decimal::new(1, 3),    // 0.001
            slippage_pct: Decimal::new(5, 4), // 0.0005
            commission_fixed: Decimal::ZERO,
        }
    }
}

pub trait FeeModel: Send + Sync {
    fn calculate_entry_cost(&self, price: Decimal, quantity: Decimal) -> Decimal;
    fn calculate_exit_cost(&self, price: Decimal, quantity: Decimal) -> Decimal;
    fn estimate_total_cost(&self, price: Decimal, quantity: Decimal) -> Decimal;
}

pub struct StandardFeeModel {
    config: FeeConfig,
}

impl StandardFeeModel {
    pub fn new(config: FeeConfig) -> Self {
        Self { config }
    }
}

impl FeeModel for StandardFeeModel {
    fn calculate_entry_cost(&self, price: Decimal, quantity: Decimal) -> Decimal {
        let value = price * quantity;
        let fee = value * self.config.taker_fee; // Assume taker for immediate entry
        let slippage = value * self.config.slippage_pct;
        let commission = self.config.commission_fixed * quantity;
        fee + slippage + commission
    }

    fn calculate_exit_cost(&self, price: Decimal, quantity: Decimal) -> Decimal {
        let value = price * quantity;
        // Exit might be limit (maker) or market (taker). Assume taker for safety (worst case)
        let fee = value * self.config.taker_fee;
        let slippage = value * self.config.slippage_pct;
        let commission = self.config.commission_fixed * quantity;
        fee + slippage + commission
    }

    fn estimate_total_cost(&self, price: Decimal, quantity: Decimal) -> Decimal {
        // Round trip cost estimation
        self.calculate_entry_cost(price, quantity) + self.calculate_exit_cost(price, quantity)
    }
}
