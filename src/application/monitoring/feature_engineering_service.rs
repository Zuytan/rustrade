use crate::application::agents::analyst_config::AnalystConfig;
use crate::domain::ports::FeatureEngineeringService;
use crate::domain::trading::types::{Candle, FeatureSet};
use rust_decimal::prelude::ToPrimitive;
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
            bb: BollingerBands::new(config.mean_reversion_bb_period, config.bb_std_dev)
                .expect("mean_reversion_bb_period from AnalystConfig must be > 0"),
            atr: AverageTrueRange::new(config.atr_period)
                .expect("atr_period from AnalystConfig must be > 0"),
            ema_fast: ExponentialMovingAverage::new(config.ema_fast_period)
                .expect("ema_fast_period from AnalystConfig must be > 0"),
            ema_slow: ExponentialMovingAverage::new(config.ema_slow_period)
                .expect("ema_slow_period from AnalystConfig must be > 0"),
            adx: ManualAdx::new(config.adx_period),
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

        FeatureSet {
            rsi: Some(rsi_val),
            macd_line: Some(macd_val.macd),
            macd_signal: Some(macd_val.signal),
            macd_hist: Some(macd_val.histogram),
            sma_20: Some(self.sma_20.next(price)),
            sma_50: Some(self.sma_50.next(price)),
            sma_200: Some(self.sma_200.next(price)),
            bb_upper: Some(bb_val.upper),
            bb_middle: Some(bb_val.average),
            bb_lower: Some(bb_val.lower),
            atr: Some(self.atr.next(&item)),
            ema_fast: Some(self.ema_fast.next(price)),
            ema_slow: Some(self.ema_slow.next(price)),
            adx: Some(self.adx.next(high, low, price)),
            timeframe: Some(crate::domain::market::timeframe::Timeframe::OneMin),
        }
    }
}
