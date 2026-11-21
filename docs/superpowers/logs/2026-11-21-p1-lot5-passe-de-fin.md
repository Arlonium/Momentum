# Phase 1, lot 5 « Agents » : la passe complète de fin, suites, essais au hasard, migration, vrai client (2026-11-21)

Méthode de R935 : tout ce que les tâches du lot n'ont pas lancé, lancé une fois. Cette moitié de la passe (points 2 à 4 du plan, « Fin du lot », et la ligne `MESURE` de la mémoire, R1659) ; l'autre moitié, la relecture finale en trois passes et les mutations, est au [compte rendu de la relecture](2026-11-21-p1-lot5-relecture-finale.md) (R1530 à R1541, défauts nommés S, I, O). **Rien du produit n'est corrigé ici** : ce document est la liste des défauts, pour la correction groupée. Rulings de cette moitié : R1510 à R1529 (R1510 à R1515 pris, R1516 à R1529 libres), au [registre](2026-11-20-p1-lot5-ledger.md), « Fin du lot : suites, essais au hasard, migration, vrai client ».

Poste de développement sous Windows (24 cœurs), partagé avec l'agent de la relecture ; un clone neuf de `main` à `900dab1`, compilé dans son propre dossier (`CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=6`), une commande lourde à la fois (les essais au hasard, sur un fil chacun, ont tourné à côté). Le code de test ajouté (point 2) est dans l'atelier `lot5-fin-essais`, non poussé : `0f0bca7`, `fe88a5d`, `e43c8ff`, `5da5977`, `3847055` (rebasés sur `1e31755`).

En bref : **toutes les suites sont vertes sur ce poste**, mesures et budgets compris ; un défaut **important**, la mémoire au repos sous Linux (D1 : 720 à 789 Mo sur trois passages de la CI, pour un budget de 150 Mo) ; l'essai hostile large trouve **une graine** (D3, 27811 de la voie du journal), antérieure au lot 5 (la même trace donne le même écart avec le code de la clôture du lot 4) ; l'essai hostile avec écritures d'agent sous `twins` trouve un défaut mineur de l'écriture par différence sur un bloc marqué étranger par un client (D13) ; l'essai dense et `reload` élargi (`Op::AgentOps`) ne trouvent rien ; la migration d'une base du lot 4 et d'une base du lot 3 passe, versions et dates comprises ; le vrai Claude Code mène le cas d'Alex de bout en bout pour 0,59 $, et bute deux fois sur le même refus (D2 : un MMD sans fin de ligne finale). Aucun défaut bloquant.

## Étapes

