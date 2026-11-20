# Phase 1, lot 4 « Recherche » : la passe complète de fin (2026-11-20)

Méthode de R935 : tout ce que les tâches du lot n'ont pas lancé, lancé une fois, sur `main` à `6f5ae38` (atelier `lot4-fin`, poste de développement sous Windows, une commande lourde à la fois). **Rien n'est corrigé ici** : ce document est la liste des défauts, pour la correction groupée. Les sondes (mutations, un fichier de test jetable) ont été retirées, l'atelier vérifié propre après chacune.

En bref : toutes les suites sont vertes sur ce poste sauf un test de bout en bout du lot 4, instable (D3) ; seize des dix-sept mutations de sécurité sont tuées, une survit (D2) ; le rouge de la CI sous Linux dans `reindex.rs` est un défaut des tests, établi par lecture, pas reproduit (D1). Aucun défaut bloquant.

## Étapes

| Étape | Commande | Durée | Résultat |
|---|---|---|---|
| 1 | `cargo fmt --all -- --check` | 0,1 s | **rouge**, hors du code du projet : voir D7 (dans l'atelier, cargo refuse `vendor/yrs` ; sur le dépôt principal, 19 écarts, tous dans `vendor/yrs`) |
| 1 | `cargo fmt --check` (la commande de la CI) | 1 s | vert |
| 1 | `cargo clippy --workspace --all-targets -- -D warnings` (clippy 1.100) | 36 s | vert, aucun avertissement |
| 2 | `cargo test --workspace --no-fail-fast` | 13 min 18 s | vert : 126 binaires, 1 578 tests passés, 0 rouge, 40 ignorés (les mesures) |
| 3 | `git diff --exit-code packages/protocol spec/schema` (après les tests) | — | vert : fichiers générés à jour |
| 4 | `corepack pnpm install --frozen-lockfile` | 3 s | vert |
| 4 | `corepack pnpm -C apps/web typecheck` (tsc 7.0.2) | 1,4 s | vert |
| 4 | `corepack pnpm -C apps/web test` | 32 s | vert : 2 651 passés, 53 échecs attendus (`knownLimit`) |
| 4 | `node tools/check-npm-licenses.mjs` | 0,2 s | vert : 117 paquets |
| 5 | `corepack pnpm -C apps/web exec playwright test` (ports 5189 et 8099) | 11 min 6 s | **1 rouge** sur 364 : 361 passés, 2 ignorés (`fixme`), `search-rights.spec.ts` « a_guest_never_finds_a_page_he_does_not_see » (D3) |
| 5 | le fichier seul, cinq fois (`--repeat-each 5`) | 5 min 15 s | 75 passés : le rouge est instable (1 passage sur 6) |
| 6 | `playwright test -c playwright.budgets.config.ts` | 6 min 28 s | vert : 16 passés |
| 7 | `cargo test --release -p mementum-sync -p mementum-cli -- --ignored --nocapture` | 3 min 45 s | vert (une ligne de panique d'un fil de fond, sans échec : D13) |
| 7 | `cargo test --release -p mementum-server --test indexer --test search_measure -- --ignored --nocapture --test-threads=1 keystroke_seen_under_150ms measure_search_on_10000_pages` | 3 min 2 s | vert |
| 7 | `cargo deny check licenses bans` (cargo-deny 0.22.0, installé dans le dossier de la tâche) | 2 s | vert (douze doublons signalés, sans blocage) |
| 7 | `cargo deny check advisories` (hors CI) | 5 s | **rouge**, un seul avis : RUSTSEC-2026-0459 (D11) |
| 8 | dix-sept mutations de sécurité | — | 16 tuées, 1 survit (D2) |

Mesures du lot relevées à l'étape 7 (ce poste, release) : frappe d'un autre pendant la première indexation de 10 000 pages, p95 5,7 ms, la pire 95,6 ms, aucune au-dessus de 150 ms ; pendant une grande page récrite et renommée en boucle, p95 6,0 ms, la pire 117,6 ms ; un mot tapé trouvable après 2,09 à 2,10 s ; démarrage avec un index de 10 000 pages, 22,5 ms jusqu'à `/health`, 10,4 Mo en propre au repos ; index reconstruit en 2,1 s, pointe de 71,8 Mo en propre. Budgets du navigateur : de la dernière touche au résultat affiché, p95 224 ms (budget 400) ; page visée par 200 pages, p95 141 ms (budget 300). Sous Linux, le job `budgets` de la CI de `6f5ae38` est vert et porte ses lignes `MESURE`, mémoire en propre (`RssAnon`) comprise : démarrage 8,3 ms, 8,5 Mo en propre au repos, 80,0 Mo à la pointe de la reconstruction.

## Défauts

Aucun **bloquant**.

### Importants

**D1. `reindex.rs` rouge sous Linux : `SearchIndex::open` rend `Busy` (deux poussées de suite, deux tests différents).**
- Où : `crates/mementum-cli/tests/reindex.rs:260` (aide `searches`, et de même `index_token`, ligne 284) ; la fenêtre est dans `crates/mementum-search/src/index.rs:1171-1212` (`open_as`).
- Preuve : CI de `f9d74d8` (`reindex_opens_the_database_by_the_backup_rule`) et de `6f5ae38` (`known_limits::a_row_changed_by_hand_with_its_fingerprint_is_not_rewritten`), `called Result::unwrap() on an Err value: Busy`, le binaire fini en 0,9 s ; jamais sous Windows (job `rust (windows-latest)` vert aux deux poussées, vert ici).
- Cause, **établie par lecture, pas reproduite** (ni Linux, ni WSL, ni Docker sur ce poste). Le verrou de l'écrivain est un `flock` sur `.tantivy-writer.lock` (Tantivy, `mmap_directory/mod.rs:482-501`, `try_lock_exclusive`) : il appartient à la description de fichier ouverte, pas au processus. Les huit tests du binaire tournent en fils parallèles et lancent des processus (`mementum serve`, `mementum reindex`) ; entre le `fork` (ou `posix_spawn`) et l'`exec`, le fils porte une copie de **tous** les descripteurs du processus de test, dont celui du verrou qu'un autre fil tient à cet instant (`O_CLOEXEC` ne le ferme qu'à l'`exec`). Quand le fil qui tenait le verrou le ferme, le `flock` vit encore le temps que le fils arrive à `exec`. Deux endroits prennent alors `Busy` : (a) dans `open_as` lui-même, qui prend le verrou (ligne 1171), le rend (`drop(lock)`, ligne 1204), puis laisse Tantivy le reprendre par `writer_with_options` (ligne 1206, `LockFailure` devenu `Busy`, ligne 215) : c'est le cas du premier rouge, où le processus de test n'avait encore jamais ouvert cet index ; (b) entre `SearchIndex::close` et l'ouverture suivante, que les tests enchaînent (`found_pages` trois fois de suite, puis `index_token`) : c'est le cas probable du second. L'hypothèse du coordinateur est donc confirmée, avec cette précision : la fenêtre (a) est dans `open_as`, pas seulement entre deux ouvertures. Écartées : un écrivain lâché en retard (la commande `reindex` est attendue jusqu'à sa fin par `Command::output`, et sa fin ferme ses descripteurs) ; une fusion (`close` attend les fusions, et une fusion ne tient pas ce verrou).
- Vient du lot 4 (Task S7). Défaut **des tests**. Le produit n'est pas exposé par ce chemin : ni `mementum serve` ni `mementum reindex` ne lancent de processus (le seul `Command::new` de `src/` est dans un module de tests, `files.rs:1196`), et un superviseur n'hérite pas de leurs descripteurs. Les deux enchaînements demandés : **la commande lancée juste après l'arrêt d'un serveur** est refusée si le processus du serveur n'a pas fini de mourir (le noyau rend le `flock` à la fin du processus), avec le message « arrête-le d'abord », sans rien avoir modifié : bénin, on relance ; **le serveur relancé juste après la commande** attend déjà deux secondes sur `Busy` (`open_text`, `indexer.rs:160-171`).
- Correction proposée : (1) dans l'aide des tests, une ouverture qui réessaie sur `Busy` pendant deux secondes (`searches`, `index_token`), ce qui suffit à remettre la CI au vert ; (2) dans `open_as`, réessayer `writer_with_options` sur `LockFailure` pendant un temps borné (on tenait le verrou l'instant d'avant : le premier `acquire_lock` garde le refus immédiat devant un serveur qui tourne). Preuve à écrire, à lire sur la CI sous Linux : un test `#[cfg(unix)]` de `mementum-search` qui ouvre et ferme un index deux mille fois pendant qu'un autre fil lance `true` en boucle, rouge avant (2), vert après.

**D2. Mutation qui survit : l'extrait d'une source non vue lu avant le jugement des droits (rétroliens).**
- Où : `crates/mementum-server/src/routes/backlinks.rs:158-190` (`seen_sources`), puis `backlink_excerpts` ligne 138.
- Preuve : sonde `b2` (un `backlink_excerpts` de tout le lot lu, posé avant le filtre) : `cargo test -p mementum-server --lib -- backlinks` (9 passés) et `--test backlinks` (10 passés), tout vert.
- Lot 4 (Task S6). Défaut **des tests** : le code est juste, rien ne le garde. Une régression de cette forme ferait lire jusqu'à dix mille textes de 64 Kio de pages non vues, bien au-delà du plancher de 150 ms, et la durée redirait leur nombre.
- Test qui manque : « les extraits ne sont demandés à la base que pour les sources gardées par `access` » : compter les pas de SQLite de la lecture des extraits (`backlink_read_steps`, ou un compteur `test-hooks` des identifiants donnés à `backlink_excerpts`) avec et sans mille sources non vues, et les affirmer égaux.

**D3. Test de bout en bout instable : `a_guest_never_finds_a_page_he_does_not_see`.**
- Où : `apps/web/e2e/search-rights.spec.ts:151-154` ; la même comparaison aux lignes 175, 208, 250, 346, 400 et 436.
- Preuve : `Expected: {"results":[],"more":false,"indexing":true}`, `Received: {"results":[],"more":false,"indexing":false}` ; 1 rouge sur 6 passages (suite entière : rouge ; seul, cinq fois : vert).
- Cause : la réponse de référence (« un mot que personne n'a écrit ») est prise moins d'une seconde après l'indexation des pages que l'invité voit ; `indexing` y vaut encore vrai (une page dont l'indexation vient de finir reste dite en retard une seconde, `LATE_KEPT` et `Queues::done`, `indexer.rs:100` et `346-359`, R1055), puis passe à faux pendant les quatre recherches comparées octet pour octet (62 ms chacune sous Windows). Ce n'est pas une fuite : `indexing` ne juge que les pages de l'ensemble permis, et ne suit que le temps.
- Lot 4 (Task X1). Défaut **du test**. La CI n'en a pas encore rougi, mais il y rougira.
- Correction proposée : une barrière avant toute réponse de référence, après la dernière écriture du test (ici la mise à la corbeille) : chercher un mot neuf au nom du compte jusqu'à lire `indexing: false`, dans `helpers.ts`, et s'en servir aux sept endroits. Ne pas retirer `indexing` de la comparaison : c'est elle qui prouve qu'il ne dit rien des pages non vues.

**D10. Échap dans le texte de l'éditeur, sans menu ouvert, laisse le focus sur `body` (R1143).**
- Où : l'éditeur de la page (`apps/web/src/editor/`) ; test en attente `apps/web/e2e/search-rights.spec.ts:694` (`test.fixme`).
- Preuve, rejouée par une sonde dans Chromium : après un clic dans un paragraphe, Fin puis Échap, `document.activeElement === document.body` vaut vrai, et le « ! » tapé ensuite n'arrive nulle part (le bloc porte toujours « Début »).
- Antérieur au lot 4. Défaut **du produit** : toujours vrai.
- Correction proposée : prendre Échap dans l'éditeur quand aucun menu de BlockNote n'est ouvert (ne rien faire, ou rendre le focus au texte), puis retirer le `fixme` du test, qui dit déjà ce qui est voulu.

### Mineurs

**D4. `equations.spec.ts`, « …mathllap_deep_in_a_list » : la formule est à 54 px de ne jamais être mise en forme.**
- Où : `apps/web/e2e/equations.spec.ts:207-215` ; `apps/web/src/editor/blocks/Formula.tsx:164` (`rootMargin: "400px 0px"`).
- Preuve : dix passages seuls, dix verts ; sonde de géométrie : fenêtre de 720 px, la formule du fond de la liste à 1 066 px du haut, l'observateur portant jusqu'à 1 120 px. Le test attend `.mm-tex-why`, qui n'existe qu'une fois la formule mise en forme, donc venue à portée de l'écran : si la mise en page la pose 54 px plus bas (deux pixels de plus par ligne, une police chargée plus tard sous Linux), rien ne la met jamais en forme, et `scrollIntoViewIfNeeded` attend jusqu'au délai d'une minute, la signature vue sur la CI.
- Antérieur au lot 4, pas touché par lui. Défaut **du test** (cause probable, pas reproduite).
- Correction proposée : amener à l'écran la dernière ligne de la liste (ou `.mm-math`), qui existe toujours, avant d'attendre `.mm-tex-why`.

**D5. Budget « open_1000_block_page_with_50_images_under_300ms » : 307 ms sur la CI, deux fois.**
- Où : `apps/web/e2e/budgets.spec.ts:361`.
- Preuve : ici, dix passages seuls, dix verts, p95 de 153 à 157 ms ; sous `MEMENTUM_BUDGETS_CPU_SLOWDOWN=1.5`, dix verts, p95 de 232 à 242 ms. Sur la CI, d'un passage à l'autre, **toutes** les ouvertures changent ensemble : 143 à 181 ms un jour (`44700949460`), 196 à 267 ms un autre (`44703104604`, les 50 images à 267).
- Cause probable : la vitesse du coureur, qui varie de 1,4 fois d'un passage à l'autre et atteint deux fois ce poste (307 pour 155), au-delà du facteur 1,5 à 1,7 de R580 ; cette page est la plus lourde des ouvertures à 300 ms (152 ms ici contre 136 pour mille paragraphes). Antérieur au lot 4. Ni instable ni cassé ici.
- Correction proposée : ne pas relâcher le budget. Soit gagner la quinzaine de millisecondes que coûtent les 50 blocs image à l'ouverture, soit accepter ce rouge rare et le dire au registre ; la calibration de R580 est à porter à 2 pour dire le pire coureur.

**D6. Mesure « keystroke_that_cuts_a_version_of_10000_block_page_under_150ms » : rouge une fois sur la CI.**
- Où : `crates/mementum-sync/tests/budgets.rs:314`.
- Preuve : dix passages seuls en release, dix verts, p95 de 36,7 à 45,7 ms ; un passage porte une frappe à 212,4 ms (une sur deux cents). La CI rouge (`44704553336`) : p50 50,9 ms, p95 211,7 ms, max 212,5 ms, version écrite en 60,7 ms (28 ms ici).
- Cause probable : la même frappe lente d'environ 212 ms existe sur ce poste, rare ; sur un coureur au disque lent, elle touche deux frappes sur vingt, et le p95 passe. C'est l'hypothèse de R487 (le point de contrôle du WAL sur le chemin de la frappe qui suit une version de 3,2 Mo). Antérieur au lot 4. Défaut **du produit** déjà connu (R447), que la mesure montre.
- Correction proposée : celle de R447 (sortir le point de contrôle du WAL du chemin de la frappe), au lot 7 ; d'ici là, noter le rouge sans toucher à la mesure.

**D7. `cargo fmt --all -- --check`, la commande de `docs/REPRISE.md`, est rouge.**
- Où : `docs/REPRISE.md:142` ; la CI lance `cargo fmt --check`, verte.
- Preuve : sur le dépôt principal, `--all` formate aussi les dépendances locales : 19 écarts, tous dans `vendor/yrs` (qu'on ne formate pas) ; dans un atelier sous `.claude/worktrees/`, cargo refuse (« current package believes it's in a workspace when it's not », `vendor/yrs/Cargo.toml`).
- Du lot 3 (yrs vendu). Défaut **de la documentation**.
- Correction proposée : écrire `cargo fmt --check` dans REPRISE.md, comme la CI.

**D8. Trois textes disent encore 75 ms et vingt mille sources pour les rétroliens.**
- Où : `crates/mementum-server/src/routes/backlinks.rs:7` (« jamais avant 75 ms », la constante vaut 150 et le reste de l'en-tête le dit) ; `CLAUDE.md`, sections « Task S6 » et « Task C3 » (75 ms, `WALKED` à 20 000), que seule une parenthèse de la section X2 corrige.
- Lot 4 (X2 a changé les valeurs). Défaut **de la documentation**.
- Correction proposée : mettre 150 ms et 10 000 aux trois endroits, et retirer la parenthèse de X2.

**D9. `POST /api/v1/invitations/lookup` n'est pas dans `READING_POSTS` (R1092).**
- Où : `apps/web/src/api.ts:502`. Toujours vrai : la liste ne nomme que la recherche ; la route ne fait que lire (`lib.rs:22`).
- Antérieur au lot 4. Défaut **du produit**, mineur : sa réponse perdue dit « ce que tu as demandé a peut-être été fait ».
- Correction proposée : ajouter son chemin à `READING_POSTS`, avec un cas dans `api.test.ts`.

**D11. `cargo deny check advisories` : RUSTSEC-2026-0459 (`libsqlite3-sys` 0.37.0), et il peut maintenant être levé.**
- Preuve : un seul avis en erreur (REPRISE.md en annonce deux : à mettre à jour). Depuis le 2026-11-18, `sqlx` 0.9.1 est publié, et `sqlx-sqlite` 0.9.1 admet `libsqlite3-sys` `<0.41` ; l'avis dit corrigées les versions 0.39.0 et plus (SQLite 3.54).
- Antérieur au lot 4. Vecteur local (une base et son `-wal` fabriqués, que le serveur ouvrirait).
- Correction proposée : monter `sqlx` à 0.9.1 et `libsqlite3-sys` à 0.39 ou plus dans un commit à part, puis toute la suite Rust et les mesures (le SQLite embarqué change). Pas essayé ici.

**D12. Le `404` d'une page non vue lit plus que celui d'une page inconnue.**
- Où : `crates/mementum-server/src/access.rs:245-260` (`seen_node`). Toujours vrai, par lecture : une page inconnue sort après une lecture (`get_node`) ; une page qui existe et qu'on ne voit pas lit en plus le rôle, puis, pour qui a un rôle dans l'espace, le calcul des accès (111 ms pour 20 403 pages, cache froid, d'après le registre). Pas mesuré ici.
- Antérieur au lot 4, sur toute route de page. Défaut **du produit**, de durcissement : le temps de réponse distingue « existe » de « n'existe pas » pour un identifiant deviné (un UUID v7, 74 bits au hasard).
- Correction proposée : au lot 7, avec les débits : faire les mêmes lectures dans les deux cas, ou un plancher commun aux `404` de page.

**D13. Une ligne de panique d'un fil de fond dans une mesure verte.**
- Où : `crates/mementum-sync/tests/budgets.rs`, `keystroke_that_cuts_a_version_of_10000_block_page_under_150ms`.
- Preuve : une fois (passage de l'étape 7), `thread 'tokio-rt-worker' panicked at tokio-1.54.1/src/runtime/time/entry.rs:534: A Tokio 1.x context was found, but it is being shutdown`, le test vert ; pas revue en dix passages seuls, absente du journal de la CI.
- Cause non établie : une tâche encore en vol pose un minuteur pendant que le moteur du test s'arrête. Défaut **d'un test** (bruit), ou d'un arrêt d'acteur sans attente : à regarder si elle revient.

### Vus sur la CI pendant le lot, hors de la liste demandée

- `e2e/undo-redo.spec.ts`, « with_two_tabs_redo_with_nothing_to_redo_takes_nothing_away » : rouge une fois (`44697914633`) ; dix passages seuls ici, dix verts. Instable, cause non cherchée.
- `e2e/rich-blocks.spec.ts`, « page_link_opens_the_page_and_reads_back » (registre) : dix passages seuls, dix verts depuis le commit qui lui fait attendre le serveur.
- `e2e/budgets.spec.ts`, « measure_open_10000_block_page_with_the_same_headings_without_a_toc », « page … absente de l'arbre » (deux passages) : vert ici et sur la CI depuis `ca3d782`.
- La CI de `a8d67f7` (Task D) est verte, les quatre jobs.

## Mutations de sécurité (jouées une fois chacune, atelier propre après chacune)

| Sonde | Tuée par |
|---|---|
| `search.rs` : ensemble permis de tout l'espace (sans les droits) | `tests/search.rs` : `withdrawn_access_hides_the_page_at_the_next_query`, `a_page_closed_to_members_is_hidden_at_the_next_query`, `a_trashed_page_is_not_found_and_comes_back_restored`, `a_right_withdrawn_while_a_search_waits_is_not_served`, `indexing_says_nothing_of_unseen_pages` |
| `search.rs` : sans le filtre de la corbeille | `routes::search::tests::the_allowed_set_leaves_out_the_trash_and_what_is_under_it` seul (les tests de la route passent : le filet d'après l'index jette la page) |
| `search.rs` : droits d'un autre espace que celui demandé | vingt-deux tests de `tests/search.rs`, dont `a_word_only_an_unseen_page_holds_answers_like_a_word_nobody_wrote` et `a_guest_finds_only_the_pages_he_was_invited_on` |
| `search.rs` : droit non relu une fois la place prise | `a_right_withdrawn_while_a_search_waits_is_not_served` |
| `search.rs` : réponse avant le plancher | `an_answer_never_leaves_before_the_floor`, `the_account_keeps_its_place_until_the_answer_leaves`, `the_instance_keeps_its_place_until_the_answer_leaves`, `an_abandoned_search_keeps_the_place_of_the_instance_until_the_floor` |
| `search.rs` : place de l'instance rendue à la fin du calcul | `the_instance_keeps_its_place_until_the_answer_leaves`, `an_abandoned_search_keeps_the_place_of_the_instance_until_the_floor`, `the_account_keeps_its_place_until_the_answer_leaves` |
| `backlinks.rs` : sources non jugées par `access` | `unseen_sources_never_hide_seen_ones`, `the_walk_stops_at_its_bound`, `the_answer_is_the_same_whatever_unseen_sources_exist`, `batches_neither_repeat_nor_skip_a_source` ; `tests/backlinks.rs` : `a_backlink_from_an_unseen_page_is_not_listed`, `a_backlink_from_a_trashed_page_is_not_listed`, `a_reader_gets_the_backlinks` |
| `backlinks.rs` : extrait lu avant le jugement | **survit** (D2) |
| `backlinks.rs` : plancher retiré | `routes::backlinks::tests::an_answer_never_leaves_before_the_floor`, `a_reader_gets_the_backlinks` |
| `index.rs` : filtre après le groupement, `skip` et `limit` | `the_filter_is_applied_in_the_collector_before_grouping`, `what_unseen_pages_hold_changes_neither_the_hits_nor_more`, `more_is_true_only_when_another_page_follows`, `the_check_of_a_long_prefix_is_bounded_and_counts_allowed_entries_only`, `the_check_of_a_long_prefix_is_bounded_in_bytes` |
| `index.rs` : statistiques de l'index (`Searcher::search` nu) | `no_search_path_reads_the_statistics_of_the_index`, `what_unseen_pages_hold_changes_no_order` |
| `index.rs` : grille retirée | `every_note_is_a_sum_on_the_grid` seul |
| `index.rs` : départage par `DocAddress` | `equal_notes_keep_their_order_whatever_the_index_holds`, `a_block_inserted_before_another_changes_no_tie_break`, `the_title_entry_wins_a_tie_in_its_page`, `the_checked_entries_do_not_depend_on_the_order_of_the_index`, `the_candidates_kept_for_a_long_prefix_are_bounded` |
| store : marqueur oublié à la suppression d'une page | `the_index_leaves_with_its_page`, `a_deletion_during_an_intent_keeps_its_line_past_the_closing`, `only_listed_places_write_a_title_or_a_node` |
| store : clôture sans comparer le jeton | `finish_feed_keeps_a_line_changed_meanwhile`, `a_deletion_during_an_intent_keeps_its_line_past_the_closing`, `a_rebuild_during_an_intent_keeps_its_lines_past_the_closing` |
| client : réponse en retard non jetée | `useSearch.test.ts` et `SearchDialog.test.tsx` : `a_late_answer_of_an_older_query_is_dropped` ; `a_failed_request_for_more_keeps_what_is_shown_and_can_be_asked_again` |
| client : extrait rendu en HTML | `SearchDialog.test.tsx` : `marks_wrap_text_nodes_only` |

Deux sondes ne sont tuées que par un test : la corbeille par un test unitaire de `allowed_pages`, la grille par un test de la note. Ce n'est pas un défaut, mais ces deux tests sont les seuls gardiens de ces règles.

## « À reprendre à la fin du lot »

| Point | État | Preuve |
|---|---|---|
| Avis `libsqlite3-sys` (RUSTSEC-2026-0459) | toujours vrai, levable | D11 |
| « open_1000_block_page_with_50_images_under_300ms » | vert ici 20 fois sur 20 ; vitesse du coureur | D5 |
| « …mathllap_deep_in_a_list » | vert ici 10 fois sur 10 ; géométrie à 54 px de la borne | D4 |
| « keystroke_that_cuts_a_version… » | vert ici 10 fois sur 10 ; une frappe à 212 ms sur 200 | D6 |
| « page_link_opens_the_page_and_reads_back » | levé | 10 verts sur 10 |
| `invitations/lookup` hors de `READING_POSTS` | toujours vrai | D9 |
| Le `404` d'une page non vue lit plus | toujours vrai (lecture) | D12 |
| Échap dans le texte de l'éditeur (R1143) | toujours vrai | D10 |
| La suite de bout en bout sous `E2E_INDEX_AFTER_MS = 200`, l'indexeur, la fenêtre et les rétroliens | fait : un seul rouge, du lot 4 | D3 ; aucun e2e de session perdue ni de rafale d'écritures n'a rougi |
| Vitest après le changement de commentaire de X2 | fait | vert |
| Les lignes `MESURE` de X2 sous Linux, `RssAnon` compris | fait | job `budgets` de `6f5ae38`, vert |
| Rétroliens : la mesure à rejouer, le temps sous le plancher | faits par X2 (R1167), pas rejoués ici | registre |
| `behind` à 100 ms pour 100 000 pages à l'outbox ; une recherche lancée ne s'interrompt pas ; l'indexeur unique et sériel ; RO-1 | toujours vrais, reportés au lot 7 par X2 (R1163, R1169, R1172) | registre, pas rejoués ici |
| Taille de `blocks_index` pour des blocs courts | toujours vrai | mesure de l'étape 7 : la base passe de 0,2 à 19,7 Mio pour 7,7 Mio de texte, dont 15,1 Mio de `blocks_index` (2,0 fois le texte) |
| Le rouge de `reindex.rs` sous Linux | cause établie par lecture | D1 |

## Ce qui n'a pas été lancé, et pourquoi

- **Rien sous Linux** : le poste n'a ni WSL ni Docker. D1 et D4 sont établis par lecture et par la géométrie mesurée sous Windows, pas reproduits.
- **Les autres mutations des listes « Preuves (mutations) » du plan** (S1, S2, S4a, S4b, S4c, S7, C2, C3, et celles de S3, S5, S6 et C1 hors des dix-sept demandées) : hors de la demande ; elles restent à jouer.
- **L'essai de migration** du point 3 de « Fin du lot » (base du lot 3 sous `0005`, copie remise en place, `search/` supprimé) et **la relecture finale en trois passes** du point 1 : hors de la demande.
- **Les mesures de `mementum-server` hors des deux filtres de la CI** (au million de blocs, formes hostiles) : X2 les a lancées ; pas rejouées.
- **`cargo fmt --all -- --check` dans l'atelier** : cargo le refuse dans un atelier imbriqué (D7) ; lancé sur le dépôt principal, en lecture seule.
- La mémoire n'a tué aucune commande ; aucune n'a été réduite.
