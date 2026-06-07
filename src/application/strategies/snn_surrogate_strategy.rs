use super::traits::{AnalysisContext, Signal, TradingStrategy};
use crate::application::ml::derivative_encoding::{FeatureInputs, encode_multi_feature};
use crate::domain::snn::competitive_network::CompetitiveSnnNetwork;
use chrono::Timelike;
use rust_decimal::prelude::ToPrimitive;
use tracing::{debug, info};

/// A trading strategy that uses a pre-trained Surrogate SNN (CompetitiveSnnNetwork).
///
/// This strategy operates on a "rolling window" basis, feeding the last N bars
/// into the SNN to generate a prediction (Buy/Sell/Hold) based on temporal integration.
pub struct SnnSurrogateStrategy {
    /// The pre-trained network.
    network: CompetitiveSnnNetwork,

    /// Threshold for the readout logit to trigger a signal.
    activation_threshold: f64,

    /// Size of the temporal window (number of bars) to feed into the SNN.
    window_size: usize,

    /// Minimum spike magnitude for encoding.
    spike_threshold: f64,

    /// Whether the model has been validated as having trained weights.
    is_model_trained: bool,
}

/// Struct holding gathered features for analysis.
#[derive(Debug, Clone)]
struct WindowFeatures {
    pub prices: Vec<f64>,
    pub atrs: Vec<f64>,
    pub volumes: Vec<f64>,
    pub ema200s: Vec<f64>,
    pub bb_widths: Vec<f64>,
    pub vwaps: Vec<f64>,
    pub rsi_values: Vec<f64>,
    pub macd_hists: Vec<f64>,
}

impl SnnSurrogateStrategy {
    /// Loads a trained model from a JSON file and creates a new strategy.
    pub fn new(
        model_path: &str,
        activation_threshold: f64,
        window_size: usize,
        spike_threshold: f64,
    ) -> anyhow::Result<Self> {
        info!("🔌 Loading SNN Surrogate model from: {}", model_path);
        let json = std::fs::read_to_string(model_path)?;
        let network: CompetitiveSnnNetwork = serde_json::from_str(&json)?;

        // Validate that the model has non-trivial weights (cold-start guard)
        let weight_sum: f64 = network.layer_a.weights.iter().map(|w| w.abs()).sum();
        let is_model_trained = weight_sum > f64::EPSILON;

        if !is_model_trained {
            tracing::warn!(
                "⚠️  SNN model loaded from {} but weights are all zero (untrained). \
                 Strategy will skip analysis until a trained model is provided.",
                model_path
            );
        }

        info!(
            "✅ SNN Surrogate loaded: hidden_a={}, hidden_b={}, input_dim={}, trained={}",
            network.layer_a.num_neurons,
            network.layer_b.num_neurons,
            network.layer_a.weights.shape()[0], // input_dim
            is_model_trained
        );

        Ok(Self {
            network,
            activation_threshold,
            window_size,
            spike_threshold,
            is_model_trained,
        })
    }

