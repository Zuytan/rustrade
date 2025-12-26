# Guide des Stratégies Financières (Pour Non-Spécialistes)

Ce document explique le fonctionnement du programme **Rustrade** dans un langage simple, sans jargon technique excessif. L'objectif est de comprendre "pourquoi le robot achète ou vend".

## 1. L'Objectif du Robot
Le programme surveille le marché boursier (actions et crypto-monnaies) en continu pour :
1.  **Acheter** quand le prix commence à monter durablement.
2.  **Vendre** avant que le prix ne s'effondre ou pour prendre ses profits.
3.  **Protéger le capital** en évitant les paris trop risqués.

---

## 2. La Stratégie Principale : "Le Croisement de Moyennes Mobiles" (Dual SMA)

C'est la stratégie de base activée par défaut. Imaginez deux coureurs sur une piste :

*   **Le Sprinteur (Moyenne Rapide)** : Il réagit très vite aux changements de direction du prix. C'est la moyenne des prix sur une courte période (ex: les 2 dernières minutes).
*   **Le Marathonien (Moyenne Lente)** : Il est plus stable et lisse les mouvements brusques. C'est la moyenne des prix sur une période plus longue (ex: les 3 ou 5 dernières minutes).

### Le Signal d'Achat (La Croix Dorée)
Quand le **Sprinteur** dépasse le **Marathonien** en accélérant, cela signifie que la tendance s'inverse à la hausse. Le robot détecte ce dépassement et achète.

### Le Signal de Vente (La Croix de la Mort)
Quand le **Sprinteur** s'essouffle et passe *derrière* le **Marathonien**, cela signifie que la tendance s'inverse à la baisse. Le robot vend pour protéger les gains.

---

## 3. Le Mode "Expert" : Triple Sécurité

Pour éviter d'acheter lors de faux départs, nous activons parfois un "Mode Avancé" qui demande trois conditions supplémentaires avant d'agir. C'est comme demander l'avis de trois experts différents :

1.  **Expert de la Tendance de Fond (Trend SMA)** : "Est-ce que la mer monte ou descend à long terme ?"
    *   *Règle* : On n'achète jamais si, globalement, le marché est en chute libre depuis des heures (même s'il y a un petit rebond de quelques secondes).
    
2.  **Expert de la Température (RSI)** : "Le marché est-il en surchauffe ?"
    *   *Règle* : Si tout le monde a déjà acheté et que le prix est monté trop vite (Surchauffe), on n'achète pas, car le risque de chute brutale est trop élevé.
    
3.  **Expert de l'Accélération (MACD)** : "La hausse est-elle franche ?"
    *   *Règle* : On vérifie que le mouvement a du "peps" (momentum positif).

**Résultat** : Le robot trade moins souvent, mais ses coups sont plus sûrs.

---

## 4. La Stratégie "Trend Riding" (Le Surfeur)

Cette stratégie est conçue pour les grandes tendances (comme une action qui monte régulièrement pendant des heures).

*   **Le Principe** : Imaginez un surfeur sur une vague.
*   **L'Entrée** : Il attend que la vague se forme (Golden Cross au-dessus de la moyenne long terme).
*   **Le "Ride"** : Une fois dessus, il ne descend pas à la moindre petite éclaboussure. Il reste sur la vague tant qu'elle le porte.
*   **La Sortie** : Il ne vend que si le prix passe franchement *sous* la courbe de tendance (avec une marge de sécurité), prouvant que la vague est cassée.

**Avantage** : Permet de garder une position gagnante beaucoup plus longtemps sans vendre trop tôt.

---

## 5. Les Garde-Fous (Gestion des Risques)

C'est sans doute la partie la plus importante. Même avec une bonne stratégie, on peut perdre de l'argent si on parie tout sur un seul coup.

*   **Pas tous les œufs dans le même panier** : Le robot ne met jamais tout l'argent sur une seule action. Il divise le capital pour pouvoir acheter plusieurs actions différentes en même temps.
*   **Contrôle de la mise** : Pour chaque pari, il ne risque qu'un tout petit pourcentage du portefeuille (ex: 1% ou 2%).
*   **La Règle Anti-Day Trading (PDT)** : Aux USA, il est interdit aux petits comptes de faire trop d'allers-retours dans la même journée. Le robot a une sécurité intégrée (`Non-PDT Mode`) qui l'empêche de revendre une action achetée le jour même si cela risque de bloquer le compte.

---

## 6. Le Radar (Scanner de Marché)

Au lieu de surveiller une liste fixe d'actions (comme Apple ou Tesla), le robot possède un **Scanner**.
Régulièrement (ex: toutes les minutes), il interroge le marché pour demander : *"Quelles sont les actions qui bougent le plus en ce moment ?"*.

Il met à jour sa liste de surveillance automatiquement pour toujours se concentrer là où il y a de l'action. C'est ce qui lui permet de ne pas rater les opportunités du moment.

**Note Importante** : Le robot est intelligent. Si vous possédez déjà une action (ex: Apple), il continuera à la surveiller même si elle ne fait plus partie des "Top Movers", pour pouvoir la vendre au bon moment.
