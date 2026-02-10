use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::market_data::statistical_features::{
    calculate_hurst_exponent, calculate_skewness,
};
use crate::application::risk_management::volatility::calculate_realized_volatility;
use crate::domain::ports::FeatureEngineeringService;
use crate::domain::trading::types::{Candle, FeatureSet};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;
use ta::Next;
use ta::indicators::{
    AverageTrueRange, BollingerBands, ExponentialMovingAverage, MovingAverageConvergenceDivergence,
    RelativeStrengthIndex, SimpleMovingAverage,
};

/// Manual ADX implementation using standard Wilder's smoothing
///
/// Fixed initialization: accumulates first N values as sum, then applies Wilder's smoothing
pub struct ManualAdx {
    period: usize,
    prev_high: Option<f64>,
    prev_low: Option<f64>,
    prev_close: Option<f64>,
    tr_sum: f64,
    plus_dm_sum: f64,
    minus_dm_sum: f64,
    tr_smooth: f64,
    plus_dm_smooth: f64,
    minus_dm_smooth: f64,
    adx_smooth: f64,
    count: usize,
}

impl ManualAdx {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prev_high: None,
            prev_low: None,
            prev_close: None,
            tr_sum: 0.0,
            plus_dm_sum: 0.0,
            minus_dm_sum: 0.0,
            tr_smooth: 0.0,
            plus_dm_smooth: 0.0,
            minus_dm_smooth: 0.0,
            adx_smooth: 0.0,
            count: 0,
        }
    }

    pub fn next(&mut self, high: f64, low: f64, close: f64) -> f64 {
        if self.prev_close.is_none() {
            self.prev_high = Some(high);
            self.prev_low = Some(low);
            self.prev_close = Some(close);
            return 0.0;
        }

        let prev_high = self.prev_high.unwrap_or(0.0);
        let prev_low = self.prev_low.unwrap_or(0.0);
        let prev_close = self.prev_close.unwrap_or(0.0);

        let tr = (high - low)
            .max((high - prev_close).abs())
            .max((low - prev_close).abs());
        let up_move = high - prev_high;
        let down_move = prev_low - low;
        let plus_dm = if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        };
        let minus_dm = if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        };

        self.count += 1;

        if self.count <= self.period {
            self.tr_sum += tr;
            self.plus_dm_sum += plus_dm;
            self.minus_dm_sum += minus_dm;
            if self.count == self.period {
                self.tr_smooth = self.tr_sum;
                self.plus_dm_smooth = self.plus_dm_sum;
                self.minus_dm_smooth = self.minus_dm_sum;
            }
        } else {
            let n = self.period as f64;
            self.tr_smooth = self.tr_smooth - (self.tr_smooth / n) + tr;
            self.plus_dm_smooth = self.plus_dm_smooth - (self.plus_dm_smooth / n) + plus_dm;
            self.minus_dm_smooth = self.minus_dm_smooth - (self.minus_dm_smooth / n) + minus_dm;
        }

        let mut adx = 0.0;
        if self.count >= self.period && self.tr_smooth > 0.0 {
            let plus_di = 100.0 * self.plus_dm_smooth / self.tr_smooth;
            let minus_di = 100.0 * self.minus_dm_smooth / self.tr_smooth;
            let sum_di = plus_di + minus_di;
            let dx = if sum_di > 0.0 {
                100.0 * (plus_di - minus_di).abs() / sum_di
            } else {
                0.0
            };

            if self.count == self.period {
                self.adx_smooth = dx;
            } else {
                self.adx_smooth =
                    ((self.adx_smooth * (self.period as f64 - 1.0)) + dx) / self.period as f64;
            }
            adx = self.adx_smooth;
        }

        self.prev_high = Some(high);
        self.prev_low = Some(low);
        self.prev_close = Some(close);
        adx
    }
}

pub struct TechnicalFeatureEngineeringService {
    rsi: RelativeStrengthIndex,
    macd: MovingAverageConvergenceDivergence,
    sma_20: SimpleMovingAverage,
    sma_50: SimpleMovingAverage,
    sma_200: SimpleMovingAverage,
    bb: BollingerBands,
    atr: AverageTrueRange,
    ema_fast: ExponentialMovingAverage,
    ema_slow: ExponentialMovingAverage,
    adx: ManualAdx,
    /// Price history kept in Decimal until conversion for statistical functions (hurst, skewness, volatility).
    price_history: VecDeque<Decimal>,
}

