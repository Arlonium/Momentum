# Reprendre Mementum

Ce document est fait pour quelqu'un, humain ou agent IA, qui ouvre ce dépôt sans rien savoir de son histoire. Il dit où en est le projet, ce qui reste à faire, et comment s'y prendre. Il renvoie aux sources plutôt que de les recopier.

Écrit le 2026-11-10, à la clôture du lot 2 de la phase 1. À mettre à jour à chaque clôture de lot.

## En une minute

- **Mementum** est un espace de travail open source façon Notion, auto-hébergeable, conçu pour que les agents IA lisent et écrivent les pages aussi proprement que les humains. Licence AGPL-3.0.
- **Pile** : serveur Rust en un binaire (axum, sqlx sur SQLite, yrs), client React avec BlockNote dans `apps/web`, texte des pages en CRDT Yjs, format Markdown canonique « MMD » pour les agents.
- **Deux personnes** : Lenny est le propriétaire du projet, ses décisions ne se remettent pas en cause sans lui (liste dans [`CLAUDE.md`](../CLAUDE.md)). Alex pilote le développement et fait la recette.
- **Où on en est** : la phase 0 (essais) est finie. La phase 1 compte sept lots. Les lots 1 et 2 sont faits et poussés. Les lots 3 à 7 restent à faire, et le lot suivant n'est pas encore choisi.

## Dans quel ordre lire

1. [`CLAUDE.md`](../CLAUDE.md) : les règles du projet et tous les pièges déjà rencontrés. À lire en entier avant de toucher au code.
2. [`README.md`](../README.md) : ce que l'application fait aujourd'hui, comment la lancer, ses limites connues, les prochaines étapes.
3. [`docs/specs/2026-11-09-mementum-design.md`](specs/2026-11-09-mementum-design.md) : la conception. C'est la référence : en cas de conflit, elle l'emporte sur les plans et sur le code. Le §9 dit ce que chaque lot livre, le §4 décrit les droits, le §5.6 le contrôle des accès, le §6 le format MMD et les agents.
4. Le registre du dernier lot clos : [`docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md`](superpowers/logs/2026-11-10-p1-lot2-ledger.md). Ses dernières sections (« Relecture finale et vagues de correction », « Clôture du lot 2 », « Après la clôture ») donnent l'état exact, les mesures, ce qui est reporté et ce qui attend le propriétaire.
5. L'en-tête de `crates/mementum-server/src/lib.rs` : le tableau de toutes les routes, avec qui y a droit et les statuts de refus.

Chaque décision prise en cours de route est un « Ruling », numéroté à la suite d'un lot à l'autre : R1 à R44 au lot 1, R45 à R127 au lot 2. On cherche « Ruling R93 » dans les registres pour savoir pourquoi le code fait ce qu'il fait.

## État au 2026-11-10

| Étape | État | Où |
|---|---|---|
| Phase 0, essais | finie, poussée | `spike/`, `spike/RESULTS.md` |
| Phase 1, lot 1 : socle et édition en direct | fini, poussé le 2026-11-09 | plan et registre datés du 2026-11-09 dans `docs/superpowers/` |
| Phase 1, lot 2 : comptes, espaces, droits et corbeille | fini, poussé le 2026-11-10 (`main`, commit `7fb1b55` et suivants) | plan et registre datés du 2026-11-10 |
| Recette du lot 2 par Alex | **à faire** | [`docs/recette/2026-11-10-p1-lot2-recette.md`](recette/2026-11-10-p1-lot2-recette.md) |
| Phase 1, lots 3 à 7 | à faire, aucun plan écrit | voir plus bas |
| Mesures ARM de la phase 0 | repoussées avant la phase 2 | `spike/RESULTS.md` |

Ce que l'application fait aujourd'hui : des comptes avec mot de passe, des espaces (plusieurs par compte), des membres et des invités, des pages en blocs rangées en arbre et en trois sections (« Espace », « Partagé », « Privé »), l'édition à plusieurs en direct avec le curseur de chacun, le partage d'une page à quatre niveaux, les invitations par lien, la lecture seule selon le droit, la corbeille, et le format MMD en lecture et en écriture par l'API.

