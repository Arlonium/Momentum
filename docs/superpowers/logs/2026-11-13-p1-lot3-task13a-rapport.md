> **Rapport de l'implémenteur de la Task 13a, recopié tel quel le 2026-11-13** (les chemins absolus du poste sont rendus relatifs à la racine du dépôt). Le code qu'il décrit n'est **pas sur `main`** : il est sur la branche locale `worktree-agent-acd5330e6b63d3097` (onze commits, base `ca29845`), jamais poussée, en attente de la fin de sa relecture. Voir l'[avancement](2026-11-12-p1-lot3-avancement.md).

# Task 13a — client, la forme à clés (R220) : rapport

**Status : `DONE_WITH_CONCERNS`** (voir « Doutes » : trois écarts à valider, aucun critère d'arrêt tombé avec ce qui est livré).

- Atelier : `.claude\worktrees\agent-acd5330e6b63d3097`
- Branche : `worktree-agent-acd5330e6b63d3097`, partie de `ca29845` (`main` au lancement). Rien n'est poussé.
- Poste : Windows 11, Node 24.16, pnpm 12.18.0, Chromium de Playwright. Ports des e2e : 5176 et 8083, serveur compilé dans `target/e2e` de l'atelier.
- Ce rapport est dans l'atelier (`task-13a-report.md`, non commité) : l'écriture hors de l'atelier m'a été refusée par l'outil.

## Commits (dans l'ordre)

| SHA | Sujet |
|---|---|
| `4ff23e5` | web : prosemirror-state en dépendance directe, à la version du verrou |
| `e18ea35` | tests : la cible des blocs déplacés, durcie avant toute correction |
| `996bb6c` | web : un bloc déplacé est recréé, les autres ne sont plus récrits (R220) |
| `7bf17ea` | web : le curseur suit son bloc quand un autre le déplace |
| `f76a18e` | tests : les octets de chaque scénario de bloc déplacé, dans les deux ordres d'identifiants |
| `94e25e6` | tests : le glisser des tests laisse le curseur que laisse un vrai dépôt |
| `cb35b06` | web : un bloc qu'on tient au bout de la souris reste entier quand un autre écrit |
| `b63932f` | web : le curseur ne suit que son bloc recréé, pas un texte fusionné dans son voisin |
| `7711001` | web : le correctif de y-prosemirror dit ce qu'un dépôt laisse vraiment (commentaire seul) |
| `1dc8af8` | e2e : ce qui est tapé à côté d'un bloc qu'un autre glisse reste à sa place |
| `65089b2` | tests : le curseur garde sa place quand un autre change le type de son bloc |

Aucune ligne d'attribution d'outil ; identité du dépôt (`LnYkLl`). **À corriger à la reprise** : le message de `996bb6c` dit de la garde de la sélection de nœud « le bloc sélectionné (celui qu'on vient de déposer) … y compris à l'annulation du déplacement ». C'est faux dans un navigateur : un dépôt laisse un curseur de texte, pas une sélection de nœud (constaté ensuite par un e2e, corrigé dans les tests par `94e25e6`). La garde vaut pour une vraie sélection de nœud (un clic sur une image). Je n'ai pas récrit l'historique (ce commit n'est plus en tête) : à reformuler au `cherry-pick` si souhaité.

## Ce qui est livré, en bref

1. **Le correctif de y-prosemirror 1.3.7** (`patches/y-prosemirror@1.3.7.patch`, 504 lignes, déclaré dans `pnpm-workspace.yaml`), dans les deux fichiers du paquet (`src/plugins/sync-plugin.js`, que chargent Vite et Vitest, et `dist/y-prosemirror.cjs`), **par ajouts seulement**, trois régions par fichier, chacune entre deux lignes `// mementum:moved-blocks:begin` / `:end` :
   - `updateMovedChildren` et ses aides, avant `updateYFragment` ;
   - son appel, en tête de la transaction de `updateYFragment` ;
   - une garde dans `restoreRelativeSelection` (sélection de nœud dont le nœud n'est plus là).
2. **`followBlock`** (`apps/web/src/editor/followBlock.ts`, branché dans `EDITOR_OPTIONS` d'`Editor.tsx`, deux lignes) : la sélection locale suit son bloc par son `id` (curseur, sélection de texte, sélection de nœud, blocs tenus pendant un glisser).
3. **Tests** : `keyedBlocks.test.ts` (102 tests), `followBlock.test.ts` (20), `blockMoves.test.ts` durci et mis à jour (137, dont 32 en échec attendu), aides partagées `src/test/editors.ts` et `src/test/node.ts`, 6 e2e neufs verts dans `e2e/block-move.spec.ts`.
4. **Fixtures pour la Task 13b** : 136 fichiers neufs sous `crates/mementum-sync/tests/fixtures/moves/{mona-first,tom-first}/<scène>/` (S1 à S11 et six scènes de plus), les quatre fichiers historiques réécrits.

### Comment le correctif décide

Dans `updateYFragment`, une fois sautés le préfixe et le suffixe identiques, **si les deux milieux sont non vides** :

1. chaque enfant du milieu, côté ProseMirror et côté Yjs, doit être un **bloc** (`isBlock`) avec un `id` chaîne non vide, distinct des autres de son côté ; sinon `false` → code d'origine, tel quel ;
2. les blocs présents des deux côtés (même `id`, même nom de nœud) sont comparés dans l'ordre : plus longue sous-suite croissante, en O(n log n) ;
3. **si aucun bloc commun n'a changé d'ordre, `false` → code d'origine, tel quel** (c'est l'écart R223, voir le critère d'arrêt plus bas) ;
4. sinon : les blocs de la sous-suite restent les mêmes éléments (mis à jour sur place par `updateYFragment` s'ils ont changé) ; les autres éléments du milieu sont supprimés, et les nœuds qui manquent sont créés à leur place (un bloc déplacé : avec le même `id`) ;
5. à longueur égale (deux voisins échangés), on garde la sous-suite qui ne contient pas les blocs que touche la sélection de l'éditeur : c'est le bloc qu'on vient de déplacer qui est recréé (un dépôt et le déplacement au clavier laissent le curseur dedans).

## Critère d'arrêt : ce qui a été mesuré, et l'écart qui le lève

**La variante « à la lettre » de la décision (« la branche à clés joue dès que tous les enfants du milieu ont un `id` distinct et non vide ») fait tomber le critère « comptes de structures ».** Mesuré le 2026-11-13, correctif installé sans la règle 3 ci-dessus, test `ordinary_gestures_write_what_they_wrote_before` (relevés faits **avant** sur le code d'origine) : 4 gestes sur 21 diffèrent.

| Geste | Code d'origine | Clés sans la règle « un bloc a changé d'ordre » |
|---|---|---|
| Entrée au milieu d'un bloc qui a des enfants | 190 octets, 4 suppressions, 1 conteneur créé, 1 élément renommé | 345 octets, 20 suppressions, 2 conteneurs + 1 `blockGroup` créés : **tous les enfants recréés** |
| Entrée à la fin d'un bloc qui a des enfants | 193 octets, 7 suppressions, 1 renommage | 330 octets, 17 suppressions, enfants recréés |
| Deux blocs remplacés par deux neufs | 51 octets, 2 renommages | 293 octets, 2 conteneurs créés |
| Un bloc supprimé et son voisin modifié | 31 octets, 1 renommage | 21 octets, 0 renommage |

(Le compte rendu de l'essai disait « mêmes comptes » : il n'avait mesuré Entrée que sur des blocs sans enfants. Avec des enfants, le code d'origine garde l'élément pour le bloc neuf, qui prend les enfants, et le renomme ; une forme à clés sans restriction viderait l'élément et recréerait tout le sous-arbre — et ferait perdre ce que d'autres tapent dans les enfants à chaque Entrée.)

**Levé sans toucher à R220**, comme le brief le permet : la branche à clés ne joue que si **au moins un bloc présent des deux côtés a changé d'ordre**. R220 parle de réordonnancement ; tout le reste est le code d'origine par construction. Preuve : les 21 gestes relevés avant le correctif repassent à l'identique (octets, structures créées par sorte, suppressions, renommages, page), y compris les 4 ci-dessus ; la mutation « sans la règle » rend ces 4 tests rouges.

Les deux autres critères : aucune divergence au hasard (`keyed_and_unkeyed_tabs_converge`, 20 graines mixtes + 20 graines à deux onglets corrigés, 40 pas chacune) ; budgets : voir « Mesures ».

## Preuves TDD, step par step

### Step 1 — durcir la cible (`e18ea35`), sur le code d'origine

- Une précondition manquée **lève** (`Missed`) ; un scénario en échec attendu ne tient pour sa perte que `Unmet` (l'exigence de la page voulue) : toute autre exception est un « accident », écrit au journal, et `test.fails` le signale rouge. Quatre tests de la cible elle-même (`la cible : ce qu'un scénario peut donner`).
- Chaque scénario joué dans les deux ordres d'identifiants (Mona 2 / Tom 3, puis Mona 3 / Tom 2). Le glisser de Tom de S6 a son témoin. La comparaison lit types et enfants (test dédié). Tab, Maj+Tab et le clavier passent par la **touche** (`KeyboardEvent` reçu par l'éditeur), plus par l'API.
- `corepack pnpm -C apps/web exec vitest run src/editor/blockMoves.test.ts` → `36 passed | 24 expected fail (60)` ; avec `MEMENTUM_SHOW_R39=1` → `24 failed | 36 passed`, chacun sur « la page voulue » (ordre inverse : « Bloc  de TomY » au lieu de « Bloc Y de Tom », S6 sans doublon mais « Bloc YY | Bloc XZ »).
- **Avant toute ligne du correctif** : relevé des comptes de structures de 21 gestes ordinaires (sonde jetable, retirée), figés ensuite dans `keyedBlocks.test.ts`.

### Step 2 — les tests de la forme, vus rouges

`corepack pnpm -C apps/web exec vitest run src/editor/keyedBlocks.test.ts`, dépendance d'origine : `56 failed | 45 passed (101)`. Rouges pour la bonne raison :
- `a_move_rewrites_only_the_moved_block`, `swapping_two_neighbours…`, `a_big_move_writes_a_small_update`, `des ids distincts` : `expected { deleted: [], created: [], … }` (récriture sur place) ;
- `typing_in_another_block_of_the_range_stays_in_its_block`, `two_moves_of_different_blocks_keep_every_block_once`, `undo_of_a_move_leaves_other_blocks_alone` (deux ordres) : textes chez le voisin, page abîmée ;
- `both_builds_of_the_dependency_carry_the_fix` : `src (import): expected [ false, false ] to deeply equal [ true, true ]` ;
- `keyed_and_unkeyed_tabs_converge` (21 tests) : `Error: le correctif de y-prosemirror n'est pas installé` ;
- `tokens_typed_while_someone_else_moves_blocks_never_change_block` : 19 graines sur 20, jetons dans un autre bloc ;
- les 21 `ordinary_gestures…` verts (relevés sur ce code), les 4 replis par position verts.
- **Constat au passage** : 2 graines du test au hasard levaient `TypeError: Cannot read properties of null (reading 'nodeSize')` dans `restoreRelativeSelection` de y-prosemirror **d'origine** (sélection de nœud dont le nœud est retiré par un autre) : d'où la garde, R224.

### Step 3 — le correctif (`996bb6c`)

Vert : `corepack pnpm -C apps/web test` → `53 passed`, `593 passed | 32 expected fail`. Mutations du correctif (chacune installée par `pnpm patch-commit`, puis le correctif juste remis, empreinte identique) :

| Mutation | Tests rouges |
|---|---|
| sans la règle « un bloc a changé d'ordre » | 4 gestes ordinaires (tableau plus haut) |
| sans la garde de `restoreRelativeSelection` | `a_selected_block_taken_away_by_someone_else…` (2) |
| sans le départage par la sélection | `swapping_two_neighbours_recreates_the_block_that_was_moved` |
| sans refuser un `id` en double, côté page / côté document | `un id en double dans la page voulue` / `… dans le document` |
| sans refuser un `id` absent ou vide | `un id absent`, `un id vide` |
| sans exiger un bloc (`isBlock`) | `des mentions échangées dans un paragraphe` |

Une septième (un raccourci « nœud égal : ne rien écrire ») ne rendait aucun test rouge : je l'ai **retirée du correctif** plutôt que de livrer une ligne non tenue.

### Step 4 — `followBlock` (`7bf17ea`, puis `cb35b06`)

Rouge (extension vide) : `7 failed | 3 passed`, le curseur en `{ id: 'w', offset: 6 }` (fin de la page) au lieu de `{ id: 'x', offset: 3 }`, la sélection de nœud sur `z` au lieu de `x` ; puis, pour les blocs tenus, `4 failed` (`expected { type: 'text', ids: [] } to deeply equal { type: 'multiple-node', ids: [ 'x' ] }`). Vert : `20 passed` à la fin. Mutations de `followBlock.ts` : les 7 règles (position relative morte, bloc quitté, retour dans le contenu, sélection de nœud, annulation, blocs tenus, mise à jour de ce qu'on tient) ont chacune au moins un test rouge.

« Rien n'est écrit dans le document » : `nothing_is_written_to_the_document_by_following` (aucune mise à jour émise, état identique à celui du serveur).

**Une régression de mon fait, prise par la suite e2e entière et corrigée (`b63932f`).** Premier passage de la suite : `153 passed, 1 failed` — `collab.spec.ts › offline_edits_merge_on_reconnect` : « A en ligneB ⁠Compte des tests⁠hors ligne », le curseur de B revenu au milieu du mot qu'il venait de taper. Cause : quand deux personnes écrivent en même temps dans un paragraphe vide, y-prosemirror verse l'un des deux textes dans l'autre et supprime le second ; ma règle « la position relative ne mène plus nulle part » prenait ce texte supprimé pour un bloc recréé. Reproduit en Vitest d'abord (`deux personnes tapent dans le même bloc vide…`, rouge : `offset 12` au lieu de `22`), puis corrigé : seul compte l'élément qui porte le texte ; s'il est toujours là, la sélection est laissée à y-prosemirror.

### Step 5 — les scénarios changent d'état (`996bb6c`, `f76a18e`)

`blockMoves.test.ts` : `105 passed | 32 expected fail (137)`. Avec `MEMENTUM_SHOW_R39=1`, les 32 échouent sur l'exigence attendue (texte absent du bloc déplacé, page sinon intacte ; S6 : `x` deux fois, textes intacts). Tests de forme récrits. Fixtures : rouge `ENOENT` avant l'export, puis `EXPORT_FIXTURES=1 … vitest run src/editor/blockMoves.test.ts -t "les octets du test"`, puis deux comparaisons de suite identiques.

### Step 6 — dans un vrai navigateur

Voir « Mesures ». Chaque e2e neuf a été vu rouge pour la bonne raison :
- sur la **dépendance d'origine** (verrou et `pnpm-workspace.yaml` de `e18ea35` réinstallés un instant) : `typing_in_a_neighbour… (hors ligne)` → « Bloc Z de Tom », « Bloc X aussi » ; `(glisser hors ligne)` → « Bloc  de TomZ », « Bloc  aussiX » ; `two_people_moving_two_blocks_keep_the_page` → « Bloc ZZ », « Témoin AX », `y` en double sous des ids neufs ;
- **sans `followBlock`** (correctif installé) : `typing_in_a_neighbour… (en ligne)` → le bloc glissé reste en tête, vidé de son texte ; `typing_goes_on_in_the_block_someone_else_just_moved` → « Témoin C! » ; `the_cursor_left_in_a_dropped_block…` → « Témoin C de Mona ».

## Comptes de structures, avant et après

Identiques, geste par geste (test `ordinary_gestures_write_what_they_wrote_before`, valeurs figées relevées sur le code d'origine) :

| Geste | Créé | Supprimé | Octets | Éléments renommés |
|---|---|---|---|---|
| frappe à la fin / au milieu d'un bloc | 1 caractère | 0 | 10 / 12 | 0 |
| Entrée au début d'un bloc | 1 conteneur, 1 paragraphe, 4 attributs | 0 | 166 | 0 |
| Entrée au milieu | idem + 1 texte, 3 caractères | 3 | 183 | 0 |
| Entrée à la fin | 1 conteneur, 1 paragraphe, 4 attributs | 0 | 166 | 0 |
| Entrée au début d'un bloc qui a des enfants | idem | 0 | 164 | 0 |
| Entrée au milieu d'un bloc qui a des enfants | idem + 1 texte, 3 caractères, 1 attribut récrit | 4 | 190 | **1** |
| Entrée à la fin d'un bloc qui a des enfants | idem + 1 texte, 6 caractères, 1 attribut récrit | 7 | 193 | **1** |
| retour arrière dans le texte | rien | 1 | 6 | 0 |
| retour arrière au début d'un bloc | 6 caractères | 13 | 19 | 0 |
| retour arrière au début d'un sous-bloc | 9 caractères | 17 | 22 | 0 |
| retour arrière après un bloc à enfants | 6 caractères | 13 | 19 | 0 |
| insertion d'un bloc (après / en tête) | 1 conteneur, 1 paragraphe, 1 texte, 6 caractères, 4 attributs | 0 | 147 / 145 | 0 |
| suppression d'un bloc (sans / avec enfants) | rien | 13 / 30 | 6 | 0 |
| changement de type | 1 `heading`, 1 texte, 6 caractères, 5 attributs | 11 | 148 | 0 |
| Tab | 1 `blockGroup`, 1 conteneur, 1 paragraphe, 1 texte, 6 caractères, 4 attributs | 13 | 165 | 0 |
| Maj+Tab | 1 conteneur, 1 paragraphe, 1 texte, 6 caractères, 4 attributs | 14 | 151 | 0 |
| deux blocs remplacés par deux neufs | 2 attributs récrits, 12 caractères | 14 | 51 | **2** |
| un bloc supprimé et son voisin modifié | 1 attribut récrit, 8 caractères | 15 | 31 | **1** |

Ce qui change, voulu : **glisser X sous Z** (trois paragraphes) passait de « 3 attributs récrits, 3 caractères, 6 suppressions, 55 octets, 3 éléments renommés » à « 1 conteneur, 1 paragraphe, 1 texte, 6 caractères, 4 attributs créés ; le conteneur d'avant supprimé ; 149 octets ; 0 renommé ». Le premier bloc de 2 000 glissé tout en bas : moins de 2 Ko (test), contre plus de 100 Ko.

Conséquence à connaître : **un élément change encore d'`id`** hors réordonnancement (les lignes en gras). La phrase du compte rendu de l'essai « un élément ne change plus jamais d'`id` » est fausse avec ce qui est livré ; la Task 13b ne doit pas s'y fier.

## Les scénarios S1 à S11

Même état dans les deux ordres d'identifiants (Mona < Tom, Tom < Mona), joués chacun dans les deux ordres d'envoi. « Mona d'abord » : le déplacement arrive au serveur avant ce que fait Tom ; « Tom d'abord » : l'inverse.

| Scénario | Avant le correctif | Après, clients seuls | Mona d'abord | Tom d'abord |
|---|---|---|---|---|
| S1 glisser X, Tom tape dans X | texte dans Y | texte absent, page intacte | limite R222 | attend la 13b |
| S2 idem, Y est un titre | texte perdu | idem | limite R222 | attend la 13b |
| S3 Tom tape dans l'enfant de X | texte perdu | idem (l'enfant est recréé avec son parent) | limite R222 | attend la 13b |
| S4 Tom tape dans Y, non déplacé | texte dans Z | **juste** | juste | juste |
| S5 W glissé en tête, Tom tape dans Z | texte dans Y | **juste** | juste | juste |
| S6 deux glissers du même bloc | textes fondus, doublon selon l'ordre | `x` deux fois, textes intacts | attend le dédoublonnage du serveur (13b) | idem |
| S7 clavier, Tom tape dans X | texte dans Y | texte absent | limite R222 | attend la 13b |
| S8 textes différents | texte dans Y | texte absent | limite R222 | attend la 13b |
| S9 Tab, Tom tape dans X | texte perdu | inchangé (Tab écrivait déjà copie + suppression) | limite R222 | attend la 13b |
| S10 Maj+Tab | texte perdu | inchangé | limite R222 | attend la 13b |
| S11 deux glissers de blocs différents | textes fondus, `y` en double | **juste** | juste | juste |

Par l'acteur réel (constat, sans rien y changer) : `CARGO_TARGET_DIR=target/e2e cargo test -p mementum-sync --test moves` compile et rend `0 passed; 4 ignored`. Avec `-- --ignored` : `browser_move_keeps_the_text_typed_in_a_block_it_shifts` **passe** désormais (les deux ordres, aux quatre endroits) ; `browser_move_keeps_the_text_typed_in_the_moved_block` échoue sur « le bloc x porte "Bloc X", pas "Bloc X de Tom" » (texte absent, plus chez Y) ; les deux tests bâtis avec yrs échouent comme avant. Je n'ai pas touché à `moves.rs` (Task 13b).

## Les huit notes de l'implémenteur précédent, vérifiées

1. **Quel fichier est chargé** — **vrai**. Vitest et BlockNote chargent `src/` (preuve : le binding de l'éditeur est une instance du `ProsemirrorBinding` importé par le test ; `import` et `require` rendent deux fonctions différentes). Les deux fichiers sont corrigés et testés (`both_builds_of_the_dependency_carry_the_fix`, `the_patch_only_adds_and_adds_the_same_to_both_files`). Le build de production : voir « Mesures ».
2. **Où pnpm écrit `patchedDependencies`** — **vrai** : dans `pnpm-workspace.yaml` (pnpm 12.18.0), plus `patchedDependencies` et `patch_hash=` dans le verrou. Le plan dit `package.json` : à corriger (R226). pnpm garde le commentaire que j'y ai mis.
3. **Où joue la branche à clés** — **vrai, en plus étroit** : seulement là où des blocs changent d'ordre (R223), et seulement pour des nœuds blocs (une mention porte un `id` qui n'en est pas un).
4. **Départage d'un échange de voisins** — **vrai pour la méthode** (deux passes en O(n log n), sélection lue par `meta.prosemirrorView.state.selection`), **faux pour l'aide `drag`** : un vrai dépôt ne laisse pas une `NodeSelection` mais un curseur de texte à la fin du bloc déposé (vu en e2e). Pendant le glisser, la sélection est la `MultipleNodeSelection` de BlockNote.
5. **Charger les deux versions de `updateYFragment`** — **fait autrement, plus simplement** : le correctif n'ajoute que des lignes, entre des repères ; le test les retire du fichier installé, vérifie que le reste a l'empreinte SHA-256 du fichier publié, et évalue ce code avec les modules mêmes que charge l'éditeur (Yjs, `keys.js`, `lib.js`) ; l'onglet « d'origine » remplace `_prosemirrorChanged` sur son seul binding. Un test vérifie qu'il écrit bien la forme d'origine.
6. **`prosemirror-state`** — **vrai** : dépendance directe à `1.4.4` (verrou : une entrée d'importateur de plus, aucun paquet neuf ; ajoutée à la main, `pnpm add --offline` échouant faute de métadonnées en cache). La transaction ajoutée porte bien la méta de y-prosemirror et `addToHistory: false`, et n'écrit rien. **Nuance** : BlockNote lit aussi cette méta (`y-sync$`) pour classer une transaction « distante ».
7. **Fixtures** — **vrai** : les quatre noms restent (ce sont `mona-first/s1` et `s4`), les neuves s'ajoutent en sous-dossiers.
8. **Le doute sur « mêmes comptes »** — **fondé, mais pas là où il était attendu**. Entrée au début d'un bloc : BlockNote donne l'`id` neuf au bloc **vide du haut** (insertion pure, aucun écart). L'écart est sur Entrée au milieu et à la fin **d'un bloc qui a des enfants**, et sur les remplacements sans réordonnancement. La piste « les deux milieux non vides » ne suffisait pas : il a fallu « un bloc a changé d'ordre ».

## Mesures

- **Vitest** (`corepack pnpm -C apps/web test`) : avant, `52 passed`, `462 passed | 12 expected fail` ; après, `54 passed`, `670 passed | 32 expected fail`, 22 s (`keyedBlocks` 102, `followBlock` 20, `blockMoves` 105 + 32 en échec attendu).
- **Types** (`corepack pnpm -C apps/web typecheck`) : propre. **Licences** : `117 paquets de production, toutes permises`.
- **e2e, `block-move.spec.ts`** : `6 passed, 2 skipped` (les deux `test.fixme`). Lancés sans `fixme`, les deux échouent seulement sur « X porte le texte que Tom y a tapé » et « la page entière » : plus aucun texte chez un voisin.
- **e2e, suite entière** (`corepack pnpm e2e`, ports 5176 et 8083) : premier passage `153 passed, 1 failed, 2 skipped` (ma régression sur le curseur, voir Step 4) ; après correction, `154 passed, 2 skipped, 0 failed`, 4,9 min. Ce passage a tourné sur `b63932f` plus le fichier des e2e ; les commits suivants ne changent qu'un commentaire du correctif (`7711001`) et un test Vitest (`65089b2`). Sur l'état final, relancé : `block-move`, `collab`, `offline` et `presence` → `16 passed, 2 skipped`.
- **Budgets du navigateur** (`corepack pnpm e2e:budgets`, release + build de production, même poste, même session) :

| Mesure | Avant le correctif | Après |
|---|---|---|
| 1 000 blocs, p50 / p95 (5 ouvertures), 3 passages | 147 / 154, 147 / 157, 149 / 155 ms | 148 / 151, 145 / 150, 147 / 154 ms |
| 1 000 blocs, arbre de 1 000 pages, p50 / p95, 3 passages | 150 / 159, 156 / 158, 157 / 160 ms | 153 / 156, 154 / 159, 160 / 164 ms |
| 10 000 blocs, une ouverture par passage, 7 passages | 1 315, 1 342, 1 422, 1 348, 1 372, 1 525, 1 522 ms (médiane 1 372) | 1 336, 1 506, 1 606, 1 347 (avec la sonde), 1 592, 1 352, 1 280 ms (médiane 1 352) |

Les cinq tests des budgets passent à chaque passage (300 ms et 3 s). **L'ouverture de 10 000 blocs est une seule ouverture par passage, et elle est bruitée sur ce poste** (d'autres agents y compilaient) : deux groupes de valeurs, vers 1,3 s et vers 1,5 à 1,6 s, **avant comme après**. Mes deuxième et troisième mesures « après » (1 506, 1 606 ms) m'ont fait craindre un recul : j'ai donc (a) remesuré le code d'avant, qui donne aussi 1 525 et 1 522 ms, et (b) mesuré directement, par une sonde jetable dans le build de production, le temps passé dans `followBlock` pendant cette ouverture : **2,3 ms en 5 appels, aucune transaction ajoutée**. Le correctif, lui, ne s'exécute pas à l'ouverture (aucune écriture locale). Je conclus qu'aucun budget ne recule ; les chiffres sont tous là pour en juger.

- **Installation à partir de rien** : `node_modules` de l'atelier supprimés (racine, `apps/web`), puis `corepack pnpm install --frozen-lockfile` : `Packages: +190 … Done in 1.9s` ; les deux fichiers de y-prosemirror portent les trois régions ; Vitest `54 passed`, `669 passed | 32 expected fail` (670 depuis le dernier test) ; types et licences verts.
- **Build de production** : `vite build --minify false` (dans un dossier jetable) : le morceau `Editor-*.js` contient `updateMovedChildren`, `longestIncreasing`, `selectedBlockKeys` et `mementum-follow-block` : le build charge bien `src/` corrigé, et l'extension. Les budgets tournent sur ce build (minifié).
- **Si le correctif ne vise plus la version installée** (simulé en renommant la clé en `y-prosemirror@1.3.8`) : `pnpm install --frozen-lockfile` → `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` ; sans verrou figé → `ERR_PNPM_UNUSED_PATCH : The following patches were not used`. L'installation échoue donc bien, bruyamment.
- **Magasin de pnpm** : juste après `pnpm patch-commit`, le fichier corrigé installé est une copie (un seul lien) ; après une installation à partir de rien, il est lié au magasin (deux liens), où pnpm range le paquet corrigé à part du paquet publié. Dans les deux cas, le fichier publié que lit le dépôt d'origine (même magasin) a gardé son empreinte : rien n'a été modifié chez lui.

## Rulings (à porter au registre)

Ruling R223: la branche à clés ne joue que si au moins un bloc présent avant et après (même `id`, même nom de nœud) a changé d'ordre, et si les deux milieux sont non vides ; sinon le code d'origine, tel quel — la variante « tous les enfants du milieu ont un `id` » fait différer 4 gestes ordinaires sur 21 (Entrée au milieu et à la fin d'un bloc qui a des enfants : tous les enfants recréés ; remplacement de blocs ; suppression d'un bloc avec modification du voisin), ce qui est le critère d'arrêt du plan ; R220 ne parle que de réordonnancement — coût si faux : hors réordonnancement, un élément change encore d'`id` sur place comme aujourd'hui (la phrase « un élément ne change plus jamais d'`id` » du compte rendu de l'essai est fausse, et la Task 13b ne doit pas s'y fier) ; une transaction qui mêle un réordonnancement et autre chose passe en entier par la branche à clés.

Ruling R224: le correctif garde aussi `restoreRelativeSelection` : une sélection de nœud dont la position ne précède plus aucun nœud n'est pas remise (elle est laissée à ProseMirror) — y-prosemirror 1.3.7 levait alors dans l'observateur du document, l'éditeur ne montrait plus les changements des autres et sa frappe suivante les effaçait ; déjà possible avant (un autre supprime le dernier bloc, sélectionné chez moi par un clic sur une image), bien plus probable depuis qu'un bloc déplacé est un bloc supprimé — coût si faux : une région de plus à porter à chaque version ; quand le nœud a disparu, la sélection tombe sur un texte voisin au lieu d'un nœud.

Ruling R225: les deux fichiers du paquet sont corrigés (`src/plugins/sync-plugin.js` et `dist/y-prosemirror.cjs`), par ajouts seulement, entre des lignes `mementum:moved-blocks:begin` / `:end` ; les tests retrouvent le code publié en retirant ces régions et en comparent l'empreinte SHA-256 à celle du fichier publié — un seul fichier corrigé ferait dépendre le comportement de la façon de charger le paquet ; des ajouts seuls rendent le portage mécanique et donnent un onglet « d'origine » sans copie gelée du code — coût si faux : deux fichiers à porter ; les deux empreintes (`PUBLISHED`, `keyedBlocks.test.ts`) sont à mettre à jour à chaque version.

Ruling R226: `patchedDependencies` est dans `pnpm-workspace.yaml`, où pnpm 12.18 l'écrit, pas dans `pnpm.patchedDependencies` du `package.json` comme le dit le plan — on suit l'outil — coût si faux : nul ; la phrase du plan est à corriger.

Ruling R227: entre plusieurs façons de garder le plus de blocs en place, on recrée ceux que touche la sélection de l'éditeur (deux passes, O(n log n)) ; sinon c'est le moins de blocs recréés qui décide, même si ce ne sont pas ceux que la personne a déplacés (trois blocs descendus d'une place : c'est le bloc croisé qui est recréé) — moins de blocs recréés, c'est moins de blocs où une frappe concurrente se perd, et un critère sans état — coût si faux : le « bloc déplacé » du serveur (Task 13b) est celui qui est recréé, pas toujours celui du geste ; le report doit valoir pour n'importe quel bloc recréé.

Ruling R228: `followBlock` fait plus que l'interface du plan. Il suit un curseur ou une sélection de texte quand un bout a quitté son bloc **ou** quand l'élément qui portait son texte a été supprimé (sa position relative ne mène plus nulle part) ; une sélection de nœud ; les blocs tenus pendant un glisser (`MultipleNodeSelection` de BlockNote), dont il tient aussi à jour la copie déposée (`view.dragging`) ; et cela après une annulation locale comme après un changement reçu — y-prosemirror remet toujours une sélection, au mauvais endroit (la sélection ne « tombe hors de tout bloc » jamais : le critère du plan n'est pas observable) ; et un e2e a montré qu'un changement reçu pendant un glisser faisait supprimer par le dépôt le texte du bloc tenu, **déjà sur `main`** — coût si faux : l'extension lit `binding.beforeTransactionSelection` de y-prosemirror, le nom `multiple-node` et `view.dragging` ; à revérifier à chaque montée de version de BlockNote ou de y-prosemirror (ses 20 tests et 3 e2e le diront).

Ruling R229: hors de la liste des fichiers du plan : `prosemirror-state` 1.4.4 en dépendance directe (décision du contrôleur), les aides de tests partagées `apps/web/src/test/editors.ts` et `node.ts`, six scènes de fixtures de plus que S1 à S11 (frappe au début, au milieu et à la fin ; gras ; sous-bloc ajouté ; auteur du déplacement qui a aussi tapé ; contenu en ligne à plusieurs textes ; suppression concurrente), et un fichier `scenario.json` par scène (pages de départ, de chacun seul, pages voulues avec types et enfants) — trois suites partagent les mêmes éditeurs ; la Task 13b cite ces cas dans son Step 1 — coût si faux : 136 petits fichiers de plus ; la Task 13b est libre de n'en lire qu'une partie.

## Ce que la documentation devra dire (Task 13c)

**Un piège neuf, « correctif de y-prosemirror » :**

- *Pourquoi il existe.* y-prosemirror 1.3.7 apparie par position les enfants qui diffèrent : un bloc déplacé faisait récrire sur place tous les blocs croisés. Le correctif (R220) fait d'un réordonnancement « le bloc déplacé est supprimé et recréé au même `id`, les autres ne sont pas touchés », et seulement d'un réordonnancement (R223).
- *Quels fichiers.* `patches/y-prosemirror@1.3.7.patch` ; `pnpm-workspace.yaml` (`patchedDependencies`) ; le verrou (`patchedDependencies` et `patch_hash=` dans chaque clé). Dans le paquet : `src/plugins/sync-plugin.js` et `dist/y-prosemirror.cjs`, trois ajouts chacun, entre des lignes `mementum:moved-blocks`.
- *Comment le porter.* `corepack pnpm patch y-prosemirror@<v>` ; reporter les trois régions dans `src` ; les reporter dans `dist` (mêmes lignes, `instanceof Y.` s'y écrit `instanceof Y__namespace.`) ; `corepack pnpm patch-commit <dossier>` ; mettre à jour les deux empreintes `PUBLISHED` de `keyedBlocks.test.ts` ; lancer `keyedBlocks.test.ts`, `followBlock.test.ts`, `blockMoves.test.ts` et `e2e/block-move.spec.ts`. Ne **jamais** modifier un fichier sous `node_modules` à la main : ce sont des liens vers le magasin de pnpm, partagé par tous les projets du poste.
- *Ce qui échoue s'il ne s'applique plus.* L'installation : `ERR_PNPM_UNUSED_PATCH` (ou `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` en verrou figé). Si le correctif est retiré : `keyedBlocks.test.ts` dit « le correctif de y-prosemirror n'est pas installé », et les e2e `typing_in_a_neighbour…` et `two_people_moving_two_blocks…` redeviennent rouges.
- *Ce qu'il ne fait pas.* Tab, Maj+Tab et un déplacement d'un niveau à l'autre s'écrivaient déjà en copie + suppression : inchangés. Un `id` absent, vide ou en double : code d'origine. Les lignes et colonnes de tableau : pas d'`id`, code d'origine.

**La note « Déplacements concurrents » de `CLAUDE.md` est à récrire** : un glisser au même niveau ou au clavier ne récrit plus les blocs entre les deux places ; le bloc déplacé est recréé au même `id` (plus aucun bloc ne change d'`id` par un déplacement) ; une frappe faite en même temps **dans le bloc déplacé** est perdue (plus « finit dans un voisin ») ; dans un autre bloc, elle reste à sa place ; deux glissers de blocs différents sont justes ; deux glissers du même bloc le laissent deux fois dans le document, au même `id`, que BlockNote renomme chez lui sans l'écrire.

**À ajouter aux pièges :**
- `followBlock` : pourquoi (y-prosemirror remet la sélection où était le bloc), ce qu'il lit (R228), et « ne pas le retirer ».
- Un dépôt de BlockNote laisse un curseur de texte à la fin du bloc déposé ; pendant le glisser, la sélection est `multiple-node`, et BlockNote pose hors de la page une copie du bloc glissé avec les classes de l'éditeur : un e2e qui lit les blocs pendant un glisser vise `.mm-editor-frame .bn-editor`.
- Un changement reçu pendant un glisser faisait supprimer par le dépôt le texte du bloc tenu (bogue antérieur, levé par `followBlock`).
- Fixtures : `crates/mementum-sync/tests/fixtures/moves/<ordre>/<scène>/`, réécrites par `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/blockMoves.test.ts`.

## Régressions constatées par rapport à avant

1. **Ce qui est tapé dans le bloc déplacé** (glisser, clavier) : « chez un voisin » devient « perdu », tant que la Task 13b n'est pas là (et au-delà pour l'ordre R222). Vu en Vitest (S1, S7, S8), par l'acteur et dans un navigateur (les deux `test.fixme`). C'est le coût accepté par R220.
2. **Annuler un déplacement** : le bloc revient à sa place, recréé une seconde fois ; ce que d'autres ont écrit **dans ce bloc** depuis le déplacement est retiré. Ce qu'ils ont écrit dans les autres blocs reste, dans les mêmes éléments (avant, l'annulation le remettait dans le mauvais bloc) : `undo_of_a_move_leaves_other_blocks_alone`. Je n'ai pas écrit de test du texte perdu dans le bloc annulé : c'est le coût accepté, pas un comportement à tenir.
3. **Un bloc déplacé deux fois en même temps** : deux exemplaires au même `id` (avant : textes fondus). BlockNote donne à chacun, chez lui, un `id` neuf sans l'écrire : tant que le serveur ne dédoublonne pas, un onglet qui écrit dans cette fenêtre fait perdre l'`id` d'origine (donc les fils de commentaires du bloc). Non mesuré en e2e.
4. **Poids d'un petit déplacement** : 149 octets au lieu de 55 pour trois paragraphes courts (le bloc est récrit en entier) ; un grand bloc avec beaucoup d'enfants déplacé pèse son sous-arbre entier.

## Ce que le compte rendu de l'essai disait, et que j'ai mesuré autrement

- « Entrée (début, milieu, fin)… : mêmes comptes de structures » : faux pour un bloc qui a des enfants, avec le correctif tel que l'essai le donne (R223).
- « Un élément ne change plus jamais d'`id` » : faux avec ce qui est livré, hors réordonnancement (R223).
- « Le curseur saute à la fin du document » : seulement si le bloc était le premier de sa liste ; sinon il passe dans le bloc qui a pris sa place. Et y-prosemirror **remet** une sélection (`selectionSet` vrai) : rien ne « tombe hors de tout bloc ».
- « ProseMirror sélectionne le bloc déposé » (note 4) : dans un navigateur, BlockNote laisse un curseur de texte à la fin du bloc déposé.
- Non dit par l'essai, trouvé ici : un changement reçu **pendant** un glisser faisait vider le bloc tenu au dépôt (sur `main` aujourd'hui) ; y-prosemirror lève sur une sélection de nœud dont le nœud est retiré par un autre ; deux frappes simultanées dans un paragraphe vide font supprimer un des deux textes par y-prosemirror (à ne pas confondre avec un bloc recréé).

## Ménage

`target/` (3,7 Go), `apps/web/dist` et `apps/web/test-results` de l'atelier sont supprimés ; les ports 5176 et 8083 sont libres ; aucun processus lancé par moi ne reste. `node_modules` de l'atelier est installé avec le correctif. Le seul fichier non suivi est ce rapport. Aucune sonde jetable n'est restée dans le code (vérifié par `git status`).

## Doutes

1. **R223 est un écart à la décision du contrôleur** (« la branche à clés ne joue que quand tous les enfants du milieu ont un `id` distinct et non vide »). Je le crois nécessaire (critère d'arrêt, mesuré) et conforme à R220, mais il enlève « un élément ne change plus d'`id` » : à valider.
2. **R224 et R228 élargissent le périmètre** (une seconde région du correctif ; `followBlock` qui tient les blocs d'un glisser). Chacun répond à un défaut constaté, antérieur, que la forme à clés rend plus fréquent ou qu'un e2e du plan a mis au jour. À valider aussi.
3. **Le départage par la sélection dans un vrai navigateur** n'est prouvé que par ce qu'un e2e lit de la sélection après un dépôt (curseur dans le bloc déposé) : aucun e2e n'échange deux voisins en vérifiant quel élément est recréé. La page finale est juste dans les deux cas ; seul change le bloc exposé à la perte.
4. **Colonnes** (`columnList`, `column`) : la branche à clés y joue par construction (ce sont des blocs avec un `id`), mais aucun test ne déplace un bloc dans une colonne ni une colonne.
5. **Le test au hasard mixte** (un onglet corrigé, un onglet d'origine) converge sur 20 graines de 40 pas, six blocs : c'est une mesure, pas une preuve. Les ids en double y sont admis (BlockNote les renomme chez lui).
6. **`a_big_move_writes_a_small_update`** monte un éditeur de 2 000 blocs dans jsdom : plusieurs secondes sur ce poste, délai de 60 s.
7. **Mémoire de `binding.mapping`** : comme le code d'origine, le correctif n'en retire que l'élément supprimé, pas ses descendants.
8. **`followBlock` a déjà eu un faux positif** (le texte fusionné, corrigé en `b63932f`), pris par la suite e2e et non par mes tests : sa règle « l'élément qui portait le texte a été supprimé » repose sur ce que y-prosemirror fait de ses textes. Un relecteur devrait chercher d'autres cas où un paragraphe est supprimé sans que le bloc soit recréé ailleurs. Celui que je connais est tenu par un test (`65089b2`) : un autre change le type du bloc ; sans l'extension le curseur passait au début du bloc **suivant** (défaut antérieur), avec elle il reste à sa place.
9. **Budget de 10 000 blocs** : médianes égales et coût mesuré de 2,3 ms, mais la mesure du plan est une seule ouverture, bruitée sur ce poste occupé ; à refaire au calme à la clôture (« toutes les suites sur un clone neuf »).
10. **Le message de `996bb6c`** est inexact sur un point (voir « Commits »).
