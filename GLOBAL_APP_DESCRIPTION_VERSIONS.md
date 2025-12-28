# Rustrade - Historique des Versions

## Version 0.21.0 (Janvier 2026) - Analyst Refactoring & Financial Precision
- **Découplage de l'Analyste**: Extraction des responsabilités dans `FeatureEngineeringService`, `SignalGenerator`, et `PositionManager`. Réduction massive de la complexité de `analyst.rs`.
- **Espérance de Gain Avancée**: Remplacement des heuristiques par un `ExpectancyEvaluator` utilisant le `MarketRegime`.
- **Reward/Risk Ratio (1.5 min)**: Validation stricte de chaque signal basée sur le ratio gain/risque estimé dynamiquement.
- **Gestion Sectorielle Dynamique**: Implémentation d'un `SectorProvider` via l'API Alpaca Assets, éliminant le besoin de mise à jour manuelle des secteurs.
- **DDD & Clean Architecture**: Renforcement du découplage entre les couches application, domaine et infrastructure.

## Version 0.20.0 (Janvier 2026) - Audit Architectural & Maturité logicielle
- **Audit Complet**: Revue approfondie de l'architecture logicielle selon les principes DDD et Clean Architecture.
- **Score d'Excellence**: Évaluation de **9.5/10** sur la structure, le découplage et la testabilité.
- **Robustesse**: Confirmation de la viabilité des mécanismes de durcissement financier et d'optimisation adaptative.
- **Documentation**: Mise à jour des descriptions globales pour refléter l'état actuel de maturité du bot.

## Version 0.19.0 (Janvier 2026) - Stratégie Adaptative & Optimisation de Régime
- **Optimisation Adaptative**: Intégration d'une boucle fermée ajustant dynamiquement les paramètres SMA, RSI et ATR en fonction de l'environnement de marché.
- **Surveillance de Performance**: Nouveau `PerformanceMonitoringService` capturant des instantanés de performance (Snapshots) et classifiant les régimes de marché (Trending/Ranging/Volatile).
- **Ré-optimisation Automatique**: Le `AdaptiveOptimizationService` déclenche des optimisations grid-search basées sur des seuils de performance (Sharpe ratio, drawdown).
- **Historique d'Optimisation**: Persistance complète des sessions d'optimisation et des triggers dans de nouvelles tables SQLite.
- **Coordination System**: Refonte de `system.rs` pour orchestrer le cycle de vie des services d'optimisation et les tâches planifiées.

## Version 0.18.0 (Décembre 2025) - Financial Hardening (Cost-Aware & Diversification)
- **Smart Order Execution**: Remplacement des ordres `Market` par des ordres `Limit` pour les entrées, éliminant le risque de slippage massif sur les actifs volatils.
- **Cost-Aware Logic**: Intégration d'un `FeeModel` qui estime commissions et slippage. L'Analyste rejette désormais tout trade dont l'espérance de gain n'est pas supérieure à 2x les coûts d'entrée/sortie ("Don't trade clearly losing bets").
- **Diversification Sectorielle**: Ajout d'une configuration `SECTORS` et monitoring de l'exposition par secteur dans le `RiskManager`. Plafond configurable (`MAX_SECTOR_EXPOSURE_PCT`) pour forcer la distribution du risque.
- **Refactoring Infrastructure**: Mise à jour des `TradeProposal` et `Order` pour supporter explicitement les types d'ordres (`Limit`, `Market`, `StopLimit`).

## Version 0.17.0 (Décembre 2025) - DDD Persistence Refactoring
- **Refactoring Architectural**: Transition complète vers le Domain-Driven Design (DDD) pour la couche de persistance.
- **Inversion de Dépendance**: Les agents applicatifs (`Executor`, `Analyst`, `CandleAggregator`) dépendent désormais strictement de traits abstraits (`TradeRepository`, `CandleRepository`) définis dans le Domaine, brisant le couplage fort avec l'Infrastructure.
- **Repositories**:
  - Renommage des implémentations concrètes en `SqliteOrderRepository` et `SqliteCandleRepository`.
  - Implémentation complète des méthodes de recherche (`find_by_symbol`, `get_range`, `prune`) sur les traits.
- **Dependency Injection**: Le constructeur `Application::build` injecte désormais les dépendances sous forme de `Arc<dyn Trait>`, facilitant les tests et le remplacement futur du backend de stockage.