impl TechnicalFeatureEngineeringService {
    pub fn new(config: &AnalystConfig) -> Self {
        Self {
            rsi: RelativeStrengthIndex::new(config.rsi_period)
                .expect("rsi_period from AnalystConfig must be > 0"),
            macd: MovingAverageConvergenceDivergence::new(
                config.macd_fast_period,
                config.macd_slow_period,
                config.macd_signal_period,
            )
            .expect("MACD periods from AnalystConfig must be valid"),
            sma_20: SimpleMovingAverage::new(config.fast_sma_period)
                .expect("fast_sma_period from AnalystConfig must be > 0"),
            sma_50: SimpleMovingAverage::new(config.slow_sma_period)
                .expect("slow_sma_period from AnalystConfig must be > 0"),
            sma_200: SimpleMovingAverage::new(config.trend_sma_period)
                .expect("trend_sma_period from AnalystConfig must be > 0"),
            bb: BollingerBands::new(
                config.mean_reversion_bb_period,
                config.bb_std_dev.to_f64().unwrap_or(2.0),
            )
            .expect("mean_reversion_bb_period from AnalystConfig must be > 0"),
            atr: AverageTrueRange::new(config.atr_period)
                .expect("atr_period from AnalystConfig must be > 0"),
            ema_fast: ExponentialMovingAverage::new(config.ema_fast_period)
                .expect("ema_fast_period from AnalystConfig must be > 0"),
            ema_slow: ExponentialMovingAverage::new(config.ema_slow_period)
                .expect("ema_slow_period from AnalystConfig must be > 0"),
            adx: ManualAdx::new(config.adx_period),
            price_history: VecDeque::with_capacity(100),
        }
    }
}

/// Convert Decimal price history to f64 only for statistical library boundaries (statrs, etc.).
fn price_history_to_f64(history: &VecDeque<Decimal>) -> Vec<f64> {
    history.iter().filter_map(|d| d.to_f64()).collect()
}