| Étape | Commande | Durée | Résultat |
|---|---|---|---|
| 1 | `cargo fmt --check` | 3 s | vert |
| 1 | `cargo clippy --workspace --all-targets -- -D warnings` (clippy 1.100) | 53 s | vert, aucun avertissement |
| 2 | `cargo test --workspace --no-fail-fast` | 20 min 57 s | vert : 143 binaires, 1 922 tests passés, 0 rouge, 60 ignorés (les mesures) |
| 3 | `git diff --exit-code packages/protocol spec/schema` (après les tests) | — | vert : fichiers générés à jour |
| 4 | `corepack pnpm install --frozen-lockfile` | 3 s | vert |
| 4 | `corepack pnpm -C apps/web typecheck` | 1,5 s | vert |
| 4 | `corepack pnpm -C apps/web test` | 31 s | vert : 127 fichiers, 2 708 passés, 53 échecs attendus (`knownLimit`) |
| 4 | `node tools/check-npm-licenses.mjs` | 0,2 s | vert : 117 paquets |
| 5 | `cargo deny check` **entier** (cargo-deny 0.22.0 installé dans le dossier de la passe, base des avis du jour) | 1,5 s | vert : `advisories ok, bans ok, licenses ok, sources ok` ; treize doublons signalés, sans blocage. `schemars` 1.2.3, `schemars_derive`, `dyn-clone` et `ref-cast` passent (R1418 levé) |
| 6 | `playwright test` (ports 5193 et 8106) | 12 min 20 s | vert : 381 passés, 1 ignoré (le `fixme` de R1143) |
| 6 | les trois e2e rouges de la CI hors des ajouts (registre, « Rouges de la CI hors des ajouts ») et `auth.spec.ts` « account_changed_in_another_tab_reloads_on_return », cinq fois chacun | 45 s | 20 sur 20 verts : non reproduits |
| 6 | `e2e/agent.spec.ts` « an_agent_edits_while_a_human_types_and_nothing_is_lost » (rouge une fois sur la CI de `900dab1`), dix fois | 1 min 18 s | 10 sur 10 verts (D9) |
| 7 | `playwright test -c playwright.budgets.config.ts` | 7 min 46 s | vert : 18 passés |
| 7 | la même, `MEMENTUM_BUDGETS_CPU_SLOWDOWN=1.5` | 5 min 51 s | vert : 18 passés |
| 8 | `cargo test --release -p mementum-sync -p mementum-cli -- --ignored --nocapture` (le job `budgets`) | 6 min 40 s | vert |
| 8 | `cargo test --release -p mementum-server --test indexer --test search_measure --test agent_budgets -- --ignored --nocapture --test-threads=1` (toutes leurs mesures, celles de la CI comprises) | 7 min 52 s | vert (une ligne de panique d'un fil de fond, sans échec : D11) |
| 8 | `cargo test --release -p mementum-crdt -- --ignored --nocapture --test-threads=1` | 4 min 21 s | vert |
| 8 | `cargo test --release -p mementum-server --test tokens_measure --test measure --test page_writes --test sync_rights -- --ignored --nocapture --test-threads=1` | 7 min 38 s | vert (même ligne de panique, une fois : D11) |
| 9 | essais au hasard (plus bas) | — | une graine antérieure au lot (D3) ; un défaut mineur de l'écriture par différence (D13) |
| 10 | essai de migration (plus bas) | — | vert |
| 11 | le cas d'Alex avec un vrai Claude Code (plus bas) | — | vert, D2, D4, D5 |

## Mesures, contre leurs budgets

Release, une à la fois, ce poste ; les chiffres du lot 5 seulement, les autres sont dans les journaux du passage et restent dans leurs budgets.

| Mesure | Chiffre | Budget | Tenu |
|---|---|---|---|
| Frappe dans une page de 10 000 blocs pendant 120 `str_replace` d'un agent | p95 65,2 ms, la pire 94,0 | < 150 ms au p95 | oui |
| …pendant 60 lectures et `replace_block` (60 `before_agent` coupées) | p95 65,2 ms, la pire 100,8 | < 150 ms | oui |
| …pendant qu'un agent crée 2 400 pages ; puis pendant que l'indexeur les relit | p95 9,0 ms ; 5,9 ms | < 150 ms | oui |
| …pendant que trois jetons du compte écrivent au débit du compte | p95 79,1 ms, la pire 125,1 | < 150 ms | oui |
| Appel d'un agent vu dans le navigateur, page de 1 000 blocs, ×1 ; ×1,5 | p95 30 ms ; 32 ms | < 150 ms | oui |
| …page de 10 000 blocs, ×1 ; ×1,5 | p95 843 ms ; 1 321 ms | aucun (R1656, LA-q) | — |
| Mémoire en propre au repos après 500 lectures d'agent, **ce poste (Windows)** | 104,3 Mo (403,8 deux secondes après, les acteurs vivants) | < 150 Mo | oui |
| …**sous Linux, CI** (`44903469281`, `44903727230`, `44908871771`) | 719,7 ; 789,4 ; 729,9 Mo | < 150 Mo | **non** (D1) |
| Recherche filtrée par la route, 10 000 pages (auteur, date, les deux) | p95 62,9 à 63,2 ms | < 100 ms | oui |
| Un mot tapé trouvable (instance de 10 000 pages ; trois agents qui récrivent leur page) | 2,09 à 2,14 s ; 2,08 à 2,65 s | < 5 s ; aucun | oui ; — |
| Écriture d'agent sur 10 000 blocs (`str_replace`, `insert`, `replace_block`) ; vue par un client | p95 73 à 75 ms ; 68,8 ms | aucun | — |
| Opération destructrice avec version, appel entier | 235 à 282 ms | aucun | — |
| Démarrage avec un index de 10 000 pages ; mémoire en propre au repos | 23,5 ms ; 10,9 Mo | < 1 s ; < 150 Mo | oui |
| Budgets du navigateur, ×1 / ×1,5 : 1 000 blocs ; 100 blocs riches ; callouts ; 50 images ; arbre de 1 000 pages ; page visée par 200 | p95 140 / 223 ; 171 / 263 ; 165 / 259 ; 157 / 259 ; 150 / 224 ; 145 / 223 ms | < 300 ms | oui |
| De la dernière touche au résultat affiché | p95 225 / 302 ms | < 400 ms | oui |
| `measure_worst_accepted_writes` (toutes formes) ; `measure_rewrites_of_a_rich_page` | 5,24 s ; 0,74 à 0,90 s | < 10 s ; < 4 s | oui |
| `diff_cost` contre le temps écrit | de 1,43 à 9,69 fois au-dessus ; 0,98 et 0,53 sur une page qui garde des blocs supprimés | — | connu (R1294, D12) |

## Essais au hasard

| Essai | Cas | Durée | Résultat |
|---|---|---|---|
| `hostile_at_random`, `main` (`900dab1`), `MEMENTUM_FUZZ_SEEDS=45000`, release, les deux yrs comparés à chaque cas | 45 000 par voie | 43 min 48 s (20 min, puis 24) | voie du journal : 100 648 hostiles acceptées, 895 505 mises à jour comparées dans les deux yrs, **1 échec** (graine 27811, D3), 0 arrêt du processus ; voie des clients : 33 537 hostiles acceptées, 834 729 mises à jour comparées, 0 échec |
| `hostile_at_random` de l'atelier (écritures d'agent un pas sur deux, retraits par identifiant comparés), 2 000 cas | 2 000 par voie | 3 min 31 s | 0 échec ; par voie, environ 4 400 écritures d'agent écrites, 360 sans effet, 5 800 plans refusés (`stale_anchor`, `no_match`, `ambiguous_match`, `protected_content`, `external_media`, `invalid_structure`, `invalid_move`) ; 55 000 et 52 500 mises à jour comparées |
| la même, `MEMENTUM_FUZZ_SEEDS=45000`, premier passage (avant le test de D13 et la règle des routes) | 45 000 par voie | 76 min 56 s | voie du journal : 106 422 hostiles acceptées, 1 240 262 mises à jour comparées, 98 771 écritures d'agent écrites, 8 029 sans effet, 128 549 plans refusés ; **5 échecs** : 27811 (D3), 6260, 8404, 15555 (D13), 35892 (un bloc étranger écrit par l'agent, que les routes n'admettent pas : un défaut de l'essai) ; voie des clients : 37 558 hostiles acceptées, 1 180 663 mises à jour comparées, 99 648 écritures d'agent écrites, **1 échec** (29862, le même défaut de l'essai que 35892) ; 0 arrêt du processus |
| la même, second passage, code final (`3847055`) | 45 000 par voie | voie du journal 33 min 54 s ; voie des clients 42 min 36 s | voie du journal : 106 424 hostiles acceptées, 1 240 399 mises à jour comparées, 98 774 écritures d'agent écrites, 8 030 sans effet, 128 556 plans refusés, **1 échec** (27811, D3), 0 arrêt ; voie des clients : 37 560 hostiles acceptées, 1 180 699 mises à jour comparées, 99 650 écritures d'agent écrites, 8 206 sans effet, **0 échec** |
| `placements_at_random` (l'essai dense), `MEMENTUM_FUZZ_SEEDS=6000` | 6 000 par voie | 15 min 10 s | 0 échec ; 211 482 mises à jour acceptées, 252 581 comparées dans les deux yrs |
| `reload` de l'atelier (`Op::AgentOps`, `Op::DiffWrite` élargi), `PROPTEST_CASES=10000`, release | 10 000 suites de 1 à 23 opérations | 1 min 9 s | vert ; sur une sonde de 300 cas, 44 % des listes d'opérations d'agent passent le plan (les autres : `stale_anchor`, `no_match`, `revision_conflict`, `ambiguous_match`, `external_media`, `protected_content`) |

**Ce qui a été ajouté** (point 2 du plan ; code de test seulement) :
- `crates/mementum-crdt/tests/common/agent_ops.rs` : une liste d'une à quatre opérations de la Task S3 tirée au hasard sur les blocs qu'un agent a lus (ancres : l'alias, l'identifiant entier, une ancre qui ne désigne rien ; MMD : paragraphe, titre, listes imbriquées, tâche, dépliant, citation, code, tableau, colonnes, lien de page, bloc inconnu, image du web, ancre écrite, texte marqué et lien ; `replace_page` sur les blocs lus sous leurs alias, retouchés), et `plan_written`, l'égalité de la page écrite et du plan au nommage près (R1512).
- `reload.rs` : `Op::AgentOps { seed, stale, token }` (`ops::plan` puis `PageDoc::apply`, comme l'acteur ; `stale` : en `rebase`, `Base::Changed`, sur la lecture de l'écriture d'agent d'avant ; `token` : aucun média du web), mêlée aux frappes, déplacements, mises à jour faites à la main et écritures par différence ; après chaque écriture d'agent, la page écrite est le plan (au nommage près) et les blocs que l'écriture rend sont la page relue (l'affirmation de débogage de l'acteur) ; après chaque pas, la page relue de son journal est la page vivante. `Op::DiffWrite` porte une à trois retouches, treize sortes au lieu de six (un sous-bloc ajouté, les sous-blocs d'un bloc retirés, un paragraphe changé en titre, une prop posée, un texte mis sous un lien, un tableau ajouté, un saut de ligne). `PROPTEST_CASES` règle le nombre de cas.
- `hostile_at_random.rs` : un pas sur deux, une écriture d'agent tirée d'un générateur à elle (R1513), ses MMD sans `foreign` comme les routes (R33), donnée aux deux yrs (`twins::written_alike`) ; pour toute écriture du serveur, par différence ou d'agent, **son seul jeu de suppressions** est aussi donné aux deux yrs sur l'état d'avant (R1511). Une écriture qui échoue après un plan vérifié est un échec ; un plan refusé est compté par code. Et la graine 27811, réduite (`fixtures/hostile/min-j27811.txt`), écrite en `known_limits` (R1515).
- `diff.rs` : D13 écrit en `known_limits` (R1515) ; les comparaisons des essais laissent la `data` des blocs marqués étrangers à ce test (`without_foreign_data`).
- **Coût dans la CI** : au nombre de cas par défaut, en debug, `hostile_at_random` passe de 321,7 s (dans `cargo test --workspace`, plus haut) à 493,9 s seul sur ce poste (les écritures d'agent un pas sur deux) ; `reload` de 5,2 à 5,6 s, `diff` inchangé. Le job `rust` de la CI s'allonge d'environ trois minutes s'il attend ce binaire ; réduire la part des écritures d'agent dans le passage ordinaire (comme les comparaisons des deux yrs, un cas sur quatre) est une décision de la correction groupée.

**La question de R1281** (les retraits par identifiant, seule mise à jour donnée à yrs hors de `PageDoc::read`, entrent-ils dans l'essai hostile ?) : **oui, et ils y entrent désormais** (R1511) : la forme exacte de ces retraits (une mise à jour qui ne porte qu'un jeu de suppressions, appliquée à l'état d'avant l'écriture) est rejouée dans le yrs corrigé et dans celui de crates.io après chaque écriture du serveur de l'essai. Rien n'a différé : aucun des échecs de la table n'est une différence entre les deux yrs (plus de 2,4 millions de mises à jour comparées dans l'essai large, retraits compris).

## Essai de migration

Sur des copies, dans le dossier de la passe, jamais `data/instance` ; binaires de release de `5d10757` (clôture du lot 4) et de `4f1d77e` (lot 3), chacun dans un atelier détaché et son propre dossier de compilation ; le binaire du lot 5 est celui du clone (`900dab1`). La base est bâtie par un script (`--version-every-secs 1`) : un compte, trois petites pages écrites deux fois, une grande page de 10 000 paragraphes écrite quarante fois à plus d'une seconde d'écart, une restauration de sa plus ancienne version après une écriture, puis 40 s sans écriture.

**Base du lot 4** : 4 pages, 47 versions (43 `periodic`, 3 `closing`, 1 `before_restore`), 362,9 Mo d'états de versions ; `mementum.db` 398,4 Mo et son WAL 18,0 Mo.
- Le binaire du lot 5 démarre en **3,8 s** jusqu'à `/health` (copie et migration comprises), écrit `mementum.db.avant-0006.bak` (380,3 Mo : la base, WAL compris, compactée ; ses 47 versions ont la même empreinte que l'original), puis migre (`0006` : « slow statement » au journal, sans erreur).
- Après : migrations 1 à 6 ; **les 47 versions, des trois raisons, ont la même empreinte** (identifiant, page, `seq`, `covers_seq`, raison, auteurs, date et état, octet pour octet) ; `agent_authors` vaut `[]` partout ; `page_edits` porte une ligne par page, **chacune à la date et à l'auteur de la dernière entrée de son journal** (la grande page, compactée, prend la date de son instantané et un auteur vide, comme le dit la migration) ; `PRAGMA integrity_check` : `ok`.
- **Coût disque** (la mesure demandée par la relecture de S1) : pendant la migration, `mementum.db` passe de 398,4 à **743,9 Mo** (+345 Mo, 0,95 fois les versions : la table rebâtie ; l'ancienne laisse 88 691 pages libres, 363 Mo, que le fichier garde), et le WAL de 18 Mo à **365,9 Mo** (une fois les versions), jusqu'au redémarrage suivant, qui le ramène à 0 ; avec la copie, **1,49 Go sur le disque pour une base de 416 Mo**, puis 1,12 Go après un redémarrage (la base de 744 Mo et la copie). La phrase du README (« environ deux fois la place des versions, en plus de la copie d'avant migration … le fichier n'est pas réduit ensuite », dite supposée) est **confirmée** ; elle ne dit pas que le WAL garde sa taille jusqu'au redémarrage (D7).
- **La copie remise en place** (`mementum.db` remplacé par `avant-0006.bak`, WAL et `-shm` retirés), le binaire du lot 5 migre de nouveau, en 3,8 s, et reconnaît la copie à côté pour identique (`kept=true`, aucune seconde copie).

**Base du lot 3** : 4 pages, 13 versions (9 `periodic`, 3 `closing`, 1 `before_restore`), migrations 1 à 4. Le binaire du lot 5 écrit `mementum.db.avant-0005.bak` (6,6 Mo, la première migration en attente), passe **`0005` puis `0006` dans la même ouverture** (migrations 1 à 6), garde les 13 versions à l'identique, date les quatre pages d'après leur journal, `integrity_check` `ok`, et sert `/api/v1/info`.

## Le cas d'Alex avec un vrai Claude Code

Claude Code 2.1.330, en mode non interactif, **par une configuration à part** (R1258) : `claude -p … --mcp-config <fichier du dossier de la passe> --strict-mcp-config --tools "" --allowedTools mcp__mementum --no-session-persistence --setting-sources project --model sonnet --max-budget-usd …`, lancé dans un dossier hors du dépôt ; la configuration porte `Authorization: Bearer ${MEMENTUM_TOKEN}` et le jeton n'est que dans la variable (Claude Code la développe dans un `--mcp-config`). Jamais `claude mcp add`, rien dans la configuration de l'utilisateur. Instance neuve (binaire de release du clone, port 8106, `--public-url http://localhost:5193 --version-every-secs 60`), préparée par l'API comme en A de la recette (« Pro », « Projet » et sa sous-page « Détails », « Autre », « Perso » dans le premier espace), jeton « Modification » limité à « Pro », 90 jours, créé par l'API avec le mot de passe ; jeton et dossier supprimés en partant. La personne qui tape est un client Yjs nu, par la session du compte (un script jetable, pas un navigateur). **Douze lancements, 0,59 $ en tout** (Sonnet).

| Case de la recette | Ce qui s'est passé | Tenu |
|---|---|---|
| C3, « qui suis-je » | `workspace_info` : le nom, « edit », la portée `space`, l'échéance, et « Pro » seulement ; le rôle dit `member` dans un espace dont le compte est propriétaire (D5) | oui |
| D1, « verse ce compte rendu » | `workspace_info`, puis `create_pages` dans « Pro » : la page est dans « Privé », avec un titre `##` par section et des listes | oui |
| D2, l'adresse | Claude Code donne `http://localhost:5193/p/…` | oui |
| D3, taper pendant qu'il complète | `search`, `fetch` (avec ancres), un `update_page` (`str_replace` à une ancre, `insert` à la fin) pendant que la personne tape 95 caractères dans la première ligne : la phrase entière et dans l'ordre, les deux opérations présentes | oui |
| D5, `replace_block` sur un paragraphe écrit par la personne | le premier appel est refusé (`invalid_markdown`, « le texte doit finir par un saut de ligne », D2), le second passe ; une version « avant une modification par un agent » est coupée (`before_agent`, auteur Alex, `agentAuthors` Alex) | oui |
| D6, `replace_page` la page ouverte | refus `page_in_use`, dit à la personne tel quel ; la page fermée, la récriture passe ; entre-temps, Claude Code a essayé `fetch` sur `mementum://spec/mmd` (D4) | oui |
| D7, chercher | « zorquartz » : rien (le jeton ne voit que « Pro ») ; « paragraphe » modifié depuis hier (`edited_after`) : « Projet », « Détails », la réunion | oui |
| D8, une page d'équipe | `search` puis `str_replace` avec l'alias rendu par `search` comme ancre | oui |
| F1 à F4 | F1 : pas tenté, l'outil le dit (« jamais sous une autre page ») ; F2 : `invalid_markdown` d'abord (D2), puis `external_media`, avec le lien proposé à la place ; F3 : `forbidden`, le message de l'option corbeille ; F4 : aucun outil, dit | oui |
| E2, l'activité | les refus de D6, F2, F3, et les deux `invalid_markdown` (code `operation_failed`) y sont, avec le titre de la page | oui |
| G1, révoquer pendant qu'il travaille | jeton révoqué dès le premier appel de lecture qui suit : l'appel d'après rend au modèle « MCP server "mementum" rejected the Authorization header in its config (update it, then run /mcp to reconnect) » (le message de R1252, inchangé en 2.1.330), que Claude Code redit à la personne ; l'appel refusé n'est pas dans l'activité (R1348) | oui |
| R1662, une lecture de 48 Kio | `fetch` à `limit` 49 152 d'une page de texte : **49 697 caractères, arrivés tels quels** (environ 21 200 jetons) ; d'une page d'emoji : **25 394 caractères** (unités UTF-16), environ **33 000 jetons**, **arrivés tels quels** aussi, au-delà des 25 000 jetons que la documentation donne pour une sortie rangée dans un fichier | R1510 |

Pas joué (la règle de R1258 et de la recette) : la section C (`claude mcp add`, la commande prête à coller dans le shell d'Alex, ce que garde la configuration), réservée à Lenny et à Alex ; l'écran (B, E par l'interface, la barre latérale qui reçoit la page sans recharger) ; D4 à l'œil.

## Défauts

Aucun **bloquant**.

### Importants

**D1. La mémoire au repos après 500 lectures d'agent vaut 720 à 789 Mo sous Linux, pour un budget de 150 Mo (R1659).**
- Où : `crates/mementum-cli/tests/budgets.rs`, `memory_after_500_agent_reads` (la borne n'y est affirmée que sous Windows, R1659) ; le serveur, la mémoire des acteurs de pages qu'un agent ouvre (R1645, R1657, LA-r).
- Preuve : trois passages du job `budgets` de la CI sous Linux, `44903469281`, `44903727230`, `44908871771` : en propre (`RssAnon`) 719,9 / 789,7 / 730,1 Mo deux secondes après les lectures, les acteurs vivants, **les mêmes chiffres quatre secondes après, les acteurs arrêtés** (R1657), et 719,7 / 789,4 / 729,9 Mo au repos, 30 s après. Sur ce poste (Windows), au même passage de la même mesure : 403,8 Mo les acteurs vivants, **104,3 Mo les acteurs arrêtés**, 104,3 au repos.
- Cause, **non établie** (pas de Linux sur ce poste) : sous Windows, l'arrêt des acteurs rend la mémoire au système ; sous Linux, rien ne baisse à leur arrêt. Deux hypothèses, qu'un seul passage de la CI sépare : (a) l'allocateur de la glibc garde ce qu'on lui rend (arènes par fil, seuil de `mmap` qui monte), si bien que `RssAnon` ne baisse pas alors que la mémoire est libre pour le processus ; (b) les acteurs ne s'arrêtent pas sous Linux dans ce délai. Vient du lot 5 (la mesure est de X2), sur une limite déjà écrite (LA-r : 4,5 Ko par bloc dans un acteur).
- Correction proposée : d'abord séparer les deux hypothèses, dans la mesure : le nombre d'acteurs vivants (`Hub`) à chaque point, et, sous Linux, `RssAnon` avant et après un `malloc_trim(0)` (sonde jetable, lue sur la CI) ; si (a), choisir entre un `malloc_trim` après l'arrêt des acteurs (ou à intervalles), un autre allocateur pour le binaire (une décision de Lenny : une dépendance), ou une borne sous Linux qui dise la mémoire gardée par l'allocateur ; si (b), un défaut du délai d'arrêt sous Linux. Tant que ce n'est pas tranché, la mémoire d'une instance sous Linux après une rafale d'agent est de l'ordre de 5 fois le budget du §5.10, et des trois quarts de la machine minimale de la conception (§3 : un VPS d'1 Go, ou un Raspberry Pi 4 de 2 Go) : noté important plutôt que bloquant parce que la cause n'est pas établie (une mémoire gardée par l'allocateur se réutilise ; une fuite non), à revoir dès la sonde.

### Mineurs

**D2. Un MMD d'opération sans fin de ligne finale est refusé, et le vrai client s'y trompe à chaque fois.**
- Où : `update_page`, les champs `markdown` d'`insert`, `replace_block` et `replace_page` (et `create_pages`) ; le refus vient de `mementum_md::parse` (`le texte doit finir par un saut de ligne`), par `ops::read_markdown` ; la description de ces champs (`spec/mcp/tools.json`) ne le dit pas, seule la ressource `mementum://spec/mmd` le dit (« Chaque ligne finit par `\n` »).
- Preuve : deux lancements sur deux qui ont écrit des blocs par `insert` ou `replace_block` (D5, F2) : premier appel `invalid_markdown`, second avec `\n`, qui passe ; un appel perdu chaque fois. `create_pages` (D1) et `replace_page` (D6) portaient leur `\n`.
- Neuf. Un défaut du produit pour les agents (un aller-retour, et un refus dans l'activité), sans perte.
- Correction proposée : admettre un MMD d'agent sans fin de ligne finale (en ajouter une avant la lecture, dans `read_markdown` et pour `create_pages`), ou dire « finit par une fin de ligne » dans la description de chaque champ `markdown` ; un test par champ.

**D3. Graine 27811 de l'essai hostile, voie du journal : l'instantané pris après une entrée hostile relue ne relit pas la page vivante.**
- Où : la relecture d'une entrée du journal (`PageDoc::apply_update`, la garde), pas le code du lot 5 ; vu par `tail_check` (R858, R865) et par `live::reloaded_from_state_differs` (R825).
- Preuve : graine 27811 de l'essai large sur `main`, déterministe (trois passages) ; trace réduite à neuf lignes (`fixtures/hostile/min-j27811.txt`) : la création, deux entrées honnêtes, trois écritures du serveur par différence (clients `SERVER_CLIENT + 1`, `+ 3`, `+ 8`, chacune retirant la précédente), trois entrées hostiles, la dernière posant des éléments contre ce que la dernière écriture du serveur a posé. **La même trace donne le même écart avec le code de `5d10757`** (clôture du lot 4) : rejouer une trace ne mène que `apply_update`, l'écriture par différence n'y est pour rien ; elle a seulement fait paraître une forme de page (des écritures du serveur entre des entrées hostiles relues) que l'essai du lot 4, sans écriture du serveur, ne tirait pas. La voie des clients, garde comprise, ne montre rien sur la même trace.
- Atteignable : seulement par un journal où une entrée d'avant les gardes du lot 3 suit une écriture du serveur (un `PUT /mmd` d'une instance des lots 1 à 3) ; dans un journal écrit depuis les gardes, une entrée de client est celle que la garde a rendue.
- Antérieur au lot 5, neuf. Écrit en `known_limits` (`a_hostile_entry_after_server_writes_reads_otherwise_from_a_snapshot`, R1515), vert tant que l'écart est là.
- Correction proposée : le tri, par qui tient la garde : une limite de la classe de R830, écrite comme elle (README, conception), ou un défaut de la relecture, à corriger avec l'essai hostile large rejoué ensuite.

**D4. `fetch` d'un identifiant qui n'est ni une page ni un espace répond « aucun espace de ce nom ni de cet identifiant », et rien d'autre.**
- Où : `agent/fetch.rs` (un identifiant qui n'est pas un UUID est pris pour un nom d'espace).
- Preuve : le vrai client a appelé `fetch` avec `"Projet de Pro"` (un titre, G1) et avec `"mementum://spec/mmd"` (la ressource, D6) ; les deux fois, `not_found` qui ne parle que d'espaces. Il s'en est remis par `search` et `workspace_info` ; pour la ressource, il a écrit sans la lire. Le second cas tient aussi à la configuration de R1258 (`--tools ""` retire les outils de ressources de Claude Code, R1514).
- Neuf. Défaut du texte d'un refus.
- Correction proposée : un message qui dit « aucune page ni aucun espace de cet identifiant ou de ce nom » et renvoie à `search` pour un titre ; pour une adresse `mementum://…`, dire que c'est une ressource MCP, qui se lit par `resources/read`.

**D5. `workspace_info` dit `member` pour un espace dont le compte est propriétaire, sans dire que c'est le rôle restreint du jeton.**
- Où : `agent/info.rs` (le rôle restreint au jeton, au plus `member`, R1217), la ligne « espaces : - Pro (…) : member ».
- Preuve : C3 ; le modèle dit à Alex « vous avez le rôle de member » dans l'espace qu'il a créé.
- Neuf. Défaut du texte (le comportement est voulu).
- Correction proposée : « member (rôle de ce jeton) », ou les deux rôles.

**D6. « Le plan appliqué est exactement le plan vérifié » (`ops.rs`, en tête) ne vaut qu'au nommage près.**
- Où : `crates/mementum-crdt/src/ops.rs` (en-tête et `PageDoc::apply`) ; `tests/ops.rs`, `run_in`, l'affirme sur des pages dont tous les blocs ont un identifiant.
- Preuve : `reload` élargi, premier passage : un bloc que la page portait sans identifiant (la racine d'une page créée par le serveur et jamais ouverte) en reçoit un neuf à toute écriture d'agent (les blocs sont nommés avant le plan, R1337) ; la page écrite diffère du plan par cet identifiant. Les blocs que l'écriture rend, eux, sont la page relue (l'affirmation de l'acteur tient).
- Neuf. Défaut de la documentation, bénin : un bloc sans identifiant n'a pas d'alias, aucun agent ne l'a désigné.
- Correction proposée : écrire « au nommage près » dans l'en-tête de `ops.rs` (et dans `run_in`, si un cas sans identifiant y entre).

**D7. Le README ne dit pas la taille du WAL pendant et après la migration `0006`, et dit encore « supposé ».**
- Où : `README.md`, « Instance du lot 4 » (« supposé d'après SQLite, à confirmer par la mesure de l'essai de migration de fin de lot »).
- Preuve : l'essai de migration, plus haut : base de 416 Mo (dont 363 Mo de versions) → 744 Mo de base, 366 Mo de WAL jusqu'au redémarrage suivant, 380 Mo de copie : 1,49 Go le temps de passer.
- Lot 5 (Task D). Défaut de la documentation.
- Correction proposée : remplacer « supposé » par la mesure, et dire que le WAL prend autant que les versions jusqu'au redémarrage suivant.

**D8. Le journal de la migration dit « (kept : une copie identique, d'une tentative précédente, y était déjà) » même quand aucune copie n'y était (`kept=false`).**
- Où : `crates/mementum-server/src/lib.rs` (le message `base migrée ; la copie d'avant migration est à côté d'elle`) ; la parenthèse explique le champ, mais se lit comme un fait.
- Antérieur au lot 5 (la sauvegarde d'avant migration). Défaut du texte d'une ligne de journal, cosmétique.
- Correction proposée : ne mettre la parenthèse que si `kept` est vrai.

**D13. Un bloc `unknown` marqué `foreign = true` par un client, sur une `data` qui n'est pas le JSON d'un nœud étranger, est récrit par toute écriture du serveur.**
- Où : l'écriture par différence (`crates/mementum-crdt/src/diff.rs` : `prepared`, puis l'écriture d'un bloc étranger, `foreign.rs`), contre la lecture (`read_blocks`, qui rend la `data` telle quelle) ; l'acteur (`crates/mementum-sync/src/actor/agent.rs`, `write_plan`, `debug_assert_eq!(blocks, self.page.read_blocks(), "l'arbre écrit")`).
- Preuve : l'essai hostile avec écritures d'agent, graines 6260, 8404 et 15555 de la voie du journal (rouges, déterministes) ; puis `known_limits::a_foreign_mark_set_by_a_client_on_data_that_is_no_node_is_rewritten_by_any_write` (`tests/diff.rs`, vert tant que le défaut est là) : un client pose sur un bloc `unknown` les attributs `originalType`, `data = {"x":1}` (en base 64) et `foreign = true`, que la garde admet ; (1) une écriture du serveur qui ne change que le premier paragraphe récrit ce bloc, dont la `data` devient `{"attrs":{},"content":[],"children":[]}` ; (2) une écriture qui le déplace en tête rend (`DiffWritten::blocks`) un arbre où il garde `{"x":1}`, alors que la page relue porte l'autre : l'affirmation de débogage de l'acteur tomberait sur un `move_block` d'agent (non mené par l'acteur ici), et en release l'agent reçoit un arbre qui n'est pas la page. Par les routes, rien n'y mène sans ce client : elles retirent `foreign` de tout MMD (R33, `unknown_not_foreign`) ; un essai qui avait oublié cette règle donnait la graine 35892, qui n'est pas un défaut.
- Neuf (l'écriture par différence est du lot 5 ; la récriture complète d'avant récrivait tout de toute façon). Défaut du produit, mineur : il faut un client qui écrit lui-même l'attribut, et la `data` perdue n'était pas celle d'un nœud.
- Correction proposée : que la lecture et l'écriture disent la même chose d'un tel bloc : lire `foreign` faux quand la `data` n'est pas un nœud (comme R690 retire une valeur hors du type), ou écrire le bloc custom `unknown` tel quel ; puis sortir le test de `known_limits`.

### Instables, ou vus sur la CI pendant la passe

**D9. `e2e/agent.spec.ts` « an_agent_edits_while_a_human_types_and_nothing_is_lost » rouge une fois sur la CI (`44908871771`).** Déjà au registre (« À la correction groupée »). La précondition du parcours 2 (`frames.sentWhileHeld() >= 5` en 8 s) a lu 0 ; dix passages seuls ici, dix verts. Défaut **du test** probable : la barrière suppose que l'onglet envoie au moins cinq trames pendant que ses réponses sont retenues, sans rien attendre qui le garantisse. À la correction : attendre une condition (la frappe chez le serveur) plutôt que cinq trames en 8 s.

**D10. `crates/mementum-sync/tests/agent.rs` « an_actor_a_connection_opened_keeps_the_long_delay », rouge sous Linux à `900dab1` (trois passages).** Corrigé sur `main` après le clone de la passe (`1e31755`, deux secondes de marge), vert à la CI suivante (`44915044871`) ; vert ici sous Windows. Rien à faire.

**D11. Une ligne de panique d'un fil de fond dans deux mesures vertes** (`A Tokio 1.x context was found, but it is being shutdown`) : `keystroke_seen_under_150ms_while_a_large_page_is_rewritten_and_renamed_in_a_loop` (`tests/indexer.rs`) et `measure_one_account_duplicating_without_pause` (`tests/measure.rs`). Déjà au registre (le D13 de la passe de fin du lot 4), revue ici dans deux autres mesures ; à regarder avec lui.

**D12. `diff_cost` sous-estime l'écriture d'une page qui garde des blocs supprimés dans sa liste** (0,98 et 0,53 fois, `measure_diff_cost_bounds_the_write`). Connu (R1294), remesuré, inchangé.

Les trois e2e rouges de la CI hors des ajouts (registre) et celui d'`auth.spec.ts` : 20 passages ici, 20 verts ; non reproduits.

## Ce qui n'a pas été fait

- **Aucun passage sous Linux** (ni WSL ni Docker sur ce poste) : la cause de D1 n'est pas séparée ; les deux hypothèses demandent une sonde lue sur la CI.
- **La section C de la recette** (`claude mcp add` dans les shells d'Alex, la commande prête à coller telle quelle, ce que garde la configuration, la variable développée à chaque lancement en portée `local`) : réservée à Lenny et à Alex (R1258, R1504). Ce qui en a été vu : Claude Code développe `${MEMENTUM_TOKEN}` dans un `--mcp-config`.
- **L'écran** (B, E par l'interface, la page qui paraît dans la barre latérale sans recharger, D4 à l'œil) : la personne était un client Yjs nu. La recette du propriétaire le couvre.
- **Un refus de plus de 11 000 caractères** (R1662 : coupé en son milieu par Claude Code) n'a pas été provoqué.
- Le `rebase` des opérations d'agent sur les pages de l'essai hostile (R1513) : seul `reload` le tire.
