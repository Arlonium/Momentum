# Reprendre Mementum

Ce document est fait pour quelqu'un, humain ou agent IA, qui ouvre ce dépôt sans rien savoir de son histoire. Il dit où en est le projet, ce qui reste à faire, et comment s'y prendre. Il renvoie aux sources plutôt que de les recopier.

Écrit le 2026-11-10, à la clôture du lot 2 de la phase 1. Mis à jour le 2026-11-12, à l'ouverture du lot 3, puis le même jour **en cours d'exécution de sa partie A**, et le soir quand Lenny a mis le travail en pause ; puis le 2026-11-13 vers 3 h 40, à la seconde pause, après une nuit de travail. À mettre à jour à chaque clôture de lot.

> **Pause du 2026-11-13, 16 h 10 (la troisième).** Le travail a repris ce jour-là à 5 h 10 et s'est arrêté à 16 h 10, sur demande de Lenny. **L'état d'où l'on reprend est dans [`l'avancement`](superpowers/logs/2026-11-12-p1-lot3-avancement.md), section « État à la pause du 2026-11-13 (16 h 10) »** : ce qui est sur `main` (poussé), les huit ateliers du poste qui portent un travail en cours (jamais poussés : un clone neuf ne les a pas), ce qui reste dans l'ordre, ce qui attend le propriétaire. En bref : la partie A attend la fin du troisième tour de correction du serveur (écrit, en re-relecture), la Task 13b (en cours), puis la documentation, la recette et la clôture ; la partie B a six tâches sur vingt et une sur `main`, cinq autres écrites ou en correction dans des ateliers. La CI de GitHub, rouge depuis l'ouverture du lot sans avoir été lue, est passée au vert une fois le 13 ; son dernier passage est rouge sous Windows sur deux tests de synchro qui comptent sur le minutage. Ce qui suit dans ce document décrit des états plus anciens.
>
> **Mise à jour du 2026-11-13, 9 h : le travail a repris à 5 h 10 et continue.** L'encadré qui suit et les chiffres de ce document décrivent l'état **à la pause de 3 h 40** ; ce qui a changé depuis est tenu à jour dans [`l'avancement`](superpowers/logs/2026-11-12-p1-lot3-avancement.md), paragraphe « Depuis la reprise ». En bref : **plus aucun travail n'est hors de `main`** (la Task 13a, le correctif de y-prosemirror, et la première tâche de la partie B y sont, poussées) ; les trois importants de la relecture du serveur sont corrigés (second tour de correction fait, sa re-relecture en cours) ; restent à la partie A la Task 13b, la documentation et la recette (13c, 14), la dernière passe et la clôture ; la partie B a quatre tâches sur vingt et une sur `main`, d'autres en cours dans des ateliers du poste (`.claude/worktrees/`, jamais poussés).
>
> **Tu reprends en plein lot, après une pause (la seconde, dans la nuit du 12 au 13 novembre, vers 3 h 40).** Douze des quatorze tâches de la partie A du lot 3 sont closes ; les seize constats de la relecture finale sont corrigés, mais la relecture des corrections du serveur a trouvé un défaut critique (corrigé, poussé) et trois importants, **pas encore corrigés**. Ce qui est fini est sur `main`, poussé, et rien n'est à moitié fait dans l'arbre. **Deux travaux ne sont pas sur `main`** : la Task 13a et la première tâche de la partie B sont sur des branches locales du poste de Lenny, jamais poussées. Lis d'abord [`docs/superpowers/logs/2026-11-12-p1-lot3-avancement.md`](superpowers/logs/2026-11-12-p1-lot3-avancement.md) (l'état tâche par tâche, ce qui était en cours à la pause, ce qui reste dans l'ordre, les reports), puis [`docs/superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md`](superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md) (chaque constat de la relecture finale, la décision, et où en est sa correction).

## En une minute