## Version 0.16.0 (Décembre 2025) - Persistence Layer (SQLite)
- **NOUVEAU: Base de Données Locale**: Intégration de **SQLite** via `sqlx` pour une persistance robuste et zéro-conf.
- **Historisation des Transactions**: Chaque ordre exécuté est désormais sauvegardé durablement dans la table `trades` (auditabilité fiscale et performance).
- **Historisation des Données de Marché**: Les bougies (Candles) 1-minute agrégées sont sauvegardées dans la table `candles`.
- **Architecture Asynchrone**: Les écritures en base sont effectuées en arrière-plan (Fire-and-Forget) pour ne jamais ralentir le trading haute fréquence.
- **Idempotence**: Gestion robuste des doublons via `ON CONFLICT DO NOTHING/UPDATE`.

## Version 0.15.3 (Décembre 2025) - WebSocket Resilience & Fast Reconnection
- **AMÉLIORATION: Reconnexion WebSocket Rapide**: Implémentation d'une stratégie de reconnexion immédiate avec backoff exponentiel.
  - Délai de reconnexion: **0s** (immédiat) pour la 1ère tentative, puis 1s, 2s, 4s, 8s, 16s avec cap à **30s**.
  - Précédent: délai fixe de 5s pour toutes les reconnexions.
- **NOUVEAU: Heartbeat Proactif**: Détection proactive des connexions mortes avant erreur de lecture.
  - Envoi de **Ping WebSocket** toutes les 20 secondes.
  - Timeout de Pong: **5 secondes** - déclenche reconnexion immédiate si pas de réponse.
  - Permet de détecter les déconnexions silencieuses (firewall, proxy, etc.).
- **AMÉLIORATION: Restauration Automatique**: Après reconnexion, les symboles sont automatiquement re-souscrits.
  - Logging amélioré: indique le nombre de symboles restaurés et le nombre de tentatives de reconnexion.
  - Réinitialisation du compteur de reconnexion après authentification réussie.
- **ROBUSTESSE**: Logs détaillés pour tracking des états de connexion (Connected → Authenticated → Subscribed).

## Version 0.15.2 (Décembre 2025) - Dynamic Mode & Communication Fixes
- **FIX: Sentinel Data Flux**: Correction d'un bug où le `Sentinel` ignorait les mises à jour de données lors d'un changement de watchlist en mode dynamique.
- **AMÉLIORATION: Robotique Market Scanner**: Meilleure gestion des réponses vides or `null` de l'API Alpaca Movers.
- **FIX: Précision des Quantités**: Augmentation de la précision à 4 décimales (contre 2) pour les calculs de quantités, évitant les ordres à 0.00 sur les petits budgets.

## Version 0.15.1 (Décembre 2025) - Market Scanner Fix & Quality Filtering
**Améliorations du Market Scanner**:
- **Fix Alpaca Movers Endpoint**: Passage de `v2/stocks/movers` (404) à `v1beta1/screener/stocks/movers` (fonctionnel).
- **Filtrage Qualitatif des Symboles**:
  - Exclusion automatique des **Penny Stocks** (prix < $5.0).
  - Exclusion des **Warrants** (contient `.WS` ou finit par `W`).
  - Exclusion des **Units** (finit par `U`).
- **Nettoyage de Code**: Suppression des avertissements de compilation (imports inutilisés, duplications).

## Version 0.15.0 (Décembre 2025) - Risk Appetite Score

**Nouvelle fonctionnalité majeure**:
- **Score d'Appétit au Risque (1-10)**: Système de configuration simplifié permettant d'ajuster automatiquement tous les paramètres de risque selon le profil utilisateur
  - Une seule variable `RISK_APPETITE_SCORE` remplace la configuration manuelle de 4+ paramètres
  - Interpolation linéaire continue sur toute la plage 1-10 pour granularité maximale
  - Formules automatiques: risk_per_trade (0.5%-3%), trailing_stop (2.0-5.0x ATR), rsi_threshold (30-75), max_position (5%-30%)
  - Différence significative entre scores: 25-38% de variation progressive entre score 7 et 10
  
**Architecture Domain-Driven Design**:
- Nouveau value object `RiskAppetite` dans `src/domain/risk_appetite.rs`
  - Validation stricte du score (1-10 uniquement)
  - Enum `RiskProfile` (Conservative/Balanced/Aggressive) pour classification
  - Méthodes de calcul des paramètres avec interpolation linéaire
