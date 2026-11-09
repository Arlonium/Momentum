# Consignes pour les agents IA (Claude Code et autres)

Tu reprends **Mementum**, un espace de travail open source façon Notion, auto-hébergeable et conçu pour les agents IA. Lis ces consignes avant toute action.

## Avant de commencer

1. Lis [`README.md`](README.md) : état du projet, décisions, ordre de lecture.
2. Lis la conception : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md). C'est **la référence**. En cas de conflit, la conception l'emporte sur les plans et sur le code.
3. Lis [`spike/RESULTS.md`](spike/RESULTS.md) : ce que la phase 0 a mesuré, confirmé ou corrigé.
4. Pour comprendre une décision prise pendant la phase 0, cherche « Ruling » dans [`docs/superpowers/logs/2026-11-09-phase0-ledger.md`](docs/superpowers/logs/2026-11-09-phase0-ledger.md).

## Règles

- **Langue** : documentation, commentaires et messages de commit en **français** (accents compris). Identifiants de code en anglais.
- **`spike/` est jetable.** Ne construis pas la phase 1 dessus. Le vrai code va dans la structure du §8 de la conception (`crates/mementum-*`, `apps/web`, …). Ce qu'on garde du spike : les fixtures (`spike/fixtures/`), les tests d'aller-retour MMD et les leçons de `RESULTS.md`.
- **Méthode** : conception validée, puis plan dans `docs/superpowers/plans/AAAA-MM-JJ-<sujet>.md`, puis exécution en TDD (test qui échoue d'abord), relecture finale. Chaque écart au plan va dans un registre sous `docs/superpowers/logs/`, sous la forme `Ruling: <décision> — <pourquoi> — <coût si faux>`.
- **Décisions de Lenny** (propriétaire du projet), à ne pas remettre en cause sans lui :
  - licence AGPL-3.0 ;
  - auto-hébergement gratuit avec toutes les fonctionnalités, et Mementum Cloud payant (le cloud ne vend jamais de fonctions) ;
  - coffres façon Obsidian ;
  - pas de miroir en fichiers, mais import Notion et Obsidian ;
  - MCP dès la phase 1 ;
  - design A « Papier ».
- **Commits** : petits, un sujet chacun, terminés par la ligne d'attribution de l'outil utilisé. Ne jamais pousser de secrets (clés, jetons, mots de passe).

## Pièges déjà rencontrés en phase 0

- **yrs** : compiler avec la feature `sync` (sinon `Awareness` ne peut pas être partagé entre tâches tokio). Toujours créer les `Doc` avec `OffsetKind::Utf16`, pour compter les positions comme Yjs dans le navigateur.
- **yrs, interblocage** : appeler `get_or_insert_xml_fragment` (et les autres `get_or_insert_*`) **avant** d'ouvrir une transaction. Avec la feature `sync`, l'appel attend le verrou que la transaction détient déjà.
- **yrs** : l'ordre des attributs dans `get_string()` n'est pas stable, donc ne jamais comparer ces chaînes dans un test.
- **BlockNote 0.57** utilise Yjs 14 RC par défaut (`@blocknote/core/y`). On utilise le chemin **Yjs 13** : `withCollaboration` de `@blocknote/core/yjs`, avec `y-prosemirror` 1.x. yrs 0.28 ne lit pas le texte des nœuds Yjs 14.
- **Page neuve** : c'est le serveur qui écrit la racine BlockNote (`blockGroup > blockContainer > paragraph`). Sinon deux clients créent chacun la leur et ProseMirror en supprime une, avec son texte.
- **Déplacements concurrents** : avec Yjs, deux personnes qui déplacent le même bloc le dupliquent (suppression + réinsertion). Il faut une passe de déduplication côté serveur (prévue en P1).
- **Format Yjs de BlockNote** : attributs typés (`level: 4`, `checked: true`). Marques simples = `{}`, couleur = `{"stringValue": …}`, lien = `{"href": …}`. `columnList` et `column` ne sont pas enveloppés dans un `blockContainer`. Détail dans `spike/mmd/src/ydoc.rs`.
- **MMD** : règles de format (espaces de bord `&#32;`, `<p/>`, ancres, marques en Markdown ou en balises…) dans la conception, §6.2.
- **sqlx 0.9** : le SQL construit dynamiquement doit être marqué `sqlx::AssertSqlSafe(...)`. Valide les noms de colonnes et lie toujours les valeurs en paramètres.
- **SQLite + JSON** : une requête doit reprendre **l'expression exacte** de l'index d'expression (un `CAST` le rend inutilisable : 13 ms → 707 ms). Rollups et formules toujours matérialisés.
- **Révision pour les agents** : hash de l'instantané Yjs (vecteur d'état **et** suppressions), pas du seul vecteur d'état.
- **Shell (Git Bash sous Windows)** : pas de backticks dans un heredoc non protégé (`<<EOF`), ils sont exécutés. Utilise `<<'EOF'`, ou écris les fichiers avec l'outil d'écriture.

## Tester

Voir la section « Lancer le prototype de la phase 0 » du README. Les tests e2e Playwright démarrent eux-mêmes le serveur Rust (port 8080) et Vite (port 5173).