- **Mementum** est un espace de travail open source façon Notion, auto-hébergeable, conçu pour que les agents IA lisent et écrivent les pages aussi proprement que les humains. Licence AGPL-3.0.
- **Pile** : serveur Rust en un binaire (axum, sqlx sur SQLite, yrs), client React avec BlockNote dans `apps/web`, texte des pages en CRDT Yjs, format Markdown canonique « MMD » pour les agents.
- **Deux personnes** : Lenny est le propriétaire du projet, ses décisions ne se remettent pas en cause sans lui (liste dans [`CLAUDE.md`](../CLAUDE.md)). Alex pilote le développement et fait la recette.
- **Où on en est** : la phase 0 (essais) est finie. La phase 1 compte sept lots. Les lots 1 et 2 sont faits et poussés. Le lot 3 est ouvert : Lenny l'a choisi le 2026-11-12 et a validé le plan de sa partie A le même jour. **Douze de ses quatorze tâches sont closes et relues** (intégrité de la synchro, barre latérale et présents en direct, commentaires, historique des versions, tests de ce que devient une frappe quand un bloc est déplacé) ; la treizième est récrite en trois parts d'après un essai : la première (13a) est écrite sur une branche locale et sa relecture est commencée, les deux autres ne sont pas commencées ; la quatorzième reste à faire ; les seize constats de la relecture finale sont corrigés, et la relecture de ces corrections demande encore un tour côté serveur. Le plan de la partie B est relu et corrigé, et sa première tâche est faite sur une branche locale. **Le travail est en pause depuis le 2026-11-13, 3 h 40 du matin.** Les lots 4 à 7 restent à faire.

## Dans quel ordre lire

1. [`CLAUDE.md`](../CLAUDE.md) : les règles du projet et tous les pièges déjà rencontrés. À lire en entier avant de toucher au code.
2. [`README.md`](../README.md) : ce que l'application fait aujourd'hui, comment la lancer, ses limites connues, les prochaines étapes.
3. [`docs/specs/2026-11-09-mementum-design.md`](specs/2026-11-09-mementum-design.md) : la conception. C'est la référence : en cas de conflit, elle l'emporte sur les plans et sur le code. Le §9 dit ce que chaque lot livre, le §4 décrit les droits, le §5.6 le contrôle des accès, le §6 le format MMD et les agents.
4. Le registre du dernier lot clos : [`docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md`](superpowers/logs/2026-11-10-p1-lot2-ledger.md). Ses dernières sections (« Relecture finale et vagues de correction », « Clôture du lot 2 », « Après la clôture ») donnent l'état exact, les mesures, ce qui est reporté et ce qui attend le propriétaire. Puis le plan et le registre du lot en cours : [`docs/superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md`](superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md) et [`docs/superpowers/logs/2026-11-12-p1-lot3-ledger.md`](superpowers/logs/2026-11-12-p1-lot3-ledger.md).
5. L'en-tête de `crates/mementum-server/src/lib.rs` : le tableau de toutes les routes, avec qui y a droit et les statuts de refus.

Chaque décision prise en cours de route est un « Ruling », numéroté à la suite d'un lot à l'autre : R1 à R44 au lot 1, R45 à R128 au lot 2, R129 et plus au lot 3. On cherche « Ruling R93 » dans les registres pour savoir pourquoi le code fait ce qu'il fait.

## État au 2026-11-12