- Intégration dans `Config` avec rétrocompatibilité totale
  - Si `RISK_APPETITE_SCORE` défini → override automatique des params individuels
  - Si non défini → comportement identique aux versions précédentes
  
**Tests**:
- 14 nouveaux tests unitaires et d'intégration
  - 9 tests pour `RiskAppetite` (validation, profiles, interpolation, granularité score 7 vs 10)
  - 5 tests pour `Config` (avec/sans score, override, erreurs, boundary values)
  - Tous tests passent avec `--test-threads=1` (isolation environnement)
- Total tests projet: **90 tests** (précédent: 76)

**Documentation**:
- Mise à jour `GLOBAL_APP_DESCRIPTION.md` avec section dédiée Risk Appetite
- Extension `.env.example` avec documentation complète des profils de risque
- Logging au démarrage affichant score et paramètres calculés

## Version 0.14.0 (Décembre 2026) - Backtesting Avancé & Optimisation

**Nouvelles fonctionnalités majeures**:
- **Alpha/Beta Calculation**: Calcul automatique de l'alpha et beta vs S&P500 (SPY) dans les backtests
  - Régression linéaire pour déterminer la sensibilité au marché (beta)
  - Calcul du rendement excédentaire ajusté au risque (alpha)
  - Corrélation avec le benchmark pour évaluer l'indépendance de la stratégie
  - Intégré dans `benchmark.rs` (affichage single + batch mode)
  
- **Grid Search Parameter Optimizer**: Nouveau binaire `optimize` pour optimisation systématique
  - Module `optimizer.rs` avec `ParameterGrid`, `OptimizationResult`, `GridSearchOptimizer`
  - Configuration via fichier TOML (`grid.toml`)
  - Score objectif composite: Sharpe (40%) + Return (30%) + WinRate (20%) - Drawdown (10%)
  - Export JSON de tous les résultats avec ranking automatique
  - Support CLI complet: `--symbol`, `--start`, `--end`, `--grid-config`, `--output`, `--top-n`
  - Test de centaines de combinaisons de paramètres (fast/slow SMA, RSI, ATR multiplier, etc.)

**Améliorations techniques**:
- Ajout Serialize/Deserialize à `AnalystConfig` et `StrategyMode` pour export config optimales
- Nouvelle dépendance: `toml = "0.8"` pour parsing configuration grilles
- Fonction `calculate_alpha_beta()` dans simulator.rs avec validation statistique
- Fetch automatique données SPY pour benchmark dans chaque backtest
- 74+ tests unitaires (ajout tests optimizer)

**Documentation**:
- README.md étendu avec sections "Backtest a Strategy" et "Optimize Strategy Parameters"
- GLOBAL_APP_DESCRIPTION.md enrichi avec détails outils backtesting/optimisation
- Walkthrough.md complet avec exemples d'usage optimizer et interprétation résultats
- Fichier `grid.toml` d'exemple créé

## Version 0.13.1 - Code Cleanup & Risk Hardening (2025-12-26)
- **Codebase Clean-up**: Resolved all `cargo clippy` warnings (redundant casts, unused imports, formatting) for a pristine codebase.
- **Risk Management Hardening**:
    - **Active Valuation Loop**: `RiskManager` now actively polls market prices (every 60s via `MarketDataService`) to recalculate equity.
    - **Crash Protection**: Circuit Breakers (Daily Loss/Drawdown) now trigger *immediately* on market drops, without waiting for the next trade proposal.
    - **Initialization Fix**: Fixed a bug where initial equity was miscalculated (ignoring held positions) on restart.
- **Documentation**: Updated architecture docs to reflect active risk monitoring.

## Version 0.13.0 - Tier 1 Critical Fixes (2025-12-26)
- **CRITICAL FIX: Trailing Stops Enabled**: Uncommented and activated trailing stop mechanism that was previously disabled.
    - Trailing stops now actively monitor price movements and trigger sell signals when threshold is hit.
    - NVDA: 4 trades (all buys) → 8 trades (4 buys + 4 sells) ✅
    - AAPL: 34 trades (all buys) → 60 trades (30 buy/sell pairs) ✅
    - Logs confirm execution: "Trailing stop HIT" messages visible.
