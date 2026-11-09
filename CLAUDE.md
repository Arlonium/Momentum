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
- **Commits** : petits, un sujet chacun, **sans aucune ligne d'attribution d'outil** (ni `Co-Authored-By: Claude…`, ni `Claude-Session:`, ni « Generated with Claude Code »), dans les commits comme dans les PR. Ne jamais pousser de secrets (clés, jetons, mots de passe).

## Pièges déjà rencontrés en phase 0

- **yrs** : compiler avec la feature `sync` (sinon `Awareness` ne peut pas être partagé entre tâches tokio). Toujours créer les `Doc` avec `OffsetKind::Utf16`, pour compter les positions comme Yjs dans le navigateur.
- **yrs, interblocage** : appeler `get_or_insert_xml_fragment` (et les autres `get_or_insert_*`) **avant** d'ouvrir une transaction. Avec la feature `sync`, l'appel attend le verrou que la transaction détient déjà.
- **yrs** : l'ordre des attributs dans `get_string()` n'est pas stable, donc ne jamais comparer ces chaînes dans un test.
- **yrs, positions** : `XmlTextRef::get_string()` écrit les marques en balises (`<bold>…</bold>`, `<link href=…>`). Ne jamais calculer une position dessus : compter sur le texte brut issu de `diff()`. Le spike corrompait ainsi le texte après du gras ou une mention, en répondant 200 (trouvé à la relecture finale).
- **BlockNote 0.57** utilise Yjs 14 RC par défaut (`@blocknote/core/y`). On utilise le chemin **Yjs 13** : `withCollaboration` de `@blocknote/core/yjs`, avec `y-prosemirror` 1.x. yrs 0.28 ne lit pas le texte des nœuds Yjs 14.
- **Page neuve** : c'est le serveur qui écrit la racine BlockNote (`blockGroup > blockContainer > paragraph`). Sinon deux clients créent chacun la leur et ProseMirror en supprime une, avec son texte. Cela ne protège que les éditions faites **après la première synchro** : avec le rendu local d'abord, il faut une racine déterministe ou une édition bloquée jusqu'à la synchro.
- **Déplacements concurrents** : avec Yjs, un déplacement vaut copie + suppression. Deux personnes qui déplacent le même bloc le dupliquent, et **les éditions faites en même temps sur l'original sont perdues**. Une déduplication ne les récupère pas : les déplacements de blocs doivent passer par le serveur.
- **y-prosemirror supprime du document partagé** tout nœud ou marque qu'il ne sait pas construire (`sync-plugin.js`, `catch` → `_item.delete`). Un client plus ancien efface donc chez tout le monde les blocs d'une version plus récente : versionnage de schéma obligatoire, client trop ancien en lecture seule.
- **Format Yjs de BlockNote** : attributs typés (`level: 4`, `checked: true`). Marques simples = `{}`, couleur = `{"stringValue": …}`, lien = `{"href": …}`. `columnList` et `column` ne sont pas enveloppés dans un `blockContainer`. Détail dans `spike/mmd/src/ydoc.rs`.
- **MMD** : règles de format (espaces de bord `&#32;`, `<p/>`, ancres, marques en Markdown ou en balises…) dans la conception, §6.2.
- **sqlx 0.9** : le SQL construit dynamiquement doit être marqué `sqlx::AssertSqlSafe(...)`. Valide les noms de colonnes et lie toujours les valeurs en paramètres.
- **SQLite + JSON** : une requête doit reprendre **l'expression exacte** de l'index d'expression (un `CAST` le rend inutilisable : 13 ms → 707 ms). Rollups et formules toujours matérialisés.
- **Révision pour les agents** : hash de l'instantané Yjs (vecteur d'état **et** suppressions), pas du seul vecteur d'état.
- **Lecture seule côté client** : `editable = false` ne suffit pas. y-prosemirror supprime un nœud inconnu dès l'affichage, même sans frappe, et la suppression part au serveur. Un onglet d'un autre schéma lie donc son éditeur à une copie locale du document, qui reçoit tout et ne renvoie rien (`apps/web/src/editor/collab.ts`, e2e `stale_schema_is_read_only`).
- **Serveur mis à jour pendant qu'un onglet est ouvert** (R35) : la version de schéma n'est pas lue qu'au chargement. Chaque WebSocket de synchro d'une session modifiable relit `/api/v1/info` **avant** de s'ouvrir (`editor/schemaGate.ts`) ; si elle diffère, rien ne part, et l'application passe en lecture seule pour de bon, sur des `Y.Doc` neufs (jamais l'ancien document, qui porte des éditions locales), sans titre, création de page ni présence (e2e `stale_schema_after_reconnect_is_read_only`). Sans cela, le serveur ignore en silence tout ce que tape l'onglet.
- **Verrou de l'éditeur** (R34) : jusqu'à la première synchro seulement, pas à chaque coupure, pour qu'on puisse écrire hors ligne.
- **Élément étranger écrit par MMD** (R33) : `PUT /mmd` écrit un `<block … foreign=true/>` en bloc custom `unknown` de BlockNote ; recréé en élément étranger, y-prosemirror le supprimait chez tous dès l'ouverture de la page.
- **`@blocknote/code-block`** : ses `codeBlockOptions` mettent `javascript` en langue par défaut ; le registre (et BlockNote) disent `text`. `schema.tsx` la remet à `text`, et `registry_matches_blocknote` le vérifie. Une langue hors de la liste de son menu (`mon langage`, écrite par MMD) fait échouer l'affichage du bloc (« Language … is not supported ») et bloque la synchro de toute la page : `schema.tsx` admet toute langue (menu sans choix, pas de coloration).
- **Largeurs de tableau** : une par colonne de la grille, comme `columnWidths` de BlockNote ; dans Yjs, `colwidth` d'une cellule fusionnée porte une valeur par colonne couverte (`[120, undefined]`). Au plus 100 colonnes (`MAX_WIDTH_COLUMNS`).
- **Shell (Git Bash sous Windows)** : pas de backticks dans un heredoc non protégé (`<<EOF`), ils sont exécutés. Utilise `<<'EOF'`, ou écris les fichiers avec l'outil d'écriture.
- **Fins de ligne** : `.gitattributes` impose LF. Les fixtures MMD sont comparées octet pour octet au rendu, et une extraction en CRLF (Git pour Windows règle `core.autocrlf=true` par défaut) fait échouer `fixture_is_canonical`.

## Tester

Structure du dépôt (conception, §8) :
- `crates/mementum-*` : le serveur Rust (workspace Cargo à la racine) ;
- `apps/web` : le client React + Vite + BlockNote (`src/editor`, `src/sidebar`, `src/page`, tests e2e dans `e2e/`) ;
- `packages/protocol` : types TypeScript générés depuis Rust (ts-rs), à ne pas modifier à la main ;
- `spec/schema/blocks.json` (registre des blocs, généré) et `spec/mmd/fixtures/` ;
- `tools/check-npm-licenses.mjs` : licences npm ;
- `spike/` : prototype jetable de la phase 0, hors des deux workspaces.

Commandes (à la racine ; workspace pnpm `apps/*` et `packages/*`) :
- Rust : `cargo test --workspace` ;
- web : `corepack pnpm install`, puis `corepack pnpm -C apps/web test` (Vitest : `registry_matches_blocknote` compare le schéma BlockNote au registre), `corepack pnpm -C apps/web typecheck` (`tsconfig.json` pour `src`, sans les types de Node ; `tsconfig.node.json` pour `e2e/` et les fichiers de configuration), `node tools/check-npm-licenses.mjs` ;
- e2e : `corepack pnpm -C apps/web exec playwright install chromium` une fois, puis `corepack pnpm e2e`. Playwright compile et lance `mementum serve` sur un dossier de données temporaire, puis Vite. Sous Windows, un `mementum` déjà lancé depuis `target/debug` empêche la recompilation (« Accès refusé ») : sans l'arrêter, compiler ailleurs avec `CARGO_TARGET_DIR=target/e2e`.

- **Fixtures BlockNote** (`e2e/fidelity.spec.ts`) : `/p/<id>?fixture=<nom>` (développement seulement) charge `src/editor/fixtures/<nom>.input.json` dans une page neuve. `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec playwright test e2e/fidelity.spec.ts` réécrit `spec/mmd/fixtures/yjs/<nom>.bin` et `blocknote/<nom>.json` ; sans la variable, le test compare. Le `<nom>.mmd` attendu s'écrit à la main d'après le §6.2, jamais recopié de la sortie du serveur.
- **Ports** : `MEMENTUM_E2E_WEB_PORT` (défaut 5173) et `MEMENTUM_E2E_API_PORT` (défaut 8080), lus par Vite et Playwright. Les e2e ne réutilisent jamais un serveur (`reuseExistingServer: false`) et s'arrêtent si un port est pris, en le nommant. Sur le poste de développement, un autre projet occupe souvent 5173 : `MEMENTUM_E2E_WEB_PORT=5174`. Ne jamais arrêter un processus qu'on n'a pas lancé.
- **Mandataire Vite** : pas de `changeOrigin` vers le serveur, qui refuse un `Host` hors de la boucle locale (`421 bad_host`, R31).
- **`window.__mementum`** (`editor`, `provider`, `doc`, `synced`, `readOnly` ; type dans `src/editor/debug.ts`) n'existe qu'en développement : les e2e s'en servent. `synced` dit que la première synchro est faite et reste vrai après une coupure : un test qui attend une resynchro lit `provider.synced`. Une session recréée (passage en lecture seule) pose un nouvel objet.
- **pnpm** absent du PATH : `corepack pnpm …` (les scripts de la racine passent déjà par corepack).

Prototype de la phase 0 (`spike/`, voir le README) :
- Playwright y réutilise tout serveur déjà présent sur 5173 ou 8080 (`reuseExistingServer`) : vérifie que les ports sont libres.
- **`measure`** lance le binaire `spike-server` posé à côté de lui : `cargo build --release -p spike-server` d'abord. Il crée aussi la page `bench-10k` dont l'e2e `open_10k_block_page` a besoin (sinon le test est ignoré).
- **Fixtures** : l'e2e `export_all_blocks_fixture` ne les régénère que sur demande (`EXPORT_FIXTURES=1`), car il réécrit des fichiers commités avec des ids neufs.