| Étape | État | Où |
|---|---|---|
| Phase 0, essais | finie, poussée | `spike/`, `spike/RESULTS.md` |
| Phase 1, lot 1 : socle et édition en direct | fini, poussé le 2026-11-09 | plan et registre datés du 2026-11-09 dans `docs/superpowers/` |
| Phase 1, lot 2 : comptes, espaces, droits et corbeille | fini, poussé le 2026-11-10 (`main`, commit `7fb1b55` et suivants) | plan et registre datés du 2026-11-10 |
| Recette du lot 2 | aucun défaut à corriger avant le lot 3 (réponse de Lenny, 2026-11-12) | [`docs/recette/2026-11-10-p1-lot2-recette.md`](recette/2026-11-10-p1-lot2-recette.md) |
| Deux reports du lot 2 qui touchent à l'intégrité du contenu | faits le 2026-11-12 (R129, R130), poussés | registre du lot 3, « Avant le plan » |
| Phase 1, lot 3, partie A « Collaboration » | **en pause, non close** : tâches 1 à 12 closes et relues ; tâche 13 récrite en 13a, 13b, 13c : 13a écrite sur une branche locale, relecture à finir, 13b et 13c à faire ; relecture finale : les 16 constats corrigés, piste client relue, piste serveur relue avec 1 critique corrigé et 3 importants à corriger ; tâche 14, troisième passe et clone neuf à faire. Poussé tel quel le 2026-11-13 | plan et registre datés du 2026-11-12 ; [avancement](superpowers/logs/2026-11-12-p1-lot3-avancement.md) ; [relecture finale](superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md) |
| Phase 1, lot 3, partie B « Blocs riches et organisation » | plan écrit le 2026-11-12, **relu et corrigé le 2026-11-13** (21 tâches, R240 à R257), pas encore validé par le propriétaire ; sa première tâche (fil d'Ariane, récents) est faite et relue sur une branche locale, à finir et à reprendre | [plan](superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md) |
| Phase 1, lots 4 à 7 | à faire, aucun plan écrit | voir plus bas |
| Mesures ARM de la phase 0 | repoussées avant la phase 2 | `spike/RESULTS.md` |

Ce que l'application fait aujourd'hui : des comptes avec mot de passe, des espaces (plusieurs par compte), des membres et des invités, des pages en blocs rangées en arbre et en trois sections (« Espace », « Partagé », « Privé »), l'édition à plusieurs en direct avec le curseur de chacun, le partage d'une page à quatre niveaux, les invitations par lien, la lecture seule selon le droit, la corbeille, et le format MMD en lecture et en écriture par l'API.

Ce que `main` y ajoute depuis le 2026-11-12, **dans un lot qui n'est pas clos** (ni relecture finale, ni recette) : la barre latérale qui suit en direct ce que font les autres ; les avatars de qui a la page ouverte, et le nom du curseur posé par le serveur d'après le compte ; les commentaires sur une page ou sur un bloc (réponses, réactions, résolution), ouverts dès le niveau « Peut commenter » ; l'historique des versions d'une page, avec aperçu et restauration ; et, pour l'intégrité, une mise à jour qui laisserait un trou d'horloge retenue jusqu'à sa suite (R131), un tableau mal formé toujours lu en grille (R54), une mise à jour sans effet ni écrite ni relayée (R181).

Ce qu'elle ne fait pas encore : les bases de données et leurs vues, la recherche, l'envoi d'e-mails, la connexion par Google ou OIDC, les jetons d'API et le serveur MCP, les imports et l'export, le binaire qui embarque l'application et l'image Docker ; et, dans ce lot, les fichiers envoyés et les blocs riches depuis l'éditeur (partie B). **Déplacer un bloc pendant qu'un autre écrit abîme encore la page** : la frappe atterrit dans un bloc voisin ou se perd, selon le geste (mesuré le 2026-11-12, voir l'avancement).

Vérification à la clôture du lot 2, sur un clone neuf, comme la CI : 548 tests Rust, 214 tests du client, 122 parcours de bout en bout, tous verts ; une page de 1 000 blocs s'ouvre en 134 ms (budget 300), une page de 10 000 blocs en 1,2 s (budget 3 s). Le détail est au registre.