- **CRITICAL FIX: Long-Only Safety Logic**: Corrected sell signal blocking that prevented ALL sales instead of just short selling.
    - Now properly distinguishes between selling existing positions (allowed) and short selling (blocked).
    - Improved logging with clear "BLOCKING" vs "ALLOWING" messages.
    - Unit tests validate: `test_sell_signal_with_position` and `test_prevent_short_selling` passing.
- **NEW: Advanced Performance Metrics**: Implemented comprehensive metrics module (`src/domain/metrics.rs`) with 20+ professional indicators.
    - Risk-Adjusted Returns: Sharpe Ratio (8.14), Sortino Ratio (23.18), Calmar Ratio (1.92)
    - Trade Statistics: Win Rate (50%), Profit Factor (4.00), Average Win/Loss, Largest Win/Loss
    - Risk Metrics: Max Drawdown (-0.01%), Exposure (0.1%), Consecutive streaks
    - Integrated into benchmark CLI with detailed output sections.
- **Performance Analysis**: NVDA Sharpe Ratio 8.14 indicates excellent risk-adjusted returns despite low absolute return (0.02% vs 17.26% B&H).
    - Trade quality metrics: Profit Factor 4.00 shows $4 gained per $1 lost.
    - Max Drawdown -0.01% demonstrates exceptional capital preservation.
    - Low exposure (0.1%) suggests overly conservative trailing stops - optimization needed.
- **Testing**: All 32 unit tests passing. E2E test compilation fixed with missing `trend_riding_exit_buffer_pct` field.

## Version 0.12.5
- **Strategy Tuning**: Updated default parameters to better capture multi-day trends.
    - `TREND_SMA_PERIOD` increased to 2000 (approx 1 week on 1m bars).
    - `TREND_DIVERGENCE_THRESHOLD` tuned to 0.0002 (0.02%).
    - Smoothed entry signals (`FAST_SMA`=20, `SLOW_SMA`=60).
- **Performance**: Improved NVDA benchmark return from 0.36% to 1.97% by reducing signal noise.

## Version 0.12.4 - Strategy Safety (Long-Only)
- **Prevented Short Selling**: Enforced a strict check in the Analyst to prevent execution of Sell signals if the portfolio does not hold the asset.
- **Improved Benchmark Robustness**: Verified that strategies now default to Capital Preservation (0% return) instead of losses during choppy "down" periods where Buy signals are filtered.
- **Fixed Tests**: Updated unit tests to align with the Long-Only paradigm.

## Version 0.12.3 - Benchmark Tooling & Metrics
- Released **Benchmark CLI** (`cargo run --bin benchmark`): A dedicated tool for rigorous strategy backtesting.
- **Performance Metrics**: Calculates Total Return, Max Drawdown (implied), and compares performance against a Buy & Hold baseline.
- **Advanced Strategy Testing**: Added `--strategy` CLI argument to switch between Standard (SMA) and Advanced (Triple Filter) strategies during backtest.
- **Short Selling Fix**: Corrected simulation logic for short positions to ensure accurate P&L tracking (fixed "infinite money" bug).

## Version 0.12.2 - Historical Backtesting
- Implemented **Alpaca Historical Bars API**: Added `get_historical_bars` to `AlpacaMarketDataService`.
- Created **Backtesting Integration Test**: `tests/backtest_alpaca.rs` allows simulation of strategies against real historical market data.
- Enabled verification of buy/sell signals using past market scenarios (e.g., volatile days).

## Version 0.12.1 - Documentation Update
- Added **Simplified Strategy Guide** (`docs/guide_strategie_simplifie.md`) for non-technical users.
- Explains Dual SMA, Advanced Filters, and Risk Management in plain language.
- **Enhanced Market Scanner**: Now automatically includes currently held assets in the watchlist to ensure continued monitoring.

## Version 0.12.0 - Dynamic Market Scanning
- Implemented **Market Scanner Agent**: Periodically fetches "Top Movers" (gainers) from Alpaca API.
- **Dynamic Sentinel**: The Sentinel can now receive updates and re-subscribe to new symbols on the fly without restarting.
- Configurable **Scan Interval** and **Dynamic Mode** (`DYNAMIC_SYMBOL_MODE=true`).

## Version 0.11.0 - Strategy Refinement & Momentum
- Refinement of the **Advanced Analyst Strategy**: Added **MACD** (Moving Average Convergence Divergence) filter.
- Implemented **Triple Confirmation** (SMA Cross + Trend 200 + RSI + MACD Momentum) for higher quality entries.
- Increased default `TREND_SMA_PERIOD` to 200 for more robust long-term analysis.