    /// Aggregates raw candles into 15m bars and computes necessary technical features.
    /// Returns None if data is insufficient or features cannot be calculated.
    fn prepare_rolling_window(&self, ctx: &AnalysisContext) -> Option<WindowFeatures> {
        if ctx.candles.len() < 2 {
            return None;
        }

        let c1 = ctx.candles.back()?;
        let c2 = &ctx.candles[ctx.candles.len() - 2];
        let interval_mins = (c1.timestamp - c2.timestamp).abs() / 60;

        // Ensure we are aligned to a 15-minute mark
        let dt = chrono::DateTime::from_timestamp(c1.timestamp, 0)?;
        if !dt.minute().is_multiple_of(15) {
            return None;
        }

        let bars_needed = self.window_size + 1;
        let candles_per_bar = if interval_mins >= 15 { 1 } else { 15 };
        let total_raw_needed = bars_needed * candles_per_bar;

        if ctx.candles.len() < total_raw_needed {
            return None;
        }

        let mut prices = Vec::with_capacity(bars_needed);
        let mut atrs = Vec::with_capacity(bars_needed);
        let mut volumes = Vec::with_capacity(bars_needed);
        let mut ema200s = Vec::with_capacity(bars_needed);
        let mut bb_widths = Vec::with_capacity(bars_needed);
        let mut vwaps = Vec::with_capacity(bars_needed);
        let mut rsi_values = Vec::with_capacity(bars_needed);
        let mut macd_hists = Vec::with_capacity(bars_needed);

        // Feature engineering service for local window calculation
        use crate::application::agents::analyst_config::AnalystConfig;
        use crate::application::monitoring::feature_engineering_service::TechnicalFeatureEngineeringService;
        use crate::domain::ports::FeatureEngineeringService;
        let mut temp_service = TechnicalFeatureEngineeringService::new(&AnalystConfig::default());

        // Collect VecDeque into contiguous Vec to avoid ring-buffer split.
        // VecDeque::as_slices().0 only returns the first contiguous segment,
        // silently ignoring wrapped elements — a correctness bug in production.
        let candles_vec: Vec<_> = ctx.candles.iter().cloned().collect();
        let start_idx = if candles_vec.len() >= total_raw_needed {
            candles_vec.len() - total_raw_needed
        } else {
            return None;
        };

        // P2.4: Pre-warm the feature service with historical candles so long-lookback
        // indicators like EMA200 are accurate. EMA200 needs 200 bars.
        let warmup_bars = 200;
        let warmup_raw_needed = warmup_bars * candles_per_bar;
        let warmup_start_idx = start_idx.saturating_sub(warmup_raw_needed);

        for b in 0..((start_idx - warmup_start_idx) / candles_per_bar) {
            let bar_start = warmup_start_idx + b * candles_per_bar;
            let bar_end = bar_start + candles_per_bar;
            let block = &candles_vec[bar_start..bar_end];
            if block.is_empty() {
                continue;
            }

            let mut high = block[0].high;
            let mut low = block[0].low;
            let mut volume = rust_decimal::Decimal::ZERO;
            for c in block {
                if c.high > high {
                    high = c.high;
                }
                if c.low < low {
                    low = c.low;
                }
                volume += c.volume;
            }
            let Some(last_candle) = block.last() else {
                continue;
            };
            let close = last_candle.close;

            let mut agg_candle = block[0].clone();
            agg_candle.high = high;
            agg_candle.low = low;
            agg_candle.close = close;
            agg_candle.volume = volume;

            temp_service.update(&agg_candle);
        }

        for b in 0..bars_needed {
            let bar_start = start_idx + b * candles_per_bar;
            let bar_end = bar_start + candles_per_bar;

            let block = &candles_vec[bar_start..bar_end];
            if block.is_empty() {
                return None;
            }

            // Aggregate OHLCV for the 15m bar
            let mut high = block[0].high;
            let mut low = block[0].low;
            let mut volume = rust_decimal::Decimal::ZERO;
            for c in block {
                if c.high > high {
                    high = c.high;
                }
                if c.low < low {
                    low = c.low;
                }
                volume += c.volume;
            }
            let close = block.last()?.close;

            let mut agg_candle = block[0].clone();
            agg_candle.high = high;
            agg_candle.low = low;
            agg_candle.close = close;
            agg_candle.volume = volume;

            let feat = temp_service.update(&agg_candle);

            let p = close.to_f64()?;
            prices.push(p);
            atrs.push(feat.atr?.to_f64()?.max(1e-10));
            volumes.push(volume.to_f64()?);
            ema200s.push(feat.sma_200?.to_f64()?);
            bb_widths.push(feat.bb_width?.to_f64()?);
            vwaps.push(feat.vwap?.to_f64()?);
            rsi_values.push(feat.rsi?.to_f64()?);
            macd_hists.push(feat.macd_hist?.to_f64()?);
        }

        Some(WindowFeatures {
            prices,
            atrs,
            volumes,
            ema200s,
            bb_widths,
            vwaps,
            rsi_values,
            macd_hists,
        })
    }
}

