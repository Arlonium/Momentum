# Reprendre Mementum

Ce document est fait pour quelqu'un, humain ou agent IA, qui ouvre ce dépôt sans rien savoir de son histoire. Il dit où en est le projet, ce qui reste à faire, et comment s'y prendre. Il renvoie aux sources plutôt que de les recopier.

Écrit le 2026-11-10, à la clôture du lot 2 de la phase 1. Récrit le 2026-11-17, quand le code des deux parties du lot 3 est sur `main` (Task D de la partie B), puis mis à jour le même jour, après la relecture finale de la partie B. À mettre à jour à chaque clôture de lot et à chaque pause. Pendant l'exécution d'un lot, l'état au jour le jour est dans son fichier d'avancement (pour le lot 3 : [`docs/superpowers/logs/2026-11-12-p1-lot3-avancement.md`](superpowers/logs/2026-11-12-p1-lot3-avancement.md)) : la plus haute des sections « Reprise du … » ou « Pause du … » prime sur celles qui la suivent ; le tableau « État des tâches », en tête du fichier, date du 13 novembre et ne dit plus l'état. S'il dit plus récent que ce document, c'est l'avancement qui a raison.

## En une minute

- **Mementum** est un espace de travail open source façon Notion, auto-hébergeable, conçu pour que les agents IA lisent et écrivent les pages aussi proprement que les humains. Licence AGPL-3.0.
- **Pile** : serveur Rust en un binaire (axum, sqlx sur SQLite, yrs), client React avec BlockNote dans `apps/web`, texte des pages en CRDT Yjs, format Markdown canonique « MMD » pour les agents.
- **Deux personnes** : Lenny est le propriétaire du projet, ses décisions ne se remettent pas en cause sans lui (liste dans [`CLAUDE.md`](../CLAUDE.md)). Alex pilote le développement et fait la recette.
- **Où on en est** : la phase 0 (essais) est finie. La phase 1 compte sept lots. Les lots 1 et 2 sont faits et poussés. Le lot 3, « Collaboration et blocs riches », est mené en deux plans, et **le code de ses deux parties est sur `main`**, poussé, chaque tâche relue et corrigée. La partie A (barre latérale et présence en direct, commentaires, historique, déplacement de blocs à plusieurs, intégrité de la synchro) a eu sa relecture finale ; sa documentation et sa checklist de recette sont relues contre le code. La partie B (fichiers, blocs riches, mentions, duplication, favoris, récents, fil d'Ariane, reports du lot 2) a sa documentation et sa checklist depuis le 2026-11-17, et sa relecture finale est faite ; sa vague de correction est en cours. La garde des attributs de bloc et la correction de « rétablir » sont sur `main`. Les lots 4 à 7 restent à faire.
- **Ce qui suit** : finir le lot 3 (« Ce qui reste pour clore le lot 3 », plus bas), puis le lot 4.

## Dans quel ordre lire

1. [`CLAUDE.md`](../CLAUDE.md) : les règles du projet et tous les pièges déjà rencontrés. À lire en entier avant de toucher au code.
2. [`README.md`](../README.md) : ce que l'application fait aujourd'hui, comment la lancer, ses limites connues, les prochaines étapes.
3. [`docs/specs/2026-11-09-mementum-design.md`](specs/2026-11-09-mementum-design.md) : la conception. C'est la référence : en cas de conflit, elle l'emporte sur les plans et sur le code. Le §9 dit ce que chaque lot livre et laisse, le §4 décrit les écrans et les droits, le §5.6 le contrôle des accès, le §5.8 les fichiers, le §6 le format MMD et les agents, le §11 les risques.
4. Le registre du dernier lot clos : [`docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md`](superpowers/logs/2026-11-10-p1-lot2-ledger.md). Puis les deux plans du lot en cours, [partie A](superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md) et [partie B](superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md), leur registre commun, [`docs/superpowers/logs/2026-11-12-p1-lot3-ledger.md`](superpowers/logs/2026-11-12-p1-lot3-ledger.md) (une section par tâche et par tour de correction), l'[avancement](superpowers/logs/2026-11-12-p1-lot3-avancement.md) et la [relecture finale de la partie A](superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md).
5. L'en-tête de `crates/mementum-server/src/lib.rs` : le tableau de toutes les routes, avec qui y a droit et les statuts de refus.

Chaque décision prise en cours de route est un « Ruling », numéroté à la suite d'un lot à l'autre : R1 à R44 au lot 1, R45 à R128 au lot 2, R129 et plus au lot 3 (R240 et plus pour la partie B, mêlés à ceux de la partie A dans le même registre). On cherche « Ruling R93 » dans les registres pour savoir pourquoi le code fait ce qu'il fait.

## État au 2026-11-17

| Étape | État | Où |
|---|---|---|
| Phase 0, essais | finie, poussée | `spike/`, `spike/RESULTS.md` |
| Phase 1, lot 1 : socle et édition en direct | fini, poussé le 2026-11-09 | plan et registre datés du 2026-11-09 dans `docs/superpowers/` |
| Phase 1, lot 2 : comptes, espaces, droits et corbeille | fini, poussé le 2026-11-10 | plan et registre datés du 2026-11-10 |
| Recette du lot 2 | aucun défaut à corriger avant le lot 3 (réponse de Lenny, 2026-11-12) | [`docs/recette/2026-11-10-p1-lot2-recette.md`](recette/2026-11-10-p1-lot2-recette.md) |
| Phase 1, lot 3, partie A « Collaboration » | **code fini sur `main`** : tâches 1 à 14, relecture finale en trois passes et ses tours de correction, remise au vert de la CI, file d'écriture du store, compte attendu ; une base au schéma du lot 2 passe les migrations `0003` et `0004`. Attend : les suites sur un clone neuf, les budgets mesurés au calme, la recette du propriétaire | [plan](superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md), registre, [relecture finale](superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md), [checklist](recette/2026-11-12-p1-lot3a-recette.md) |
| Phase 1, lot 3, partie B « Blocs riches et organisation » | **code fini sur `main`** : vingt tâches, chacune relue et corrigée, puis la documentation et la checklist (Task D). Relecture finale faite (sécurité, intégration, documentation). Essai de migration fait (point 4, plus bas). Attend : sa vague de correction, les suites sur un clone neuf, la recette du propriétaire | [plan](superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md), registre, [checklist](recette/2026-11-12-p1-lot3b-recette.md) |
| Phase 1, lots 4 à 7 | à faire, aucun plan écrit | voir plus bas |
| Mesures ARM de la phase 0 | repoussées avant la phase 2 | `spike/RESULTS.md` |

Ce que l'application fait aujourd'hui : des comptes avec mot de passe, des espaces (plusieurs par compte), des membres et des invités, des pages en blocs rangées en arbre et en trois sections (« Espace », « Partagé », « Privé »), l'édition à plusieurs en direct avec le curseur et l'avatar de chacun, la barre latérale qui suit en direct ce que font les autres, le partage d'une page à quatre niveaux, les invitations par lien, la lecture seule selon le droit, la corbeille, les commentaires sur une page ou un bloc, l'historique des versions avec aperçu et restauration, le format MMD en lecture et en écriture par l'API. Depuis la partie B : les fichiers envoyés dans les pages (images, vidéos, sons, fichiers à télécharger), le callout, le signet, les équations, la table des matières, le lien vers une page, les mentions `@` et `[[`, la duplication d'un bloc (Ctrl+D) et d'une page, les favoris, les pages récentes et le fil d'Ariane.

Ce qu'elle ne fait pas encore : les bases de données et leurs vues, la recherche, l'envoi d'e-mails, la connexion par Google ou OIDC, les jetons d'API et le serveur MCP, les imports et l'export, le binaire qui embarque l'application et l'image Docker, les quotas et les plafonds contre un compte qui abuse (lot 7). Ses limites connues, lot par lot, sont au README.

Derniers relevés, sur le poste de Lenny et **sans clone neuf** (avancement, 2026-11-17) : à la clôture de la garde des attributs, 1 133 tests Rust, 2 474 tests du client (et 53 échecs attendus) et la suite de bout en bout entière, 304 parcours verts ; les budgets du navigateur, 14 sur 14, à la clôture de la Task C8. La CI (`gh run list --branch main --limit 5`) est à relire à la reprise. La migration `0004` de la partie B peut encore changer jusqu'à sa clôture (Ruling R243) : **n'ouvre pas les données d'une instance qu'on regarde avec un binaire de ce lot avant la clôture**.

## Ce qui reste pour clore le lot 3

Dans l'ordre ; le détail de chaque point est dans l'[avancement](superpowers/logs/2026-11-12-p1-lot3-avancement.md), section « Reprise du 2026-11-17 ».

Déjà sur `main` : la **garde des attributs de bloc** (R689 à R705, R716 à R725, R736 à R739 : un attribut de bloc d'un autre type, et une entrée qui pouvait arrêter le serveur, refusés ; fusionnée avant sa dernière relecture ciblée, qui reste à faire, parce que `main` était exposé) et la **correction de « rétablir »** (`editor/redoKeys.ts`, R731 à R734 : Ctrl+Maj+Z sans rien à rétablir retirait la dernière frappe, depuis le lot 1). La **relecture finale de la partie B** est faite, en trois passes (sécurité, intégration entre client et serveur, documentation contre le code) ; ses rapports sont sous `.claude/worktrees/tmp-agents/` (`rfb-sec`, `rfb-int`, `rfb-docs`).

1. **La vague de correction de la relecture finale**, chaque correction relue :
   - la **provenance d'un bloc collé** (R746 à R752, sur `main`) : un fichier n'est rattaché que pour un bloc copié depuis la page de ce fichier, dans le même navigateur, sous le même compte ; en relecture ;
   - les **corrections d'intégration** entre client et serveur (une réponse perdue après l'écriture dite comme un refus, et trois mineurs ; branche `rfb-serveur`, atelier `rfb-int`) ;
   - quatre **défauts du produit** trouvés par la passe documentation : la table des matières n'est pas décalée par niveau (D-1) ; le menu « … » de la page se referme au premier clic qui suit une frappe (D-2) ; des libellés de BlockNote faussent la règle « tout en français, tutoiement » (D-3) ; une date en chiffres peut rester trois frappes sans proposition (D-4). La recette de la partie B est écrite pour le produit corrigé ;
   - la dernière relecture ciblée de la garde des attributs.
2. **La documentation** suit chaque correction (la passe documentation de la relecture finale est corrigée, R771 à R773).
3. **Sur un clone neuf**, étape par étape comme la CI (« Commandes », plus bas), avec `cargo test --release -p mementum-server -- --ignored` en plus et les budgets du navigateur au calme.
4. **L'essai de migration : fait le 2026-11-17**, sur des copies, jamais sur `data/instance` (rapport et scripts rejouables sous `.claude/worktrees/tmp-agents/essai-migration/`, `rapport.md`). Une base du lot 2 passe les migrations `0003` et `0004` dans la même ouverture, avec une seule copie (`avant-0003.bak`) : 34 vérifications sur 34 (tables, droits relus par l'API pour chaque compte, fichier envoyé puis relu après un redémarrage, copie remise en place sans qu'aucun fichier quitte le disque). L'instance d'Alex, base du lot 2, est dans ce cas. Une base de la partie A à la `0003` finale passe aussi (`avant-0004.bak`, 34 sur 34), mais elle **n'a pu être écrite que par un binaire reconstitué** : aucun commit n'a la `0003` finale (`e4a065d`, 15/11) sans la `0004` (`60e0a60`, 13/11), et toute base ouverte par un vrai binaire de la partie A est refusée, base intacte, comme le veulent R139 et R243 ; la reprise est la copie d'avant la première migration du lot 3 appliquée par ce binaire. Les trois défauts mineurs de l'essai sont corrigés (R840 : refus avant toute copie, en français, qui nomme la copie à reprendre ; R841 : une copie remise en place est reconnue ; R842 : une erreur du démarrage dite une fois) et ses trois écarts de documentation repris au README et dans la conception (R843, R844).
5. **Les recettes du propriétaire**, avec les checklists de la [partie A](recette/2026-11-12-p1-lot3a-recette.md) et de la [partie B](recette/2026-11-12-p1-lot3b-recette.md).
6. **La clôture** : au registre, les mesures de clôture et les reports ; ce document à jour ; le fichier d'avancement, celui de la relecture finale de la partie A et le rapport de la Task 13a rangés ou supprimés ; les ateliers retirés, avec leur dossier `target/`.

Hors de ce chemin, connus et pas encore confiés : une page dont une liste est imbriquée sur une soixantaine de niveaux fait planter l'onglet (Chromium), même sans formule (vu à la Task C2 ; une borne d'imbrication est à poser, côté serveur ou côté client) ; le budget `longest_frame_scrolling_through_50_distinct_long_formulas_under_600ms` a dépassé deux fois son délai de quatre minutes en CI le 2026-11-16, vert depuis, cause non cherchée.