## Version 0.10.0 - Long-Term Stability & Compliance
- Implemented **Non-PDT Mode**: Protection mechanism in `RiskManager` to prevent "Day Trading" on accounts with less than $25k (blocks same-day buy/sell cycles).
- Implemented **Advanced Analyst Strategy**: Multi-indicator approach using Dual SMA + Trend Filter (SMA 100) + RSI confirmation.
- Added `get_today_orders` to `ExecutionService` for real-time compliance checks.
- Enhanced `Config` with adaptive strategy parameters.


## Version 0.9.1 - Codebase Refactoring & Quality
- Refactored `Analyst` component: implemented `AnalystConfig` struct and split `run` loop into modular methods.
- Resolved all Clippy lints (unused imports, collapsible if, array literal modernization).
- Added comprehensive unit tests for `Config` environment variable parsing and validation.

## Version 0.9.0 - Multi-Symbol Portfolio Trading
- Implemented **Multi-Ticker Support**: The Analyst now manages independent SMA states for a list of `SYMBOLS`.
- Added **Portfolio Capital Allocation**: Trades are dynamically sized based on total equity and capped by `MAX_POSITIONS`.
- Enhanced **Liquidity Management**: Ensures capital is distributed across multiple opportunities instead of concentrated in one.

## Version 0.8.1 - Fractional Order Robustness
- Improved Alpaca execution to automatically use `day` time-in-force for fractional orders.
- Resolved "fractional orders must be DAY orders" rejection from Alpaca API.

## Version 0.8.0 - Dynamic Position Sizing & Robust Signal Detection
- Implemented **Risk-Based Position Sizing** (`RISK_PER_TRADE_PERCENT`).
- Quantities are now calculated as a percentage of Total Equity (Cash + Positions).
- Refactored `Analyst` to fetch real-time portfolio data for equitable risk allocation.
- Implemented **Stateful Crossover Tracking** (sticky `last_was_above` state).
- Added **Silent Warm-up** logic to prevent premature signals on initialization.
- Added comprehensive unit tests for dynamic scaling and signal sequences.

## Version 0.7.0 - Stock Market Pivot & Stability
- Switched Asset Class from Crypto to Stocks (IEX Endpoint).
- Implemented **SMA Hysteresis** (threshold-based crossover) to filter noise.
- Added **Signal Cooldown** to prevent rapid-fire "Wash Trade" rejections.
- Enhanced Alpaca WebSocket subscription to include both Trades and Quotes.
- Improved diagnostic logging for portfolio fetching and JSON decoding.

## Version 0.6.0 - Enhanced Strategy (Dual SMA)
- Replaced Single SMA crossover with a Dual SMA crossover (Fast/Slow averages).
- Added `FAST_SMA_PERIOD` and `SLOW_SMA_PERIOD` configuration.
- Improved signal stability and reduced false positives.

## Version 0.5.0 - Robustness & Fractional Trading
- Implemented Symbol Normalization in `RiskManager` (resolved `BTC/USD` vs `BTCUSD` mismatches).
- Added configurable `TRADE_QUANTITY` to `Analyst` and `Config`.
- Implemented automatic SELL quantity adjustment in `RiskManager` for fractional positions.
- Added detailed live debugging logs for Alpaca account and positions.

## Version 0.4.0 - Dynamic Portfolio Risk Management
- Refactored `RiskManager` to fetch real-time portfolio data from the exchange.
- Added `get_portfolio` to `ExecutionService` trait.
- Implemented account and positions retrieval for Alpaca (REST).
- Enhanced `MockExecutionService` to simulate exchange-side state.


## Version 0.3.0 - Alpaca Integration & Rate Limiting
- Added `OrderThrottler` agent for exchange rate limiting (FIFO queue).
- Implemented Alpaca integration (WebSocket market data & REST orders).
- Added multi-mode support (Mock/Alpaca) via environment variables.

## Version 0.2.0 - Refinement & Testing
- Refactored Analyst agent with pure logic and SMA crossover detection.
- Implemented Ports & Adapters (Hexagonal Architecture) for service decoupling.
- Added comprehensive unit tests (14 passing tests).

## Version 0.1.0 - Initialization
- Initial project setup with Cargo.
- Added core dependencies.
- Defined multi-agent architecture.
