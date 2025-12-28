use crate::domain::ports::FeatureEngineeringService;
use crate::domain::types::FeatureSet;
use crate::application::analyst::AnalystConfig;
use ta::indicators::{
    AverageTrueRange, BollingerBands, MovingAverageConvergenceDivergence, RelativeStrengthIndex,
    SimpleMovingAverage,
};
use ta::Next;

pub struct TechnicalFeatureEngineeringService {
    rsi: RelativeStrengthIndex,
    macd: MovingAverageConvergenceDivergence,
    sma_20: SimpleMovingAverage,
    sma_50: SimpleMovingAverage,
    sma_200: SimpleMovingAverage,
    bb: BollingerBands,
    atr: AverageTrueRange,
}

impl TechnicalFeatureEngineeringService {
    pub fn new(config: &AnalystConfig) -> Self {
        Self {
            rsi: RelativeStrengthIndex::new(config.rsi_period as usize).unwrap(),
            macd: MovingAverageConvergenceDivergence::new(
                config.macd_fast as usize,
                config.macd_slow as usize,
                config.macd_signal as usize,
            )
            .unwrap(),
            sma_20: SimpleMovingAverage::new(config.fast_sma_period).unwrap(),
            sma_50: SimpleMovingAverage::new(config.slow_sma_period).unwrap(),
            sma_200: SimpleMovingAverage::new(config.trend_sma_period).unwrap(),
            bb: BollingerBands::new(config.bb_period as usize, config.bb_std_dev).unwrap(),
            atr: AverageTrueRange::new(config.atr_period as usize).unwrap(),
        }
    }
}

impl FeatureEngineeringService for TechnicalFeatureEngineeringService {
    fn update(&mut self, price: f64) -> FeatureSet {
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
        }
    }
}
