# Bilan Technique : Agent Analyst

**Date**: 25 Janvier 2026
**Version**: Rustrade v0.2.x
**Responsable**: Senior Developer (AI)

## 1. État Actuel

L'agent **Analyst** est le "cerveau" du système de trading. Il a récemment subi une refonte majeure pour adopter une architecture en pipeline, améliorant sa lisibilité et sa testabilité.

### Architecture
L'agent repose sur une architecture **Pipeline** (`CandlePipeline`) qui traite chaque bougie en 6 étapes distinctes :
1. **Regime Analysis** : Détection du régime de marché (Bull/Bear/Sideways).
2. **Indicator Updates** : Mise à jour des indicateurs techniques via `SymbolContext`.
3. **Position Sync** : Synchronisation avec le portefeuille réel.
4. **Trailing Stop** : Gestion des stops suiveurs dynamiques.
5. **Signal Generation** : Génération de signaux via `SignalProcessor`.
6. **Evaluation** : Validation finale et création de `TradeProposal`.

### Forces (Strengths)
*   **Modularité Exemplaire** : La séparation entre `Analyst` (orchestrateur), `CandlePipeline` (logique séquentielle) et `SymbolContext` (état) est très propre. Elle facilite l'ajout de nouvelles étapes (ex: filtres ML) sans casser l'existant.
*   **Traitement Robuste des Erreurs** : L'utilisation de `Result` et la gestion des timeouts sur les ordres (via `manage_pending_orders`) rendent l'agent résilient aux pannes d'exécution.
*   **Configuration Typée** : L'extraction de `AnalystConfig` permet une configuration fine et centralisée, incluant les paramètres de risque et de stratégie.
*   **Approche Multi-Timeframe** : La structure `SymbolContext` est prête pour l'analyse multi-temporelle (`timeframe_features`), bien que son utilisation soit encore basique.
*   **Protection du Capital** : Intégration native de la gestion des coûts (Spread/Fees) et validation par `RiskManager` avant toute proposition.

### Faiblesses (Weaknesses)
*   **Complexité du `SymbolContext`** : Cet objet tend à devenir un "God Object". Il contient à la fois l'état des indicateurs, l'historique des bougies, l'état des positions, et des métriques avancées comme l'OFI (Order Flow Imbalance). Cela pourrait compliquer la gestion de la mémoire à l'échelle (ex: 100+ symboles).
*   **Latence de Sync** : L'agent dépend fortement de `ExecutionService::get_portfolio` pour valider ses positions à chaque bougie. Si ce service ralentit (API externe), l'Analyst ralentit.
*   **ML Embryonnaire** : La collecte de données ML (`DataCollector`) est implémentée mais basique (CSV). Il n'y a pas encore de boucle de rétroaction active (inférence) dans le pipeline.
*   **Couplage Stratégie** : Bien que dynamique, le choix de la stratégie est encore très lié à la configuration initiale (`warmup_service`). Le changement dynamique de stratégie en temps réel (Adaptive Switching) est présent mais complexe à tuner.

---

## 2. Projections d'Amélioration

### Court Terme (3-6 mois)
*   **Optimisation de l'État (State Management)** :
    *   Refactorer `SymbolContext` pour séparer les *Données de Marché* (lourdes, partageables) de l'*État de Trading* (léger, propre à l'agent).
    *   Implémenter un cache local plus intelligent pour le Portefeuille afin de réduire les appels à `ExecutionService`.
*   **Inférence ML Temps Réel** :
    *   Intégrer un modèle léger (ex: ONNX ou XGBoost via Rust bindings) directement dans le step 5 du pipeline pour filtrer les faux signaux générés par les indicateurs techniques classiques.
*   **Multi-Timeframe Avancé** :
    *   Activer pleinement la logique de confirmation via Timeframes supérieurs (ex: Signal M1 confirmé par tendance M15).

### Moyen/Long Terme (1-2 ans)
*   **Agent Autonome (Reinforcement Learning)** :
    *   Remplacer les stratégies heuristiques (SMA, RSI) par un agent RL entraîné qui prend des décisions basées sur l'état brut du marché (Price Action + Order Flow).
    *   L'agent apprendrait à optimiser non plus un "Win Rate" mais une "Market Expectancy" globale.
*   **Architecture Micro-Services** :
    *   Si le nombre de symboles explose, extraire `Analyst` en un service distribué capable de scaler horizontalement (Sharding par symboles).
*   **Analyse de Sentiment Avancée** :
    *   Coupler l'Analyst avec un module NLP temps réel (traitant les news) pour ajuster dynamiquement l'appétence au risque (`RiskAppetite`) avant même que les prix ne bougent.

## Conclusion
L'agent Analyst est dans un état **mature et stable**. La dette technique est faible grâce au récent refactoring. Les prochains efforts doivent se concentrer sur l'intelligence (ML/RL) et l'optimisation de la performance (State Management) pour passer d'un bot algorithmique classique à un système de trading adaptatif de pointe.
