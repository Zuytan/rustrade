# RustTrade Agentic Bot 🦀

## Objectif du Projet
Développer un système multi-agents capable de surveiller le marché des actions et ETF (via Alpaca) et Forex/CFDs (via OANDA), d'analyser les tendances en temps réel et d'exécuter des ordres de manière autonome avec une gestion d'état ultra-précise et sécurisée.

> 🚀 **Production Ready (v0.27.0 - Dec 2025):** **Phase 1 Critical Fixes Complete**. Élimination des race conditions critiques via PortfolioStateManager, prévention des fuites mémoire avec canaux bornés, et résilience API via Circuit Breaker. **125 tests unitaires passent**. Système prêt pour déploiement production.

> 📘 **Nouveau (v0.29.0) :** **Interface Agentique Native**. Transformation en application Desktop interactive. Chat avec l'agent ("buy AAPL 10", "stop"), visualisation temps réel du portefeuille et logs système intégrés via `eframe` et `egui`.
>
> 📘 **Précédent (v0.28.0) :** **Refactoring & Clean Architecture**. Nettoyage complet de la codebase, adoption de structures de configuration pour l'Analyste et les Stratégies, et élimination de tous les warnings Clippy.
>
> 📘 **Précédent (v0.26.0) :** **Durcissement Architectural & Financier**. Protection contre les Deadlocks (Timeouts), Calcul Empirique de l'Espérance de Gain (Historical Win Rate), Protection PDT stricte, et suivi des ordres en temps réel.

> 📘 **Nouveau (v0.25.0) :** Stratégie **"Trend & Profit"** activée par défaut. Transition du Scalping vers le **Swing Trading** avec EMA 50/150, Stops Larges (4x ATR) et Prise de Profit Partielle (+5%).
> 📘 **Nouveau (v0.24.0) :** Support expérimental **OANDA** pour le trading Forex et CFDs, et adaptation **Crypto 24/7**.
> 📘 **Métaux Précieux** : Le trading de l'Or et de l'Argent est désormais possible via les ETFs **GLD** et **SLV** sur Alpaca (voir `metals.env`).

> 📘 **Nouveau  :** Pour une explication simplifiée des stratégies, voir [Guide des Stratégies (Non-Spécialistes)](docs/guide_strategie_simplifie.md).

## Score d'Appétit au Risque (Risk Appetite)

Le bot supporte désormais un **Score d'Appétit au Risque** configurable de 1 à 9, permettant d'ajuster automatiquement les paramètres de trading selon votre tolérance au risque :

- **Scores 1-3 (Conservateur)** : Préservation du capital, positions petites (5-10%), stops serrés (2.0-2.5x ATR).
- **Scores 4-6 (Équilibré)** : Approche modérée, positions moyennes (10-20%), stops modérés (2.5-3.5x ATR). **Le score 5 est le centre exact.**
- **Scores 7-9 (Agressif)** : Recherche de rendement, positions larges (20-30%), stops lâches (3.5-5.0x ATR).

