# Rapport d'Analyse Architecturelle - Couche de Persistance

## 1. Vue d'ensemble
La couche de persistance a été introduite via `infrastructure/persistence` utilisant SQLite (`sqlx`). Cette analyse vise à vérifier la conformité de cette implémentation avec les principes d'architecture logicielle du projet (Domain Driven Design).

## 2. Problèmes Détectés (Violations DDD)

### 2.1 Violation du Principe d'Inversion de Dépendance (DIP)
**Le problème majeur :** La couche `Application` dépend directement de la couche `Infrastructure`.

*   **Preuve dans `src/application/executor.rs` :**
    ```rust
    // Dépendance directe vers le type concret de l'infrastructure
    use crate::infrastructure::persistence::repositories::OrderRepository;
    
    pub struct Executor {
        // ...
        repository: Option<Arc<OrderRepository>>, // Type concret
    }
    ```
*   **Preuve dans `src/application/system.rs` :**
    L'application instancie directement les repositories concrets et les passe aux agents.

**Impact :** Le cœur du système est couplé à SQLite. Il est impossible de remplacer le stockage (ex: PostgreSQL ou In-Memory pour les tests) sans modifier le code de l'application.

### 2.2 Traits du Domaine Ignorés
**Le problème :** Des abstractions existent mais ne sont pas utilisées.

*   Le fichier `src/domain/repositories.rs` définit le trait `TradeRepository` :
    ```rust
    #[async_trait]
    pub trait TradeRepository: Send + Sync {
        async fn save(&self, trade: &Order) -> Result<()>;
        // ...
    }
    ```
*   Cependant, l'implémentation `src/infrastructure/persistence/repositories.rs` définit une struct `OrderRepository` qui **n'implémente pas** ce trait. Elle expose ses propres méthodes `pub async fn save`.

### 2.3 Nommage Incohérent
*   Le Domaine parle de `TradeRepository`.
*   L'Infrastructure implémente `OrderRepository`.
*   Ubiquitous Language (Langage omniprésent) non respecté.

## 3. Analyse détaillée des fichiers

| Composant | Status | Observation |
|-----------|--------|-------------|
| `domain/repositories.rs` | ✅ Correct | Définit bien les contrats (`TradeRepository`, `PortfolioRepository`). |
| `infra/.../repositories.rs` | ❌ Incorrect | Définit des structs concrets (`OrderRepository`, `CandleRepository`) sans lien avec les traits du domaine. |
| `application/executor.rs` | ❌ Incorrect | Importe `infrastructure::persistence`. Devrait importer `domain::repositories`. |
| `application/analyst.rs` | ❌ Incorrect | Importe `infrastructure::persistence::CandleRepository`. |

## 4. Recommandations de Refactoring

Pour rétablir la conformité DDD et la flexibilité de l'architecture, les actions suivantes sont nécessaires :

1.  **Renommer** `OrderRepository` en `SqliteOrderRepository` dans l'infrastructure pour expliciter que c'est une implémentation.
2.  **Implémenter le Trait** : Faire en sorte que `SqliteOrderRepository` implémente `rustrade::domain::repositories::TradeRepository`.
3.  **Créer une Abstraction pour les Candles** : Ajouter `trait CandleRepository` (ou `MarketDataRepository`) dans `domain/repositories.rs`.
4.  **Inverser les Dépendances** :
    *   Modifier `Executor` pour qu'il attende un `Arc<dyn TradeRepository>`.
    *   Modifier `Analyst` pour qu'il attende un `Arc<dyn CandleRepository>`.
    *   Mettre à jour `Application::build` pour injecter l'implémentation concrète via l'interface abstraite.

## 5. Conclusion

## 6. Résolution (Mise à jour v0.17.0 - Janvier 2026) -> **RESOLU** ✅

Suite à cet audit, un refactoring complet a été effectué dans la version 0.17.0 :
1.  **DIP Respecté** : `Executor` et `Analyst` ne dépendent plus que des traits `TradeRepository` et `CandleRepository`.
2.  **Infrastructure Isolée** : L'injection de dépendance se fait au démarrage (`Application::build`).
3.  **Repositories** : Les implémentations SQLite (`SqliteOrderRepository`) implémentent désormais correctement les traits du Domaine.

L'architecture est maintenant conforme aux principes DDD.
