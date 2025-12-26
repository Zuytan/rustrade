Projet : RustTrade Agentic Bot 🦀

Langage : Rust

🎯 Objectif du Projet

Développer un système multi-agents capable de surveiller le marché des actions et ETF, d'analyser les tendances en temps réel et d'exécuter des ordres de manière autonome avec une gestion d'état ultra-précise et sécurisée.

🏗️ Gestion de l'État du Portefeuille (State Management)

Pour garantir l'intégrité des fonds, le bot maintient une Source de Vérité locale synchronisée avec le courtier.

    Structure Portfolio : Utilisation d'un Arc<RwLock<Portfolio>> pour permettre une lecture concurrente par l'Analyste et une écriture sécurisée par l'Exécuteur.

    Synchronisation Initiale : Au lancement, le bot effectue un "Cold Boot" via REST pour récupérer :

        Le cash disponible (Buying Power).

        Les positions actuelles (Symbole, Quantité, Prix moyen).

    Synchronisation Temps Réel : Mise à jour incrémentale via le flux WebSocket AccountEvents (remplissage d'ordres, dividendes, frais).

    Boucle de Réconciliation : Un thread de vérification compare périodiquement (ex: toutes les 1h) l'état local et l'API du courtier pour corriger toute dérive.

🤖 Architecture des Agents
1. L'Agent "Sentinel" (Data Ingestion)

    Rôle : Oreilles et yeux sur le marché.

    Responsabilités :

        Maintenir les WebSockets (Prix & Événements de compte).

        Pousser les ticks de prix vers l'Analyst via mpsc::channel.

    Stack : tungstenite-rs, tokio-stream.

2. L'Agent "Analyst" (Strategy)

    Rôle : Le cerveau décisionnel.

    Responsabilités :

        Lire le Portfolio pour vérifier l'exposition actuelle.

        Calculer les indicateurs techniques via la crate ta.

        Émettre des TradeProposals basées sur la stratégie (ex: croisement de moyennes mobiles).

3. L'Agent "Risk Manager" (Safety Gate)

    Rôle : Contrôleur de conformité financier.

    Responsabilités :

        Vérification de Solvabilité : proposal.cost < portfolio.cash.

        Gestion du Risque : Vérifier que la position ne dépasse pas X% du capital total.

        Calcul des Protections : Injection automatique de Stop-Loss et Take-Profit sur l'ordre.

4. L'Agent "Executor" (Order Management)

    Rôle : Le bras armé.

    Responsabilités :

        Transmission des ordres via API REST signée.

        Mise à jour du Portfolio dès réception de la confirmation d'exécution.

        Gestion des erreurs (ex: ordre rejeté par le marché).

📦 Dépendances Rust Critiques
tokio,
rust_decimal,
polars
ta
serde
reqwest

🛡️ Règles de Sécurité Antigravity

Strict Decimal Policy : Interdiction d'utiliser f64 pour les calculs de cash. Utiliser rust_decimal::Decimal.

Graceful Shutdown : En cas de Ctrl+C ou d'erreur critique, l'agent Executor doit tenter d'annuler les ordres LIMIT ouverts avant de fermer le programme.

Circuit Breaker : Si le bot subit 3 échecs de connexion consécutifs, toutes les opérations d'achat sont bloquées par le Risk Manager.

Paper Trading : Le bot est conçu pour fonctionner en paper trading par défaut, c'est-à-dire qu'il simule les ordres sur un compte paper et vérifie les ordres sur un compte paper.