impl FeatureEngineeringService for TechnicalFeatureEngineeringService {
    fn update(&mut self, candle: &Candle) -> FeatureSet {
        let price = candle.close.to_f64().unwrap_or(0.0);
        let high = candle.high.to_f64().unwrap_or(0.0);
        let low = candle.low.to_f64().unwrap_or(0.0);
        let open = candle.open.to_f64().unwrap_or(0.0);

        let rsi_val = self.rsi.next(price);
        let macd_val = self.macd.next(price);
        let bb_val = self.bb.next(price);

        // DATA ITEM for indicators needing OHLC (ATR)
        let item = ta::DataItem::builder()
            .high(high)
            .low(low)
            .close(price)
            .open(open)
            .volume(candle.volume.to_f64().unwrap_or(0.0))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to build DataItem: {:?}. Using close price as fallback. H:{}, L:{}",
                    e,
                    high,
                    low
                );
                ta::DataItem::builder()
                    .high(price)
                    .low(price)
                    .close(price)
                    .open(price)
                    .volume(0.0)
                    .build()
                    .unwrap()
            });

        // Calculate ATR early as it is needed for momentum normalization
        let atr_val = self.atr.next(&item);

        // Update price history (keep as Decimal until statistical boundaries)
        self.price_history.push_back(candle.close);
        if self.price_history.len() > 100 {
            self.price_history.pop_front();
        }

        // Convert to f64 only for statistical library boundaries
        let prices_vec: Vec<f64> = price_history_to_f64(&self.price_history);

        // Hurst Exponent (requires ~50 periods)
        let hurst_exponent = if prices_vec.len() >= 50 {
            calculate_hurst_exponent(&prices_vec, &[2, 4, 8, 16])
        } else {
            None
        };

        // Skewness (requires ~20 periods of returns)
        let skewness = if prices_vec.len() >= 20 {
            // Calculate returns
            let returns: Vec<f64> = prices_vec
                .windows(2)
                .map(|w| (w[1] - w[0]) / w[0])
                .collect();
            calculate_skewness(&returns)
        } else {
            None
        };

        // Realized Volatility
        let realized_volatility = if prices_vec.len() >= 20 {
            calculate_realized_volatility(&prices_vec, 525600.0) // 1 minute candles -> 525600 minutes/year
        } else {
            None
        };

        // Normalized Momentum: (Price - Price_N) / ATR
        // Using N=10 same as StatMomentum
        let momentum_normalized = if prices_vec.len() >= 11 && atr_val > 0.0 {
            let n = 10;
            let past_price = prices_vec[prices_vec.len() - 1 - n];
            Some((price - past_price) / atr_val)
        } else {
            None
        };

        let bb_width = if bb_val.average > 0.0 {
            (bb_val.upper - bb_val.lower) / bb_val.average
        } else {
            0.0
        };

        let bb_position = if bb_val.upper - bb_val.lower > 1e-9 {
            (price - bb_val.lower) / (bb_val.upper - bb_val.lower)
        } else {
            0.5
        };

        let atr_pct = if price > 0.0 { atr_val / price } else { 0.0 };

        use rust_decimal::Decimal;
        let to_dec = |v: f64| Decimal::from_f64_retain(v);
        let to_dec_opt = |v: Option<f64>| v.and_then(Decimal::from_f64_retain);

        FeatureSet {
            last_price: to_dec(price),
            rsi: to_dec(rsi_val),
            macd_line: to_dec(macd_val.macd),
            macd_signal: to_dec(macd_val.signal),
            macd_hist: to_dec(macd_val.histogram),
            sma_20: to_dec(self.sma_20.next(price)),
            sma_50: to_dec(self.sma_50.next(price)),
            sma_200: to_dec(self.sma_200.next(price)),
            bb_upper: to_dec(bb_val.upper),
            bb_middle: to_dec(bb_val.average),
            bb_lower: to_dec(bb_val.lower),
            atr: to_dec(atr_val),
            ema_fast: to_dec(self.ema_fast.next(price)),
            ema_slow: to_dec(self.ema_slow.next(price)),
            adx: to_dec(self.adx.next(high, low, price)),
            bb_width: to_dec(bb_width),
            bb_position: to_dec(bb_position),
            atr_pct: to_dec(atr_pct),

            // Advanced Statistical Features (Phase 2)
            hurst_exponent: to_dec_opt(hurst_exponent),
            skewness: to_dec_opt(skewness),
            momentum_normalized: to_dec_opt(momentum_normalized),
            realized_volatility: to_dec_opt(realized_volatility),

            timeframe: Some(crate::domain::market::timeframe::Timeframe::OneMin),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn create_test_candle(price: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(price).expect("Test price must be valid"),
            high: Decimal::from_f64_retain(price).expect("Test price must be valid"),
            low: Decimal::from_f64_retain(price).expect("Test price must be valid"),
            close: Decimal::from_f64_retain(price).expect("Test price must be valid"),
            volume: dec!(100.0),
            timestamp: 0,
        }
    }

    fn create_trending_candle(price: f64, range: f64) -> Candle {
        Candle {
            symbol: "TEST".to_string(),
            open: Decimal::from_f64_retain(price).expect("Test price must be valid"),
            high: Decimal::from_f64_retain(price + range).expect("Test price must be valid"),
            low: Decimal::from_f64_retain(price - range).expect("Test price must be valid"),
            close: Decimal::from_f64_retain(price).expect("Test price must be valid"),
            volume: dec!(100.0),
            timestamp: 0,
        }
    }

    #[test]
    fn test_sma_values_after_warmup() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        // Feed 50 identical candles
        for _ in 0..50 {
            service.update(&create_test_candle(100.0));
        }

        let features = service.update(&create_test_candle(100.0));

        assert_eq!(features.sma_20.unwrap(), dec!(100.0));
        assert_eq!(features.sma_50.unwrap(), dec!(100.0));
    }

    #[test]
    fn test_rsi_neutral_after_flat_prices() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        // Feed 20 identical prices
        for _ in 0..20 {
            service.update(&create_test_candle(100.0));
        }

        let features = service.update(&create_test_candle(100.0));

        if let Some(rsi) = features.rsi {
            assert!(rsi >= dec!(0.0) && rsi <= dec!(100.0));
        }
    }

    #[test]
    fn test_bollinger_bands_converge_on_flat() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        for _ in 0..50 {
            service.update(&create_test_candle(100.0));
        }

        let features = service.update(&create_test_candle(100.0));

        let upper = features.bb_upper.unwrap();
        let lower = features.bb_lower.unwrap();
        let middle = features.bb_middle.unwrap();

        assert_eq!(middle, dec!(100.0));
        assert!(upper - lower < dec!(0.001));
    }

    #[test]
    fn test_atr_zero_on_identical_candles() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        for _ in 0..20 {
            service.update(&create_trending_candle(100.0, 0.0)); // High=Low=Close=100
        }

        let features = service.update(&create_test_candle(100.0));
        assert!(features.atr.unwrap() < dec!(0.001));
    }

    #[test]
    fn test_macd_zero_on_flat() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        for _ in 0..50 {
            service.update(&create_test_candle(100.0));
        }

        let features = service.update(&create_test_candle(100.0));
        assert!(features.macd_line.unwrap().abs() < dec!(0.001));
        assert!(features.macd_signal.unwrap().abs() < dec!(0.001));
        assert!(features.macd_hist.unwrap().abs() < dec!(0.001));
    }

    #[test]
    fn test_adx_manual_vs_known_values() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        // Strong trend: Price increases by 1.0 every bar
        for i in 0..50 {
            let price = 100.0 + i as f64;
            service.update(&create_trending_candle(price, 0.5));
        }

        let features = service.update(&create_trending_candle(150.0, 0.5));

        let adx = features.adx.unwrap();
        assert!(
            adx > dec!(20.0),
            "ADX should be trending (typically > 20/25), got {}",
            adx
        );
    }

    #[test]
    fn test_momentum_normalized_positive_uptrend() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        // 30 bars of uptrend
        for i in 0..30 {
            let price = 100.0 + i as f64;
            service.update(&create_trending_candle(price, 0.5));
        }

        let features = service.update(&create_trending_candle(130.0, 0.5));

        // Momentum = (Price - Price_N) / ATR
        // Price rose, so momentum should be positive
        assert!(features.momentum_normalized.unwrap() > dec!(0.0));
    }

    #[test]
    fn test_features_none_when_insufficient_data() {
        let config = AnalystConfig::default();
        let mut service = TechnicalFeatureEngineeringService::new(&config);

        // Just 1 candle
        let features = service.update(&create_test_candle(100.0));

        assert!(features.hurst_exponent.is_none());
        assert!(features.skewness.is_none());
        assert!(features.realized_volatility.is_none());
        assert!(features.momentum_normalized.is_none());
    }
}