À la pause du 2026-11-13, sur le poste de Lenny et **sans clone neuf** : `cargo fmt` et `cargo clippy -D warnings` propres, 723 tests Rust, 516 tests du client (et 12 scénarios du bloc déplacé marqués comme des échecs attendus), typecheck propre ; la suite complète de bout en bout relancée sur `main` : 159 parcours verts, 2 marqués `fixme` exprès, 1 échec dû au test lui-même et corrigé dans la foulée (la suite entière n'a pas été relancée après) ; budgets du navigateur mesurés sur un poste occupé : 160 ms au p95 pour 1 000 blocs, 1,38 s pour 10 000. La migration `0003` de ce lot a changé plusieurs fois et peut encore changer jusqu'à la clôture : **ne pas ouvrir `data/instance` avec un binaire de ce lot avant la clôture**.

## Ce qui attend le propriétaire

Rien de ceci ne se décide à sa place.

1. **Le lot 3, partie A** : le plan ([`docs/superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md`](superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md)) est validé (Lenny, 2026-11-12), et Lenny a demandé le même jour d'avancer sans validation intermédiaire, d'après la conception, puis de pousser l'état en cours. Ce qui l'attend est listé dans l'[avancement](superpowers/logs/2026-11-12-p1-lot3-avancement.md), « Ce qui attend le propriétaire » : un défaut critique vieux du lot 1, corrigé sur `main` le 2026-11-13, qu'une instance d'avant cette date a encore ; l'historique réservé à « Peut modifier » (décidé à la relecture finale, fait, réversible) ; la façon de corriger le déplacement de blocs à plusieurs et ce qu'elle laisse (Task 13 : R220, R222, R223) ; les choix de produit pris pour la partie B ; la frappe perdue pendant une restauration (R188) ; la lenteur d'une écriture sur une très grande page ; et les deux branches locales, à pousser ou non. À la clôture : la recette de la partie A.
2. **Quatre règles à valider**, parce qu'elles se voient à l'usage :
   - les membres ont l'accès complet aux pages d'équipe, comme dans Notion (R48) ;
   - les inscriptions sont sur invitation par défaut (R50) ;
   - un invité en accès complet peut inviter à son tour (R86) ;
   - un simple membre peut fermer une page d'équipe aux autres, après confirmation (R122, R127).
3. **D'autres choix déjà pris, à confirmer** : le menu du compte dans le sélecteur d'espace (R80), une page sortie à la racine garde son accès d'équipe (R94), la barre latérale relue au retour sur l'onglet et toutes les dix secondes, et non en direct (R97, R128), une session de 30 jours au plus dans un navigateur (R117), les plafonds contre la surcharge reportés au lot 7 (R121), et deux décisions du lot 1 : `PUT /mmd` refuse une propriété inconnue (R41), deux avis de sécurité ouverts sans mise à jour possible (R44).
4. **Pousser** : `main` a été poussé le 2026-11-12 à la demande de Lenny, lot en cours ; il a ajouté le même jour que les commits du lot 3 se poussent au fil du travail, sans redemander (« les commits tu peux les push seul »). Cet accord vaut pour l'exécution du lot 3, tant que l'état poussé tient (format, clippy, tests) et que ce document et le fichier d'avancement disent où on en est.

Les espaces façon Notion, à la place des coffres façon Obsidian, sont confirmés par Lenny (2026-11-10).

## Ce qui reste à construire

L'ordre prévu des lots est dans le README (« Prochaines étapes ») et la conception (§9). En bref :

| Lot | Sujet | Ce qu'il reprend des lots faits |
|---|---|---|
| 3 | Collaboration et blocs riches : commentaires, historique, arbre des pages et présence en direct, blocs riches | la barre latérale en direct (R97, R128 : le vrai direct remplacera la relecture toutes les dix secondes), la présence vérifiée (R109 et les reports de présence ci-dessous), les déplacements de blocs sans perte d'édition (R39), les tableaux mal formés écrits par un client (R54) |
| 4 | Recherche | |
| 5 | Agents : API REST complète, serveur MCP, jetons d'API sur un espace ou sur des pages | l'écriture MMD par différence (R40), un porteur durable pour les propriétés inconnues (R41) |
| 6 | Imports et export | |
| 7 | Livraison : binaire qui embarque l'application, image Docker, fichier `mementum.toml`, inscription par domaine | les plafonds et limites de débit (R54, R121), la relecture périodique des sessions (R125), le contrôle des avis de sécurité en CI (R44) |

Sans date, à décider avec le propriétaire : l'envoi d'e-mails (invitations, mot de passe oublié), la connexion par Google ou OIDC, la suppression d'un espace ou d'un compte, le vidage automatique de la corbeille, les passkeys (R46). Les bases de données et leurs vues sont la phase 2.

### Reports précis du lot 2

La liste complète est au registre du lot 2, section « Reportés au lot suivant ». Les deux premiers sont faits (2026-11-12, registre du lot 3) ; les autres sont repris par le plan du lot 3, ou restent à leur lot :

1. ~~Le client n'a aucune garde à lui contre une présence mal formée.~~ **Fait (R129)** : l'éditeur ne lit des autres que les états de présence en règle (`apps/web/src/editor/presenceGuard.ts`).
2. ~~Les mises à jour du document sont relayées telles que reçues.~~ **Fait pour la lecture des octets (R130)** : le serveur écrit et relaie ce qu'il a lu, réencodé. L'examen a trouvé un second écart, **fermé par la Task 1 du lot 3 (R131)** : le serveur intégrait un bloc arrivé avant ses prédécesseurs, que le navigateur garde en attente, et lisait un texte qu'aucun navigateur n'affichait ; il retient désormais une telle mise à jour jusqu'à ce que la suite arrive.
3. **La surcharge par un compte autorisé** (R121) : aucun plafond de connexions de synchro par compte ni par page, et la file d'une connexion qui ne lit pas peut grossir. Prévu au lot 7 ; à avancer si l'instance s'ouvre plus tôt.
4. **La présence après une reconnexion** met jusqu'à 30 secondes à revenir pour une personne qui ne tape pas. Piste écrite au registre : retirer l'état sans avancer l'horloge, et faire annoncer un état neuf par le client à chaque connexion.
5. **Une session supprimée en ligne de commande** (`mementum user set-password`) laisse ouvertes les pages déjà connectées (R125). D'ici là : redémarrer le serveur après la commande.
6. **Le cookie de session n'est pas renvoyé** quand la session est prolongée (R117).
7. Des mineurs de la relecture finale : l'écran du premier compte quand l'état de l'instance a changé entre-temps, une invitation consommée pour rien, un titre non enregistré affiché en lecture seule, une invitation de page sans nom de page dans les réglages, deux tests de concurrence à durcir.

## Par où continuer

Lenny a choisi le 2026-11-12 : **le lot 3, comme prévu**. Il lève les limites les plus visibles du lot 2 (barre latérale qui ne suit pas en direct, présence fragile) et apporte les commentaires et l'historique. Le lot 5 (agents, serveur MCP : Alex veut que Claude Code puisse verser du contenu dans un de ses espaces) vient ensuite, dans l'ordre de la conception.

La suite, dans l'ordre (le détail de chaque point est dans l'[avancement](superpowers/logs/2026-11-12-p1-lot3-avancement.md)) :

