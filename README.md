# Mementum

> **Écrivez ensemble, en direct, chez vous.**

Mementum est un espace de travail **open source et auto-hébergeable** de la famille de Notion :
- pages en blocs ;
- bases de données relationnelles (relations, rollups, formules, vues) ;
- équipes et permissions ;
- édition à plusieurs en direct ;
- contenu rangé dans des **espaces** à la manière de Notion : chacun crée les siens (« Perso », « Pro »…) et y invite qui il veut, dans tout l'espace ou sur une page.

Il est **conçu dès le départ pour que les agents IA lisent et écrivent les pages proprement** : format Markdown canonique (MMD), serveur MCP intégré, agents membres de l'espace avec des droits, un journal et un mode suggestion.

*Mementum* = *memento* (« souviens-toi ») + *momentum* (l'élan) : la mémoire de l'équipe et de ses agents, toujours en mouvement.

---

## Où en est le projet (9 novembre 2026)

| Étape | État |
|---|---|
| Recherche (Notion 2026, alternatives open source, architecture temps réel, accès des agents IA) | ✅ faite : [`docs/research/`](docs/research/) |
| Conception complète | ✅ validée : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md) |
| Maquettes (3 directions, **A « Papier » retenue**) | ✅ faites : [`docs/design/`](docs/design/) |
| Phase 0 : prototype jetable qui valide les choix techniques | ✅ faite sur PC et relue (relecture finale du 9 novembre, voir [`spike/RESULTS.md`](spike/RESULTS.md)) ; ⏳ **mesures ARM repoussées avant la phase 2** (bases de données) |
| Phase 1 « Écrire ensemble » (MVP) | 🚧 en cours, découpée en 7 lots. Lot 1 « Socle et édition en direct » **fait** : 10 tâches closes, relecture finale faite, corrections comprises ([plan](docs/superpowers/plans/2026-11-09-p1-lot1-socle-edition.md), [registre](docs/superpowers/logs/2026-11-09-p1-lot1-ledger.md)). Lots 2 à 7 à venir |

Le lot 1 livre le vrai code (`crates/mementum-*`, `apps/web`, `packages/protocol`, `spec/`) : un arbre de pages, l'édition à plusieurs en direct dans BlockNote et le format MMD de la phase 1, sans comptes ni droits (lot 2). Il laisse cinq limites connues, décrites dans la conception (§9) :
- une édition faite sur un bloc pendant qu'un autre client le déplace est perdue (Ruling R39, à traiter au lot 3) ;
- la réécriture complète d'une page de 10 000 blocs par `PUT /mmd` bloque cette page 2,5 à 4 s selon la machine et sa charge (Ruling R40, remède prévu au lot 5) ;
- `PUT /mmd` refuse une prop hors du registre des blocs, parce qu'un navigateur l'effacerait ensuite sans bruit (Ruling R41) ;
- l'ouverture d'une page ralentit avec le nombre de pages affichées dans l'arbre, et dépasse le budget de 300 ms vers 350 à 400 pages (Ruling R43, correctif prévu au lot 2) ;
- deux avis de sécurité RustSec sont ouverts sans mise à jour possible, sur `libsqlite3-sys` et `smallstr` ; le premier ne touche qu'un serveur qui ouvrirait une base écrite par un tiers, ce que Mementum ne fait pas (Ruling R44).

Le dossier `spike/` est le **prototype jetable** de la phase 0 : on en garde les leçons, les fixtures et les tests d'aller-retour, pas le code.

## Décisions déjà prises

