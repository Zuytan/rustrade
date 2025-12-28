# RustTrade Agentic Bot 🦀

## Objectif du Projet
Développer un système multi-agents capable de surveiller le marché des actions et ETF, d'analyser les tendances en temps réel et d'exécuter des ordres de manière autonome avec une gestion d'état ultra-précise et sécurisée.

> 📘 **Nouveau  :** Pour une explication simplifiée des stratégies, voir [Guide des Stratégies (Non-Spécialistes)](docs/guide_strategie_simplifie.md).

## Score d'Appétit au Risque (Risk Appetite)

Le bot supporte désormais un **Score d'Appétit au Risque** configurable de 1 à 10, permettant d'ajuster automatiquement les paramètres de trading selon votre tolérance au risque :

- **Scores 1-3 (Conservateur)** : Préservation du capital, positions petites (5-10%), stops serrés (2.0-2.5x ATR), seuil RSI bas (30-45)
- **Scores 4-7 (Équilibré)** : Approche modérée, positions moyennes (10-20%), stops modérés (2.5-3.5x ATR), seuil RSI médian (45-65)
- **Scores 8-10 (Agressif)** : Recherche de rendement, positions larges (20-30%), stops lâches (3.5-5.0x ATR), seuil RSI élevé (65-75)

**Configuration** : Définir `RISK_APPETITE_SCORE=5` dans `.env`. Si non défini, les paramètres individuels sont utilisés (rétrocompatibilité).

## Durcissement Financier (Financial Hardening)

Pour garantir la viabilité économique des stratégies, le bot intègre désormais des mécanismes avancés de protection du capital :

### 1. Exécution Intelligente (Smart Execution)
- **Limit Orders pour les Entrées** : Contrairement aux ordres Market qui garantissent l'exécution mais pas le prix, le bot utilise désormais des ordres **Limit** pour toutes les entrées en position. Cela évite le "Slippage" (glissement) excessif lors de pics de volatilité.
- **Market Orders pour les Sorties** : Les Stop-Loss et Take-Profit restent exécutés au marché pour garantir la sortie de position, la priorité étant la liquidation rapide plutôt que le prix parfait en cas de danger.

### 2. Trading "Cost-Aware" (Conscience des Coûts)
- Avant chaque trade, l'Analyste calcule une **Estimation des Coûts** incluant :
    - **Commissions Broker** (ex: $0.005/share).
    - **Slippage Estimé** (ex: 0.1%).
    - **Spread** (écart achat-vente).
- **Filtre de Profitabilité** : Un signal d'achat est rejeté si l'Espérance de Gain n'est pas au moins **2x supérieure** aux coûts estimés (Break-Even Ratio > 2.0).

### 3. Diversification Sectorielle
- **Gestion des Risques** : Le Risk Manager surveille l'exposition par secteur (Tech, Energy, Crypto, etc.).
- **Plafond d'Exposition** : Si un secteur dépasse `MAX_SECTOR_EXPOSURE_PCT` (ex: 30% du portefeuille), tout nouvel achat dans ce secteur est bloqué, forçant la diversification vers d'autres opportunités.

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
    - **Dual SMA Crossover** : Stratégie standard de croisement de moyennes mobiles.
    - **Advanced Analyst** : Stratégie "Triple Confirmation" (Crossover + Trend + RSI + MACD) pour ne choisir que les meilleurs moments.
    - **Trend Riding** : Stratégie de suivi de tendance long-terme. Achète sur Golden Cross et maintient la position tant que le prix reste au-dessus de la tendance (avec buffer), ignorant les fluctuations mineures pour capturer les grands mouvements. 
    - **Long-Only Safety**: Par sécurité, l'Analyste vérifie systématiquement que le portefeuille détient l'actif avant d'émettre un signal de Vente, empêchant tout Short Selling involontaire.
    - **Smart Execution**: Utilisation d'ordres `Limit` pour maîtriser les coûts à l'entrée.

### 3. Agent "Risk Manager" (Safety Gate)
- **Rôle**: Contrôleur de conformité financier.
- **Responsabilités**: 
    - **Validation des Risques**: Vérifie la taille de position, le drawdown max, et la perte journalière.
    - **Contrôle Sectoriel**: Bloque les transactions si l'exposition à un secteur dépasse le seuil défini (`MAX_SECTOR_EXPOSURE_PCT`).
    - **Protection PDT**: Empêche le Day Trading pour les petits comptes.
    - **Valuation Temps Réel**: Surveillance continue de l'équité pour déclenchement immédiat des Circuit Breakers.

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