Ce qu'elle ne fait pas encore : les bases de données et leurs vues, les commentaires, l'historique, la recherche, l'envoi d'e-mails, la connexion par Google ou OIDC, les jetons d'API et le serveur MCP, les imports et l'export, le binaire qui embarque l'application et l'image Docker.

Vérification à la clôture du lot 2, sur un clone neuf, comme la CI : 548 tests Rust, 214 tests du client, 122 parcours de bout en bout, tous verts ; une page de 1 000 blocs s'ouvre en 134 ms (budget 300), une page de 10 000 blocs en 1,2 s (budget 3 s). Le détail est au registre.

## Ce qui attend le propriétaire

Rien de ceci ne se décide à sa place.

1. **La recette du lot 2**, avec la checklist (93 cases). Il rend la liste des numéros qui ne vont pas : chacun est un défaut à corriger avant d'ouvrir le lot suivant, ou une limite à écrire.
2. **Quatre règles à valider**, parce qu'elles se voient à l'usage :
   - les membres ont l'accès complet aux pages d'équipe, comme dans Notion (R48) ;
   - les inscriptions sont sur invitation par défaut (R50) ;
   - un invité en accès complet peut inviter à son tour (R86) ;
   - un simple membre peut fermer une page d'équipe aux autres, après confirmation (R122, R127).
3. **D'autres choix déjà pris, à confirmer** : le menu du compte dans le sélecteur d'espace (R80), une page sortie à la racine garde son accès d'équipe (R94), la barre latérale relue au retour sur l'onglet et non en direct (R97), une session de 30 jours au plus dans un navigateur (R117), les plafonds contre la surcharge reportés au lot 7 (R121), et deux décisions du lot 1 : `PUT /mmd` refuse une propriété inconnue (R41), deux avis de sécurité ouverts sans mise à jour possible (R44).
4. **Le choix du lot suivant.** Voir « Par où continuer ».

Les espaces façon Notion, à la place des coffres façon Obsidian, sont confirmés par Lenny (2026-11-10).

## Ce qui reste à construire

L'ordre prévu des lots est dans le README (« Prochaines étapes ») et la conception (§9). En bref :

| Lot | Sujet | Ce qu'il reprend des lots faits |
|---|---|---|
| 3 | Collaboration et blocs riches : commentaires, historique, arbre des pages et présence en direct, blocs riches | la barre latérale en direct (R97), la présence vérifiée (R109 et les reports de présence ci-dessous), les déplacements de blocs sans perte d'édition (R39), les tableaux mal formés écrits par un client (R54) |
| 4 | Recherche | |
| 5 | Agents : API REST complète, serveur MCP, jetons d'API sur un espace ou sur des pages | l'écriture MMD par différence (R40), un porteur durable pour les propriétés inconnues (R41) |
| 6 | Imports et export | |
| 7 | Livraison : binaire qui embarque l'application, image Docker, fichier `mementum.toml`, inscription par domaine | les plafonds et limites de débit (R54, R121), la relecture périodique des sessions (R125), le contrôle des avis de sécurité en CI (R44) |

Sans date, à décider avec le propriétaire : l'envoi d'e-mails (invitations, mot de passe oublié), la connexion par Google ou OIDC, la suppression d'un espace ou d'un compte, le vidage automatique de la corbeille, les passkeys (R46). Les bases de données et leurs vues sont la phase 2.

### Reports précis du lot 2

La liste complète est au registre du lot 2, section « Reportés au lot suivant ». Les plus importants, dans l'ordre où je les traiterais :

