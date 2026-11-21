# Reprendre Mementum

Ce document est fait pour quelqu'un, humain ou agent IA, qui ouvre ce dépôt sans rien savoir de son histoire. Il dit où en est le projet, ce qui reste à faire, et comment s'y prendre. Il renvoie aux sources plutôt que de les recopier.

Écrit le 2026-11-10, à la clôture du lot 2 de la phase 1. Récrit le 2026-11-17, quand le code des deux parties du lot 3 est sur `main` (Task D de la partie B), puis mis à jour le même jour, après la relecture finale de la partie B, et à la pause du soir, puis aux pauses du 2026-11-18 et du 2026-11-19, et le 2026-11-20 (clôture de la Task S0 du lot 4, décisions de Lenny sur la méthode et sur les poussées), puis le même jour à la fin du code du lot 4 (Task D), et encore le même jour, quand les quatre premières tâches du lot 5 sont faites et que le reste attend la clôture du lot 4, puis le 2026-11-21, à la clôture du lot 4, et le même jour quand le code du lot 5 est sur `main` (Task D du lot 5). À mettre à jour à chaque clôture de lot et à chaque pause. Pendant l'exécution d'un lot, l'état au jour le jour est dans son fichier d'avancement (pour le lot 3 : [`docs/superpowers/logs/2026-11-12-p1-lot3-avancement.md`](superpowers/logs/2026-11-12-p1-lot3-avancement.md) ; pour le lot 4 : [`docs/superpowers/logs/2026-11-20-p1-lot4-avancement.md`](superpowers/logs/2026-11-20-p1-lot4-avancement.md) ; pour le lot 5 : [`docs/superpowers/logs/2026-11-20-p1-lot5-avancement.md`](superpowers/logs/2026-11-20-p1-lot5-avancement.md)) : la plus haute des sections « Reprise du … » ou « Pause du … » prime sur celles qui la suivent ; le tableau « État des tâches », en tête du fichier, date du 13 novembre et ne dit plus l'état. S'il dit plus récent que ce document, c'est l'avancement qui a raison.

## En une minute

