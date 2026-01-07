use crate::application::agents::analyst::AnalystConfig;
use crate::domain::ports::FeatureEngineeringService;
use crate::domain::trading::types::{Candle, FeatureSet};
use rust_decimal::prelude::ToPrimitive;
use ta::indicators::{
    AverageTrueRange, BollingerBands, ExponentialMovingAverage, MovingAverageConvergenceDivergence,
    RelativeStrengthIndex, SimpleMovingAverage,
};
use ta::Next;

pub struct ManualAdx {
    period: usize,
    prev_high: Option<f64>,
    prev_low: Option<f64>,
    prev_close: Option<f64>,
    tr_smooth: f64,
    plus_dm_smooth: f64,
    minus_dm_smooth: f64,
    dx_smooth: f64,
    initialized: bool,
    count: usize,
}

impl ManualAdx {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prev_high: None,
            prev_low: None,
            prev_close: None,
            tr_smooth: 0.0,
            plus_dm_smooth: 0.0,
            minus_dm_smooth: 0.0,
            dx_smooth: 0.0,
            initialized: false,
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

        let prev_high = self.prev_high.unwrap();
        let prev_low = self.prev_low.unwrap();
        let prev_close = self.prev_close.unwrap();

        // Calculate True Range
        let tr1 = high - low;
        let tr2 = (high - prev_close).abs();
        let tr3 = (low - prev_close).abs();
        let tr = tr1.max(tr2).max(tr3);

        // Calculate Directional Movement
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

        // Smoothing (Wilder's Smoothing usually)
        // For first 'period' values, usually sum. But simpler approach:
        // smooth = (prev_smooth * (n-1) + current) / n
        if !self.initialized {
             self.count += 1;
             self.tr_smooth += tr;
             self.plus_dm_smooth += plus_dm;
             self.minus_dm_smooth += minus_dm;

             if self.count >= self.period {
                 self.initialized = true;
                 // Initial average
                 // But typically Wilder starts subsequent smoothing
             }
        } else {
            let n = self.period as f64;
            self.tr_smooth = self.tr_smooth - (self.tr_smooth / n) + tr;
            self.plus_dm_smooth = self.plus_dm_smooth - (self.plus_dm_smooth / n) + plus_dm;
            self.minus_dm_smooth = self.minus_dm_smooth - (self.minus_dm_smooth / n) + minus_dm;
        }

        // Calculate DI and DX
        let mut adx = 0.0;
        if self.initialized && self.tr_smooth > 0.0 {
            let plus_di = 100.0 * self.plus_dm_smooth / self.tr_smooth;
            let minus_di = 100.0 * self.minus_dm_smooth / self.tr_smooth;
            let sum_di = plus_di + minus_di;
            
            let dx = if sum_di > 0.0 {
                100.0 * (plus_di - minus_di).abs() / sum_di
            } else {
                0.0
            };

            // Smooth DX to get ADX
            // First ADX is average of DX over period? 
            // Simplified: use same smoothing
            let n = self.period as f64;
            if self.dx_smooth == 0.0 {
                self.dx_smooth = dx; // Initialization hack
            } else {
                self.dx_smooth = ((self.dx_smooth * (n - 1.0)) + dx) / n;
            }
            adx = self.dx_smooth;
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
            rsi: RelativeStrengthIndex::new(config.rsi_period).unwrap(),
            macd: MovingAverageConvergenceDivergence::new(
                config.macd_fast,
                config.macd_slow,
                config.macd_signal,
            )
            .unwrap(),
            sma_20: SimpleMovingAverage::new(config.fast_sma_period).unwrap(),
            sma_50: SimpleMovingAverage::new(config.slow_sma_period).unwrap(),
            sma_200: SimpleMovingAverage::new(config.trend_sma_period).unwrap(),
            bb: BollingerBands::new(config.bb_period, config.bb_std_dev).unwrap(),
            atr: AverageTrueRange::new(config.atr_period).unwrap(),
            ema_fast: ExponentialMovingAverage::new(config.ema_fast_period).unwrap(),
            ema_slow: ExponentialMovingAverage::new(config.ema_slow_period).unwrap(),
            adx: ManualAdx::new(config.adx_period),
        }
    }
}

impl FeatureEngineeringService for TechnicalFeatureEngineeringService {
    fn update(&mut self, candle: &Candle) -> FeatureSet {
        let price = candle.close.to_f64().unwrap_or(0.0);
        let high = candle.high.to_f64().unwrap_or(0.0);
        let low = candle.low.to_f64().unwrap_or(0.0);

        let rsi_val = self.rsi.next(price);
        let macd_val = self.macd.next(price);
        let bb_val = self.bb.next(price);

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
            atr: Some(self.atr.next(price)),
            ema_fast: Some(self.ema_fast.next(price)),
            ema_slow: Some(self.ema_slow.next(price)),
            adx: Some(self.adx.next(high, low, price)),
            timeframe: Some(crate::domain::market::timeframe::Timeframe::OneMin), // Primary timeframe
        }
    }
}