impl TradingStrategy for SnnSurrogateStrategy {
    fn analyze(&self, ctx: &AnalysisContext) -> Option<Signal> {
        // Cold-start guard: skip analysis if model has untrained weights
        if !self.is_model_trained {
            return None;
        }

        // 1. Prepare data and features
        let wf = self.prepare_rolling_window(ctx)?;

        // 2. Encode into spikes (v2.1 - 10 features including Market Pulse)
        let inputs = FeatureInputs {
            prices: &wf.prices,
            atrs: &wf.atrs,
            volumes: &wf.volumes,
            ema200s: &wf.ema200s,
            bb_widths: &wf.bb_widths,
            vwaps: &wf.vwaps,
            rsi_values: &wf.rsi_values,
            macd_hist: &wf.macd_hists,
        };
        let features = encode_multi_feature(inputs, self.spike_threshold, 5);

        if features.pos_channel.len() < self.window_size {
            debug!(
                "Insufficient spikes for SNN window: {} < {}",
                features.pos_channel.len(),
                self.window_size
            );
            return None;
        }

        // 3. Run Forward Pass.
        // We feed the full encoded window (already sized to window_size+1 via prepare_rolling_window).
        // The network integrates spikes over all timesteps to produce logits.
        let (logits, _cache) = self
            .network
            .forward(&features.pos_channel, &features.neg_channel);

        // 4. Decision Logic
        // logits: [0]=Buy, [1]=Sell, [2]=Hold
        if logits.len() < 3 {
            return None;
        }

        let buy_logit = logits[0];
        let sell_logit = logits[1];
        let hold_logit = logits[2];

        // Activation threshold guard
        if buy_logit < self.activation_threshold && sell_logit < self.activation_threshold {
            return None;
        }

        // Identify dominant class
        let mut max_idx = 2; // Default to Hold
        let mut max_val = hold_logit;

        if buy_logit > max_val {
            max_val = buy_logit;
            max_idx = 0;
        }
        if sell_logit > max_val {
            max_idx = 1;
        }

        match max_idx {
            0 => {
                let confidence = (buy_logit - hold_logit).clamp(0.0, 1.0);
                let atr = ctx.atr.unwrap_or_else(|| {
                    use rust_decimal_macros::dec;
                    ctx.current_price * dec!(0.0075) // fallback to 0.75% of price as 1 ATR
                });
                use rust_decimal_macros::dec;
                let stop_loss = ctx.current_price - (atr * dec!(2.0));
                let take_profit = ctx.current_price + (atr * dec!(4.0));
                Some(
                    Signal::buy("SNN Surrogate (15m): Bullish Pattern Detected")
                        .with_confidence(confidence)
                        .with_stop_loss(stop_loss)
                        .with_take_profit(take_profit),
                )
            }
            1 => {
                let confidence = (sell_logit - hold_logit).clamp(0.0, 1.0);
                let atr = ctx.atr.unwrap_or_else(|| {
                    use rust_decimal_macros::dec;
                    ctx.current_price * dec!(0.0075) // fallback to 0.75% of price as 1 ATR
                });
                use rust_decimal_macros::dec;
                let stop_loss = ctx.current_price + (atr * dec!(2.0));
                let take_profit = ctx.current_price - (atr * dec!(4.0));
                Some(
                    Signal::sell("SNN Surrogate (15m): Bearish Pattern Detected")
                        .with_confidence(confidence)
                        .with_stop_loss(stop_loss)
                        .with_take_profit(take_profit),
                )
            }
            _ => None,
        }
    }

    fn name(&self) -> &str {
        "SnnSurrogate"
    }
}