Les trois premiers points n'ont aucun fichier commun et se mènent de front, chacun dans son arbre :

1. côté serveur, dans l'arbre principal : finir le tour de correction de la relecture finale (une session fermée qui n'arrête pas la file des écritures, dont les tests sont gardés dans un fichier `.patch` à côté du registre ; l'écriture d'une grande page, quadratique ; les plafonds des commentaires, insuffisants ; cinq mineurs), puis le faire re-relire ;
2. la Task 13a, dans son atelier (`.claude/worktrees/agent-acd5330e6b63d3097`) : finir sa relecture, en commençant par les deux pistes que le relecteur a nommées, puis ses corrections, son rebase et sa reprise sur `main` ;
3. la première tâche de la partie B, dans son atelier (`.claude/worktrees/agent-a4c105cefdfc410eb`) : trois mineurs, les suites, le rebase, la re-relecture, la reprise ;
4. faire relire les parcours de bout en bout ajoutés le 2026-11-13, et, côté client, le bouton « Supprimer le fil » qui va avec les plafonds ;
5. la Task 13b (le serveur reporte sur le bloc recréé ce qui a été tapé dans l'original), après les points 1 et 2 ;
6. la Task 13c et la Task 14 : documentation relue contre le code, la limite qui reste écrite telle quelle, checklist de recette de la partie A ;
7. la troisième passe de la relecture finale (documentation contre le code), puis toutes les suites sur un clone neuf, les budgets au calme, et l'essai de migration sur une copie d'une base du lot 2 ;
8. à la clôture : mettre ce document à jour, supprimer le fichier d'avancement, celui de la relecture finale et le rapport de la 13a ;
9. la partie B, d'après son plan : sa piste serveur dès que le point 1 est clos, sa piste cliente après le point 3.

Si les deux branches locales ne sont plus là (autre poste, dépôt recloné), les points 2 et 3 sont à refaire : le rapport de la 13a ([`docs/superpowers/logs/2026-11-13-p1-lot3-task13a-rapport.md`](superpowers/logs/2026-11-13-p1-lot3-task13a-rapport.md)) et l'avancement disent ce qu'elles contenaient.

## Comment mener un lot

La méthode est écrite dans le README (« Méthode de travail ») et a été suivie telle quelle aux lots 1 et 2 :

1. **Un plan** pour le lot, dans `docs/superpowers/plans/`, écrit d'après la conception : des tâches petites, chacune avec ses fichiers, ses interfaces, ses tests nommés. Le propriétaire le relit avant qu'on commence.
2. **Un registre** pour le lot, dans `docs/superpowers/logs/` : chaque écart au plan y devient un Ruling, au format `Ruling: <décision> — <pourquoi> — <coût si faux>`.
3. **L'exécution tâche par tâche**, en TDD : le test qui échoue d'abord. Aux lots 1 et 2, chaque tâche a été confiée à un sous-agent dans un worktree à part, puis relue par un autre, puis reprise sur `main`. Au lot 3, faute de disque, les tâches sont exécutées l'une après l'autre directement sur `main` (R140) : un sous-agent implémente, un autre relit en prouvant ses constats par des tests jetables, les constats importants repartent en tour de correction, puis une re-relecture ciblée vérifie le tour.
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