**Configuration** : Définir `RISK_APPETITE_SCORE=5` dans `.env`. Si non défini, les paramètres individuels sont utilisés (rétrocompatibilité).
> ℹ️ **Note** : Pour une liste exhaustive de tous les paramètres de configuration, voir la section [Configuration du README](../README.md#⚙️-configuration).

> 💡 **Évaluation** : Les performances de ces profils peuvent être évaluées via l'outil de benchmark en faisant varier le score de 1 à 9 pour observer l'impact sur le Drawdown et le Return.

## Durcissement Financier (Financial Hardening)

Pour garantir la viabilité économique des stratégies, le bot intègre désormais des mécanismes avancés de protection du capital :


### 1. Prévention des Deadlocks (Architectural Hardening)
- **Timeouts sur Locks** : Tous les verrous critiques (`RwLock` sur Portfolio / Orders) sont désormais protégés par des timeouts (2s).
- **Fail-Fast** : Le système échoue rapidement avec une erreur explicite plutôt que de geler indéfiniment en cas de contention extrême.

### 2. Espérance de Gain Empirique (Empirical Models)
- **WinRateProvider** : Remplacement des hypothèses codées en dur (ex: "60% win rate") par une analyse réelle de l'historique des trades.
- **Historical Back-fill** : Utilise les 30 derniers jours de trading pour calculer la probabilité de gain réelle par symbole avant d'engager du capital.

### 3. Suivi des Ordres Temps Réel (v0.26.0)

- **Flux WebSocket Dédié** : Connexion permanente au flux `trade_updates` d'Alpaca.
- **Réconciliation Instantanée** : Les positions internes sont mises à jour à la milliseconde près lors des exécutions partielles ou totales.
- **Timing Attack Prevention** : Les ordres en attente ("Pending") sont comptabilisés dans l'exposition projetée, empêchant le contournement des limites par saturation d'ordres.

### 4. Exécution Intelligente (Smart Execution)

- **Limit Orders pour les Entrées** : Contrairement aux ordres Market qui garantissent l'exécution mais pas le prix, le bot utilise désormais des ordres **Limit** pour toutes les entrées en position. Cela évite le "Slippage" (glissement) excessif lors de pics de volatilité.
- **Market Orders pour les Sorties** : Les Stop-Loss et Take-Profit restent exécutés au marché pour garantir la sortie de position, la priorité étant la liquidation rapide plutôt que le prix parfait en cas de danger.

### 5. Trading "Cost-Aware" (Conscience des Coûts)

- Avant chaque trade, l'Analyste calcule une **Estimation des Coûts** incluant :
    - **Commissions Broker** (ex: $0.005/share).
    - **Slippage Estimé** (ex: 0.1%).
    - **Spread** (écart achat-vente).
- **Filtre de Profitabilité** : Un signal d'achat est rejeté si l'Espérance de Gain n'est pas au moins **2x supérieure** aux coûts estimés (Break-Even Ratio > 2.0).

### 6. Diversification Sectorielle

- **Gestion des Risques** : Le Risk Manager surveille l'exposition par secteur (Tech, Energy, Crypto, etc.).
- **Plafond d'Exposition** : Si un secteur dépasse `MAX_SECTOR_EXPOSURE_PCT` (ex: 30% du portefeuille), tout nouvel achat dans ce secteur est bloqué, forçant la diversification vers d'autres opportunités.

## Optimisation Adaptative (Adaptive Optimization)

Le bot intègre désormais un système d'optimisation en boucle fermée qui ajuste dynamiquement les paramètres des stratégies en fonction de la performance réelle et du régime de marché :

### 1. Surveillance de Performance (`PerformanceMonitoringService`)
- Capture des instantanés quotidiens de l'équité, du drawdown et du Sharpe ratio.
- **Détection de Régime** : Analyse les bougies récentes pour classifier le marché (Tendance haussière/baissière, Range, Volatile).

### 2. Ré-optimisation Automatique (`AdaptiveOptimizationService`)
- **Évaluation Quotidienne** : Analyse les performances par rapport à des seuils définis (`EVALUATION_THRESHOLDS`).
- **Trigger de Ré-optimisation** : Si la performance se dégrade (ex: Sharpe < 1.0 ou Drawdown > 15%), un processus de re-calcul des paramètres est déclenché.
- **Grid Search Intégré** : Utilise le simulateur pour tester des milliers de combinaisons de paramètres sur les données historiques récentes afin de trouver la configuration optimale pour le régime actuel.

### 3. Transition de Paramètres
- Les nouveaux paramètres sont sauvegardés dans le `StrategyRepository`.
- L'Analyste bascule automatiquement sur les nouveaux paramètres sans redémarrage, assurant une continuité opérationnelle.

## Architecture des Agents

### 1. L'Agent "Sentinel" (Data Ingestion)
- **Rôle**: Oreilles et yeux sur le marché.
- **Responsabilités**:
    - Maintenir les WebSockets (Mock ou Alpaca).
    - Pousser les ticks de prix vers l'Analyst via `mpsc::channel`.
    - **Re-configuration Dynamique** : Capable de changer sa "Watchlist" en temps réel sur ordre du Market Scanner.
    - **Reconnexion Automatique Rapide** : En cas de perte de connexion WebSocket, reconnexion immédiate (0s) avec backoff exponentiel (1s, 2s, 4s, 8s, 16s, cap à 30s).
    - **Heartbeat Proactif** : Envoi de pings toutes les 20 secondes pour détecter rapidement les connexions mortes (timeout pong de 5 secondes).
    - **Restauration Automatique des Souscriptions** : Après reconnexion, les symboles sont automatiquement re-souscrits sans intervention manuelle.

### 2. L'Agent "Market Scanner" (Discovery)
- **Rôle**: L'éclaireur.
- **Responsabilités**:
    - Scanner périodiquement le marché (API Top Movers).
    - Identifier les actifs les plus volatils (Gainers).
    - **Filtrage Qualitatif** : Exclure les penny stocks (<$5), warrants et units pour assurer une meilleure liquidité et sécurité.
    - Ordonner au Sentinel de changer de cible.

### 3. Agent "Analyst" (Strategy)
- **Rôle**: Le cerveau décisionnel.
- **Responsabilités**: Détecter les signaux via trois modes principaux :
    - **Architecture Découplée (v0.18.0)** : L'Analyste est désormais modulaire, déléguant le calcul des indicateurs au `FeatureEngineeringService`, la gestion des signaux au `SignalGenerator` et la gestion des stops au `PositionManager`.
    - **Espérance de Gain Dynamique (`ExpectancyEvaluator`)** : Utilise le régime de marché (`MarketRegime`) pour valider chaque trade. Un Reward/Risk Ratio minimum de 1.5 est désormais exigé pour toute nouvelle position.
    - **Stratégies de Trading** : Supporte Advanced Analyst, Trend Riding, et Mean Reversion avec des paramètres auto-adaptatifs.
    - **Long-Only Safety**: Vérifie systématiquement la possession de l'actif avant une vente.
    - **Smart Execution**: Utilisation d'ordres `Limit` pour maîtriser les coûts à l'entrée.
    - **Architecture Modulaire (v0.28.0)** : Décomposition en moteurs spécialisés :
        - `SizingEngine` : Calcul isolé et testable de la taille des positions (Risk-Based).
        - `TradeFilter` : Validation centralisée des signaux (Coûts, Espérance, Cooldowns).

### 4. Agent "Risk Manager" (Safety Gate)
- **Rôle**: Contrôleur de conformité financier.
- **Responsabilités**: 
    - **Validation des Risques**: Vérifie la taille de position, le drawdown max, et la perte journalière.
    - **Gestion Sectorielle Dynamique (v0.18.0)** : Plus de `sector_map` manuel. Utilise un `SectorProvider` (via Alpaca Asset API) pour identifier le secteur de chaque actif en temps réel et garantir la diversification.
    - **Protection PDT (v0.26.0)**: Blocage strict des ouvertures de positions si le compteur de Day Trades est saturé (>=3) sur un compte < $25k. Utilise la donnée officielle du courtier.
    - **Valuation Temps Réel**: Surveillance continue de l'équité pour déclenchement immédiat des Circuit Breakers.
    - **Active Liquidation (v0.22.0)**: Si un Circuit Breaker est déclenché, le Risk Manager envoie immédiatement des ordres de vente pour TOUTES les positions, bypassant les protections PDT. Objectif: "Cash is King" pendant un krach.
    - **Flash Crash Protection (v0.24.0)** : Utilisation d'ordres **Limit Marketables** (avec tolérance de slippage de 5%) lors des liquidations d'urgence pour éviter les exécutons à prix aberrant sur les carnets d'ordres vides.
    - **Session Continue (Crypto)** : Gestion spécifique des actifs `Crypto` avec réinitialisation automatique des compteurs de perte journalière ("Daily Loss") à 00:00 UTC.
    - **Consecutive Loss Circuit Breaker (v0.29.0)**: Arrêt automatique du trading et liquidation des positions après N (défaut: 3) trades perdants consécutifs. Protection contre les dysfonctionnements de stratégie.
    - **Phantom Position Protection (v0.29.0)**: Suivi précis des ordres "Pending" (remplis mais non synchronisés) avec TTL (Time-To-Live) de 5 minutes pour prévenir les blocages de capital et les Race Conditions.

### 4. L'Agent "Order Throttler" (Rate Limiting)
- **Rôle**: Garde-fou technique.
- **Responsabilités**:
    - Garantir le respect des limites de l'API de l'exchange (ex: 10 ordres/min).
    - Mise en file d'attente (FIFO) des ordres excédentaires.

### 5. L'Agent "Executor" (Order Management)
- **Rôle**: Le bras armé.
- **Responsabilités**:
    - Transmission des ordres via API REST Alpaca ou Mock.
    - Mise à jour du Portfolio interne.
    - **Persistance des Transactions**: Sauvegarde asynchrone de chaque ordre exécuté (succès ou échec) dans une base SQL locale.

## Couche de Persistance (Persistence Layer)
Le bot intègre une architecture de persistance conforme au **Domain-Driven Design (DDD)**. Les agents interagissent uniquement avec des abstractions (`TradeRepository`, `CandleRepository`), tandis que l'implémentation concrète utilise **SQLite** (`rustrade.db`) :

- **Transactions (`trades`)**: Stockage immuable de tous les ordres exécutés (ID, Symbole, Prix, Quantité, Side, Timestamp).
- **Bougies Consolidez (`candles`)**: Historisation des bougies 1-minute générées par le `CandleAggregator` pour analyse post-mortem et replay.
- **Performance**: Utilisation du journal WAL (Write-Ahead Logging) et exécution asynchrone (non-bloquante) via `tokio::spawn`.

## Gestion de l'État du Portefeuille (State Management)
Pour garantir l'intégrité des fonds, le bot maintient une Source de Vérité locale synchronisée avec le courtier.

- **Structure Portfolio**: Utilisation d'un `Arc<RwLock<Portfolio>>` pour permettre une lecture concurrente par l'Analyste et une écriture sécurisée par l'Exécuteur.
- **Synchronisation Initiale**: "Cold Boot" via REST pour récupérer le cash et les positions.
- **Synchronisation Temps Réel**: Mise à jour incrémentale via WebSocket AccountEvents.
- **Boucle de Réconciliation**: Thread de vérification périodique.

## Règles de Sécurité Antigravity
1. **Strict Decimal Policy**: Calculs de cash obligatoirement en `rust_decimal::Decimal`. `f64` interdit pour le cash.
2. **Graceful Shutdown**: Annulation des ordres ouverts en cas d'arrêt.
3. **Circuit Breaker**: Arrêt des achats après 3 échecs de connexion consécutifs.
4. **Paper Trading**: Activé par défaut.

## Vérification & Backtesting

### Tools de Backtesting

- **Utilitaire de Benchmark (`src/bin/benchmark.rs`)**: Outil CLI permettant de simuler l'exécution d'une stratégie sur une période donnée et de calculer des métriques de performance précises.
    - **Métriques Avancées** (v0.13.0+): Sharpe Ratio, Sortino Ratio, Calmar Ratio, Max Drawdown, Win Rate, Profit Factor, Average Win/Loss, Exposure.
    - **Alpha/Beta vs S&P500**: Calcul automatique de l'alpha (rendement excédentaire) et beta (sensibilité au marché) via régression linéaire contre SPY.
    - Support plusieurs modes de stratégie (Standard, Advanced, Dynamic, TrendRiding, MeanReversion).
    - **Batch Mode**: Segmentation de période en fenêtres pour analyse de stabilité.
    - Simule l'exécution des ordres avec gestion précise du portefeuille (Sorties via trailing stops, Cash, Positions).
    - Pairing automatique Buy/Sell pour calcul du P&L réalisé.

- **Optimiseur de Paramètres (`src/bin/optimize.rs`)**: Outil de grid search pour trouver les meilleurs paramètres de stratégie.
    - **Grid Search**: Teste systématiquement toutes les combinaisons de paramètres définis dans un fichier TOML.
    - **Objective Scoring**: Score composite pondéré (Sharpe 40% + Return 30% + WinRate 20% - Drawdown 10%).
    - **Export JSON**: Sauvegarde tous les résultats pour analyse approfondie.
    - **Top-N Ranking**: Affiche les meilleures configurations automatiquement.
    - Exemple: Optimiser fast/slow SMA, RSI threshold, ATR multiplier, etc.

### Harnais de Test

- **Harnais de Test Historique**: Capacité de rejouer des données historiques (Alpaca Bars v2) pour vérifier les décisions de l'Analyste.
- **Trailing Stops Actifs**: Mécanisme de sortie automatique basé sur ATR (Average True Range) pour protection du capital. Surveille en continu les positions et déclenche des ventes quand le prix descend sous le seuil calculé.
- **Support Intégration Continue**: Test d'intégration `tests/backtest_alpaca.rs` et `tests/e2e_trading_flow.rs` prêts pour vérifier les stratégies sur des scénarios réels.
- **90+ Unit Tests**: Couverture complète des modules critiques (Analyst, Risk Manager, Portfolio, Metrics, Simulator, Optimizer).

## Résultats de Benchmark Multi-Actions (Dec 2025)

### Méthodologie d'Évaluation

Test complet de **21 actions diversifiées** à travers 7 secteurs pendant la période "Election Rally" (6 Nov - 6 Déc 2024) pour évaluer la performance du bot dans des conditions réelles de marché.

**Actions Testées :**
- **Tech :** AAPL, MSFT, GOOGL, NVDA, META
- **Mega Cap :** AMZN, TSLA
- **Finance :** JPM, BAC, V, MA
- **Energie :** XOM, CVX
- **Santé :** JNJ, ABBV, LLY
- **Consommation :** WMT, COST, KO
- **Industrie :** CAT, GE

### Résultats Clés

✅ **Infrastructure :** 21/21 benchmarks complétés sans erreur  
⚠️ **Activité de Trading :** Activité minimale (0 trades pour 20/21 actions)  
📊 **Performance Moyenne :** ~0.00% (stratégie en cash)  
💡 **Sélectivité :** La stratégie Advanced a correctement évité les entrées défavorables

### Analyse

La **stratégie Triple Filter (Advanced)** a démontré une **sélectivité extrême** durant cette période :
- **Discipline :** Aucun trade forcé en conditions sous-optimales
- **Préservation du Capital :** Protection du capital en restant en cash
- **Opportunité Manquée :** Coût d'opportunité potentiel durant une période haussière

**Hypothèse :** Les conditions de marché (consolidation post-rally, signaux techniques mixtes, RSI oscillant) n'ont pas satisfait les **trois critères simultanés** requis (EMA Trend + RSI Momentum + Signal Confirmation).

### Recommandations

1. **Tester d'autres régimes de marché** : Flash Crash (Aug 2024), Bull Trend (Feb 2024), Recent Market (Dec 2024)
2. **Optimiser les paramètres d'entrée** : Réduire `RSI_THRESHOLD` de 60 → 55, ajuster `SIGNAL_CONFIRMATION_BARS`
3. **Comparer les stratégies** : Tester `standard` et `mean_reversion` sur la même période
4. **Utiliser le Batch Mode** : Analyser 30-day rolling windows sur l'année complète

**Fichiers Générés :**
- Script de Benchmark : `scripts/benchmark_stocks.sh`
- Résultats CSV : `benchmark_results/stocks_YYYYMMDD_HHMMSS.csv`
- Rapport Détaillé : Voir walkthrough dans `.gemini/antigravity/brain/`

## Production Hardening (v0.27.0) - Phase 1 Critical Fixes

**Élimination des Blocages Production** : Corrections critiques suite à audit de sécurité.

### 1. Race Conditions Éliminées (CRITICAL-01/02)
- ✅ **PortfolioStateManager** : Snapshots versionnés remplacent l'accès direct `Arc<RwLock<Portfolio>>`
- ✅ **Exposure Reservations** : Système de réservation optimiste pour ordres BUY
- ✅ **Staleness Detection** : Rafraîchissement automatique si snapshot > 5s
- ✅ **Periodic Refresh** : Tâche de fond toutes les 2 secondes

### 2. Fuites Mémoire Prévenues (BLOCKER-02)
- ✅ **Canaux Bornés** : market(500), proposal(100), order(50), cmd(10)
- ✅ **Backpressure** : `try_send()` dans Analyst avec logging de congestion
- ✅ **Memory Safety** : Croissance mémoire limitée sous forte charge

### 3. Résilience API (Circuit Breaker)
- ✅ **Fast-Fail** : Rejet immédiat si API down (évite boucles infinies)
- ✅ **Auto-Recovery** : 30s timeout, 2 succès requis pour ré-ouverture
- ✅ **Configuration** : 5 échecs → circuit ouvert

### Validation
- **125 unit tests** ✅ PASSING
- **Backpressure test** ✅ PASSING  
- **Circuit breaker** ✅ TESTED

**Prêt pour Production** : Validation paper trading 24h recommandée avant live.