## Ce qui attend le propriétaire

Rien de ceci ne se décide à sa place.

1. **Le lot 3, partie A** : la forme à clés du déplacement de blocs et ses régressions acceptées (R220, R223), et la limite qui reste, une frappe arrivée au serveur après le déplacement de son bloc (R222) ; l'historique réservé à « Peut modifier » (R203) ; la restauration qui remplace tout le contenu (R188). Puis sa recette.
2. **Le lot 3, partie B** : les choix de produit pris sans lui, listés en tête de [sa checklist](recette/2026-11-12-p1-lot3b-recette.md), à confirmer ou défaire : les cinq du 2026-11-13 (pas de quota avant le lot 7, R247 ; le titre copié par une mention, R251 ; la place de la copie d'une page, R252 ; un favori qui n'avance pas la version de l'espace, R254 ; une invitation sans effet non consommée, R257) et ceux pris depuis (R632, R641, R646, R658, R667, R685, R713) ; R608 (16 Mio par copie de page) est confirmé (2026-11-16). Aussi : le SVG et le PDF en téléchargement (R246), le signet sans aperçu (R250), les récents gardés dans le navigateur (R255), et les lignes de la P1 du §4.5 de la conception qu'aucun lot écrit ne porte (oEmbed, Mermaid rendu, icône et couverture de page, emojis personnalisés, pleine largeur, petite police, verrouillage, « Modifié il y a… », aperçu du signet). Puis sa recette.
3. **Quatre règles à valider**, parce qu'elles se voient à l'usage :
   - les membres ont l'accès complet aux pages d'équipe, comme dans Notion (R48) ;
   - les inscriptions sont sur invitation par défaut (R50) ;
   - un invité en accès complet peut inviter à son tour (R86) ;
   - un simple membre peut fermer une page d'équipe aux autres, après confirmation (R122, R127).
4. **D'autres choix déjà pris, à confirmer** : le menu du compte dans le sélecteur d'espace (R80), une page sortie à la racine garde son accès d'équipe (R94), une session de 30 jours au plus dans un navigateur (R117), les plafonds contre la surcharge reportés au lot 7 (R121), et deux décisions du lot 1 : `PUT /mmd` refuse une propriété inconnue (R41), deux avis de sécurité ouverts sans mise à jour possible (R44).
5. **Pousser** : `main` a été poussé le 2026-11-12 à la demande de Lenny, lot en cours ; il a ajouté le même jour que les commits du lot 3 se poussent au fil du travail, sans redemander (« les commits tu peux les push seul »). Cet accord vaut pour l'exécution du lot 3, tant que l'état poussé tient (format, clippy, tests) et que ce document et le fichier d'avancement disent où on en est. Le 2026-11-15, Lenny a décidé de ne pas pousser les ateliers comme sauvegarde : ils restent locaux.

Les espaces façon Notion, à la place des coffres façon Obsidian, sont confirmés par Lenny (2026-11-10).

## Ce qui reste à construire

L'ordre prévu des lots est dans le README (« Prochaines étapes ») et la conception (§9). En bref :

| Lot | Sujet | Ce qu'il reprend des lots faits |
|---|---|---|
| 3 | Collaboration et blocs riches | code fait ; clôture en cours (plus haut) |
| 4 | Recherche | les rétroliens (il leur faut l'index des blocs, `blocks_index`) |
| 5 | Agents : API REST complète, serveur MCP, jetons d'API sur un espace ou sur des pages | l'écriture MMD par différence, pour `PUT /mmd` et la restauration (R40, R188), un porteur durable pour les propriétés inconnues (R41) |
| 6 | Imports et export | les pièces jointes d'Obsidian et de Notion |
| 7 | Livraison : binaire qui embarque l'application, image Docker, fichier `mementum.toml`, inscription par domaine | les plafonds et limites de débit (R54, R121, R511, R516), la relecture périodique des sessions (R125), le contrôle des avis de sécurité en CI (R44) ; pour les fichiers : quotas par espace, envois simultanés et débit par compte, délai de lecture des corps (R247, R505), ménage des fichiers qu'aucun bloc ne montre, purge différée (R575), miniatures, antivirus ; la copie d'une page en plusieurs étapes au-delà de 16 Mio (R608) |

Après le lot 3, un plan à part pour la frappe arrivée au serveur après le déplacement de son bloc (R222). Sans date, à décider avec le propriétaire : l'envoi d'e-mails (invitations, mot de passe oublié), la connexion par Google ou OIDC, la suppression d'un espace ou d'un compte, le vidage automatique de la corbeille, les passkeys (R46). Les bases de données et leurs vues sont la phase 2 ; les notifications, `@rappel` et les liens publics, la phase 3 ; S3 et les URL signées, la phase 5.

### Reports du lot 2

La liste complète est au registre du lot 2, section « Reportés au lot suivant ». Où ils en sont :

1. ~~Le client n'a aucune garde à lui contre une présence mal formée.~~ **Fait (R129)**.
2. ~~Les mises à jour du document sont relayées telles que reçues.~~ **Fait (R130, R131)**.
3. **La surcharge par un compte autorisé** (R121) : prévue au lot 7 ; à avancer si l'instance s'ouvre plus tôt.
4. ~~La présence après une reconnexion met jusqu'à 30 secondes à revenir.~~ **Fait au lot 3 (R136, R165)**.
5. **Une session supprimée en ligne de commande** (`mementum user set-password`) laisse ouvertes les pages déjà connectées (R125) : relecture périodique des sessions au lot 7 ; d'ici là, redémarrer le serveur après la commande.
6. **Le cookie de session n'est pas renvoyé** quand la session est prolongée (R117).
7. Les mineurs de la relecture finale : M5, M7, M9, M11 et m1 sont **faits à la partie B** (R257) ; M2 et M4 (non reproduit) restent, au registre du lot 2.

## Par où continuer

Lenny a choisi le 2026-11-12 : **le lot 3, comme prévu**. Le lot 5 (agents, serveur MCP : Alex veut que Claude Code puisse verser du contenu dans un de ses espaces) vient ensuite, dans l'ordre de la conception, après le lot 4. La suite immédiate est la liste « Ce qui reste pour clore le lot 3 », plus haut, dans son ordre. Un poste peut disparaître : le 2026-11-15, tout ce qui n'était pas poussé a été perdu avec l'ancien poste, et refait d'après l'avancement et le registre. Ce qui n'est que dans un atelier n'existe que là.

## Comment mener un lot

La méthode est écrite dans le README (« Méthode de travail ») et a été suivie telle quelle aux lots 1 à 3 :

1. **Un plan** pour le lot, dans `docs/superpowers/plans/`, écrit d'après la conception : des tâches petites, chacune avec ses fichiers, ses interfaces, ses tests nommés. Le propriétaire le relit avant qu'on commence.
2. **Un registre** pour le lot, dans `docs/superpowers/logs/` : chaque écart au plan y devient un Ruling, au format `Ruling: <décision> — <pourquoi> — <coût si faux>`.
3. **L'exécution tâche par tâche**, en TDD : le test qui échoue d'abord. Aux lots 1 et 2, chaque tâche a été confiée à un sous-agent dans un worktree à part, puis relue par un autre, puis reprise sur `main`. Au lot 3, un contrôleur mène les tâches (R140) : un sous-agent implémente, un autre relit en prouvant ses constats par des tests jetables, les constats importants repartent en tour de correction, puis une re-relecture ciblée vérifie le tour. La partie A a commencé directement sur `main`, faute de disque, puis a pris des ateliers (`git worktree`) pour les travaux menés de front ; la partie B s'est menée en deux pistes de front, serveur et client (R241), chaque tâche dans un atelier rebasé sur `main`, puis reprise par avance rapide.
4. **Une relecture finale de tout le lot**, en passes séparées (sécurité, intégration entre client et serveur, documentation contre le code), puis une vague de correction et des re-relectures ciblées.
5. **Toutes les suites sur un clone neuf**, étape par étape comme la CI, avant de dire que c'est fini.
6. **Une checklist de recette** pour le propriétaire, sur ce qu'un test ne juge pas.

Ce que les lots faits ont appris, et qui ne se devine pas :

- La relecture finale trouve ce que les relectures de tâche ne voient pas. Au lot 2, ses trois passes ont trouvé une perte de frappes, une session qui survivait à un changement de mot de passe et une page qui ne pouvait pas redevenir privée, dans du code dont chaque tâche avait été approuvée. Au lot 3, celle de la partie A a demandé sept tours de correction côté serveur (lectures quadratiques, puis exponentielles, d'une entrée hostile ; une relecture du journal qui effaçait la lettre d'un autre).
- Un relecteur qui prouve ses constats par un test jetable vaut mieux qu'un relecteur qui lit. Demander la preuve.
- Une règle de protection est à relire comme une attaque : la première version d'une limite fermait la connexion fautive, et donnait ainsi à un lecteur le moyen de faire fermer celle des autres (R113).
- Une garde de débit suit le travail qu'elle borne, pas la requête qui l'a demandé : une duplication abandonnée par son client continuait de calculer sans être comptée (R630).
- Les compilations parallèles remplissent le disque (10 à 40 Go par worktree). Supprimer le dossier `target` d'un atelier dès qu'il a rendu.
- Ne jamais relâcher un budget de performance, sa statistique ni sa définition pour faire passer un test.
- Lire la CI après chaque poussée de `main` : elle est restée rouge deux jours sans que personne le voie.

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
- La CI (`.github/workflows/ci.yml`) lance ces étapes sous Linux, sauf `cargo deny check advisories` (elle ne contrôle que les licences et les interdits, `cargo deny check licenses bans`, R44), avec les mesures ignorées de `mementum-cli` en plus de celles de `mementum-sync` ; les tests Rust tournent aussi sous Windows.

## L'instance d'Alex

Alex fait tourner une instance sur son poste pour regarder l'application et faire la recette.

- **Données** : `data/instance/mementum.db`, dans le dépôt (le dossier `data/` est ignoré par git). C'est sa base du lot 1, migrée au lot 2 le 2026-11-10. **Elle n'existe plus sur le poste réinstallé le 2026-11-15** (le dossier n'était pas versionné) : une instance neuve partira d'une base vide, ou d'une copie remise par Alex. Sa copie d'avant migration (`mementum.db.avant-0002.bak`) et la sauvegarde de la base du lot 1 (`data/sauvegardes/2026-11-10-avant-lot2/`) n'existent plus non plus sur ce poste : `data/` n'y est pas.
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
- **Pour tester à plusieurs par Tailscale** (adresse du PC d'Alex : `100.87.123.94`), le serveur reçoit en plus l'adresse publique de la page, et un second Vite écoute sur l'adresse Tailscale :

  ```
  .\target\release\mementum.exe serve --bind 127.0.0.1:8090 --data-dir data\instance --public-url http://100.87.123.94:5127
  ```

  ```
  $env:MEMENTUM_E2E_WEB_PORT = "5127"; $env:MEMENTUM_E2E_API_PORT = "8090"; corepack pnpm -C apps/web dev --host 100.87.123.94
  ```

  Créer les invitations depuis `http://100.87.123.94:5127` : le lien reprend l'adresse de la page. Le serveur reste sur la boucle locale ; seule la page est exposée, et elle relaie l'API. Pour une personne hors du tailnet d'Alex, partager la machine dans Tailscale et n'ouvrir que le port 5127 (`autogroup:shared`), plutôt que de l'ajouter comme membre.
- **Avant toute mise à jour du serveur** : l'arrêter, copier `data/instance` ailleurs, puis relancer. Une migration écrit sa propre copie, mais une sauvegarde à part ne coûte rien.
- **Ne jamais lancer de tests sur les ports 5175 et 8090**, ni sur `data/instance`.
- La page est servie par Vite depuis l'arbre de travail : dès que le code du client change sur `main`, la page change aussi. Si le client et le serveur ne sont plus de la même version, la page dit « Impossible de joindre Mementum » : recompiler et relancer le serveur.
- Avant d'ouvrir l'instance à quelqu'un d'autre (tunnel) : lire les limites connues du README, et n'inviter que des personnes de confiance tant que les plafonds du lot 7 ne sont pas là.

## Règles à ne pas enfreindre

Elles sont dans [`CLAUDE.md`](../CLAUDE.md). Les quatre qui comptent le plus :

- **Aucune ligne d'attribution d'outil** dans les commits ni les pull requests (ni `Co-Authored-By`, ni mention d'un assistant).
- **Ne rien pousser sans une demande explicite** d'Alex ou de Lenny. Un accord donné pour un lot ne vaut pas pour le suivant. Pour le lot 3, Lenny a donné cet accord le 2026-11-12 : ses commits se poussent au fil du travail. Jamais de poussée forcée.
- **Aucun secret** dans le dépôt, les journaux, la documentation.
- **Tout sur `main`** : pas de branche durable. Les worktrees sont des ateliers, leurs commits sont repris sur `main`.
