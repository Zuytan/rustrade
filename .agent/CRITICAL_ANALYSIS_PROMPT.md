# Prompt d'Analyse Critique du Projet Rustrade

## Instructions
Ce prompt est conçu pour effectuer une analyse critique approfondie du projet Rustrade, à la fois sur les aspects financiers et logiciels. Utilisez ce template pour obtenir une évaluation complète et actionnable du projet à n'importe quel stade de son développement.

---

## Prompt à soumettre

```
Effectue une analyse critique approfondie et sans concession du projet Rustrade, en suivant les axes suivants :

## 1. ANALYSE FINANCIÈRE

### 1.1 Coûts de Développement et Maintenance
- Évalue le coût estimé de développement (temps-homme, ressources investies)
- Analyse les coûts récurrents (APIs, infrastructure, services tiers)
- Identifie les dépenses cachées ou sous-estimées
- Projette les coûts futurs de maintenance et d'évolution

### 1.2 Performance Économique du Système de Trading
- Analyse la stratégie de gestion du risque et son efficacité
- Évalue les métriques de performance (Sharpe ratio, max drawdown, win rate)
- Compare les résultats attendus vs les standards de l'industrie
- Identifie les faiblesses dans la logique de trading qui pourraient entraîner des pertes
- Analyse la robustesse face aux différentes conditions de marché (bull, bear, sideways)

### 1.3 Viabilité Commerciale
- Évalue le potentiel de monétisation du projet
- Analyse le retour sur investissement (ROI) potentiel
- Identifie les risques financiers majeurs
- Compare avec les solutions existantes sur le marché (coût/bénéfice)

### 1.4 Scalabilité Financière
- Évalue la capacité à gérer des volumes de capital croissants
- Analyse les limitations dues aux coûts d'API ou de trading
- Identifie les points de friction pour la mise à l'échelle

---

## 2. ANALYSE LOGICIELLE

### 2.1 Architecture et Design
- Évalue la cohérence et la clarté de l'architecture (DDD, séparation des couches)
- Identifie les violations de principes SOLID ou de l'architecture hexagonale
- Analyse la modularité et le couplage entre composants
- Identifie les dettes techniques architecturales
- Évalue la conformité aux patterns et best practices Rust

### 2.2 Qualité du Code
- Analyse la lisibilité et la maintenabilité du code
- Identifie le code dupliqué, complexe ou obscur
- Évalue la gestion des erreurs et des edge cases
- Analyse l'utilisation idiomatique de Rust (ownership, lifetimes, traits)
- Identifie les anti-patterns et code smells
- Évalue la documentation du code (commentaires, doc strings)

### 2.3 Couverture et Qualité des Tests
- Analyse la couverture de tests (unitaires, intégration, E2E)
- Évalue la pertinence et la robustesse des tests
- Identifie les zones non testées ou sous-testées
- Analyse la stratégie de TDD et son application réelle
- Évalue la qualité des mocks et fixtures

### 2.4 Performance et Optimisation
- Identifie les goulots d'étranglement potentiels
- Analyse l'efficacité des algorithmes utilisés
- Évalue la gestion de la mémoire et des ressources
- Identifie les opportunités d'optimisation
- Analyse la concurrence et l'utilisation d'async/await

### 2.5 Sécurité et Fiabilité
- Identifie les vulnérabilités de sécurité potentielles
- Analyse la gestion des secrets et des credentials
- Évalue la robustesse face aux erreurs et panics
- Analyse la gestion des données sensibles
- Évalue les mécanismes de circuit breaker et fault tolerance

### 2.6 Maintenabilité et Évolutivité
- Évalue la facilité d'ajout de nouvelles fonctionnalités
- Analyse la documentation projet (README, guides, architecture docs)
- Identifie les points de friction pour les nouveaux développeurs
- Évalue la stratégie de versioning et de changelog
- Analyse la dette technique accumulée

### 2.7 Dépendances et Ecosystem
- Analyse la pertinence et la santé des dépendances externes
- Identifie les dépendances obsolètes ou non maintenues
- Évalue les risques de sécurité dans les dépendances
- Analyse la gestion des versions et des mises à jour

### 2.8 DevOps et Déploiement
- Évalue la stratégie de CI/CD (si applicable)
- Analyse la facilité de déploiement et de rollback
- Identifie les risques opérationnels
- Évalue le monitoring et l'observabilité

---

## 3. ANALYSE COMPARATIVE

### 3.1 Benchmarking
- Compare le projet avec des solutions open-source similaires
- Identifie les fonctionnalités manquantes par rapport aux leaders du marché
- Évalue les avantages compétitifs du projet

### 3.2 Standards de l'Industrie
- Évalue la conformité aux standards de trading algorithmique
- Compare avec les best practices de l'industrie fintech
- Identifie les écarts critiques

---

## 4. SYNTHÈSE ET RECOMMANDATIONS

### 4.1 Points Forts
Liste les forces majeures du projet (maximum 5-7 points)

### 4.2 Points Faibles Critiques
Liste les faiblesses qui représentent des risques majeurs (maximum 5-7 points)

### 4.3 Recommandations Prioritaires
Fournis 5-10 recommandations actionnables, classées par priorité (P0 = critique, P1 = important, P2 = souhaitable), avec pour chaque recommandation :
- Description du problème
- Impact si non résolu
- Estimation d'effort (faible/moyen/élevé)
- Retour sur investissement attendu

### 4.4 Roadmap Suggérée
Propose un plan d'action sur 3-6 mois pour adresser les points critiques

### 4.5 Score Global
Attribue une note globale sur 10 pour chacun des aspects suivants :
- Qualité du code : X/10
- Architecture : X/10
- Tests : X/10
- Performance : X/10
- Sécurité : X/10
- Maintenabilité : X/10
- Viabilité financière : X/10
- Stratégie de trading : X/10

**Score moyen global : X/10**

---

## FORMAT DE LA RÉPONSE
- Sois direct et sans concession
- Utilise des exemples concrets tirés du code
- Fournis des références à des fichiers/lignes spécifiques
- Priorise les problèmes par impact et urgence
- Propose des solutions actionnables, pas seulement des critiques
- Utilise un langage technique approprié mais accessible
```

---

## Notes d'Utilisation

1. **Quand utiliser ce prompt** :
   - Avant une release majeure
   - Après l'ajout de fonctionnalités significatives
   - Lors d'une revue trimestrielle/semestrielle
   - Avant une présentation à des investisseurs ou partenaires
   - Quand la dette technique semble s'accumuler

2. **Adaptations possibles** :
   - Ajouter des sections spécifiques selon l'évolution du projet
   - Ajuster les axes financiers si le projet évolue vers un produit commercial
   - Intégrer des métriques spécifiques à mesurer (ex: latence, throughput)

3. **Complément recommandé** :
   - Exécuter ce prompt avec accès complet au codebase
   - Fournir les métriques de performance récentes si disponibles
   - Inclure les résultats de backtesting ou paper trading
   - Partager les logs d'erreurs ou incidents récents