| Sujet | Décision |
|---|---|
| Nom | **Mementum** (Memento écarté : *Memento Database* existe déjà dans la même catégorie) |
| Licence | **AGPL-3.0** pour le serveur et l'application web ([LICENSE](LICENSE)), **Apache-2.0** pour le SDK et la spec du format MMD ; **DCO sans CLA** |
| Modèle économique | Auto-hébergement **entièrement gratuit, avec toutes les fonctionnalités** ; offre hébergée payante **Mementum Cloud** pour ceux qui ne veulent pas gérer de serveur (le cloud vend l'hébergement, jamais des fonctions) |
| Pile | Serveur **Rust** en binaire unique (Axum, tokio, yrs, sqlx), **SQLite** par défaut, Postgres et S3 en option ; client React + Vite + **BlockNote** ; cache SQLite WASM |
| Temps réel | **Yjs** (CRDT) pour le texte des pages, confirmé par la phase 0 face à Loro ; journal d'opérations dont le serveur fait autorité pour l'arbre, les bases et les permissions |
| Organisation du contenu | **Espaces** façon Notion (demande d'Alex du 2026-11-09, à confirmer avec Lenny ; ils remplacent les organisations et les coffres façon Obsidian) : un compte a plusieurs espaces (« Perso », « Pro »), on invite quelqu'un dans l'espace ou sur une page, et les jetons d'API portent sur un espace ou sur des pages |
| Autonomie | Pas de miroir en fichiers ; **import Notion et vault Obsidian dès la phase 1**, export complet sans perte |
| IA | **Serveur MCP dès la phase 1** ; agents en mode suggestion par défaut ; IA indépendante du fournisseur (Claude, GPT, Mistral ou 100 % local via Ollama) |
| Design | Direction **A « Papier »**, la plus proche de Notion pour ne pas dépayser |

La liste complète, avec les raisons, est dans la conception, §12 et §13.

## Lire dans cet ordre pour reprendre le projet

1. **Ce README.**
2. **La conception** : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md). C'est la référence : vision, fonctionnalités, architecture, format MMD, MCP, feuille de route (§9).
3. **Les résultats de la phase 0** : [`spike/RESULTS.md`](spike/RESULTS.md). Ce qui a été mesuré, ce qui est confirmé ou corrigé, et **ce qui change pour la phase 1**.
4. **Le plan de la phase 0** : [`docs/superpowers/plans/2026-11-09-phase0-spike.md`](docs/superpowers/plans/2026-11-09-phase0-spike.md), et son registre de décisions : [`docs/superpowers/logs/2026-11-09-phase0-ledger.md`](docs/superpowers/logs/2026-11-09-phase0-ledger.md).
5. **Si besoin, la recherche** : [`docs/research/2026-11-09-etat-de-l-art.md`](docs/research/2026-11-09-etat-de-l-art.md) (synthèse) et [`docs/research/notes/`](docs/research/notes/) (notes sourcées).
6. **Pour un agent IA** : [`CLAUDE.md`](CLAUDE.md) (conventions et pièges déjà rencontrés).

## Organisation du dépôt

```
LICENSE                     AGPL-3.0
README.md                   ce fichier
CLAUDE.md                   consignes pour les agents IA (Claude Code, etc.)
docs/
  specs/                    conception (la référence)
  research/                 état de l'art + notes sourcées
  design/                   maquettes des 3 directions (A retenue)
  superpowers/
    plans/                  plans d'implémentation (phase 0, lot 1 de la phase 1)
    logs/                   registres d'exécution (décisions prises en cours de route)
crates/mementum-*/          serveur Rust : schema, md, crdt, store, sync, server, cli
apps/web/                   client React + Vite + BlockNote, tests e2e Playwright
packages/protocol/          types TypeScript de l'API, générés depuis Rust
spec/                       registre des blocs (schema/) et fixtures MMD (mmd/fixtures/)
tools/                      contrôle des licences npm
spike/                      phase 0 : prototype JETABLE
  RESULTS.md                mesures et verdict
  server/                   serveur de synchro Yjs (axum + yrs + SQLite) + agent pair CRDT
  mmd/                      convertisseur MMD ⇄ arbre de blocs ⇄ Yjs (format BlockNote)
  dbbench/                  bases de 100 000 lignes (SQLite JSON + index)
  crdtbench/, crdtbench-js/ benchmark Yjs/yrs contre Loro
  web/                      éditeur BlockNote collaboratif + tests e2e Playwright
  fixtures/                 fixtures partagées (MMD canonique, Yjs, BlockNote)
```

La structure cible du vrai projet (crates `mementum-*`, `apps/web`, etc.) est décrite dans la conception, §8.

## Lancer Mementum

Prérequis : Rust 1.96+, Node 24+ et pnpm 11+ (`corepack pnpm` si pnpm n'est pas dans le PATH).

```bash
corepack pnpm install                     # une fois, à la racine
cargo run -p mementum-cli -- serve        # serveur sur 127.0.0.1:8080, données dans ./data
corepack pnpm -C apps/web dev             # dans un autre terminal : Vite sur le port 5173
```

Puis ouvrir `http://localhost:5173`. Vite relaie `/api` et `/sync` (WebSocket comprise) vers le serveur. Le lot 1 n'a pas de comptes : chaque navigateur écrit sous un nom d'invité (« Invité » et 4 chiffres).

Au démarrage, si une migration de schéma attend une base existante, le serveur en écrit d'abord une copie à côté (`data/mementum.db.avant-0002.bak` avant la migration `0002`), et ne démarre pas s'il ne peut pas l'écrire. Chaque tentative refait la copie : si la base a changé depuis la précédente (une migration ratée, puis des pages écrites avec l'ancienne version), la nouvelle prend un nom daté en UTC (`mementum.db.avant-0002.20261110T081500Z.bak`) ; aucune copie n'est écrasée ni supprimée automatiquement. Pour restaurer une copie : serveur arrêté, la remettre à la place de `data/mementum.db`, et supprimer `mementum.db-wal` et `mementum.db-shm` s'ils existent (ils appartiennent à la base remplacée).

> **Ne pas écouter hors de la boucle locale** (`serve --bind 0.0.0.0:8080`, ou une adresse du réseau). Le lot 1 n'a aucune authentification : toute personne qui atteint l'adresse lit et modifie toutes les pages. Les protections contre le DNS rebinding ne valent, elles aussi, que sur la boucle locale. Le serveur l'écrit dans son journal au démarrage.

Autres ports : `MEMENTUM_E2E_WEB_PORT` (Vite) et `MEMENTUM_E2E_API_PORT` (le serveur que vise le mandataire), à donner aussi à `serve --bind 127.0.0.1:<port>`.

Tests :

```bash
cargo test --workspace                                  # Rust
corepack pnpm -C apps/web test                          # Vitest (schéma BlockNote ⇄ registre)
corepack pnpm -C apps/web typecheck                     # TypeScript
node tools/check-npm-licenses.mjs                       # licences npm de production
corepack pnpm -C apps/web exec playwright install chromium
corepack pnpm e2e                                       # Playwright : lance le serveur et Vite
```

Les tests e2e ne réutilisent jamais un serveur déjà lancé : si le port 5173 ou 8080 est pris, ils s'arrêtent en le nommant. Choisir alors d'autres ports, par exemple `MEMENTUM_E2E_WEB_PORT=5174 corepack pnpm e2e`.

## Lancer le prototype de la phase 0

Prérequis : Rust 1.96+, Node 24+, pnpm 11+.

```bash
cd spike
cargo test -p spike-server -p spike-mmd -p spike-db
cargo test --release -p spike-crdt-bench
```

Tests e2e (lancent eux-mêmes le serveur et Vite) :

```bash
cd spike/web
pnpm install
pnpm exec playwright install chromium
pnpm exec playwright test
```

Les ports 5173 et 8080 doivent être libres : Playwright réutilise un serveur déjà lancé, même s'il appartient à un autre projet. Pour régénérer les fixtures BlockNote et Yjs : `EXPORT_FIXTURES=1 pnpm exec playwright test e2e/export-fixture.spec.ts`. Le test de la page de 10 000 blocs est ignoré tant que `measure` n'a pas créé la page `bench-10k`.

Mesures (à refaire sur un Raspberry Pi 4 ou un VPS ARM, voir la fin de `spike/RESULTS.md`) :

```bash
cd spike
cargo build --release -p spike-server   # measure lance ce binaire
cargo run --release -p spike-server --bin measure
cargo test --release -p spike-db -- --ignored budgets_100k --nocapture
cargo run --release -p spike-db -- bench --rows 100000
cargo run --release -p spike-crdt-bench
```

L'éditeur de démo s'ouvre avec `cargo run --release -p spike-server --bin spike-server` (dans `spike/`), puis `pnpm dev` (dans `spike/web/`), à l'adresse `http://localhost:5173/?page=demo&user=Moi`.

## Prochaines étapes

1. **Mesures ARM** de la phase 0 (Pi 4 ou Hetzner CAX11), à faire **avant la phase 2**, et report dans `spike/RESULTS.md`. Le budget le plus à risque est « filtre + tri < 300 ms ».
2. **Lot 2 de la phase 1** : comptes, espaces, droits et corbeille. Son [plan](docs/superpowers/plans/2026-11-10-p1-lot2-comptes-espaces-droits.md) est écrit (14 tâches), sur le modèle des espaces façon Notion, et son [registre](docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md) est ouvert. Il reprend ce que le [registre du lot 1](docs/superpowers/logs/2026-11-09-p1-lot1-ledger.md) lui reporte : arbre des pages et budgets du navigateur (R43, R38), écritures du store en transactions, version de schéma qu'un client ne doit pas pouvoir écraser. Les fusions de tableau écrites par un client Yjs restent à traiter (lot 3).
3. **Lots 3 à 7**, dans cet ordre : collaboration et blocs riches ; recherche ; agents (REST complet, MCP v1, jetons) ; imports et export ; livraison. Ils reprennent les limites connues du lot 1 :
   - lot 3 : déplacements de blocs sans perte d'édition (pistes (a) et (b) du Ruling R39, conception §11), et, là ou au lot 5, un porteur durable pour les props hors registre (Ruling R41) ;
   - lot 5 : écriture MMD par différence, pour ne plus bloquer une page pendant sa réécriture (Ruling R40).

## Méthode de travail

Le projet suit la méthode *superpowers* :
- conception validée avant tout code ;
- plan découpé en tâches ;
- exécution en TDD (test qui échoue, puis code, puis test qui passe) ;
- relecture de toute la branche ;
- chaque écart au plan consigné comme « Ruling » dans le registre.

Les documents vont dans `docs/specs/`, les plans dans `docs/superpowers/plans/`, les registres dans `docs/superpowers/logs/`.

## Licence

Serveur et application web : [GNU AGPL-3.0](LICENSE). SDK, bibliothèques clientes et spécification du format MMD : Apache-2.0, à publier avec ces composants. Contributions sous DCO (`git commit -s`).