1. **Le client n'a aucune garde à lui contre une présence mal formée.** Un curseur mal formé qui atteindrait un navigateur figerait son éditeur, qui effacerait ensuite les frappes des autres. Aujourd'hui seul le serveur l'empêche (R118, R119). À faire : un `renderCursor` et un filtre des états dans `apps/web/src/editor/Editor.tsx`.
2. **Les mises à jour du document sont relayées telles que reçues.** La question qu'on a fermée pour la présence (le serveur et le navigateur ne lisent pas les octets pareil, R123) reste posée pour elles. À examiner avant d'ouvrir l'instance à des inconnus.
3. **La surcharge par un compte autorisé** (R121) : aucun plafond de connexions de synchro par compte ni par page, et la file d'une connexion qui ne lit pas peut grossir. Prévu au lot 7 ; à avancer si l'instance s'ouvre plus tôt.
4. **La présence après une reconnexion** met jusqu'à 30 secondes à revenir pour une personne qui ne tape pas. Piste écrite au registre : retirer l'état sans avancer l'horloge, et faire annoncer un état neuf par le client à chaque connexion.
5. **Une session supprimée en ligne de commande** (`mementum user set-password`) laisse ouvertes les pages déjà connectées (R125). D'ici là : redémarrer le serveur après la commande.
6. **Le cookie de session n'est pas renvoyé** quand la session est prolongée (R117).
7. Des mineurs de la relecture finale : l'écran du premier compte quand l'état de l'instance a changé entre-temps, une invitation consommée pour rien, un titre non enregistré affiché en lecture seule, une invitation de page sans nom de page dans les réglages, deux tests de concurrence à durcir.

## Par où continuer

Le propriétaire n'a pas choisi. Deux suites se défendent :

- **Le lot 3, comme prévu.** Il lève les limites les plus visibles du lot 2 (barre latérale qui ne suit pas en direct, présence fragile) et apporte les commentaires. C'est la suite naturelle si le but est un Notion agréable à plusieurs.
- **Le lot 5 d'abord.** Alex veut que Claude Code puisse verser du contenu dans un de ses espaces, par un serveur MCP qu'il branche dans son client. C'est la suite si le but est l'usage par les agents. Il s'appuie sur les droits du lot 2 : un jeton agit au nom d'un humain, dans la limite de ses droits (conception §6.7).

Dans les deux cas, commencer par les défauts que la recette aura trouvés, puis par les reports 1 et 2 ci-dessus, qui touchent à l'intégrité du contenu.

## Comment mener un lot

La méthode est écrite dans le README (« Méthode de travail ») et a été suivie telle quelle aux lots 1 et 2 :

1. **Un plan** pour le lot, dans `docs/superpowers/plans/`, écrit d'après la conception : des tâches petites, chacune avec ses fichiers, ses interfaces, ses tests nommés. Le propriétaire le relit avant qu'on commence.
2. **Un registre** pour le lot, dans `docs/superpowers/logs/` : chaque écart au plan y devient un Ruling, au format `Ruling: <décision> — <pourquoi> — <coût si faux>`.
3. **L'exécution tâche par tâche**, en TDD : le test qui échoue d'abord. Aux lots 1 et 2, chaque tâche a été confiée à un sous-agent dans un worktree à part, puis relue par un autre, puis reprise sur `main`.
4. **Une relecture finale de tout le lot**, en passes séparées (sécurité, intégration entre client et serveur, documentation contre le code), puis une vague de correction et des re-relectures ciblées.
5. **Toutes les suites sur un clone neuf**, étape par étape comme la CI, avant de dire que c'est fini.
6. **Une checklist de recette** pour le propriétaire, sur ce qu'un test ne juge pas.

Ce que les deux premiers lots ont appris, et qui ne se devine pas :

