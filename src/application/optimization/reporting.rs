//! Reporting utilities for optimization results.
//!
//! Provides formatted console output and JSON export capabilities.

use crate::application::optimization::optimizer::{OptimizationResult, ParameterGrid};
use anyhow::{Context, Result};
use rust_decimal_macros::dec;
use std::path::Path;

/// Reporter for optimization results output.
pub struct OptimizeReporter {
    output_dir: String,
}

impl OptimizeReporter {
    /// Creates a new reporter with the given output directory.
    pub fn new(output_dir: &str) -> Self {
        Self {
            output_dir: output_dir.to_string(),
        }
    }

    /// Prints the parameter grid configuration.
    pub fn print_grid_info(&self, grid: &ParameterGrid) {
        println!("\n📊 Parameter Grid:");
        println!("  Fast SMA:       {:?}", grid.fast_sma);
        println!("  Slow SMA:       {:?}", grid.slow_sma);
        println!("  RSI Threshold:  {:?}", grid.rsi_threshold);
        println!("  Trend Div:      {:?}", grid.trend_divergence_threshold);
        println!("  ATR Mult:       {:?}", grid.trailing_stop_atr_multiplier);
        println!("  Cooldown (s):   {:?}", grid.order_cooldown_seconds);

        let total_combos = grid.fast_sma.len()
            * grid.slow_sma.len()
            * grid.rsi_threshold.len()
            * grid.trend_divergence_threshold.len()
            * grid.trailing_stop_atr_multiplier.len()
            * grid.order_cooldown_seconds.len();

        println!("\n🔢 Total combinations to test: {}", total_combos);
    }

    /// Prints a formatted table of results.
    pub fn print_results_table(&self, results: &[OptimizationResult], top_n: usize) {
        println!("\n{}", "=".repeat(80));
        println!("✅ OPTIMIZATION COMPLETE - Top {} Results", top_n);
        println!("{}", "=".repeat(80));

        println!(
            "{:<4} | {:<6} | {:<6} | {:>8} | {:>8} | {:>8} | {:>7} | {:>7} | {:>8}",
            "#", "Fast", "Slow", "Sharpe", "Return%", "WinRate", "Trades", "MaxDD%", "Score"
        );
        println!("{}", "-".repeat(80));

        for (i, result) in results.iter().enumerate() {
            println!(
                "{:<4} | {:<6} | {:<6} | {:>8.2} | {:>8.2} | {:>8.1} | {:>7} | {:>7.2} | {:>8.4}",
                i + 1,
                result.params.fast_sma_period,
                result.params.slow_sma_period,
                result.sharpe_ratio,
                result.total_return,
                result.win_rate,
                result.total_trades,
                result.max_drawdown,
                result.objective_score
            );
        }

        println!("{}\n", "=".repeat(80));
    }

    /// Prints detailed information about the best configuration.
    pub fn print_best_config(&self, best: &OptimizationResult) {
        println!("🏆 BEST CONFIGURATION:");
        println!("  Fast SMA:         {}", best.params.fast_sma_period);
        println!("  Slow SMA:         {}", best.params.slow_sma_period);
        println!("  RSI Threshold:    {:.1}", best.params.rsi_threshold);
        println!(
            "  Trend Div:        {:.4}",
            best.params.trend_divergence_threshold
        );
        println!(
            "  ATR Multiplier:   {:.1}",
            best.params.trailing_stop_atr_multiplier
        );
        println!("  Cooldown (s):     {}", best.params.order_cooldown_seconds);
        println!("\n  Sharpe Ratio:     {:.2}", best.sharpe_ratio);
        println!("  Total Return:     {:.2}%", best.total_return);
        println!("  Win Rate:         {:.1}%", best.win_rate);
        println!("  Max Drawdown:     {:.2}%", best.max_drawdown);
        println!("  Alpha:            {:.4}%", best.alpha * dec!(100.0));
        println!("  Beta:             {:.2}", best.beta);
        println!("{}\n", "=".repeat(80));
    }

    /// Exports results to a JSON file.
    pub fn export_json(&self, results: &[OptimizationResult], filename: &str) -> Result<()> {
        let output_path = if filename.contains('/') || filename.contains('\\') {
            filename.to_string()
        } else {
            format!("{}/{}", self.output_dir, filename)
        };

        // Ensure directory exists
        if let Some(parent) = Path::new(&output_path).parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create directory: {:?}", parent))?;
        }

        let json_output =
            serde_json::to_string_pretty(results).context("Failed to serialize results to JSON")?;

        std::fs::write(&output_path, json_output)
            .context(format!("Failed to write results to {}", output_path))?;

        println!("💾 Results saved to: {}", output_path);
        Ok(())
    }

    /// Prints the header banner for the optimization run.
    pub fn print_header(&self, symbol: &str, start: &str, end: &str, strategy: &str, output: &str) {
        println!("{}", "=".repeat(80));
        println!("🔍 GRID SEARCH PARAMETER OPTIMIZER");
        println!("{}", "=".repeat(80));
        println!("Symbol:       {}", symbol);
        println!("Period:       {} to {}", start, end);
        println!("Strategy:     {}", strategy);
        println!("Output:       {}", output);
        println!("{}", "=".repeat(80));
    }
}

impl Default for OptimizeReporter {
    fn default() -> Self {
        Self::new(".")
    }
}
