use crate::config::StrategyMode;
use crate::domain::ports::{ExecutionService, MarketDataService};
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use super::grid::{GeneBounds, decode_genome};
use super::types::{OptimizationResult, SinglePeriodBars};
use super::walk_forward::run_single_period_eval;
use rand::RngExt;

/// Genetic algorithm optimizer
pub struct GeneticOptimizer {
    pub(crate) market_data: Arc<dyn MarketDataService>,
    pub(crate) execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
    pub(crate) bounds: GeneBounds,
    pub(crate) strategy_mode: StrategyMode,
    pub(crate) min_profit_ratio: Decimal,
    pub(crate) population_size: usize,
    pub(crate) generations: usize,
    pub(crate) mutation_rate: f64,
}

impl GeneticOptimizer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        market_data: Arc<dyn MarketDataService>,
        execution_service_factory: Arc<dyn Fn() -> Arc<dyn ExecutionService> + Send + Sync>,
        bounds: GeneBounds,
        strategy_mode: StrategyMode,
        min_profit_ratio: Decimal,
        population_size: usize,
        generations: usize,
        mutation_rate: f64,
        _risk_score: Option<u8>,
    ) -> Self {
        Self {
            market_data,
            execution_service_factory,
            bounds,
            strategy_mode,
            min_profit_ratio,
            population_size,
            generations,
            mutation_rate,
        }
    }

    pub async fn run_optimization(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        _timeframe: &str,
    ) -> Result<Vec<OptimizationResult>> {
        let bars = self
            .market_data
            .get_historical_bars(symbol, start, end, "1Min")
            .await?;
        if bars.is_empty() {
            return Err(anyhow::anyhow!("No candles found for {}", symbol));
        }

        let spy_bars = self
            .market_data
            .get_historical_bars("SPY", start, end, "1Day")
            .await
            .unwrap_or_default();

        let prefetched = Arc::new(SinglePeriodBars {
            bars,
            start,
            end,
            spy_bars,
        });

        let mut rng = rand::rng();
        let mut population = vec![[0.0_f64; 14]; self.population_size];
        for genome in &mut population {
            for gene in genome.iter_mut() {
                *gene = rng.random_range(0.0..=1.0);
            }
        }

        let mut best_overall_results = Vec::new();
        const PARALLEL_WORKERS: usize = 4;

        for generation in 0..self.generations {
            let start_gen = Instant::now();

            let eval_results = stream::iter(population.clone())
                .map(|genome| {
                    let market_data = self.market_data.clone();
                    let exec_factory = self.execution_service_factory.clone();
                    let config = decode_genome(
                        &genome,
                        &self.bounds,
                        self.strategy_mode,
                        self.min_profit_ratio,
                    );
                    let symbol = symbol.to_string();
                    let prefetched = prefetched.clone();
                    async move {
                        run_single_period_eval(
                            market_data,
                            exec_factory,
                            config,
                            symbol,
                            prefetched,
                        )
                        .await
                    }
                })
                .buffer_unordered(PARALLEL_WORKERS)
                .collect::<Vec<_>>()
                .await;

            let mut fitness_scores = Vec::new();
            let mut valid_results = Vec::new();
            for (i, res) in eval_results.into_iter().enumerate() {
                if let Ok(opt_res) = res {
                    fitness_scores.push((i, opt_res.objective_score.to_f64().unwrap_or(0.0)));
                    valid_results.push(opt_res);
                } else {
                    fitness_scores.push((i, -1000.0));
                }
            }

            fitness_scores
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            best_overall_results = valid_results;
            best_overall_results.sort_by(|a, b| {
                b.objective_score
                    .partial_cmp(&a.objective_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let mut new_population = Vec::with_capacity(self.population_size);
            for i in 0..(self.population_size / 10).max(2) {
                if i < fitness_scores.len() {
                    new_population.push(population[fitness_scores[i].0]);
                }
            }

            while new_population.len() < self.population_size {
                let p1 = self.tournament_select(&population, &fitness_scores);
                let p2 = self.tournament_select(&population, &fitness_scores);
                let mut child = self.crossover(&p1, &p2);
                self.mutate(&mut child);
                new_population.push(child);
            }

            population = new_population;
            debug!(
                "Generation {} completed in {:?}",
                generation,
                start_gen.elapsed()
            );
        }

        best_overall_results.truncate(20);
        Ok(best_overall_results)
    }

    fn tournament_select(
        &self,
        population: &[[f64; 14]],
        fitness_scores: &[(usize, f64)],
    ) -> [f64; 14] {
        let mut rng = rand::rng();
        let mut best_idx = rng.random_range(0..population.len());
        let mut best_fitness = -1000.0;

        for (idx, score) in fitness_scores {
            if *idx == best_idx {
                best_fitness = *score;
                break;
            }
        }

        for _ in 0..3 {
            let idx = rng.random_range(0..population.len());
            let mut score = -1000.0;
            for (f_idx, f_score) in fitness_scores {
                if *f_idx == idx {
                    score = *f_score;
                    break;
                }
            }
            if score > best_fitness {
                best_idx = idx;
                best_fitness = score;
            }
        }
        population[best_idx]
    }

    fn crossover(&self, p1: &[f64; 14], p2: &[f64; 14]) -> [f64; 14] {
        let mut rng = rand::rng();
        let mut child = [0.0_f64; 14];
        for i in 0..14 {
            child[i] = if rng.random::<bool>() { p1[i] } else { p2[i] };
        }
        child
    }

    fn mutate(&self, genome: &mut [f64; 14]) {
        let mut rng = rand::rng();
        for g in genome.iter_mut() {
            if rng.random::<f64>() < self.mutation_rate {
                *g = (*g + rng.random_range(-0.2..=0.2)).clamp(0.0, 1.0);
            }
        }
    }
}
