# Mementum

> **Écrivez ensemble, en direct, chez vous.**

Mementum est un espace de travail **open source et auto-hébergeable** de la famille de Notion :
- pages en blocs ;
- bases de données relationnelles (relations, rollups, formules, vues) ;
- organisations et équipes ;
- édition à plusieurs en direct ;
- contenu rangé dans des **coffres** à la manière d'Obsidian, privés ou partagés.

Il est **conçu dès le départ pour que les agents IA lisent et écrivent les pages proprement** : format Markdown canonique (MMD), serveur MCP intégré, agents membres de l'espace avec des droits, un journal et un mode suggestion.

*Mementum* = *memento* (« souviens-toi ») + *momentum* (l'élan) : la mémoire de l'équipe et de ses agents, toujours en mouvement.

---

## Où en est le projet (9 novembre 2026)

| Étape | État |
|---|---|
| Recherche (Notion 2026, alternatives open source, architecture temps réel, accès des agents IA) | ✅ faite : [`docs/research/`](docs/research/) |
| Conception complète | ✅ validée : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md) |
| Maquettes (3 directions, **A « Papier » retenue**) | ✅ faites : [`docs/design/`](docs/design/) |
| Phase 0 : prototype jetable qui valide les choix techniques | ✅ faite sur PC : [`spike/RESULTS.md`](spike/RESULTS.md) ; ⏳ **mesures ARM à faire** |
| Phase 1 « Écrire ensemble » (MVP) | ⏭️ prochaine étape : écrire son plan |

Aucun code de production n'existe encore. Le dossier `spike/` est un **prototype jetable** : on en garde les leçons, les fixtures et les tests d'aller-retour, pas le code.

## Décisions déjà prises

| Sujet | Décision |
|---|---|
| Nom | **Mementum** (Memento écarté : *Memento Database* existe déjà dans la même catégorie) |
| Licence | **AGPL-3.0** pour le serveur et l'application web ([LICENSE](LICENSE)), **Apache-2.0** pour le SDK et la spec du format MMD ; **DCO sans CLA** |
| Modèle économique | Auto-hébergement **entièrement gratuit, avec toutes les fonctionnalités** ; offre hébergée payante **Mementum Cloud** pour ceux qui ne veulent pas gérer de serveur (le cloud vend l'hébergement, jamais des fonctions) |
| Pile | Serveur **Rust** en binaire unique (Axum, tokio, yrs, sqlx), **SQLite** par défaut, Postgres et S3 en option ; client React + Vite + **BlockNote** ; cache SQLite WASM |
| Temps réel | **Yjs** (CRDT) pour le texte des pages, confirmé par la phase 0 face à Loro ; journal d'opérations dont le serveur fait autorité pour l'arbre, les bases et les permissions |
| Organisation du contenu | **Coffres** façon Obsidian, à la place des espaces d'équipe : « Mon coffre » privé, coffres partagés à toute l'organisation, à des groupes ou à des personnes, exceptions par page |
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
    plans/                  plans d'implémentation (phase 0)
    logs/                   registres d'exécution (décisions prises en cours de route)
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

Mesures (à refaire sur un Raspberry Pi 4 ou un VPS ARM, voir la fin de `spike/RESULTS.md`) :

```bash
cd spike
cargo run --release -p spike-server --bin measure
cargo test --release -p spike-db -- --ignored budgets_100k --nocapture
cargo run --release -p spike-db -- bench --rows 100000
cargo run --release -p spike-crdt-bench
```

L'éditeur de démo s'ouvre avec `cargo run --release -p spike-server --bin spike-server` (dans `spike/`), puis `pnpm dev` (dans `spike/web/`), à l'adresse `http://localhost:5173/?page=demo&user=Moi`.

## Prochaines étapes

1. **Mesures ARM** de la phase 0 (Pi 4 ou Hetzner CAX11) et report dans `spike/RESULTS.md`. Le budget le plus à risque est « filtre + tri < 300 ms ».
2. **Écrire le plan de la phase 1** « Écrire ensemble » à partir de la conception (§9, ligne P1) et de la section « Ce qui change pour la P1 » de `spike/RESULTS.md` :
   - cache local et chargement paresseux des blocs ;
   - déduplication des blocs côté serveur ;
   - rollups matérialisés (pour la P2).
3. **Implémenter la phase 1** dans la structure cible (§8 de la conception), en repartant de zéro mais en reprenant les fixtures et les tests d'aller-retour MMD du prototype.

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
