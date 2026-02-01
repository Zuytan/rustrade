use crate::application::agents::analyst_config::AnalystConfig;
use crate::application::market_data::statistical_features::{
    calculate_hurst_exponent, calculate_skewness,
};
use crate::application::risk_management::volatility::calculate_realized_volatility;
use crate::domain::ports::FeatureEngineeringService;
use crate::domain::trading::types::{Candle, FeatureSet};
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
    price_history: VecDeque<f64>,
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

        // Update price history
        self.price_history.push_back(price);
        if self.price_history.len() > 100 {
            self.price_history.pop_front();
        }

        // Calculate advanced features
        // Convert VecDeque to Vec for calculations
        let prices_vec: Vec<f64> = self.price_history.iter().copied().collect();

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