- **Mementum** est un espace de travail open source façon Notion, auto-hébergeable, conçu pour que les agents IA lisent et écrivent les pages aussi proprement que les humains. Licence AGPL-3.0.
- **Pile** : serveur Rust en un binaire (axum, sqlx sur SQLite, yrs), client React avec BlockNote dans `apps/web`, texte des pages en CRDT Yjs, format Markdown canonique « MMD » pour les agents.
- **Deux personnes** : Lenny est le propriétaire du projet, ses décisions ne se remettent pas en cause sans lui (liste dans [`CLAUDE.md`](../CLAUDE.md)). Alex pilote le développement et fait la recette.
- **Où on en est** : la phase 0 (essais) est finie. La phase 1 compte sept lots. Les lots 1 et 2 sont faits et poussés. Le lot 3, « Collaboration et blocs riches », est mené en deux plans, et **le code de ses deux parties est sur `main`**, poussé, chaque tâche relue et corrigée. La partie A (barre latérale et présence en direct, commentaires, historique, déplacement de blocs à plusieurs, intégrité de la synchro) a eu sa relecture finale ; sa documentation et sa checklist de recette sont relues contre le code. La partie B (fichiers, blocs riches, mentions, duplication, favoris, récents, fil d'Ariane, reports du lot 2) a sa documentation et sa checklist depuis le 2026-11-17, et sa relecture finale est faite, avec sa vague de correction. L'essai de migration est fait. Reste ouvert un point de sécurité : la **garde du serveur contre les mises à jour Yjs hostiles** (yrs 0.28.1 n'est pas sûr en mémoire devant elles), sur `main` jusqu'à son tour 11 depuis le 2026-11-19, yrs étant vendu dans `vendor/yrs` avec un correctif. La migration `0004` est figée depuis le 2026-11-20 (R936).
- **Reprise du 2026-11-21.** Lenny a demandé que le coordinateur joue lui-même la recette du lot 4 ; elle a été jouée dans Chromium, ses choix confirmés par Lenny (« oui pour tout »), ses corrections relues et fusionnées : **le lot 4 est clos** (R1596), la `0005` figée, et le lot 5 repart à sa Task S1. Pendant la limite Fable (jusqu'au 2026-11-26), tout tourne en Opus 5.5.
- **Le lot 4, « Recherche », est clos le 2026-11-21** : seize tâches sur `main`, passe complète de fin de lot, correction groupée et passe de contrôle (2026-11-20), essai de migration, puis la [recette](recette/2026-11-20-p1-lot4-recette.md) jouée par le coordinateur et ses corrections (R1577 à R1595 : surlignage du bloc atteint, Échap qui reste dans le texte, replis ouverts à l'arrivée, focus rendu aux écrans qui remplacent la page). On cherche dans un espace (« Rechercher », Ctrl+K), on arrive sur le bloc trouvé, on lit sous une page « Pages qui mènent ici » ; un indexeur de fond tient l'index Tantivy d'accord avec la base, et `mementum reindex` le refait serveur arrêté. Le détail : l'[avancement](superpowers/logs/2026-11-20-p1-lot4-avancement.md), le [registre](superpowers/logs/2026-11-19-p1-lot4-ledger.md) (R921 à R1209, puis R1577 à R1596).
- **Le lot 5, « Agents » : code fini sur `main` le 2026-11-21.** Un agent (Claude Code, un script) lit et écrit les pages d'un compte avec un jeton d'API, par le serveur MCP intégré (`/mcp`, six outils, deux révisions du protocole) ou par l'API REST ; ses écritures passent par l'acteur de la page, par différence, et ne font pas perdre ce qu'une personne tape dans les autres blocs ; l'écran « Jetons d'API » crée, révoque et montre l'activité d'un jeton ; l'historique dit « par un agent ». Le cas d'Alex (Claude Code verse un compte rendu dans son espace « Pro » pendant qu'il tape dans la page) passe de bout en bout. Les quinze tâches de son [plan](superpowers/plans/2026-11-20-p1-lot5-agents.md) sont faites, chacune relue une fois et corrigée ; [registre](superpowers/logs/2026-11-20-p1-lot5-ledger.md) R1250 à R1662, [avancement](superpowers/logs/2026-11-20-p1-lot5-avancement.md), [recette](recette/2026-11-20-p1-lot5-recette.md). Les lots 6 et 7 restent à faire.
- **Ce qui suit** : la passe complète de fin du lot 5, sa correction groupée et sa passe de contrôle ; la vérification de `claude mcp add` avec Alex ; la recette du lot 5 (« Le lot 5 : ce qui reste avant sa clôture »). Et la clôture du lot 3, qui attend encore ses mesures de clôture et les recettes de Lenny (« Ce qui reste pour clore le lot 3 »). La `0006` du lot 5 peut changer jusqu'à la clôture du lot 5 : **n'ouvre pas avec un binaire de ce lot les données d'une instance qu'on regarde**.

## Dans quel ordre lire

1. [`CLAUDE.md`](../CLAUDE.md) : les règles du projet et tous les pièges déjà rencontrés. À lire en entier avant de toucher au code.
2. [`README.md`](../README.md) : ce que l'application fait aujourd'hui, comment la lancer, ses limites connues, les prochaines étapes.
3. [`docs/specs/2026-11-09-mementum-design.md`](specs/2026-11-09-mementum-design.md) : la conception. C'est la référence : en cas de conflit, elle l'emporte sur les plans et sur le code. Le §9 dit ce que chaque lot livre et laisse, le §4 décrit les écrans et les droits, le §5.6 le contrôle des accès, le §5.8 les fichiers, le §6 le format MMD et les agents, le §11 les risques.
4. Le registre du dernier lot clos : [`docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md`](superpowers/logs/2026-11-10-p1-lot2-ledger.md). Puis les deux plans du lot en cours, [partie A](superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md) et [partie B](superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md), leur registre commun, [`docs/superpowers/logs/2026-11-12-p1-lot3-ledger.md`](superpowers/logs/2026-11-12-p1-lot3-ledger.md) (une section par tâche et par tour de correction), l'[avancement](superpowers/logs/2026-11-12-p1-lot3-avancement.md) et la [relecture finale de la partie A](superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md). Pour le lot 4 : son [plan](superpowers/plans/2026-11-19-p1-lot4-recherche.md) (le tableau des dix-neuf décisions en tête, les limites L-a à L-s en fin), son [registre](superpowers/logs/2026-11-19-p1-lot4-ledger.md) et son [avancement](superpowers/logs/2026-11-20-p1-lot4-avancement.md). Pour le lot 5 : son [plan](superpowers/plans/2026-11-20-p1-lot5-agents.md) (l'encadré d'état en tête, les décisions L1 à L18 et les limites LA-a à LA-o en fin), son [registre](superpowers/logs/2026-11-20-p1-lot5-ledger.md) et son [avancement](superpowers/logs/2026-11-20-p1-lot5-avancement.md) ; le README, « Brancher un agent », et `spec/mmd/SPEC.md`, la spécification que lisent les agents.
5. L'en-tête de `crates/mementum-server/src/lib.rs` : le tableau de toutes les routes, avec qui y a droit et les statuts de refus.

Chaque décision prise en cours de route est un « Ruling », numéroté à la suite d'un lot à l'autre : R1 à R44 au lot 1, R45 à R128 au lot 2, R129 et plus au lot 3 (R240 et plus pour la partie B, mêlés à ceux de la partie A dans le même registre), R900 et plus au lot 4 (R900 à R920 dans son plan, R921 et plus dans son registre ; R890 à R899 restent à la clôture du lot 3), R1210 et plus au lot 5 (R1210 à R1249 dans son plan, R1250 et plus dans son registre, par plages réservées à chaque tâche ; les débordements partent de R1560, puis de R1600 ; R1577 à R1599 sont au registre du lot 4). On cherche « Ruling R93 » dans les registres pour savoir pourquoi le code fait ce qu'il fait.

## État au 2026-11-21

| Étape | État | Où |
|---|---|---|
| Phase 0, essais | finie, poussée | `spike/`, `spike/RESULTS.md` |
| Phase 1, lot 1 : socle et édition en direct | fini, poussé le 2026-11-09 | plan et registre datés du 2026-11-09 dans `docs/superpowers/` |
| Phase 1, lot 2 : comptes, espaces, droits et corbeille | fini, poussé le 2026-11-10 | plan et registre datés du 2026-11-10 |
| Recette du lot 2 | aucun défaut à corriger avant le lot 3 (réponse de Lenny, 2026-11-12) | [`docs/recette/2026-11-10-p1-lot2-recette.md`](recette/2026-11-10-p1-lot2-recette.md) |
| Phase 1, lot 3, partie A « Collaboration » | **code fini sur `main`** : tâches 1 à 14, relecture finale en trois passes et ses tours de correction, remise au vert de la CI, file d'écriture du store, compte attendu ; une base au schéma du lot 2 passe les migrations `0003` et `0004`. Attend : les suites sur un clone neuf, les budgets mesurés au calme, la recette du propriétaire | [plan](superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md), registre, [relecture finale](superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md), [checklist](recette/2026-11-12-p1-lot3a-recette.md) |
| Phase 1, lot 3, partie B « Blocs riches et organisation » | **code fini sur `main`** : vingt tâches, chacune relue et corrigée, puis la documentation et la checklist (Task D). Relecture finale faite (sécurité, intégration, documentation), sa vague de correction sur `main`. Essai de migration fait (point 4, plus bas). Attend : la clôture de la garde des mises à jour (point 1), les suites sur un clone neuf, la recette du propriétaire | [plan](superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md), registre, [checklist](recette/2026-11-12-p1-lot3b-recette.md) |
| Phase 1, lot 4 : recherche | **clos le 2026-11-21** (R1596) : seize tâches, passe de fin, correction groupée, passe de contrôle, essai de migration, recette jouée par le coordinateur à la demande de Lenny, choix confirmés par lui, corrections sur `main` (`71e6b49`) ; `0005` figée | [plan](superpowers/plans/2026-11-19-p1-lot4-recherche.md), [registre](superpowers/logs/2026-11-19-p1-lot4-ledger.md), [avancement](superpowers/logs/2026-11-20-p1-lot4-avancement.md), [recette](recette/2026-11-20-p1-lot4-recette.md) |
| Phase 1, lot 5 : agents | **code fini sur `main`** le 2026-11-21 (`aad795e`) : quinze tâches, chacune relue une fois et corrigée en un tour, la documentation et la recette (Task D). Attend : la passe complète de fin de lot, sa correction groupée et sa passe de contrôle, `claude mcp add` avec Alex, la recette du propriétaire ; `0006` peut encore changer | [plan](superpowers/plans/2026-11-20-p1-lot5-agents.md), [registre](superpowers/logs/2026-11-20-p1-lot5-ledger.md), [avancement](superpowers/logs/2026-11-20-p1-lot5-avancement.md), [mesures de S0](superpowers/logs/2026-11-20-p1-lot5-s0-mesures.md), [recette](recette/2026-11-20-p1-lot5-recette.md) |
| Phase 1, lots 6 et 7 | à faire, aucun plan écrit | voir plus bas |
| Mesures ARM de la phase 0 | repoussées avant la phase 2 | `spike/RESULTS.md` |

Ce que l'application fait aujourd'hui : des comptes avec mot de passe, des espaces (plusieurs par compte), des membres et des invités, des pages en blocs rangées en arbre et en trois sections (« Espace », « Partagé », « Privé »), l'édition à plusieurs en direct avec le curseur et l'avatar de chacun, la barre latérale qui suit en direct ce que font les autres, le partage d'une page à quatre niveaux, les invitations par lien, la lecture seule selon le droit, la corbeille, les commentaires sur une page ou un bloc, l'historique des versions avec aperçu et restauration, le format MMD en lecture et en écriture par l'API. Depuis la partie B : les fichiers envoyés dans les pages (images, vidéos, sons, fichiers à télécharger), le callout, le signet, les équations, la table des matières, le lien vers une page, les mentions `@` et `[[`, la duplication d'un bloc (Ctrl+D) et d'une page, les favoris, les pages récentes et le fil d'Ariane. Depuis le lot 4 : la recherche plein texte dans un espace (Ctrl+K), filtrée par les droits, qui mène au bloc trouvé, et les rétroliens sous chaque page. Depuis le lot 5 : les jetons d'API du compte (« Jetons d'API » : créer, brancher Claude Code, révoquer, lire l'activité), le serveur MCP intégré et ses six outils, les opérations d'édition des agents et l'écriture par différence (aussi pour `PUT /mmd` et la restauration), « par un agent » dans l'historique.

Ce qu'elle ne fait pas encore : les bases de données et leurs vues, la recherche dans les commentaires, les fichiers ou tous les espaces à la fois, l'envoi d'e-mails, la connexion par Google ou OIDC, le mode suggestion des agents, les agents d'espace, OAuth pour le serveur MCP, les imports et l'export, le binaire qui embarque l'application et l'image Docker, les quotas et les plafonds contre un compte qui abuse (lot 7). Ses limites connues, lot par lot, sont au README.

Derniers relevés, sur le poste de Lenny et **sans clone neuf** (avancement, 2026-11-17) : à la clôture de la garde des attributs, 1 133 tests Rust, 2 474 tests du client (et 53 échecs attendus) et la suite de bout en bout entière, 304 parcours verts ; les budgets du navigateur, 14 sur 14, à la clôture de la Task C8. La CI était verte sur `main` à la pause du 2026-11-19 (`eddbecb`) ; à relire à la reprise (`gh run list --branch main --limit 5`). La migration `0004` de la partie B est figée depuis le 2026-11-20 (R936) ; la `0005` du lot 4 est figée depuis sa clôture, le 2026-11-21 (R1596). Relevé du lot 4, à sa passe de contrôle du 2026-11-20 (atelier `lot4-corr`, poste de développement, pas un clone neuf) : 1 580 tests Rust en 126 binaires, 2 652 tests du client (et 53 échecs attendus), la suite de bout en bout entière, 362 parcours verts et 2 en attente, les seize budgets du navigateur, les mesures du job `budgets`, `cargo deny check` en entier.

## Le lot 4 est clos

Clos le 2026-11-21 (R1596). La passe complète de fin de lot, la correction groupée et la passe de contrôle datent du 2026-11-20 ([compte rendu](superpowers/logs/2026-11-20-p1-lot4-passe-de-fin.md), registre R1200 à R1209) ; l'essai de migration d'une base du lot 3 est au dépôt ([`2026-11-20-p1-lot4-essai-de-migration.md`](superpowers/logs/2026-11-20-p1-lot4-essai-de-migration.md)). La recette a été jouée le 2026-11-21 par le coordinateur, à la demande de Lenny, dans Chromium : 33 cases OK, sept écarts (cinq cases mal écrites, récrites ; deux défauts du produit), six jugements, que Lenny a confirmés avec les choix 1 à 36 et la réponse (a) à la question 37 ; ses corrections, relues une fois et corrigées en un tour, sont sur `main` (registre, « Recette jouée par le coordinateur », R1577 à R1595).

Ce que la fin du lot a reporté, sans le corriger (registre, R1206 et R1207) : le budget « open_1000_block_page_with_50_images_under_300ms », à 307 ms deux fois sur la CI pour 153 à 157 ms ici (D5 : la vitesse du coureur ; **ne jamais relâcher le budget**) ; la mesure « keystroke_that_cuts_a_version_of_10000_block_page_under_150ms », rouge une fois sur la CI (D6 : le point de contrôle du WAL, R447, lot 7) ; le `404` d'une page non vue qui lit plus que celui d'une page inconnue (D12, lot 7) ; une ligne de panique d'un fil de fond vue une fois dans une mesure verte (D13) ; `e2e/undo-redo.spec.ts` « with_two_tabs_redo_with_nothing_to_redo_takes_nothing_away », rouge une fois sur la CI ; les mutations des listes « Preuves » du plan hors des dix-sept de sécurité, non jouées ; le risque RO-1 (rien ne borne ce qu'un compte qui écrit fait coûter à l'indexeur, lot 7).

## Le lot 5 : ce qui reste avant sa clôture

Le code des quinze tâches est sur `main` (`aad795e`, 2026-11-21) ; la Task D (cette documentation, la [recette](recette/2026-11-20-p1-lot5-recette.md), la spécification MMD) est dans l'atelier `lot5-d`. Dans l'ordre, le détail étant dans l'[avancement](superpowers/logs/2026-11-20-p1-lot5-avancement.md), « Ce qui reste pour la fin du lot », et dans « Fin du lot » du plan :

1. **La passe complète de fin de lot, une fois**, sur un clone neuf, étape par étape comme la CI : toutes les suites, la suite de bout en bout entière, les budgets du navigateur, les mesures ignorées (celles de `mementum-server` comprises), `cargo deny check` (les crates neuves de M1), **l'essai hostile large** (dû : depuis S2, il mêle les écritures du serveur par différence, R1299), `Op::AgentOps` et les opérations des agents sous `twins::watching`, les mutations que chaque tâche a listées, et la relecture finale en trois passes (sécurité, intégration, documentation contre le code). Puis la liste des défauts au registre, la correction groupée, la passe de contrôle.
2. **L'essai de migration**, sur des copies, jamais `data/instance` : une base du lot 4 passe `0006`, avec `avant-0006.bak`, ses pages datées et toutes ses versions ; mesurer la durée et la taille du fichier (le coût disque de la reconstruction de `doc_versions`, supposé au README) ; une base du lot 3 passe `0005` puis `0006`.
3. **Le cas d'Alex avec un vrai Claude Code**, une fois, le navigateur ouvert sur la page pendant que l'agent la complète, et ce que Claude Code fait d'une longue sortie (R1662) ; un agent qui le mène suit la forme de R1258 (une configuration à part, jamais celle de l'utilisateur).
4. **Avec Alex : `claude mcp add` dans ses shells** (R1464, R1488, R1576 ; section C de la recette) : ce que la configuration garde de l'en-tête, si Claude Code développe `${MEMENTUM_TOKEN}` en portée `local`, la ligne de Windows PowerShell 5.1, et `--scope user` dans la commande ou non. L'écran passe à `--scope project` si la portée `local` ne développe pas la variable.
5. **La recette du propriétaire**, avec la [checklist](recette/2026-11-20-p1-lot5-recette.md) (une heure et demie) : elle commence par les dix-huit décisions et les choix pris en route.
6. **La clôture** : au registre, les mesures de clôture et les reports ; ce document à jour ; `0006` figée ; les ateliers retirés, leur `target/` supprimé.

## Ce qui reste pour clore le lot 3

Dans l'ordre ; le détail de chaque point est dans l'[avancement](superpowers/logs/2026-11-12-p1-lot3-avancement.md), section « Reprise du 2026-11-17 ».

Déjà sur `main` : la **garde des attributs de bloc** (R689 à R705, R716 à R725, R736 à R739 : un attribut de bloc d'un autre type, et une entrée qui pouvait arrêter le serveur, refusés ; devenue depuis la garde des mises à jour, point 1) et la **correction de « rétablir »** (`editor/redoKeys.ts`, R731 à R734 : Ctrl+Maj+Z sans rien à rétablir retirait la dernière frappe, depuis le lot 1). La **relecture finale de la partie B** est faite, en trois passes (sécurité, intégration entre client et serveur, documentation contre le code) ; ses rapports sont sous `.claude/worktrees/tmp-agents/` (`rfb-sec`, `rfb-int`, `rfb-docs`).

1. **La garde des mises à jour, jusqu'à sa clôture.** La vague de correction de la relecture finale est sur `main` (provenance d'un bloc collé, R746 à R752 et R776 à R780 ; intégration, R757 à R762 ; défauts du produit D-1 à D-4, R763 à R769 ; documentation, R771 à R773). La garde du serveur (grammaire positive de ce que notre client écrit, `crates/mementum-crdt/src/guard/`) est sur `main` jusqu'à son tour 4 (`77be2aa`, R800 à R814). Ses tours de correction 5 à 11 (R815 à R885) sont sur `main` depuis le 2026-11-19 (`5481391`) : yrs 0.28.1 est vendu dans `vendor/yrs` avec un correctif de sa fusion quadratique (`squash_left`, R874 à R879), et la borne mémoire de la garde est retirée. Le détail : section « Reprise du 2026-11-19 (journée) » de l'avancement. Le texte du signalement du défaut à yrs est prêt pour Lenny (`docs/superpowers/logs/2026-11-19-yrs-squash-left-upstream.md`).
2. **La documentation** suit chaque correction.
3. **Sur un clone neuf : fait le 2026-11-19**, sur `dcf0495`. Chaque étape de la CI y passe, une à la fois : `cargo fmt --check`, clippy, `cargo test --workspace` (1 318 verts, 110 binaires), fichiers générés à jour, types, Vitest (2 516 et 53 échecs attendus), licences npm, e2e (328 réussis, 1 ignoré : la limite R222), budgets du navigateur (14 sur 14), mesures de `mementum-sync` et de `mementum-cli` (10 sur 10), et en plus celles de `mementum-server` (9 sur 9). Seul `cargo deny` n'a pas tourné (outil absent du poste ; le job `deny` de la CI est vert sur ce commit). Les chiffres sont dans l'avancement, section « Reprise du 2026-11-19 (journée) ».
4. **L'essai de migration : fait le 2026-11-17**, sur des copies, jamais sur `data/instance` (rapport et scripts rejouables sous `.claude/worktrees/tmp-agents/essai-migration/`, `rapport.md`). Une base du lot 2 passe les migrations `0003` et `0004` dans la même ouverture, avec une seule copie (`avant-0003.bak`) : 34 vérifications sur 34 (tables, droits relus par l'API pour chaque compte, fichier envoyé puis relu après un redémarrage, copie remise en place sans qu'aucun fichier quitte le disque). L'instance d'Alex, base du lot 2, est dans ce cas. Une base de la partie A à la `0003` finale passe aussi (`avant-0004.bak`, 34 sur 34), mais elle **n'a pu être écrite que par un binaire reconstitué** : aucun commit n'a la `0003` finale (`e4a065d`, 15/11) sans la `0004` (`60e0a60`, 13/11), et toute base qu'un vrai binaire de la partie A a migrée est refusée, base intacte, comme le veulent R139 et R243 ; la reprise est la copie d'avant la première migration du lot 3 appliquée par ce binaire, et une base qu'il a créée n'a ni copie ni sauvegarde qui s'ouvre (elle se rouvre avec lui, ou l'on repart d'une base neuve). Les trois défauts mineurs de l'essai sont corrigés (R840 : refus avant toute copie, en français, qui nomme la copie à reprendre ; R841 : une copie remise en place est reconnue ; R842 : une erreur du démarrage dite une fois) et ses trois écarts de documentation repris au README et dans la conception (R843, R844) ; la relecture de ces corrections a demandé un tour (R845 à R852 : un refus ne modifie pas non plus une base hors du mode WAL, une ligne notée inachevée est refusée avant toute copie, la copie nommée est mieux choisie, une base créée par un binaire intermédiaire est dite telle).
5. **Les recettes du propriétaire**, avec les checklists de la [partie A](recette/2026-11-12-p1-lot3a-recette.md) et de la [partie B](recette/2026-11-12-p1-lot3b-recette.md). Un agent les a déroulées dans Chromium le 2026-11-19 (153 cases sur 173 passent, deux défauts de focus corrigés, R888 et R889) : restent à Lenny 18 cases de jugement, la case B.M5 et les choix de produit de la partie B ; le détail est dans l'avancement, « La recette déroulée par un agent ».
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
5. **Pousser** : `main` a été poussé le 2026-11-12 à la demande de Lenny, lot en cours ; il a ajouté le même jour que les commits du lot 3 se poussent au fil du travail, sans redemander (« les commits tu peux les push seul »). Le 2026-11-20, il a étendu cet accord à tous les lots (« pas besoin de me demander tu le fais pour tous les lots ») : plus rien à lui demander là-dessus. L'accord tient tant que ce document dit où on en est ; ce que l'état poussé doit tenir pendant un lot est dit dans « Comment mener un lot ». Le 2026-11-15, Lenny a décidé de ne pas pousser les ateliers comme sauvegarde : ils restent locaux.
6. **Le lot 4** : rien. Ses décisions et les choix pris en route sont confirmés par Lenny le 2026-11-21 (« oui pour tout »), Échap compris.
7. **Le lot 5** : les décisions L1 à L18 du [plan](superpowers/plans/2026-11-20-p1-lot5-agents.md), appliquées par défaut (L13, L16 et L17 tranchées par le coordinateur à la relecture du plan, L18 ajoutée à la relecture de M1 ; L16 est levée par la clôture du lot 4), et les vingt-cinq choix pris en route, listés en tête de la [recette](recette/2026-11-20-p1-lot5-recette.md) (choix 19 à 43). Trois demandent une réponse : une trace des sondes d'un jeton sur les routes qui lui sont fermées (R1361, choix 21) ; l'avertissement d'une page piégée et l'ordre des portées dans l'écran des jetons (R1500, choix 24) ; `--scope user` dans la commande prête à coller (R1464, choix 28). Rien ne bloque le travail.

Les espaces façon Notion, à la place des coffres façon Obsidian, sont confirmés par Lenny (2026-11-10).

## Ce qui reste à construire

L'ordre prévu des lots est dans le README (« Prochaines étapes ») et la conception (§9). En bref :

| Lot | Sujet | Ce qu'il reprend des lots faits |
|---|---|---|
| 3 | Collaboration et blocs riches | code fait ; clôture en cours (plus haut) |
| 4 | Recherche | clos le 2026-11-21 (plus haut) |
| 5 | Agents : serveur MCP, jetons d'API sur un espace ou sur des pages, opérations d'édition ; **code fait**, clôture en cours (plus haut) ; de l'API REST, seulement les routes des six outils (R1210) | a levé en partie R40 et R188 (l'écriture par différence) ; a renvoyé au lot 6 le porteur durable des propriétés inconnues (R41, décision L15) |
| 6 | Imports et export | les pièces jointes d'Obsidian et de Notion ; le porteur durable des propriétés inconnues (R41) et `[[Page#Titre]]`, renvoyés par le lot 5 |
| 7 | Livraison : binaire qui embarque l'application, image Docker, fichier `mementum.toml`, inscription par domaine | les plafonds et limites de débit (R54, R121, R511, R516), la relecture périodique des sessions (R125), le contrôle des avis de sécurité en CI (R44) ; pour les fichiers : quotas par espace, envois simultanés et débit par compte, délai de lecture des corps (R247, R505), ménage des fichiers qu'aucun bloc ne montre, purge différée (R575), miniatures, antivirus ; la copie d'une page en plusieurs étapes au-delà de 16 Mio (R608) ; pour la recherche, les reports du lot 4 (liste plus bas) |

Après le lot 3, un plan à part pour la frappe arrivée au serveur après le déplacement de son bloc (R222). Sans date, à décider avec le propriétaire : l'envoi d'e-mails (invitations, mot de passe oublié), la connexion par Google ou OIDC, la suppression d'un espace ou d'un compte, le vidage automatique de la corbeille, les passkeys (R46). Les bases de données et leurs vues sont la phase 2 ; les notifications, `@rappel` et les liens publics, la phase 3 ; S3 et les URL signées, la phase 5.

### Reportés par le lot 5

Rassemblés du [registre du lot 5](superpowers/logs/2026-11-20-p1-lot5-ledger.md) et de la fin de son plan ; chacun est aussi une limite écrite (README, « Limites connues du lot 5 » ; conception, §9).

1. **Une tâche dédiée, proposée juste après le lot** (R1656, R1661, limite LA-q) : sur une page de 10 000 blocs, tout changement distant fige l'onglet qui le reçoit environ 0,7 s (y-prosemirror remplace tout le document, et deux greffons de BlockNote relisent chaque bloc). Piste : dans `patches/y-prosemirror@1.3.7.patch`, ne remplacer que l'intervalle qui diffère ; deux à trois jours, risque moyen à élevé (les déplacements de blocs, la sélection suivie et les fils de commentaires en dépendent).
2. **Au lot 6** : le porteur durable des propriétés inconnues (R41, décision L15) ; `[[Page#Titre]]`.
3. **Au lot 7** : le pont stdio (`mementum mcp`, décision L12) ; l'envoi de fichiers par jeton, avec les quotas (R1222) ; les plafonds généraux par compte et par adresse IP, dont la trace des sondes de routes fermées (R1361) ; un plafond des acteurs sans connexion pour toute l'instance (R1657) ; un débit pondéré par le temps d'écriture (R1660) ; l'indexeur sans part par compte (RO-C) ; la preuve d'un compte sans mot de passe pour créer un jeton (décision L2) ; relire le jeton dans toute transaction faite par jeton (R1658), si la passe de fin ne le fait pas.
4. **Phase 2** : l'OpenAPI (décision L11), le filtre « type » de la recherche des agents (décision L9), les propriétés de `set_properties` (écart E12), le frontmatter du MMD (écart E5).
5. **Phase 3** : le mode suggestion (le défaut de `mode` deviendra `suggest`), le journal d'audit (écart E10).
6. **Phase 4** : OAuth 2.1 et CIMD pour le serveur MCP (écart E7), les agents d'espace et leurs jetons d'intégration, le niveau « complet » par jeton (décision L13), la présence d'un agent, l'annulation par exécution (écart E19), qu'un admin voie les jetons ouverts sur son espace (décision L10).

### Reportés au lot 7 par le lot 4

Rassemblés du [registre du lot 4](superpowers/logs/2026-11-19-p1-lot4-ledger.md) ; chacun est aussi une limite écrite (README, « Limites connues du lot 4 » ; conception, §9).

1. **RO-1, le passage borné en octets** (R928, R972, R1169, limite L-g) : la pointe de mémoire de l'indexation vient des entrées en file devant l'écrivain du moteur, bornées en nombre (10 000) et pas en octets ; 100 Mio de mots courts tous différents coûtent 276 Mio de tas et 979 Mio de mémoire de travail. Un passage borné en octets, mesuré avant d'être posé (une politique de fusion bornée laisserait les entrées supprimées dans les grands segments).
2. **Un débit par compte de l'indexeur** (R997, R860, R1169) : l'indexeur est unique et sériel ; une page coûteuse à relire, écrite sans relâche, le tient pour toute l'instance.
3. **Une place bornée de l'index sur le disque** (L-g), avec les quotas de fichiers (R247).
4. **Une borne de durée d'un calcul de recherche** (R1045, R1172) : un collecteur qui regarde l'heure ; aujourd'hui un calcul lancé va au bout (269 ms pour huit mots au million de blocs).
5. **`behind` qui lit toute l'outbox** (R1047, R1055) : savoir si une page vue est en retard parcourt toute la file de l'indexeur (100 ms pour 100 000 pages), au plus une fois par seconde et par espace hors repos ; une requête qui ne lit que l'espace demandé, côté store.
6. **Un verrou de toute l'instance**, pris avant d'ouvrir la base (R1013, R1069) : aujourd'hui, un second serveur, ou un serveur lancé pendant `mementum reindex`, ouvre et migre la base avant d'être refusé par le verrou de l'index.
7. **Un débit par compte des lectures de rétroliens** (R1019) : une boucle de lectures sur une page dont dix mille sources sont cachées coûte la borne du parcours à chaque appel.
8. **La relecture périodique de la session des connexions ouvertes** (R125), et pendant l'attente d'une place au débit de la recherche (non faite en S5).
9. **Une forme plus courte de `blocks_index`** (R937, R1170) : de l'ordre de 73 Mio de trop par million de blocs.
10. **Le temps de réponse d'un `404`** d'une page non vue contre une page inconnue (antérieur au lot 4, noté par S6, R1022), avec les autres écarts de temps déjà reportés.

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

Lenny a choisi le 2026-11-12 : **le lot 3, comme prévu**, puis le lot 4, mené le 2026-11-20. Le lot 5 (agents, serveur MCP : Alex veut que Claude Code puisse verser du contenu dans un de ses espaces) est commencé le 2026-11-20 ; son code est sur `main` depuis le 2026-11-21. La suite immédiate est la fin du lot 5 (« Le lot 5 : ce qui reste avant sa clôture ») ; puis la recette de Lenny du lot 3, et la clôture du lot 3 ; puis le lot 6, imports et export, dont le plan reste à écrire. Un poste peut disparaître : le 2026-11-15, tout ce qui n'était pas poussé a été perdu avec l'ancien poste, et refait d'après l'avancement et le registre. Ce qui n'est que dans un atelier n'existe que là.

## Comment mener un lot

La méthode est écrite dans le README (« Méthode de travail ») et a été suivie telle quelle aux lots 1 à 3. **Lenny l'a changée le 2026-11-20, à partir du lot 4** : voir « Depuis le lot 4 », sous la liste, qui remplace ses points 3 à 5.

1. **Un plan** pour le lot, dans `docs/superpowers/plans/`, écrit d'après la conception : des tâches petites, chacune avec ses fichiers, ses interfaces, ses tests nommés. Le propriétaire le relit avant qu'on commence.
2. **Un registre** pour le lot, dans `docs/superpowers/logs/` : chaque écart au plan y devient un Ruling, au format `Ruling: <décision> — <pourquoi> — <coût si faux>`.
3. **L'exécution tâche par tâche**, en TDD : le test qui échoue d'abord. Aux lots 1 et 2, chaque tâche a été confiée à un sous-agent dans un worktree à part, puis relue par un autre, puis reprise sur `main`. Au lot 3, un contrôleur mène les tâches (R140) : un sous-agent implémente, un autre relit en prouvant ses constats par des tests jetables, les constats importants repartent en tour de correction, puis une re-relecture ciblée vérifie le tour. La partie A a commencé directement sur `main`, faute de disque, puis a pris des ateliers (`git worktree`) pour les travaux menés de front ; la partie B s'est menée en deux pistes de front, serveur et client (R241), chaque tâche dans un atelier rebasé sur `main`, puis reprise par avance rapide.
4. **Une relecture finale de tout le lot**, en passes séparées (sécurité, intégration entre client et serveur, documentation contre le code), puis une vague de correction et des re-relectures ciblées.
5. **Toutes les suites sur un clone neuf**, étape par étape comme la CI, avant de dire que c'est fini.
6. **Une checklist de recette** pour le propriétaire, sur ce qu'un test ne juge pas.

**Depuis le lot 4** (décision de Lenny du 2026-11-20, R935 ; ses mots : « faire ceux qui doivent être faits pour l'ajout et seulement à la fin […] un test complet pour voir où sont les problèmes et corriger tout d'un coup », « par lot ») :

- **Pendant le lot, une tâche n'écrit et ne lance que les tests de son ajout et les suites qu'elle touche directement** (le fichier de tests d'un module modifié, le test d'un contrat qu'elle change). Le test qui échoue d'abord reste la règle pour ce qu'elle ajoute.
- **Entre les tâches, plus de** suite complète (`cargo test --workspace`, Vitest entier), de suite de bout en bout entière, d'essai au hasard large, de campagne de mutations, ni de tours de relecture en boucle. Une tâche a **une seule relecture**, sans re-relecture : ses constats sont corrigés une fois par la tâche, ou notés pour la fin du lot.
- **La CI de chaque poussée reste lue** (`gh run list --branch main --limit 5`). Un rouge dans l'ajout en cours se corrige tout de suite ; un rouge hors de lui se note dans la liste de fin de lot (registre du lot, « À reprendre à la fin du lot »), sans arrêter la tâche. `main` peut donc être rouge pendant un lot : le registre dit pourquoi.
- **À la fin du lot, une fois** : (1) une passe complète unique, sur un clone neuf, étape par étape comme la CI (toutes les suites, la suite de bout en bout, les budgets, les mesures, les essais au hasard larges des parties touchées, les mutations que chaque tâche a listées, et la relecture finale en ses passes : sécurité, intégration, documentation) ; (2) **la liste de tous les défauts** qu'elle trouve, au registre ; (3) **une correction groupée** ; (4) **une passe de contrôle**, qui rejoue ce qui était rouge et ce que la correction touche. Pas de troisième tour sans un défaut neuf et grave, dit à Lenny.
- Ce qui ne change pas : la conception l'emporte, aucun budget relâché, aucun test existant affaibli, un Ruling par écart, la recette du propriétaire à la fin.

Ce que les lots faits ont appris, et qui ne se devine pas :

- La relecture finale trouve ce que les relectures de tâche ne voient pas. Au lot 2, ses trois passes ont trouvé une perte de frappes, une session qui survivait à un changement de mot de passe et une page qui ne pouvait pas redevenir privée, dans du code dont chaque tâche avait été approuvée. Au lot 3, celle de la partie A a demandé sept tours de correction côté serveur (lectures quadratiques, puis exponentielles, d'une entrée hostile ; une relecture du journal qui effaçait la lettre d'un autre).
- Un relecteur qui prouve ses constats par un test jetable vaut mieux qu'un relecteur qui lit. Demander la preuve.
- Une règle de protection est à relire comme une attaque : la première version d'une limite fermait la connexion fautive, et donnait ainsi à un lecteur le moyen de faire fermer celle des autres (R113).
- Une garde de débit suit le travail qu'elle borne, pas la requête qui l'a demandé : une duplication abandonnée par son client continuait de calculer sans être comptée (R630).
- Les compilations parallèles remplissent le disque (10 à 40 Go par worktree). Supprimer le dossier `target` d'un atelier dès qu'il a rendu.
- Ne jamais relâcher un budget de performance, sa statistique ni sa définition pour faire passer un test.
- Lire la CI après chaque poussée de `main` : elle est restée rouge deux jours sans que personne le voie. Depuis le lot 4, la lire ne veut plus dire tout arrêter pour un rouge hors de l'ajout en cours, mais le noter : ce qui est interdit, c'est de ne pas le savoir.

## Commandes

Elles sont détaillées dans le README. Les plus utiles :

```
cargo fmt --check
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

- `cargo fmt --check` est la commande de la CI, et pas `cargo fmt --all -- --check` : `--all` formate aussi les dépendances locales, donc `vendor/yrs`, qu'on ne formate jamais (yrs recopié de crates.io, hors de l'atelier Cargo : dix-neuf écarts qui ne sont pas les nôtres) ; et dans un atelier posé sous `.claude/worktrees/`, cargo le refuse (« current package believes it's in a workspace when it's not »).
- `pnpm` n'est pas dans le PATH du poste d'Alex : passer par `corepack pnpm`.
- Les parcours de bout en bout lancent leurs propres serveurs. Pour ne pas gêner une instance qui tourne, leur donner d'autres ports : `MEMENTUM_E2E_WEB_PORT` et `MEMENTUM_E2E_API_PORT`.
- `cargo deny check advisories` est vert depuis la fin du lot 4 (R1205 : `sqlx` 0.9.1 et `libsqlite3-sys` 0.40.0 lèvent RUSTSEC-2026-0459, l'avis de R44) ; un avis qui y paraît est donc neuf. `smallstr`, non maintenu et tiré par `yrs`, est toujours au verrou, sans que cargo-deny 0.22.0 le signale.
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
- **Pousser** : depuis le 2026-11-20, l'accord de Lenny de pousser sur `main` au fil du travail vaut **pour tous les lots**, sans le redemander (il l'avait donné pour le lot 3 le 2026-11-12). Jamais de poussée forcée, jamais de secrets, jamais la branche d'un atelier. Une demande d'Alex, elle, ne vaut que pour ce qu'elle nomme : un accord qu'il donne pour un lot ne vaut pas pour le suivant.
- **Aucun secret** dans le dépôt, les journaux, la documentation.
- **Tout sur `main`** : pas de branche durable. Les worktrees sont des ateliers, leurs commits sont repris sur `main`.