- La relecture finale trouve ce que les relectures de tâche ne voient pas. Au lot 2, ses trois passes ont trouvé une perte de frappes, une session qui survivait à un changement de mot de passe et une page qui ne pouvait pas redevenir privée, dans du code dont chaque tâche avait été approuvée.
- Un relecteur qui prouve ses constats par un test jetable vaut mieux qu'un relecteur qui lit. Demander la preuve.
- Une règle de protection est à relire comme une attaque : la première version d'une limite fermait la connexion fautive, et donnait ainsi à un lecteur le moyen de faire fermer celle des autres (R113).
- Les compilations parallèles remplissent le disque (10 à 40 Go par worktree). Supprimer le dossier `target` d'un atelier dès qu'il a rendu.
- Ne jamais relâcher un budget de performance, sa statistique ni sa définition pour faire passer un test.

## Commandes

Elles sont détaillées dans le README. Les plus utiles :

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo test --release -p mementum-sync -- --ignored
cargo deny check advisories
corepack pnpm install --frozen-lockfile
corepack pnpm -C apps/web typecheck
corepack pnpm -C apps/web test
corepack pnpm -C apps/web exec playwright test
corepack pnpm -C apps/web exec playwright test -c playwright.budgets.config.ts
node tools/check-npm-licenses.mjs
```

- `pnpm` n'est pas dans le PATH du poste d'Alex : passer par `corepack pnpm`.
- Les parcours de bout en bout lancent leurs propres serveurs. Pour ne pas gêner une instance qui tourne, leur donner d'autres ports : `MEMENTUM_E2E_WEB_PORT` et `MEMENTUM_E2E_API_PORT`.
- `cargo deny check advisories` signale deux avis connus (R44) : c'est attendu tant que `sqlx` et `yrs` n'ont pas de version qui les lève.
- La CI (`.github/workflows/ci.yml`) lance les mêmes étapes sous Linux, et les tests Rust sous Windows aussi.

## L'instance d'Alex

Alex fait tourner une instance sur son poste pour regarder l'application et faire la recette.

- **Données** : `data/instance/mementum.db`, dans le dépôt (le dossier `data/` est ignoré par git). C'est sa base du lot 1, migrée au lot 2 le 2026-11-10. Une copie d'avant migration est à côté (`mementum.db.avant-0002.bak`), et une sauvegarde de la base du lot 1 est dans `data/sauvegardes/2026-11-10-avant-lot2/`.
- **Lancer le serveur** (PowerShell, à la racine du dépôt) :

  ```
  cargo build --release -p mementum-cli
  .\target\release\mementum.exe serve --bind 127.0.0.1:8090 --data-dir data\instance
  ```

- **Lancer la page** (second terminal) :

  ```
  $env:MEMENTUM_E2E_WEB_PORT = "5175"; $env:MEMENTUM_E2E_API_PORT = "8090"; corepack pnpm -C apps/web dev
  ```

  Puis ouvrir `http://localhost:5175`.
- **Avant toute mise à jour du serveur** : l'arrêter, copier `data/instance` ailleurs, puis relancer. Une migration écrit sa propre copie, mais une sauvegarde à part ne coûte rien.
- **Ne jamais lancer de tests sur les ports 5175 et 8090**, ni sur `data/instance`.
- La page est servie par Vite depuis l'arbre de travail : dès que le code du client change sur `main`, la page change aussi. Si le client et le serveur ne sont plus de la même version, la page dit « Impossible de joindre Mementum » : recompiler et relancer le serveur.
- Avant d'ouvrir l'instance à quelqu'un d'autre (tunnel) : lire les limites connues du README, et n'inviter que des personnes de confiance tant que les plafonds du lot 7 ne sont pas là.

## Règles à ne pas enfreindre

Elles sont dans [`CLAUDE.md`](../CLAUDE.md). Les quatre qui comptent le plus :

- **Aucune ligne d'attribution d'outil** dans les commits ni les pull requests (ni `Co-Authored-By`, ni mention d'un assistant).
- **Ne rien pousser sans une demande explicite d'Alex.** Un accord donné pour un lot ne vaut pas pour le suivant.
- **Aucun secret** dans le dépôt, les journaux, la documentation.
- **Tout sur `main`** : pas de branche durable. Les worktrees sont des ateliers, leurs commits sont repris sur `main`.
