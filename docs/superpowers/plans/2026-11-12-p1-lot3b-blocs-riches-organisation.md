# Mementum — Phase 1, lot 3 « Collaboration et blocs riches », partie B « Blocs riches et organisation » : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Relu le 2026-11-13 par un relecteur indépendant, avant exécution** : 7 constats bloquants, 12 importants, 17 mineurs, tous reportés dans ce texte. Ce qui a été vérifié par un essai : les en-têtes de R246 dans Chromium (une image s'affiche, un son et une vidéo se lisent avec des plages, un HTML ouvert comme document est téléchargé sans s'exécuter). Choix de produit pris par le contrôleur à cette occasion, **à confirmer par le propriétaire** :
> - M7 : une invitation qui ne donnerait rien au compte qui l'accepte, jugée sur son niveau effectif, n'est pas consommée (R257) ;
> - dupliquer une page : la copie va sous la page parente si l'on peut y créer ; sinon, pour un membre, à la racine dans son « Privé » (R252) ;
> - un favori n'avance ni `rev` ni `lsn` : il paraît dans les autres onglets du compte à leur prochaine lecture de l'arbre (R254) ;
> - mentionner une page copie son titre dans le document : qui lit la mention lit ce titre, même sans voir la page (R251) ;
> - pas de quota de fichiers avant le lot 7 : qui peut modifier une page peut remplir le disque (R247).

**Goal :** dans une page, on dépose une image ou un fichier, on pose un callout, une équation, un signet, une table des matières, un lien de page, on mentionne une page, une personne ou une date ; dans l'espace, on duplique une page, on garde ses favoris, on retrouve ses pages récentes, et la barre du haut dit où est la page. Et les cinq petits reports du lot 2 sont levés.

**Le lot 3 est mené en deux plans** (Ruling R132). Celui-ci est la partie B. La partie A, « Collaboration », est dans [`2026-11-12-p1-lot3-collaboration-blocs-riches.md`](2026-11-12-p1-lot3-collaboration-blocs-riches.md) ; ce plan en reprend la forme, les contraintes et la méthode (R140), et s'exécute en **21 tâches** (piste serveur : S1, S2, S3, S4a, S4b, S5, S6 ; piste client : C1a, C1b, C2 à C10 ; relais : X1, X2, D), **en deux pistes menées de front** (voir « Mener deux pistes de front »).

**Architecture :**
- `mementum-store` : une migration `0004` crée `files`, `attachments` et `favorites` (R243) ; une inscription ouvre sa session dans sa transaction ; une page se duplique, avec son sous-arbre, en une transaction.
- `mementum-server` : un dépôt de fichiers sur disque, rangés par le SHA-256 de leur contenu (`files.rs`) ; les routes d'envoi, de lecture et de recopie d'un fichier, sous les droits de la page qui le porte ; les favoris ; la duplication d'une page.
- `mementum-crdt`, `mementum-sync`, `mementum-md`, `mementum-schema` : **rien ne change**. Tous les blocs de ce plan sont déjà dans le schéma 1.
- `apps/web` : le menu « / » propose les blocs riches, chacun avec son écran de réglage ; `@` et `[[` ; Ctrl+D ; l'envoi de fichiers branché sur BlockNote ; les sections « Favoris » et « Récents » ; le fil d'Ariane ; « Dupliquer ».

**Tech Stack :** celle de la partie A. Aucun paquet npm nouveau. Aucune crate neuve dans `Cargo.lock` : `mementum-server` prend en dépendances directes des crates qui y sont déjà (R247).

**Spec :** `docs/specs/2026-11-09-mementum-design.md`. Sections utiles : §4.2 (barre latérale : « Favoris »), §4.3 (barre du haut : fil d'Ariane), §4.5 (lignes de la P1 de « Éditeur et blocs » et de « Collaboration et organisation »), §5.2 (ligne « Fichiers »), §5.3 (`files`, `attachments`), §5.6 (réponses, fenêtres acceptées), §5.8, §5.10, §6.2 (formes MMD de `image`, `video`, `audio`, `file`, `callout`, `equation`, `bookmark`, `pageLink`, `toc`, `<mention>`, `$…$`), §8, §9. Lire aussi `CLAUDE.md` en entier, et au registre du lot 3 : R132, R139, R140, les sections des tâches 8 à 11 (ce que les relectures ont corrigé), et le tableau de la Task 12 (ce que chaque geste écrit dans Yjs).

**Ce que la conception demande en P1 sur ces sujets, et ce qu'en fait ce plan** (Ruling R240) :

| Sujet | Conception | Ce plan livre | Laissé, et où |
|---|---|---|---|
| Fichiers | §5.8 : stockage par SHA-256, dédoublonné ; miniatures ; URL signées ; quotas ; antivirus. §5.3 : `files`, `attachments`. §4.5 : « Image, vidéo, audio, fichier, PDF » | envoi, dépôt par contenu, droits de la page, blocs image, vidéo, audio et fichier alimentés par l'envoi (un PDF est un bloc fichier) | miniatures, quotas, antivirus : lot 7 ; URL signées, S3 : P5 ; visionneuse de PDF : aucun bloc au registre |
| Callout | §4.5, §6.2 | insertion, icône, couleur, « Transformer en » | — |
| Équation | « équation KaTeX (bloc et en ligne) » | les deux, avec leur écran d'édition | — |
| Signet | « signet (aperçu de lien) » | la carte : hôte et adresse | l'aperçu distant (titre, image) : R250 |
| Table des matières | §4.5, §6.2 `<toc/>` | les titres de niveau 1 à 4 de la page, cliquables | — |
| Lien de page | §6.2 `<page id>titre</page>` | insertion par un sélecteur, titre à jour pour qui voit la page | — |
| Mentions, `[[` | « @personne, @page, @date (langage naturel), @rappel ; `[[` pour lier ou créer une page » ; notifications de mention en P3 | `@` page, personne, date ; `[[` lie ou crée | `@rappel` (rien ne prévient avant la P3) ; `[[Page#Titre]]` ; rétroliens (il leur faut `blocks_index`, lot 4) |
| Duplication | « duplication » (pages) ; « Ctrl+D » (blocs) | les deux | — |
| Favoris | §4.2, §4.5 | section « Favoris », par compte | cache hors ligne (§5.9, P5) |
| Récents | §4.5 | section « Récents », par navigateur (R255) | les récents qui suivent d'un appareil à l'autre |
| Fil d'Ariane | §4.3 | celui de la barre du haut | le bloc « fil d'Ariane » (§4.5, absent du registre) ; « Modifié il y a… » |
| Reports du lot 2 | registre du lot 2, « Reportés au lot suivant » | M5, M7, M9, M11, m1 (R257) | M4 (non reproduit), M2 |

**Hors de ce plan :**
- la fin de la partie A : ses tâches 12 à 14, sa relecture finale, et **le déplacement de blocs à plusieurs (R39)**, qui a sa propre tâche, encore ouverte, avec son correctif de `y-prosemirror` ;
- lot 4 : recherche, `blocks_index`, rétroliens ; lot 5 : jetons d'API, REST complet, MCP, écriture MMD par différence (R40), alias courts ; lot 6 : imports et export (dont les pièces jointes d'Obsidian et de Notion) ; lot 7 : quotas par espace, envois simultanés par compte, limites de débit, ménage des fichiers qu'aucun bloc ni aucune version ne nomme et des contenus que la base ne connaît pas (R245), antivirus, miniatures, en-têtes de l'application embarquée (CSP, `Referrer-Policy`) ;
- P2 : bases, propriété « fichiers » ; P3 : blocs synchronisés, notifications (une mention ne prévient personne), `@rappel`, liens publics ; P5 : S3, URL signées, hors ligne, onglets ;
- lignes de la P1 du §4.5 qui ne sont dans aucun lot écrit, **à placer par le propriétaire** : intégrations oEmbed, Mermaid rendu en diagramme, icône et couverture de page, emojis personnalisés, pleine largeur, petite police, verrouillage de page, « Modifié il y a… », aperçu de lien du signet.

---

## Mener deux pistes de front

Ruling R241 fixe la règle : **deux pistes, deux arbres de travail**.

- **Piste S (serveur)** : `crates/`, `packages/protocol/src/generated/` (régénéré), `spec/`. Elle travaille dans l'arbre principal, sur `main`, comme la partie A.
- **Piste C (client)** : `apps/web/`. Elle travaille dans **un atelier** (`git worktree add ../Mementum-client -b lot3b-client main`), avec son `node_modules` (`corepack pnpm install --frozen-lockfile`) et son `target` (les e2e y compilent le serveur depuis du Rust commité, jamais depuis le Rust en cours de la piste S). Avant chaque tâche, l'atelier se rebase sur `main` ; à la clôture d'une tâche C (relue, corrigée), le contrôleur la reprend sur `main` par avance rapide après un dernier rebase. La branche de l'atelier n'est jamais poussée. À la fin : `git worktree remove`, et son `target` supprimé.
- **Tâches X (relais)** : sans fichier commun avec les autres, elles sont prises par la piste qui se libère.
- **Ports des e2e.** Les deux pistes lancent des e2e en même temps, et une suite s'arrête si un port est pris (`apps/web/e2e/servers.ts`). Piste C : `MEMENTUM_E2E_WEB_PORT=5177` et `MEMENTUM_E2E_API_PORT=8084`. Piste S et relais : `5178` et `8085`. Jamais 5173, 5175, 5176, 8080, 8083 ni 8090 (les défauts, l'instance d'Alex, l'atelier de la Task 13a). Chaque tâche qui lance un e2e les répète.
- **Si le disque a moins de 60 Go libres** à l'ouverture de l'atelier (78 Go le 2026-11-12), pas d'atelier : une seule piste, aux ports de la piste S, dans l'ordre S1, C1a, S2, C1b, S3, C2, S4a, C3, S4b, C4, S5, C5, S6, C6, X1, C7, X2, C8, C9, C10, D.

**Avant de commencer.** (1) La piste S de la relecture finale de la partie A et la Task 13b sont closes, ou la 13b reportée par écrit : elles touchent les mêmes fichiers que la piste S de ce plan (`mementum-crdt`, `mementum-sync`, les routes du serveur), et la 13b retire `block_moves` de `0003`, figée ensuite (R243). (2) La Task 13a (le correctif de `y-prosemirror`, R220) est **close** : livrée et reprise sur `main`, ou arrêtée sur son critère et consignée. Dans les deux cas la piste C commence : aucun geste de ce plan n'est un réordonnancement de blocs, et le correctif ne joue que sur les listes d'enfants à identifiants, jamais à l'intérieur d'un bloc. La dépendance est de fichiers : la 13a récrit `blockMoves.test.ts`, dont C1a sort le banc, branche `followBlock` dans `Editor.tsx` et change le verrou pnpm. Tant que la 13a n'est pas close, la piste C commence par C6, puis C7 dès que S5 est close.

| Créneau | Piste S (arbre principal) | Piste C (atelier) |
|---|---|---|
| 1 | S1 reports du lot 2, serveur | C1a banc à deux éditeurs et mesures des gestes |
| 2 | S2 store : fichiers et favoris | C1b menu « / », callout, signet |
| 3 | S3 dépôt des fichiers | C2 équations |
| 4 | S4a envoyer et lire un fichier | C3 table des matières, lien de page |
| 5 | S4b recopier une attache, ménage, durée de vie | C4 mentions et `[[` |
| 6 | S5 favoris | C5 dupliquer un bloc |
| 7 | S6 dupliquer une page | C6 fil d'Ariane, récents |
| 8 | X1 reports du lot 2, client (après S1) | C7 favoris (après S5) |
| 9 | X2 fixture `rich-blocks` (après C1b) | C8 fichiers (après S4a) |
| 10 | relecture finale, passe « sécurité », sur les tâches S closes | C9 fichier d'une autre page (après C8 et S4b) |
| 11 | | C10 dupliquer une page (après S6) |
| 12 | D documentation et recette | |

**Fichiers partagés par nature : qui y touche, quand.**

| Fichier | Règle |
|---|---|
| `crates/mementum-server/src/types.rs`, `packages/protocol/src/generated/` | la piste S seule, dans la tâche qui livre la route. Une tâche C ne régénère jamais. Tout type neuf de ce plan est **un type neuf** (`FileInfo`, `Favorites`…), jamais un champ ajouté à `Tree`, `Info` ou `PageNode` : le client compile sans attendre. Seule S1 change deux types existants (`Accepted`, `PendingInvitation`) : elle corrige, dans un commit `web :` à part, les seules constructions littérales que le typecheck du client casse (des fichiers de test, qu'aucune tâche C menée de front ne touche), sans rien brancher |
| `crates/mementum-server/src/lib.rs` (tableau de l'en-tête, `router`) | la piste S ; chaque tâche y ajoute ses lignes |
| `crates/mementum-store/migrations/0004_files_favorites.sql` | S2 seule (R243). `0003_collaboration.sql` n'est touchée par aucune tâche de ce plan |
| `apps/web/src/api.ts`, `theme.css`, `icons.tsx`, `editor/Editor.tsx`, `editor/schema.tsx`, `page/PageMenu.tsx`, `page/PageView.tsx`, `sidebar/*`, `App.tsx`, `e2e/helpers.ts`, `e2e/servers.ts`, `playwright.config.ts` | la piste C seule, une tâche à la fois. X1 et X2 n'y touchent pas |
| `docs/superpowers/logs/2026-11-12-p1-lot3-ledger.md` | avant la première tâche, le contrôleur y pose trois titres : « Partie B, piste serveur », « Partie B, piste client », « Partie B, relais et clôture ». Chaque tâche écrit sous le titre de sa piste, à la fin de sa section. Numéros : R260 à R289 pour la piste S, R290 à R329 pour la piste C, R330 à R349 pour X, D et le contrôleur. À l'ouverture de la partie B, le contrôleur relève le dernier numéro pris au registre et décale les trois plages s'il le faut |
| `CLAUDE.md` | chaque tâche y ajoute ses pièges sous « Pièges rencontrés à la partie B du lot 3 », dans le sous-titre de sa piste (« Serveur », « Client »), posé par le contrôleur |
| `README.md`, `docs/REPRISE.md`, conception §9 et §11 | la tâche D seule. Chaque tâche note au registre, sous « À porter au README », ce qu'elle change pour l'utilisateur |
| conception §4 (écrans) | piste C ; conception §5 et §6 : piste S |

**Relectures.** Comme à la partie A (R140) : un implémenteur, puis un relecteur qui prouve chaque constat par un test jetable et relit toute règle de protection comme une attaque ; les constats importants repartent en tour de correction, cinq tours au plus ; les mineurs vont à la liste de la relecture finale. Deux tâches de front ont chacune leur relecteur. Aucune tâche C qui dépend d'une tâche S ne commence avant que celle-ci soit **close et relue**.

---

## Global Constraints

Celles du plan de la partie A restent valables telles quelles (langue, commits sans aucune ligne d'attribution d'outil, contraintes des lots 1 et 2, budgets, ne jamais arrêter un processus qu'on n'a pas lancé, `data/instance` jamais ouvert par un binaire d'un lot en cours) ; les ports des e2e sont ceux de « Mener deux pistes de front », pas les défauts. S'y ajoute ce que la partie A a appris ; chaque ligne vaut pour toute tâche.

**Droits et routes**
- Les droits passent par `crates/mementum-server/src/access.rs`, et nulle part ailleurs.
- Ordre des refus : `401` ; `404 not_found` pour une page qu'on ne voit pas, **exactement** la réponse d'une page inconnue ; `403 forbidden` sous le niveau ; la ressource (`404` d'un fichier qui n'est pas de la page, avec le corps d'une page introuvable, comme une version, R186) ; `409 trashed` ; enfin le corps (`413`, `422`).
- Toute route neuve entre dans `foreign_page_is_indistinguishable_from_missing` (`tests/permissions.rs`, par `page_routes` de `tests/common/mod.rs`, ou par une liste à elle quand la route nomme une ressource de la page : `file_routes`, comme `version_routes`) ou, pour une route d'espace, dans `space_routes_by_role` (`tests/spaces.rs`).
- Un id nommé dans le corps qu'on ne voit pas reçoit l'erreur d'un id inconnu (`422 invalid_source`), jamais `403` ni `404`.
- Une route qui attend avant d'écrire (page à charger, corps à recevoir) **revérifie les droits après l'attente** (R89). Ce qui se vérifie puis s'écrit comme droit durable se revérifie dans la transaction qui écrit (R100).
- Aucun identifiant ni titre de page ne sort vers quelqu'un qui ne la voit pas, par aucune route neuve. Un message de refus tutoie et nomme les niveaux comme l'écran (`level_name`).

**Contenu et synchro**
- Jamais d'octets d'un client écrits ni relayés tels quels dans un document (R123, R130). Un fichier envoyé est gardé tel quel, mais n'est jamais servi comme un document (R246).
- Toute écriture dans une page existante passe par le chemin de l'acteur (`client_update`, `write`, `server_write`) : sinon elle échappe à l'historique. Ce plan n'en ajoute aucune ; une page dupliquée est une page **neuve**, écrite avec son nœud comme `POST /api/v1/pages`.
- Une écriture qui change ce que montre la barre latérale ouvre une `WriteTx` (`Store::write`) et appelle `bump_rev` ou `bump_lsn` ; jamais d'`UPDATE spaces SET lsn` à la main. Un favori ne change que la barre de son auteur : il n'appelle ni l'un ni l'autre (R254).
- **Schéma des blocs** : y-prosemirror supprime du document partagé ce qu'il ne sait pas construire ; un bloc, une prop ou une marque neufs demandent une version de schéma neuve. **Ce plan n'en ajoute aucun : `SCHEMA_VERSION` reste à 1** (R242). Une tâche qui croit avoir besoin d'une prop neuve s'arrête et rend `BLOCKED`.
- **Gestes de l'éditeur** : ne rien supposer de ce qu'un geste écrit dans Yjs. Chaque tâche qui ajoute un geste commence par le test qui le relit dans le document (`apps/web/src/editor/gestures.test.ts`). Ce fichier dit en tête sous quel `y-prosemirror` il a été mesuré (avec ou sans le correctif de la Task 13a) et reste vert dans les deux. Aucun geste de ce plan ne déplace un bloc ; le correctif ne change rien à l'intérieur d'un bloc (changement de type, contenu en ligne) : une limite mesurée là reste une limite, écrite au registre et portée à la tâche D.

**Client**
- Rien sur le chemin de l'ouverture d'une page : budgets de 300 ms au p95 pour 1 000 blocs, 3 s pour 10 000, frappe vue chez un autre sous 150 ms. Ce qui n'est pas affiché à l'ouverture est chargé à la demande. Ne jamais parcourir tous les blocs à chaque frappe.
- Les relectures passent par `apps/web/src/rereads.ts` (`scheduleRereads`) : jamais annulées en route, état lié à la page ou à l'espace affiché, plus rien après le démontage. Une lecture qui trouve la session perdue le dit (`SESSION_LOST`) et repart au retour du compte (`afterLogin`), comme celles des commentaires et de l'historique.
- Aucun écouteur posé sur le document d'une session par autre chose que l'éditeur ; aucun `applyUpdate` dans l'événement `update` d'un autre document sans attraper ses exceptions.
- Texte venu d'ailleurs (noms, titres, noms de fichiers, URL, LaTeX, libellés de mention) : toujours un nœud de texte, jamais du HTML ni un attribut `style` ; `dir="auto"` et `unicode-bidi: isolate` ; U+2068 et U+2069 autour de lui dans un attribut. KaTeX reste à `trust: false`. **Une URL n'est suivie ou chargée que si son schéma est permis** : `http:`, `https:`, ou un chemin de fichier de l'instance (`isFilePath`, C8).
- Clavier et accessibilité : tout se fait au clavier ; un raccourci se reconnaît à `event.key`, jamais à `event.code`, sans avaler un caractère tapé avec AltGr ; le focus ne tombe jamais sur `body` ; Échap ne détruit jamais un texte tapé ; une liste est un `ul` ou un `ol` de `li`, un menu un `role="menu"` ; contraste d'au moins 4,5:1, calculé par un test.
- Un bouton qui crée quelque chose passe par `firstClick`. Un refus du serveur s'affiche tel qu'il le dit, et fait relire ce qu'il concerne.

**Tests**
- Chaque test est vu rouge d'abord, pour la bonne raison ; un test qui passe du premier coup ne prouve rien.
- Pas d'attente fixe : une condition, avec une borne large, qui peut faire échouer à tort et jamais passer à tort.
- Un témoin vert pour chaque test qui prouve une absence (rien n'est servi, rien n'est écrit, rien n'est montré). Un test qu'une écriture naturelle du code ne peut pas faire voir rouge est dit « garde » dans sa tâche : il ne compte pas comme preuve du livrable.
- Un e2e qui retient les lectures de la page retient aussi celles de l'arbre. Un e2e qui change des droits les pose avant d'ouvrir les pages. Les e2e à deux personnes là où la fonction se voit à plusieurs.
- Aucun budget relâché, aucun test existant affaibli ; un test qui change de sens est dit au registre.

**Exécution**
- **Dépendances** : aucune ne s'ajoute hors de R247. `cargo deny check licenses bans` et `node tools/check-npm-licenses.mjs` restent au vert.
- **Migration** : une par partie (R139, amendé par R243). `0003` est figée à la clôture de la partie A ; `0004` peut changer jusqu'à la clôture de B ; aucun binaire de la partie B n'ouvre une instance que quelqu'un regarde avant sa clôture.
- **Ports** : tout e2e se lance aux ports de sa piste (« Mener deux pistes de front »), jamais aux défauts.
- **Pousser** : selon `docs/REPRISE.md` (accord de Lenny pour le lot 3, tant que format, clippy et tests tiennent) ; jamais de poussée forcée ; `lot3b-client` n'est jamais poussée.
- **Registre des écarts** : forme `Ruling RNN: <décision> — <pourquoi> — <coût si faux>`. Les décisions de ce plan sont R240 à R257 (R198 à R239 sont à la fin de la partie A) ; l'exécution suit les plages dites plus haut.

## Review Focus

1. **Un fichier dont le type ment** (du HTML ou un SVG nommé `photo.png`, déclaré `image/png`) : il n'est jamais rendu comme un document depuis l'origine de l'application. Tests : S4a, `download_headers_never_let_a_file_run` ; C8, e2e `html_named_png_never_runs`.
2. **Un droit qui change pendant ou après l'envoi** : l'envoi est revérifié une fois le corps reçu ; un fichier cesse d'être servi dès que la page n'est plus lisible, cache du navigateur compris. Tests : S4a, `upload_checks_rights_again_once_the_body_is_received`, `revalidation_is_checked_like_a_read` ; C8, e2e `image_is_no_longer_served_once_access_is_withdrawn`.
3. **Un fichier dont le bloc part, puis revient** (annulation, version restaurée), **dont la page d'origine est supprimée** (bloc collé ailleurs, page dupliquée), **ou dont la base est revenue en arrière** (copie d'avant migration remise en place) : le fichier est toujours là. Tests : S3, `opening_empties_tmp_and_keeps_what_the_database_does_not_know` ; S4b, `file_survives_its_block_and_comes_back_with_a_restored_version`, `upload_of_a_content_being_swept_keeps_its_file` ; S6, `duplicated_page_keeps_its_files_after_the_original_is_deleted` ; C9, e2e `pasted_image_is_seen_by_someone_who_cannot_read_its_first_page`.
4. **Un lien ou une mention vers une page que le lecteur ne voit pas** : il ne lit que le libellé copié dans le document, jamais le titre à jour, et aucun sélecteur ne lui propose cette page. Tests : C3, `link_to_a_page_the_reader_cannot_see_shows_only_what_the_document_holds` ; C4, e2e `guest_is_offered_only_the_pages_he_sees`.
5. **Une copie qui reprend des identifiants** : un bloc dupliqué avec l'`id` de l'original serait défait par le dédoublonnage du serveur, qui supprime le second exemplaire. Tests : C5, `duplicated_block_has_fresh_ids_at_every_depth`, e2e `ctrl_d_duplicates_and_the_server_keeps_both` ; S6, `duplicate_copies_content_with_fresh_block_ids`.

---

## Décisions de ce plan

Chacune est un Ruling du registre, à y recopier à l'ouverture de la partie B.

**Périmètre (R240).** `Ruling R240:` la partie B livre ce que dit le tableau de l'en-tête, ni plus, ni moins : « duplication » couvre la page (§4.5, « Collaboration et organisation ») et le bloc (Ctrl+D, « Éditeur et blocs ») ; « mentions » couvre `@page`, `@personne` et `@date`, pas `@rappel` — pourquoi : R132 nomme les sujets, la conception dit ce que la P1 en attend ; un rappel qui ne prévient personne avant la P3 n'en est pas un ; les autres lignes de la P1 du §4.5 (oEmbed, Mermaid, icône et couverture, verrouillage…) ne sont dans aucun sujet de R132 — coût si faux : ces lignes restent à placer, et le propriétaire le lit dans « Hors de ce plan ».

**Deux pistes (R241).** `Ruling R241:` la partie B s'exécute en deux pistes de front, serveur dans l'arbre principal et client dans un atelier `git worktree` rebasé sur `main`, les tâches d'une piste l'une après l'autre, chaque piste à ses ports d'e2e ; sous 60 Go de disque libre, une seule piste — pourquoi : le propriétaire veut finir vite sans rien rogner ; les deux pistes n'ont aucun fichier commun, mais les e2e du client compilent le serveur depuis l'arbre où ils tournent, et un Rust en cours d'écriture les a déjà fait échouer (Task 9, correction 1) ; R140 écartait un atelier **par tâche**, pas un seul atelier durable — coût si faux : un second `target` de debug (une dizaine de gigaoctets) et un rebase par tâche ; un conflit de reprise reste possible dans la documentation, d'où les règles de « Fichiers partagés ».

**Schéma (R242).** `Ruling R242:` aucun bloc, aucune prop, aucune marque ne s'ajoute ; `SCHEMA_VERSION` reste à 1, et aucun client ne passe en lecture seule à cause de ce plan — pourquoi : tout y est déjà (tableau ci-dessous, relevé dans `crates/mementum-schema/src/blocks.rs` et `inline.rs`) ; ce plan rend ces blocs **utilisables**, il ne les crée pas ; un onglet resté sur le client d'avant affiche déjà chacun d'eux (un cadre gris) et n'en supprime aucun — coût si faux : ce qu'une prop de plus aurait permis manque (titre et image d'un signet, taille et type d'un fichier dans le bloc, légende d'un callout) ; le jour où il la faut, c'est le schéma 2, avec ce qu'il entraîne.

| Bloc ou contenu | Dans le schéma 1 ? | Props | Proposé par l'éditeur aujourd'hui ? |
|---|---|---|---|
| `image`, `video`, `audio`, `file` | oui (BlockNote) | `url`, `name`, `caption`, `showPreview`, `previewWidth`… | oui, par adresse seulement (pas d'envoi) |
| `callout` | oui | `icon` (« 💡 »), `color` (`default`) | non |
| `equation` | oui | `latex` | non |
| `bookmark` | oui | `url` | non |
| `toc` | oui | aucune | non (cadre gris sans contenu) |
| `pageLink` | oui | `pageId`, `title` | non |
| `mention` (en ligne) | oui | `kind` (`page`, `user`, `date`), `id`, `label` | non |
| `math` (en ligne) | oui | `latex` | non |

**Migration (R243, récrit le 2026-11-13 après relecture du plan).** `Ruling R243:` les trois tables de la partie B (`files`, `attachments`, `favorites`) vont dans une migration à elles, `0004_files_favorites.sql`, écrite par la Task S2 et elle seule ; R139 est amendé : une migration par partie, `0003` figée à la clôture de la partie A (la Task 13b y retire `block_moves` avant), `0004` libre de changer jusqu'à la clôture de B — pourquoi : les migrations sont embarquées avec leur somme de contrôle (`sqlx::migrate!`, `crates/mementum-store/src/lib.rs`), et une base où `0003` est déjà appliquée ne s'ouvre plus si ce fichier change, sans copie préalable puisque rien n'y est « en attente » ; l'instance d'Alex sert à la recette de la partie A, et doit pouvoir passer ensuite à B par une migration ordinaire, avec sa copie `avant-0004.bak` — coût si faux : une dizaine d'assertions mécaniques à porter de trois à quatre migrations (`tests/migration.rs`, `tests/backup.rs`) et un test neuf, `lot3a_database_gains_files_and_favorites` ; une base ouverte par un binaire intermédiaire de la partie B refuse de démarrer si `0004` change ensuite, d'où la règle : aucun binaire de la partie B n'ouvre une instance que quelqu'un regarde avant sa clôture.

**Fichiers : où, et sous quel nom (R244).** La conception a tranché : « stockage adressé par contenu (hash SHA-256), donc dédoublonné » (§5.8), sur le disque local (§5.2), avec `files(hash, size, mime, storage_key)` et `attachments(node_id, file_hash)` (§5.3). `Ruling R244:` un fichier est gardé sous `<data_dir>/files/<2 premiers caractères hexadécimaux du hash>/<les 62 autres>`, écrit d'abord dans `<data_dir>/files/tmp/<UUIDv7>` ; `storage_key` n'existe pas (la clé se calcule du hash) ; une **attache** (`attachments`, un UUIDv7) dit qu'une page porte un fichier, sous un nom d'affichage ; il n'y a pas d'envoi « par hash » : qui attache des octets les a envoyés, et la réponse est la même que le contenu soit déjà connu ou non — pourquoi : **aucun nom venu d'un client n'entre dans un chemin** ; sans envoi par hash, le dédoublonnage ne dit à personne qu'un fichier existe déjà sur l'instance — coût si faux : `storage_key` reviendra avec S3 (P5) ; un même fichier envoyé deux fois coûte deux envois, pas deux copies.

**Fichiers : droits et durée de vie (R245, précisé le 2026-11-13 après relecture du plan).** `Ruling R245:` un fichier appartient à **la page où il a été envoyé** : le lire demande `read` sur elle, l'envoyer `edit` ; son adresse nomme la page ; il vit tant que la page existe, même si plus aucun bloc ne le nomme. Page dont les droits changent : la lecture suivante est jugée sur les droits du moment. Corbeille : il se lit, rien ne s'y envoie (`409 trashed`). Page supprimée pour de bon : ses attaches partent avec elle, et le contenu quitte le disque quand plus aucune page ne l'attache. **Rien d'autre ne retire un contenu du disque**, hormis celui qu'un envoi vient de poser lui-même et n'a pas pu attacher : au démarrage, seul `tmp/` est vidé, et un contenu que la base ne connaît pas reste, compté dans le journal. Bloc supprimé, annulé, ou revenu par une version restaurée : le fichier n'a pas bougé. Aucune route ne liste les attaches d'une page : un fichier dont le bloc est parti ne se retrouve que par son adresse, que seul l'historique (lu à partir de « Peut modifier », décision de la relecture finale de la partie A) montre encore. **Pas d'URL signées dans ce lot** — pourquoi : §5.3 attache un fichier à une page ; un bloc revient par l'historique (cent versions), et supprimer le fichier avec le bloc le perdrait ; restaurer la copie d'avant migration est une procédure du README, et un dépôt purgé au démarrage d'après une base revenue en arrière perdrait sans retour tous les fichiers envoyés depuis ; une URL signée est un jeton au porteur, écrit dans le document, qui servirait encore le fichier à qui vient de perdre l'accès, alors que le cookie de session suffit à un navigateur — coût si faux : les fichiers retirés d'une page pèsent jusqu'à sa suppression (ménage et quotas au lot 7) ; un contenu posé puis jamais attaché (arrêt du serveur entre les deux) reste sur le disque jusqu'au ménage du lot 7 ; un téléchargement en cours finit même si le droit tombe pendant qu'il passe (fenêtre acceptée, à écrire en §5.6) ; chaque image demande un contrôle des droits (servi par le cache des accès) ; les URL signées de §5.8 restent à faire, avec S3.

**Fichiers : ce qui est servi, et comment (R246, précisé le 2026-11-13 après relecture du plan).** `Ruling R246:` **toute réponse qui porte un fichier est un téléchargement** : `Content-Disposition: attachment` pour tous les types, `X-Content-Type-Options: nosniff`, `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox`, `Cross-Origin-Resource-Policy: same-origin`, `Accept-Ranges: bytes`. Le nom va dans `Content-Disposition` sous deux formes : `filename="…"`, le nom nettoyé où tout caractère hors de l'ASCII imprimable, `"`, `\` et `%` devient `_` ; `filename*=UTF-8''…`, le nom nettoyé, encodé en pour-cent hors des `attr-char` de la RFC 5987. Le type est **lu par le serveur dans les premiers octets**, à l'envoi, par une table fermée ; tout le reste est `application/octet-stream` ; le type déclaré par le client n'est jamais lu. La page ne montre un fichier « en ligne » que comme sous-ressource d'un bloc image, vidéo ou audio (`<img>`, `<video>`, `<audio>` ne suivent pas `Content-Disposition`). Cache : `Cache-Control: private, no-cache` et `ETag: "<hash>"`, donc une revalidation, et un contrôle des droits, à chaque usage ; un `Range` simple est servi (`206`), pour la vidéo. Quand une requête porte les deux, `If-None-Match` est jugé d'abord (RFC 9110, §13.2.2) : s'il nomme l'`ETag`, `304`, quel que soit `Range` ; un `If-Range` qui ne nomme pas l'`ETag` fait ignorer `Range` (`200`). Le `304` et le `206` portent les mêmes en-têtes que le `200`. **Vérifié par un essai le 2026-11-13** (Chromium de Playwright, serveur jetable, même origine) : sous ces en-têtes, une image s'affiche, un son et une vidéo se chargent, se déplacent par une plage et jouent ; au rechargement l'image repart avec `If-None-Match` ; un HTML à script ouvert comme document, ou dans un cadre, est téléchargé ou laissé vide, sans s'exécuter ; Chromium envoie bien `Range` et `If-None-Match` ensemble pour un média — pourquoi : un HTML ou un SVG ouvert comme document depuis l'origine de l'application y exécute son script, avec la session de qui l'ouvre ; rien n'étant jamais un document, la sûreté ne dépend ni de la table ni d'un oubli dans une liste ; le bac à sable est la seconde barrière — coût si faux : un SVG n'est pas affiché comme image et un PDF n'est pas lu dans le navigateur (blocs fichier, à télécharger) ; « ouvrir l'image dans un onglet » la télécharge ; lever le SVG plus tard est une ligne dans la table (`image/svg+xml`), les deux barrières restant ; le contenu d'un fichier reste dans le cache disque du navigateur après la déconnexion (il n'est jamais resservi sans revalidation, donc sans contrôle des droits) ; Chromium affiche une image d'après ses octets, même servie `application/octet-stream` (mesuré) : passer en bloc fichier ce qui est hors de la table (C8) est un choix du client, pas une barrière ; hors de Chromium, rien n'est mesuré.

| Premiers octets | Type servi |
|---|---|
| `89 50 4E 47 0D 0A 1A 0A` | `image/png` |
| `FF D8 FF` | `image/jpeg` |
| `GIF87a`, `GIF89a` | `image/gif` |
| `RIFF` ‥ `WEBP` (octets 8 à 11) | `image/webp` |
| `RIFF` ‥ `WAVE` | `audio/wav` |
| octets 4 à 7 `ftyp`, marque (octets 8 à 11) `isom`, `iso2` à `iso6`, `mp41`, `mp42`, `avc1`, `M4V ` ou `dash` | `video/mp4` |
| octets 4 à 7 `ftyp`, marque `M4A ` ou `M4B ` | `audio/mp4` |
| `1A 45 DF A3` | `video/webm` |
| `ID3`, `FF FB`, `FF F3`, `FF F2` | `audio/mpeg` |
| `OggS` | `audio/ogg` |
| `%PDF-` | `application/pdf` |
| tout le reste (SVG, HTML, texte, archives, toute autre marque `ftyp` : `qt  `, `heic`, `avif`…) | `application/octet-stream` |

**Fichiers : forme de l'envoi, bornes, dépendances (R247, précisé le 2026-11-13 après relecture du plan).** `Ruling R247:` un envoi est **une requête**, `POST /api/v1/pages/{id}/files?name=<nom>`, dont le corps est le fichier brut, lu par morceaux et écrit sur le disque en calculant son hash, jamais gardé en mémoire ; pas de reprise : un envoi interrompu ne laisse rien. Bornes : 100 Mio par fichier par défaut (`MEMENTUM_MAX_FILE_MB`, option `--max-file-mb`, de 1 à 4 096 ; une valeur invalide arrête le démarrage en la nommant), refusé sur `Content-Length` avant de lire, puis en cours de lecture (`413 file_too_large`) ; un envoi sans octet pendant 30 s est abandonné : la connexion est fermée sans réponse, et c'est journalisé ; un fichier de zéro octet est accepté. Le nom n'est qu'un libellé : sans caractère de contrôle ni de direction (U+061C, U+200E, U+200F, U+202A à U+202E, U+2066 à U+2069), `/` et `\` remplacés par `_`, espaces de bord retirées, 255 caractères au plus, « fichier » s'il n'en reste rien. Dépendances directes de `mementum-server`, toutes déjà dans `Cargo.lock` : `http-body-util` (lire un corps trame par trame), `http-body` et `bytes` (servir par morceaux), fonctions `fs` et `io-util` de tokio (le dépôt s'écrit en `async` : aucun appel bloquant sur les fils qui relaient les frappes) — pourquoi : un corps brut évite un analyseur `multipart` ; une reprise demande un état d'envoi côté serveur, que rien d'autre n'utilise — coût si faux : sur un réseau qui coupe, un gros fichier repart de zéro ; **reporté au lot 7, avec R54 et R121 : quota par espace, envois simultanés par compte, débit, nombre d'attaches par page** ; d'ici là, qui peut modifier une page, invité compris, peut remplir le disque, et un disque plein arrête toute écriture de l'instance, pour tout le monde : n'inviter que des personnes de confiance. **Accepté comme R121 par le contrôleur le 2026-11-13, à confirmer par le propriétaire** ; la tâche D l'écrit au README.

**Fichiers : le lien entre un bloc et son fichier (R248).** `Ruling R248:` le bloc porte l'adresse du fichier dans sa prop `url`, relative à l'instance : `/api/v1/pages/<pageId>/files/<fileId>`, et son libellé dans `name` ; le MMD ne change pas d'un octet (`![légende](url)`, `<video src>`, `<audio src>`, `<file src name>`, §6.2). Une adresse externe (`https://…`) reste permise, comme depuis le lot 1. `PUT /mmd` ne vérifie pas et ne va pas chercher une adresse : c'est une prop. Un agent envoie un fichier par la même route que le navigateur, puis écrit le bloc en MMD ; il le lit par son adresse — pourquoi : une adresse relative vaut quel que soit l'hôte par lequel on atteint l'instance (boucle locale, Tailscale, adresse publique) ; le document ne donne aucun droit, c'est la route qui les juge — coût si faux : une adresse externe fait charger au lecteur une image chez un tiers, qui voit son adresse IP (inchangé depuis le lot 1 ; CSP au lot 7) ; un bloc peut nommer un fichier que son lecteur ne peut pas lire (R249).

**Fichier d'une autre page (R249).** `Ruling R249:` un bloc collé dans une autre page garde l'adresse de sa première page ; le client, après un collage, fait **recopier l'attache** dans la page où il colle (`POST /api/v1/pages/{id}/files/copies`, qui demande `edit` ici et `read` là-bas, sans renvoyer un octet), puis récrit `url` ; la duplication d'une page le fait d'elle-même, dans sa transaction — pourquoi : sans cela, qui colle voit son image, pas ses lecteurs, et elle disparaît avec la première page — coût si faux : hors du collage (`PUT /mmd` d'un agent, glisser d'un onglet à l'autre, annulation), le bloc reste lié à sa première page ; le bloc dit alors « fichier indisponible » à qui ne la lit pas ; entre le contrôle (`read` là-bas) et l'écriture de l'attache, quelques millisecondes : une fenêtre acceptée, comme pour une création de page (à écrire en §5.6).

**Signet (R250).** `Ruling R250:` le signet montre l'hôte et l'adresse, sans aperçu distant : le serveur ne va chercher aucune adresse — pourquoi : aller lire une adresse choisie par un utilisateur depuis le serveur ouvre son réseau (routeur, services de la boucle locale, métadonnées d'un hébergeur) à qui peut modifier une page ; il y faut un client HTTP (une dépendance), un cache, des plafonds ; et le registre ne donne au signet que `url` : garder l'aperçu dans le document demanderait le schéma 2 — coût si faux : « signet (aperçu de lien) » du §4.5 n'est livré qu'à moitié, et le propriétaire le lit ici.

**Mentions, `[[`, lien de page (R251).** La conception a tranché la forme : `<mention type="page" id="…">Spécification</mention>`, `<page id="…">titre</page>` (§6.2) : **le libellé est dans le document**. `Ruling R251:` une mention ou un lien de page copie le titre, le nom ou la date au moment où on le pose ; à l'affichage, le titre à jour remplace le libellé **seulement pour qui voit la page** (il vient de l'arbre, déjà filtré par les droits) ; `@` propose les pages de l'arbre du lecteur, les personnes (pour un membre : les membres et invités de l'espace, par la route existante ; pour un invité : les comptes qui ont la page ouverte), et une date ; `[[` propose les pages, et « Créer la page » en fait une sous-page de la page courante ; une mention ne prévient personne et ne donne aucun droit — pourquoi : le libellé copié est du contenu que l'auteur a choisi d'écrire, comme s'il l'avait tapé ; aucun titre ne sort par une route ; « qui a accès à une page » ne se lit qu'avec l'accès complet (lot 2) — coût si faux : qui ne voit pas la page lit un titre qui a pu changer ; un invité ne mentionne que les présents. **Forme gardée par le contrôleur le 2026-11-13, à confirmer par le propriétaire** : qui lit la page où est la mention lit le titre copié, même sans voir la page mentionnée (Notion y montre « pas d'accès ») ; coût si faux : mentionner une page privée dans une page d'équipe en montre le titre à toute l'équipe, sans que rien le dise à qui mentionne ; la tâche D l'écrit au README et à la recette.

**Dupliquer une page (R252, récrit le 2026-11-13 après relecture du plan).** `Ruling R252:` `POST /api/v1/pages/{id}/duplicate` copie la page et ses sous-pages **que `access.rs` montre à la personne**, hors corbeille (100 pages au plus, sinon `422 subtree_too_large`) : contenus relus puis récrits dans des documents neufs, **identifiants de blocs neufs**, attaches recopiées et adresses récrites ; titre de la racine suivi de « (copie) » ; ni commentaires, ni versions, ni favoris, ni accès nommés. Il faut `read` sur la page, hors corbeille. **Où va la copie** (choix pris par le contrôleur le 2026-11-13, à confirmer par le propriétaire) : sous la page parente, juste après la page, si la personne voit la page parente et peut y créer (`CreateChild`), et la copie en hérite ; sinon, pour un membre, **à la racine, dans son « Privé »** (`(user:<lui>, full)`) ; sinon (un invité), `403`. Les droits sont revérifiés **juste avant d'écrire**, une fois les pages lues et les documents construits (R89) — pourquoi : une copie ne partage rien que la personne n'ait choisi de partager ; c'est la règle de « Nouvelle page » ; refuser la sous-page d'une page parente qu'on ne peut pas modifier, alors que la page de racine entière se copie dans « Privé », sous-page comprise, n'avait pas de sens ; des identifiants repris se croiseraient dès le premier bloc recollé d'une page à l'autre ; construire cent documents prend des secondes, et un contrôle fait avant laisserait copier après un accès retiré ; prendre le sous-arbre dans le calcul des accès, pas dans le store, tient encore le jour où une sous-page pourra être plus fermée que sa page parente (P3) — coût si faux : la copie d'une page d'équipe est à repartager à la main (Notion la laisse dans sa section) ; un lien interne de la copie pointe encore vers l'original ; au-delà de 100 pages, on duplique par morceaux ; une page dupliquée naît sans version : son contenu de départ n'est dans aucune (l'original le porte).

**Dupliquer un bloc (R253).** `Ruling R253:` Ctrl+D (Cmd+D) et « Dupliquer » dans le menu de la poignée insèrent, juste après le ou les blocs choisis, leur copie **sans aucun identifiant** : BlockNote en donne de neufs, à chaque profondeur — pourquoi : mesuré (plus bas) : une copie insérée ne touche aucun élément existant, et une frappe faite au même instant dans l'original ou dans le bloc suivant est gardée ; avec l'`id` de l'original, le document porte deux blocs du même `id`, et le dédoublonnage du serveur supprime le second (`crates/mementum-crdt/src/dedupe.rs`) — coût si faux : Ctrl+D est le raccourci « favori » du navigateur ; il n'est pris que le focus dans un éditeur modifiable.

**Favoris (R254, récrit le 2026-11-13 après relecture du plan).** `Ruling R254:` un favori est une ligne `(compte, page)` ; `PUT` et `DELETE /api/v1/pages/{id}/favorite` (`read`, sans effet s'il y est déjà) ; `GET /api/v1/spaces/{id}/favorites` rend les pages favorites **que le compte voit, hors corbeille**, dans l'ordre où il les a ajoutées ; **poser ou retirer un favori n'avance ni `spaces.rev` ni `spaces.lsn`** : l'onglet qui le pose relit ses favoris aussitôt, les autres onglets du compte à leur prochaine lecture de l'arbre (retour au premier plan, changement dans l'espace) (choix pris par le contrôleur le 2026-11-13, à confirmer par le propriétaire) — pourquoi : une liste filtrée par `access.rs` ne peut pas nommer une page devenue invisible ; `lsn` dit « la barre latérale de l'espace a changé » (R133) et fait relire l'arbre à tous ses onglets, alors qu'un favori ne change que la barre de son auteur ; l'avancer donnait au compte le moins doté d'un espace (un invité en « Peut lire ») le moyen de faire relire l'arbre à tout le monde une fois par seconde, sans fin, et comptait des gestes privés dans un numéro que tous reçoivent — coût si faux : un favori posé dans un onglet ne paraît dans un autre onglet du compte, resté visible, qu'à sa prochaine lecture de l'arbre ; si le propriétaire veut l'effet immédiat, il y faut un signal par compte, pas la version de l'espace.

**Récents (R255).** `Ruling R255:` les pages récentes sont gardées **dans le navigateur** (`localStorage`, par compte et par espace, vingt identifiants, rien d'autre, **effacés à la déconnexion**), et la barre latérale montre les cinq premières que l'arbre contient — pourquoi : un journal « qui a ouvert quoi, quand » tenu par le serveur est une donnée que la conception ne demande qu'en P5 (analytique de page, avec opt-out) ; des identifiants filtrés par l'arbre ne laissent rien voir ; sur un navigateur partagé, ceux d'un compte ne restent pas lisibles après sa déconnexion — coût si faux : les récents ne suivent pas d'un navigateur à l'autre, et repartent de zéro après une déconnexion ; la conception ne dit pas où les montrer, et ce plan choisit la barre latérale, sous « Favoris ».

**Fil d'Ariane (R256).** `Ruling R256:` le fil d'Ariane de la barre du haut se calcule dans le client, sur l'arbre : les pages parentes **visibles**, la page en dernier ; au-delà de quatre niveaux, la première, « … » (un menu), et les deux dernières — pourquoi : l'arbre est déjà filtré ; pour un invité, le fil commence à la plus haute page qu'il voit — coût si faux : une page hors de l'arbre (corbeille, arbre pas encore lu) ne montre que son titre.

**Reports du lot 2 (R257, récrit le 2026-11-13 après relecture du plan).** Le registre du lot 2 n'en garde qu'une ligne chacun (« Reportés au lot suivant »), et les rapports de sa relecture ne sont pas dans le dépôt : ce qui suit est relu dans le code, et **chaque tâche commence par reproduire le défaut**. `Ruling R257:` M5 : l'écran « premier compte » (`info.setup`) relit `GET /api/v1/info` après toute réponse de son formulaire, échec réseau compris, et passe à la connexion si un compte existe désormais ; M7 : **une invitation qui ne donnerait rien au compte qui l'accepte n'est pas consommée**, « rien » se jugeant, pour une invitation de page, sur le **niveau effectif** du compte (`mementum_acl::chain_level`, dans la transaction), pas sur sa seule ligne propre : rien n'est alors écrit, ni ligne d'accès ni acceptation, et l'écran le dit (choix pris par le contrôleur le 2026-11-13, à confirmer par le propriétaire) ; M9 : un titre tapé et pas encore enregistré quand la page passe en lecture seule n'est plus montré comme le titre de la page : le champ revient au titre du serveur, le texte tapé est gardé dans un message, et il est **proposé** quand la page redevient modifiable (« Reprendre ce titre »), sans rentrer seul dans le champ : R210 tient (rien ne part sans une saisie d'ici) ; M11 : une invitation de page en attente porte le titre de sa page (`pageTitle`) ; m1 : l'inscription ouvre sa session dans la transaction qui crée le compte — pourquoi : M7 renverse l'« usage unique » de R47, tel que deux tests du lot 2 l'affirmaient (« consommée, même sans effet ») : qui teste son propre lien le tuait pour son destinataire ; la borne de R47 tient encore (un lien ne donne un droit qu'à un seul compte, puis ne sert plus) ; juger sur la ligne propre ne levait pas le cas qui a fait naître le report : l'auteur d'une invitation de page y a `full` le plus souvent par l'accès général ou par une page parente, sans ligne à son nom (`raise_level` ne lit que celle-ci), et consommait donc toujours son lien, en y gagnant une ligne ; M9 : R210 (relecture finale de la partie A) fait déjà suivre le titre distant à un champ en lecture seule, et dit que ce qui y a été tapé ne part plus — coût si faux : un lien accepté sans effet reste valable jusqu'à son échéance, pour quelqu'un d'autre ; un membre qui accepte une invitation à un niveau qu'il a déjà par l'accès général n'est plus nommé sur la page, et la perd comme les autres si elle est fermée aux membres ensuite ; deux tests du lot 2 changent de sens.

---

## Ce qui a été vérifié avant d'écrire

**Par un essai** (Vitest jetable, deux éditeurs BlockNote 0.57 liés à deux `Y.Doc`, un troisième pour le serveur, sur le modèle de `blockMoves.test.ts` ; **`y-prosemirror` 1.3.7 sans le correctif**, l'état de `main` le 2026-11-12). Chaque ligne est revérifiée par le test commité de la tâche nommée, qui dit sous quel `y-prosemirror` il mesure. Le correctif de la Task 13a ne joue que sur les listes d'enfants à identifiants (`blockGroup`, `columnList`, `column`) et retombe sur le code d'origine dès qu'un `id` manque ou se répète : toutes ces lignes sont attendues telles quelles avec lui, les trois limites comprises (frappe perdue au changement de type, frappe qui passe avant une mention insérée au milieu, deux conteneurs du même `id`).

| Geste | Ce qu'il écrit dans Yjs | Frappe d'un autre au même instant | Tâche |
|---|---|---|---|
| Insérer un ou des blocs après un bloc (`insertBlocks`, copies sans `id`, enfants compris) | des conteneurs neufs, d'`id` neufs ; aucun élément existant touché | gardée, dans l'original comme dans le bloc suivant | C1a, C5 |
| Insérer un bloc en lui donnant l'`id` d'un bloc vivant | **deux conteneurs du même `id`** dans le document | — | C5 (interdit) |
| Changer des props (`updateBlock` : couleur d'un callout, `latex`, `url` d'une image) | les attributs du même élément ; texte intact | gardée ; deux changements de la même prop : le dernier gagne | C1a, C2, C8 |
| Changer le type d'un bloc (`updateBlock({ type })` : paragraphe vide en callout par le menu « / », paragraphe en callout, image en fichier) | le conteneur reste, **l'élément de contenu est supprimé et recréé**, le texte recopié | **perdue** (famille de R39, déjà vraie pour « Transformer en titre ») | C1a, C8 |
| Insérer une mention au milieu d'un texte (`insertInlineContent`) | le texte d'avant reste, un élément `mention` suit, **le texte d'après est supprimé et recréé** dans un texte neuf | tapée après le point d'insertion : gardée, mais **elle atterrit avant la mention** | C4 |
| Insérer une mention, ou une formule, en fin ou en début de texte | un élément de plus ; le texte reste | gardée, en place | C2, C4 |

**Par lecture du code de BlockNote 0.57** (`node_modules/.pnpm/@blocknote+core@0.57.0…/dist`, `@blocknote+react@0.57.0…/dist`) :
- `uploadFile(file, blockId)` : appelé par le panneau de fichier (l'onglet « Envoyer » n'existe que si l'option est donnée) et par le dépôt ou le collage d'un fichier, qui insère d'abord un bloc du type que donne le type MIME (`props: { name }`), puis pose ce que rend `uploadFile` : une chaîne (l'`url`), ou un objet passé tel quel à `updateBlock` (donc `{ type, props }` permis). Un échec du dépôt est avalé sans message.
- `resolveFileUrl(url)` : appelé avant d'afficher une image, une vidéo ou un son, et par « Télécharger » de la barre de mise en forme (`window.open`, `javascript:` seul écarté).
- Le bloc `file` rend son nom dans un `<p>` de texte, sans lien : **dans un éditeur en lecture seule, rien ne permet de le télécharger**. L'attente d'un envoi affiche « Loading... », écrit en dur.
- Menu « / » par défaut : titres, citation, listes, paragraphe, code, séparateur, tableau, image, vidéo, audio, fichier, emoji. Aucun bloc maison. `SuggestionMenuController` accepte un déclencheur de plusieurs caractères (`[[`).
- `editor.onChange` donne, par `getChanges()`, la source de chaque changement (`local`, `paste`, `drop`, `yjs-remote` ; avec Yjs, une annulation porte `undo-redo`, pas `undo`).
- Aucune duplication de bloc dans BlockNote 0.57 (ni commande, ni raccourci).
- Le dépôt ou le collage d'un fichier sur un paragraphe **vide** ne l'insère pas : il change le type de ce paragraphe (`updateBlock`), qui garde son `id` (famille du changement de type).
- La barre de mise en forme traite tout bloc qui a une prop `url` en chaîne comme un bloc fichier (« Remplacer », « Supprimer », « Télécharger ») : le signet en a une (lu, non vérifié à l'écran).

**Par un essai de la relecture du plan** (2026-11-13 ; Chromium 158 de Playwright sous Windows, serveur jetable hors du dépôt, même origine, sans le mandataire de Vite ; un PNG, et un WAV pour le son comme pour la vidéo) : sous les en-têtes de R246, l'image s'affiche, le son et la vidéo se chargent, se déplacent à 100 s sur 120 par une requête `Range` (`206`) et jouent ; au rechargement l'image repart avec `If-None-Match` ; pour un média, `Range` et `If-None-Match` partent ensemble ; un HTML à script servi `application/octet-stream` est téléchargé quand on l'ouvre comme document, et reste vide dans un cadre, sans s'exécuter ; un SVG servi `application/octet-stream` ne s'affiche pas dans `<img>`, un PNG servi `application/octet-stream` s'y affiche.

**Pas vérifié, donc écrit en première étape de la tâche** : l'affichage d'une image et la lecture d'une vidéo sous les en-têtes de R246 hors de Chromium, et les codecs du Chromium de Playwright sous Linux, celui de la CI (C8) ; ce que BlockNote rend d'un collage de blocs venus d'une autre page (C9) ; la mention comme îlot non modifiable où un lien se suit (C4) ; les attributs du DOM où lire les titres (C3) ; le coût d'écrire 10 000 blocs dans un document neuf (S6) ; ce que voit un navigateur d'un refus arrivé pendant qu'il envoie encore (C8) ; ce que la barre de mise en forme offre sur un signet (C1b) ; ce qu'écrit le changement de la formule d'un contenu en ligne (C2) ; le poids d'une transaction de cent documents (S6).

---

## Structure des fichiers

| Fichier | Rôle | Tâche |
|---|---|---|
| `crates/mementum-store/migrations/0004_files_favorites.sql` (neuf) | `files`, `attachments`, `favorites` | S2 |
| `crates/mementum-store/src/files.rs`, `favorites.rs` (neufs) | attaches, ménage ; favoris | S2 |
| `crates/mementum-store/src/signup.rs`, `users.rs`, `invitations.rs` | session dans l'inscription ; invitation sans effet | S1 |
| `crates/mementum-store/src/duplicate.rs` (neuf) | copie d'un sous-arbre en une transaction | S6 |
| `crates/mementum-server/src/files.rs` (neuf) | dépôt sur disque, type lu dans les octets, nom d'affichage, `Content-Disposition` | S3 |
| `crates/mementum-server/src/routes/files.rs` (neuf) | envoi, lecture, limites ; puis recopie et ménage | S4a, puis S4b |
| `crates/mementum-server/src/routes/favorites.rs` (neuf) | favoris | S5 |
| `crates/mementum-server/src/routes/duplicate.rs` (neuf) | duplication d'une page | S6 |
| `crates/mementum-server/src/config.rs`, `guard.rs`, `error.rs` | `max_file_bytes` ; en-têtes de cache ; codes d'erreur | S4a (codes de la recopie : S4b) |
| `crates/mementum-server/tests/files_alloc.rs` (neuf) | un envoi et une lecture ne tiennent pas le fichier en mémoire | S4a |
| `apps/web/src/test/editors.ts`, `editor/gestures.test.ts` (neufs) | banc à deux éditeurs ; ce que chaque geste écrit | C1a, puis C2, C4, C5, C8 |
| `apps/web/src/editor/slashItems.tsx`, `editor/blocks/` (neufs) | menu « / », un fichier par bloc riche | C1b, C2, C3 |
| `apps/web/src/space/pageDirectory.ts` (neuf) | les pages que le compte voit, pour l'éditeur | C3 |
| `apps/web/src/mentions/` (neuf) | `@`, `[[`, dates | C4 |
| `apps/web/src/editor/duplicate.ts` (neuf) | Ctrl+D | C5 |
| `apps/web/src/page/Breadcrumb.tsx`, `space/recents.ts` (neufs) | fil d'Ariane, récents | C6 |
| `apps/web/src/space/useFavorites.ts` (neuf) | favoris | C7 |
| `apps/web/src/editor/files.ts`, `editor/blocks/FileBlock.tsx` (neufs) | envoi, adresses permises, téléchargement | C8, C9 |
| `apps/web/e2e/fixtures/` (neuf) | un PNG, un HTML nommé `.png`, une vidéo WebM | C8 |
| `spec/mmd/fixtures/rich-blocks.mmd`, `yjs/rich-blocks.bin`, `blocknote/rich-blocks.json` (neufs) | fidélité MMD des blocs riches | X2 |

---

## Piste S

### Task S1 : reports du lot 2, côté serveur (m1, M7, M11)

**Piste :** S. **Dépend de :** rien. **Livrable :** trois défauts reproduits par un test rouge, puis levés ; deux tests du lot 2 changent de sens, dits au registre.

**Files:**
- Modify: `crates/mementum-store/src/signup.rs`, `users.rs`, `invitations.rs`
- Modify: `crates/mementum-server/src/routes/auth.rs`, `routes/invitations.rs`, `routes/sharing.rs`, `types.rs`, `lib.rs` (tableau)
- Modify (commit `web :` à part, constructions littérales seulement) : les tests de `apps/web/src/settings/`, `share/` et `session/` que le typecheck casse
- Test: `crates/mementum-store/tests/signup.rs`, `tests/invitations.rs` ; `crates/mementum-server/tests/spaces.rs`, `tests/sharing.rs` (les tests d'inscription de `tests/auth.rs` restent verts tels quels)

**Interfaces:**
- Produces:
  - `pub struct NewSession { pub token_hash: [u8; 32], pub expires_at: i64 }` ; `Store::sign_up(&self, user: NewUser, invitation: Option<[u8; 32]>, first_space_name: &str, session: NewSession, now: i64) -> Result<SignedUp, StoreError>` ;
  - `Store::accept_invitation(&self, token_hash: [u8; 32], user_id: Uuid, now: i64) -> Result<(Invitation, bool), StoreError>` : le booléen dit que l'invitation a donné quelque chose ; faux, rien n'est écrit, et l'invitation **n'est pas** marquée acceptée. Pour une invitation de page, « ne donne rien » se juge sur le **niveau effectif** du compte, dans la transaction : il a déjà un rôle dans l'espace, et `mementum_acl::chain_level` sur la chaîne de la page, avec ce rôle, rend un niveau au moins égal (comme le partage, `crates/mementum-store/src/acl.rs`) ; pas sur sa seule ligne propre, que lit `raise_level` ;
  - `Accepted { spaceId, pageId, changed: bool }` ; `PendingInvitation { …, pageTitle: string | null }` (le titre de la page d'une invitation de page ; `null` pour une invitation d'espace).

- [ ] **Step 1 : m1, test du store, vu rouge.** `sign_up_that_cannot_open_its_session_creates_nothing` (`tests/signup.rs`) : un déclencheur `BEFORE INSERT ON sessions` qui lève, posé par une connexion à côté du store ; `sign_up` échoue ; aucun compte, l'adresse est libre, aucun espace, l'invitation donnée attend toujours. Témoin : `sign_up_opens_its_session` (`session_user(token_hash)` rend le compte). Aujourd'hui le compte existe sans session : c'est le défaut.
- [ ] **Step 2 : m1, écrire.** `sign_up` insère la session dans sa `WriteTx` ; `register` (`routes/auth.rs`) tire le jeton avant d'appeler le store, et n'appelle plus `open_session`. La fermeture de la session portée (R90) reste après le commit. `Store::create_session` ne sert plus qu'aux tests : le dire dans son commentaire.
- [ ] **Step 3 : M7, tests vus rouges.** Store, `invitation_that_gives_nothing_is_not_consumed` (`tests/invitations.rs`, à la place de l'assertion « Même sans effet, l'invitation est consommée ») : pour un membre invité au même rôle ou plus bas, et pour une page où le compte a déjà ce niveau ou plus, dans trois cas : par une ligne à son nom sur la page ; par une page parente ; par l'accès général `members`, pour un membre et pour le propriétaire. Dans tous : `Ok((_, false))`, aucune ligne `acl` de plus, `rev` inchangé, `invitation_by_token` la rend encore ; puis un autre compte l'accepte : `Ok((_, true))`, consommée. Témoin : un niveau plus haut que le niveau effectif écrit sa ligne et consomme. Serveur, la boucle « Consommée, même sans effet » de `tests/spaces.rs` devient `invitation_without_effect_stays_valid` : la réponse porte `"changed": false`, et le destinataire l'accepte ensuite (`"changed": true`), une seule fois. Et `page_invitation_tried_by_its_author_stays_valid` (`tests/sharing.rs`) : l'auteur d'une invitation sur une page d'« Espace », qui n'y a aucune ligne à son nom, l'accepte lui-même : `"changed": false`, les accès de la page n'ont pas bougé (`GET …/access`), l'invitation attend toujours.
- [ ] **Step 4 : M7, écrire.** `accept` (`invitations.rs`) juge d'abord si l'invitation donnerait quelque chose (rôle : comme aujourd'hui ; page : le niveau effectif, par `chain_level`), et n'écrit la ligne, l'acceptation et la révision que dans ce cas ; à l'inscription (`sign_up`), un compte neuf reçoit toujours quelque chose : rien ne change.
- [ ] **Step 5 : M11, test vu rouge, puis écrire.** `pending_page_invitation_names_its_page` (`tests/sharing.rs`) : `GET /api/v1/spaces/{id}/invitations`, par qui peut partager la page, rend `"pageTitle": "Plan"` ; `GET /api/v1/pages/{id}/access` aussi ; une invitation d'espace : `null`. Témoin, déjà vrai et réaffirmé : un admin qui ne peut pas partager la page ne voit pas l'invitation, donc pas son titre. `pending_invitation(invitation, page_title)` prend le titre que l'appelant a lu (`Store::get_node`).
- [ ] **Step 6 : types et client.** `UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed` ; puis `corepack pnpm -C apps/web typecheck` et Vitest, rendus verts par les seuls littéraux à compléter (`changed: true`, `pageTitle: null`).
- [ ] **Step 7 : relancer** `cargo test --workspace --no-fail-fast`. Commits : `store : une inscription ouvre sa session dans sa transaction (m1)`, `store : une invitation qui ne donne rien n'est pas consommée (M7)`, `serveur : une invitation de page en attente porte le titre de sa page (M11)`, `web : les types d'invitation du serveur`. Puis l'en-tête de `routes/invitations.rs` et de `lib.rs`, et le registre : R257 pour sa part, les deux tests qui changent de sens, ce que le niveau effectif change (un membre déjà servi par l'accès général n'est plus nommé sur la page), et, pour la tâche D, la phrase de la conception §4.4 à reprendre (« Il sert une fois » : une fois qu'il a donné quelque chose).

### Task S2 : store, fichiers et favoris

**Piste :** S. **Dépend de :** rien (R243 : `0004`). **Livrable :** les trois tables, dans une migration à elles, migrées depuis une base de la partie A comme depuis une base du lot 2, et leurs accès.

**Files:**
- Create: `crates/mementum-store/migrations/0004_files_favorites.sql`, `crates/mementum-store/src/files.rs`, `favorites.rs`
- Modify: `crates/mementum-store/src/lib.rs` ; **pas** `0003_collaboration.sql`
- Test: `crates/mementum-store/tests/migration.rs`, `tests/backup.rs`, `tests/common/mod.rs` (`LOT3A`), `tests/files.rs`, `tests/favorites.rs` (neufs)

```sql
-- Un contenu, rangé sur le disque sous son SHA-256 (R244). Partagé par toutes les attaches du
-- même contenu ; supprimé par le ménage quand plus aucune ne le nomme.
CREATE TABLE files (
    hash BLOB PRIMARY KEY NOT NULL,     -- SHA-256, 32 octets
    size INTEGER NOT NULL,
    mime TEXT NOT NULL,                 -- lu par le serveur dans les premiers octets (R246)
    created_at INTEGER NOT NULL
);

-- Une page porte un fichier, sous un nom d'affichage (R245).
CREATE TABLE attachments (
    id TEXT PRIMARY KEY NOT NULL,
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    file_hash BLOB NOT NULL REFERENCES files(hash),
    name TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL
);
CREATE INDEX attachments_by_node ON attachments (node_id, created_at);
CREATE INDEX attachments_by_file ON attachments (file_hash);

CREATE TABLE favorites (
    user_id TEXT NOT NULL REFERENCES users(id),
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, node_id)
);
CREATE INDEX favorites_by_node ON favorites (node_id);
```

**Interfaces:**
- Produces:
  - `pub struct Attachment { pub id: Uuid, pub node_id: Uuid, pub hash: [u8; 32], pub size: i64, pub mime: String, pub name: String }` ;
  - `pub struct NewAttachment<'a> { pub id: Uuid, pub node_id: Uuid, pub hash: [u8; 32], pub size: i64, pub mime: &'a str, pub name: &'a str, pub created_by: Uuid }` ;
  - `Store::attach_file(&self, new: NewAttachment<'_>, now: i64) -> Result<Attachment, StoreError>` : la ligne `files` (si elle manque) et l'attache, en une transaction ; `NotFound` pour une page inconnue, sans rien écrire ;
  - `Store::attachment(&self, node_id: Uuid, id: Uuid) -> Result<Option<Attachment>, StoreError>` : `None` si l'attache n'est pas de cette page ;
  - `Store::copy_attachment(&self, from_node: Uuid, from_id: Uuid, id: Uuid, to_node: Uuid, by: Uuid, now: i64) -> Result<Option<Attachment>, StoreError>` : une attache neuve, même contenu, même nom, la source lue et la copie écrite dans **la même** transaction d'écriture (un ménage ne passe pas entre les deux) ; `None` si la source n'est pas de `from_node` ;
  - `Store::sweep_files(&self) -> Result<Vec<[u8; 32]>, StoreError>` : supprime les lignes `files` qu'aucune attache ne nomme, et rend leurs hash ;
  - `Store::known_files(&self) -> Result<Vec<[u8; 32]>, StoreError>` ;
  - `Store::set_favorite(&self, user_id: Uuid, node_id: Uuid, on: bool, now: i64) -> Result<bool, StoreError>` : `true` si quelque chose a changé ; **ni `bump_rev` ni `bump_lsn`** (R254), rien n'est annoncé sur `changes()` ; `NotFound` pour une page inconnue ;
  - `Store::favorites(&self, user_id: Uuid, space_id: Uuid) -> Result<Vec<Uuid>, StoreError>`, par `(created_at, node_id)`.

- [ ] **Step 1 : tests de la migration, vus rouges.** Neuf, `lot3a_database_gains_files_and_favorites` (`migration.rs`) : une base de la partie A, bâtie par `database_with` avec `LOT3A` (`0001` à `0003`, constante neuve de `tests/common/mod.rs`) et remplie (un fil de commentaires, une version), s'ouvre : `0004` est appliquée, tout ce qui y était reste à l'identique, valeur pour valeur ; les trois tables existent, vides, et leurs clés étrangères visent ce qu'il faut (`attachments` : `nodes`, `files`, `users` ; `favorites` : `users`, `nodes`) ; la copie d'avant migration s'appelle `avant-0004.bak` ; une seconde ouverture ne migre plus rien. Assertions à porter de trois à quatre migrations, relevées par une recherche de `_sqlx_migrations` dans `crates/` : les listes `[1, 2, 3]` et les comptes de `tests/migration.rs` et de `tests/backup.rs` ; `ALL_TABLES` gagne les trois tables (`COLLABORATION_TABLES`, celles de `0003`, ne bouge pas). Le test qui supprime une page et ses lignes filles (`deleting_a_page_deletes_its_threads_versions_and_moves`, sous le nom que la Task 13b lui laisse) gagne les attaches et les favoris de la page (la ligne `files` reste : c'est au ménage).
- [ ] **Step 2 : tests des fichiers, vus rouges** (`tests/files.rs`) : `same_content_attached_twice_is_one_file_and_two_attachments` ; `attachment_is_read_only_through_its_page` (`attachment(autre_page, id)` rend `None`) ; `attach_to_a_missing_page_writes_nothing` (pas même la ligne `files`) ; `copy_attachment_shares_the_file` (attache neuve sur l'autre page, même hash, même nom ; `None` et rien d'écrit si la source n'est pas de la page nommée) ; `sweep_returns_only_what_no_page_keeps` (deux pages attachent le même contenu ; l'une est supprimée : le ménage ne rend rien, la ligne reste ; l'autre l'est : il rend ce hash, une fois, et la ligne est partie).
- [ ] **Step 3 : tests des favoris, vus rouges** (`tests/favorites.rs`) : `favorite_twice_is_one_favorite` (premier appel : `true` ; second : `false`, rien d'écrit) ; `favorite_advances_neither_rev_nor_lsn` (poser, puis retirer : `rev` et `lsn` de l'espace inchangés, rien sur `changes()` ; témoin : un renommage fait ensuite avance `lsn` d'un pas et s'annonce) ; `favorites_are_per_account_in_the_order_added` ; `favorites_of_a_deleted_page_are_gone` ; `removing_a_favorite_that_is_not_there_writes_nothing`.
- [ ] **Step 4 : écrire**, relancer `cargo test -p mementum-store`. L'en-tête de `0004` dit les trois tables, leurs Rulings, et qu'elle peut changer jusqu'à la clôture de la partie B ; chaque clé étrangère suivie a un index qui commence par elle, comme dans `0003`.
- [ ] **Step 5 : commits** `store : tables des fichiers et des favoris, migration 0004 (R243)`, `store : attaches d'une page et ménage des fichiers (R244, R245)`, `store : favoris d'un compte (R254)`. Registre.

### Task S3 : serveur, dépôt des fichiers

**Piste :** S. **Dépend de :** rien (pas même S2). **Livrable :** un module sans route, jugé sur ses tests unitaires : on y écrit des octets, on y relit un contenu par son hash, rien d'autre ne nomme un fichier.

**Files:**
- Create: `crates/mementum-server/src/files.rs`
- Modify: `crates/mementum-server/src/lib.rs` (`mod files;`), `crates/mementum-server/Cargo.toml` (R247)

**Interfaces:**
- Produces (`pub(crate)`) :
  - `FileStore::open(data_dir: &Path) -> io::Result<FileStore>` (synchrone : au démarrage) : crée `files/` et `files/tmp/`, vide `tmp/`, **et rien d'autre** : aucun contenu n'est supprimé au démarrage (R245) ;
  - `FileStore::strangers(&self, known: &[[u8; 32]]) -> io::Result<(u64, u64)>` (synchrone) : le nombre et le poids des contenus du dépôt que la base ne connaît pas, pour le journal du démarrage ; ne lit que les entrées de la forme `<2 hex>/<62 hex>`, ne touche à rien ;
  - le reste est en `async` (`tokio::fs`, R247) : `async fn begin(&self) -> io::Result<Upload>` ; `async fn Upload::write(&mut self, chunk: &[u8]) -> io::Result<()>` ; `Upload::len(&self) -> u64` ; un `Upload` lâché sans `finish` supprime son fichier temporaire (dans `Drop`, par un appel synchrone, le seul) ;
  - `async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()>` : un seul verrou pour « poser un contenu puis l'attacher » et pour le ménage ;
  - `async fn Upload::finish(self, guard: &MutexGuard<'_, ()>) -> io::Result<Stored>`, `pub struct Stored { pub hash: [u8; 32], pub size: u64, pub mime: &'static str, pub fresh: bool }` : vide le fichier temporaire sur le disque (`sync_all`), puis le déplace à sa place s'il n'y a rien (`fresh` vrai) ; si un fichier y est déjà : de même taille, le temporaire est supprimé ; d'une autre taille (contenu tronqué par une coupure), le temporaire **le remplace**, et c'est journalisé ;
  - `async fn open_file(&self, hash: &[u8; 32]) -> io::Result<tokio::fs::File>` ; `async fn remove(&self, guard: &MutexGuard<'_, ()>, hash: &[u8; 32]) -> io::Result<()>` ;
  - `sniff(head: &[u8]) -> &'static str` (table de R246) ; `display_name(raw: &str) -> String` (règle de R247) ; `content_disposition(name: &str) -> HeaderValue` (les deux formes de R246, pour un nom déjà nettoyé) ; `hex(hash: &[u8; 32]) -> String`.

- [ ] **Step 1 : tests, vus rouges.**
  - `type_is_read_in_the_bytes` : chaque ligne de la table de R246, chaque marque `ftyp` nommée ; et `application/octet-stream` pour `<svg xmlns=…`, `<!doctype html>`, `<script>`, zéro octet, trois octets, les sept premiers octets d'un PNG, `RIFF` suivi d'autre chose, `ftyp` de marque `qt  `, `heic`, `avif` et `zzzz`.
  - `content_disposition_is_well_formed` : `été "2026".pdf` (le guillemet et les accents deviennent `_` dans `filename="…"`, le nom entier est encodé dans `filename*`) ; `a;b%.png` ; 255 caractères hors ASCII (l'en-tête reste sous 2 Kio et se construit) ; `fichier`.
  - `display_name_is_only_a_label` : `../../etc/passwd` → `.._.._etc_passwd` ; `C:\Windows\x` → `C:_Windows_x` ; `photo\u{202E}gnp.exe` → `photognp.exe` ; un saut de ligne retiré ; 300 caractères → 255 ; vide, ou que des espaces → `fichier`.
  - `same_bytes_are_kept_once_under_their_hash` : « abc » envoyé deux fois : un seul fichier, à `files/ba/7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.
  - `abandoned_upload_leaves_nothing` (`Upload` lâché : `tmp/` vide, dépôt vide).
  - `opening_empties_tmp_and_keeps_what_the_database_does_not_know` : un dépôt qui porte un contenu connu, un contenu inconnu, un fichier d'un autre nom posé là par quelqu'un, et un reste dans `tmp/` ; après `open`, seul `tmp/` est vide ; `strangers` compte un contenu et son poids, et ne compte pas le fichier d'un autre nom.
  - `a_truncated_content_is_replaced_by_the_next_upload` : le fichier d'un contenu est tronqué à la main ; le même contenu renvoyé le remet entier (`fresh` faux) ; témoin : renvoyé sur un contenu entier, rien n'est récrit.
  - `finish_and_sweep_never_cross` : le test tient le verrou, lance un ménage simulé qui veut supprimer le hash (il attend), finit un envoi du même contenu et l'ajoute à ce que le ménage relira, rend le verrou : le fichier est toujours là. Sans attente fixe : c'est le verrou qui ordonne. **Ce test ne prouve que le verrou** : la course réelle, entre un envoi et le ménage d'une page supprimée, est prouvée en S4b.
- [ ] **Step 2 : écrire**, relancer `cargo test -p mementum-server files::`, `cargo deny check licenses bans`.
- [ ] **Step 3 : commit** `serveur : dépôt des fichiers par leur contenu, type lu dans les octets (R244, R246)`.

### Task S4a : serveur, envoyer et lire un fichier

**Piste :** S. **Dépend de :** S2, S3. **Livrable :** trois routes (limites, envoi, lecture), leur matrice de droits, ce qui est servi et les bornes : les preuves 1 et 2 du « Review Focus » pour le serveur. La recopie, le ménage et la durée de vie sont la Task S4b.

**Files:**
- Create: `crates/mementum-server/src/routes/files.rs`, `crates/mementum-server/tests/files.rs`, `crates/mementum-server/tests/files_alloc.rs`
- Modify: `crates/mementum-server/src/lib.rs` (tableau, `router`, `AppState.files: Arc<FileStore>`, ouverture du dépôt dans `build`, avec le journal des contenus inconnus), `config.rs`, `guard.rs`, `error.rs`, `types.rs`, `crates/mementum-cli/src/main.rs` (`--max-file-mb`)
- Test: `tests/files.rs`, `tests/files_alloc.rs`, `tests/permissions.rs`, `tests/common/mod.rs` (`file_routes(id, file)`, sur le modèle de `version_routes`), `tests/sync_rights.rs`, `tests/measure.rs`, `crates/mementum-cli/tests/cli.rs`

**Interfaces:**
- Consumes: S2 (`attach_file`, `attachment`, `known_files`), S3 (`FileStore`, `content_disposition`).
- Produces (types `ts-rs`, ajoutés à `bindings()`) : `FileInfo { id, url, name, size: number, mime }` ; `FileLimits { maxFileBytes: number }` ; `ErrorCode::FileTooLarge` (413), `RangeNotSatisfiable` (416). `Config.max_file_bytes: u64` (`MEMENTUM_MAX_FILE_MB`, 100 par défaut) et `Config.upload_idle: Duration` (30 s ; ni variable ni option, comme `events_ping`). `FileResponse` : la marque qu'une réponse de fichier servi (`200`, `206`, `304`) porte dans ses extensions, et que lit `guard::api_headers`.

| Route | Niveau | Réponse |
|---|---|---|
| `GET /api/v1/files/limits` | une session | `FileLimits` |
| `POST /api/v1/pages/{id}/files?name=…` | `edit` | `201`, `FileInfo` (`url` = `/api/v1/pages/{id}/files/{fileId}`) ; `409 trashed` ; `413 file_too_large` ; droits revérifiés une fois le corps reçu |
| `GET /api/v1/pages/{id}/files/{fileId}` | `read` (à la corbeille aussi) | les octets, en-têtes de R246 ; `304` si `If-None-Match` nomme l'`ETag`, jugé **avant** `Range` ; sinon `206` pour un `Range` simple, `416` s'il sort du fichier ; un `If-Range` qui ne nomme pas l'`ETag` fait ignorer `Range` (`200`) ; deux plages, ou un en-tête illisible : `200`, le fichier entier ; un fichier qui n'est pas de la page, inconnu ou mal nommé : la réponse d'une page introuvable |

Ordre de l'envoi : `401`, `404`, `403`, `409 trashed`, `413` sur `Content-Length`, lecture du corps (`413` s'il dépasse ; s'il stagne `upload_idle`, la connexion est fermée **sans réponse**, et c'est journalisé), **second contrôle des droits**, puis, sous le verrou du dépôt et dans `to_completion` : `finish`, `attach_file`. Un fichier de zéro octet est accepté. La limite de 2 Mio des corps (`DefaultBodyLimit`) est levée sur cette route seule.

`guard::api_headers` pose toujours `Cache-Control: no-store`, sauf sur une réponse qui porte la marque `FileResponse` : seule la lecture d'un fichier y échappe, par cette marque, pas par la présence d'un `Cache-Control` (une route qui en poserait un par mégarde reste en `no-store`). Le `304` et le `206` portent les mêmes en-têtes que le `200` (`ETag`, `Cache-Control: private, no-cache`, ceux de R246).

- [ ] **Step 1 : tests des droits, vus rouges.**
  - `file_routes_follow_the_levels` : `read`, `comment`, `edit`, `full`, page vivante et à la corbeille, pour l'envoi et la lecture ; le refus de niveau nomme « Peut modifier » ; à la corbeille, la lecture passe et l'envoi répond `409 trashed`.
  - `foreign_page_is_indistinguishable_from_missing` (`permissions.rs`) gagne les routes, et essaie le vrai identifiant d'un fichier d'une page qu'on ne voit pas.
  - `file_of_another_page_is_not_found` : par quelqu'un qui lit les deux pages, `GET /pages/{B}/files/{fichier de A}` rend le statut, le code et le corps d'une page inconnue.
  - `upload_checks_rights_again_once_the_body_is_received` (`sync_rights.rs`, sur le modèle de `put_mmd_checks_rights_again_once_the_page_is_loaded` ; le corps est envoyé en deux morceaux par une connexion brute, et le droit change entre les deux) : rétrogradée à la lecture `403`, page mise à la corbeille `409`, accès retiré `404` ; rien n'est attaché, `tmp/` est vide. Témoin : sans changement, `201`.
  - `revalidation_is_checked_like_a_read` : `If-None-Match` avec l'`ETag` rend `304` sans corps à qui lit la page, avec `ETag`, `Cache-Control: private, no-cache` et les en-têtes de R246 ; l'accès retiré, la même requête rend `404`.
- [ ] **Step 2 : tests de ce qui est servi, vus rouges.**
  - `download_headers_never_let_a_file_run` : pour un vrai PNG, un HTML nommé `x.png` et déclaré `image/png`, un SVG à script, un PDF et des octets quelconques, la réponse porte **exactement** `Content-Disposition` tel que le rend `content_disposition` (S3), `X-Content-Type-Options: nosniff`, `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox`, `Cross-Origin-Resource-Policy: same-origin`, `Accept-Ranges: bytes`, `Cache-Control: private, no-cache`, `ETag: "<hash en hexadécimal>"`, `Content-Length` ; et `Content-Type` : `image/png`, `application/octet-stream`, `application/octet-stream`, `application/pdf`, `application/octet-stream`. Un nom à guillemet et à accents (`été "2026".png`) donne un en-tête lisible.
  - `upload_is_kept_under_its_hash_and_never_under_its_name` : après des envois aux noms de `display_name_is_only_a_label`, le dossier des données ne contient, sous `files/`, que des chemins `<2 hex>/<62 hex>` ; `FileInfo.name` est le nom nettoyé.
  - `only_a_served_file_escapes_no_store` : la lecture d'un fichier porte `private, no-cache` ; une réponse d'une autre route de l'API qui poserait `Cache-Control: max-age=60` (route de test montée dans le module de tests de `guard.rs`) repart en `no-store`.
  - `range_requests_serve_a_part` : `bytes=0-` (`206`, le fichier entier), `bytes=0-0` (`206`, `Content-Range: bytes 0-0/N`), `bytes=5-`, `bytes=-3` ; `bytes=N-` : `416` et `Content-Range: bytes */N` ; deux plages, ou un en-tête illisible : `200`, le fichier entier.
  - `range_with_a_matching_if_none_match_is_a_304` (les deux en-têtes ensemble, comme Chromium les envoie pour un média : `304` ; avec un autre `ETag` : `206`) ; `if_range_that_does_not_match_serves_the_whole_file` (`200` ; avec le bon `ETag` : `206`).
  - Gardes (une écriture naturelle ne les fait pas voir rouges) : `declared_content_type_is_never_read` (un PNG déclaré `text/html` est servi `image/png`) ; `refusals_of_file_routes_are_not_cached` (`no-store` sur un `404`, un `403`, un `413`).
- [ ] **Step 3 : tests des bornes, vus rouges** (serveur de test réglé à 1 Mio) : `file_too_large_is_refused_before_and_while_reading` (`Content-Length` de 1 Mio + 1 : `413` sans lire ; un corps sans longueur qui dépasse : `413`, `tmp/` vide, aucune ligne ; 1 Mio juste : `201`) ; `stalled_upload_is_abandoned` (`upload_idle` à 200 ms, connexion brute : attendre que `tmp/` soit vide ; la connexion est fermée sans un octet de réponse, et le journal le dit, lu par `Logs::capture`) ; `interrupted_upload_leaves_nothing` (le client coupe au milieu) ; `empty_file_is_accepted` (zéro octet : `201`, `size` 0, relu vide) ; `limits_route_says_the_maximum` (`{"maxFileBytes":104857600}` par défaut) ; dans `config.rs`, les tests de `MEMENTUM_MAX_FILE_MB` (`0`, `4097`, `nimporte` : le démarrage s'arrête en nommant la variable), et dans `crates/mementum-cli/tests/cli.rs` celui de `--max-file-mb`. **Seul dans son fichier**, `tests/files_alloc.rs` (l'allocateur compteur est global au binaire de test, comme dans `crates/mementum-sync/tests/frame_alloc.rs`) : `upload_and_download_never_hold_the_file_in_memory` (32 Mio envoyés puis relus, le corps produit au fil de l'eau et la réponse lue trame par trame, sinon c'est le test qui alloue ; pointe d'allocation sous 1 Mio).
- [ ] **Step 4 : écrire.** Au démarrage, `build` ouvre le dépôt (`FileStore::open`), puis journalise en `warn`, s'il y en a, le nombre et le poids des contenus que la base ne connaît pas (`known_files`, `FileStore::strangers`) : **il n'en supprime aucun** (R245). Un contenu que ce `finish` vient de poser (`Stored.fresh`) et dont l'attache échoue (la page supprimée entre le second contrôle et l'écriture) est retiré aussitôt, sous le même verrou ; la réponse est celle d'une page introuvable. Un contenu qui était déjà là reste.
- [ ] **Step 5 : mesure** (`tests/measure.rs`, `#[ignore]`, en release) : pendant un envoi de 100 Mio, une frappe faite sur une autre page est vue par un autre en moins de 150 ms au p95 ; durée de l'envoi consignée au registre.
- [ ] **Step 6 : relancer** `cargo test --workspace --no-fail-fast`, `bindings_are_committed` régénéré, `corepack pnpm -C apps/web typecheck` (vert sans rien changer : des types neufs). Commit : `serveur : envoyer et lire un fichier, sous les droits de sa page (R245 à R247)`. Puis conception §5.8 (ce qui est livré, ce qui reste), §5.6 (« Réponses » : toute réponse de l'API porte `no-store`, **sauf un fichier servi** ; « Fenêtres acceptées » : un téléchargement en cours finit), §7.2 (le réglage), en-tête de `lib.rs`, `CLAUDE.md` (piste « Serveur » : les en-têtes de R246 ne se retirent pas ; aucun nom dans un chemin ; rien n'est supprimé au démarrage ; et la puce « En-têtes de l'API », mise d'accord avec `FileResponse`), registre.

### Task S4b : serveur, recopier une attache, ménage et durée de vie

**Piste :** S. **Dépend de :** S4a. **Livrable :** la route de recopie, le ménage à la suppression d'une page, et la preuve 3 du « Review Focus » pour le serveur.

**Files:**
- Modify: `crates/mementum-server/src/routes/files.rs` (la recopie, le ménage, et un module de tests), `routes/pages.rs` (`delete` : le ménage après `Hub::forget`), `lib.rs` (tableau, `router`), `types.rs`, `error.rs`
- Test: `crates/mementum-server/tests/files.rs`, `tests/permissions.rs`, `tests/common/mod.rs` (`file_routes` gagne la recopie)

**Interfaces:**
- Consumes: S2 (`copy_attachment`, `sweep_files`), S3 (`lock`, `remove`), S4a (`FileInfo`, `AppState.files`).
- Produces (types `ts-rs`) : `CopyFile { pageId, fileId }` (`deny_unknown_fields`) ; `ErrorCode::InvalidSource` (422).

| Route | Niveau | Réponse |
|---|---|---|
| `POST /api/v1/pages/{id}/files/copies` | `edit` ici, `read` sur `pageId` | `201`, `FileInfo` de l'attache neuve ; `409 trashed` ; `422 invalid_source` pour une page d'origine qu'on ne voit pas, inconnue ou mal nommée, ou un fichier qui n'est pas d'elle : la même réponse. Un `GET` sur cette adresse répond `405` pour toute page, vue ou non (la route passe avant `{fileId}`) : le dire dans le tableau de `lib.rs` |

Ordre de la recopie : `401`, `404`, `403`, `409 trashed`, puis le corps (`422 invalid_json`, `422 invalid_source`). Entre le contrôle (`read` là-bas) et l'écriture de l'attache, quelques millisecondes : une fenêtre acceptée (R249).

- [ ] **Step 1 : tests de la recopie, vus rouges.** `copy_route_follows_the_levels` (`read`, `comment`, `edit`, `full` ici, page vivante et à la corbeille ; recopier **depuis** une page à la corbeille passe) ; `foreign_page_is_indistinguishable_from_missing` gagne la route ; `copy_gives_the_page_its_own_attachment` (recopié de A vers B, puis A supprimée : le fichier de B se lit) ; `copy_of_a_file_one_cannot_read_is_an_invalid_source` (les quatre cas, même statut, même code, même corps) ; `copy_refusals_come_before_the_body`.
- [ ] **Step 2 : tests de la durée de vie, vus rouges.** `file_survives_its_block_and_comes_back_with_a_restored_version` (un bloc image nomme le fichier ; une version est coupée ; `PUT /mmd` retire le bloc : la lecture du fichier rend toujours `200` ; la version restaurée : le MMD porte le bloc, la lecture `200`) ; `trashed_page_serves_its_files_and_takes_none` ; `deleting_a_page_removes_its_files_from_disk_once_no_page_keeps_them` (le même contenu sur A et B ; A supprimée pour de bon : le fichier de B se lit ; B supprimée : attendre que le fichier quitte le disque).
- [ ] **Step 3 : la course, vue rouge** (test unitaire dans `routes/files.rs`, sur l'état interne, comme `access::tests::state`) : `upload_of_a_content_being_swept_keeps_its_file`. Le test tient le verrou du dépôt, lance la suppression de la seule page qui porte un contenu (elle attend au ménage) et l'envoi du même contenu vers une autre page (il attend à `finish`), puis rend le verrou : quel que soit celui des deux qui passe le premier, l'attache neuve se lit (`200`, les bons octets). Témoin : sans le second envoi, le fichier quitte le disque. Rouge d'abord : avec un ménage qui ne prend pas le verrou.
- [ ] **Step 4 : écrire.** Après `Store::delete_node`, `routes/pages.rs::delete` fait un ménage sous le verrou (`sweep_files`, puis `remove` de chaque hash rendu, et d'eux seuls). Un échec du ménage est journalisé, il n'échoue pas la suppression.
- [ ] **Step 5 : relancer** `cargo test --workspace --no-fail-fast`, `bindings_are_committed` régénéré, typecheck du client. Commits : `serveur : recopier l'attache d'un fichier dans une autre page (R249)`, `serveur : le ménage des fichiers à la suppression d'une page`. Puis conception §5.8 et §5.6 (« Fenêtres acceptées » : la recopie), en-tête de `lib.rs`, `CLAUDE.md` (piste « Serveur » : le verrou du dépôt ; seul le ménage d'une page supprimée retire un fichier du disque), registre.

### Task S5 : serveur, favoris

**Piste :** S. **Dépend de :** S2. **Livrable :** trois routes ; une liste qui ne nomme jamais une page qu'on ne voit pas.

**Files:**
- Create: `crates/mementum-server/src/routes/favorites.rs`
- Modify: `crates/mementum-server/src/lib.rs`, `types.rs`
- Test: `crates/mementum-server/tests/favorites.rs` (neuf), `tests/permissions.rs`, `tests/spaces.rs`, `tests/common/mod.rs`

**Interfaces:**
- Consumes: `Store::set_favorite`, `Store::favorites` (S2).
- Produces: `Favorites { pages: string[] }` (`ts-rs`).

| Route | Il faut | Réponse |
|---|---|---|
| `GET /api/v1/spaces/{id}/favorites` | un rôle | `Favorites` : les favoris du compte que `space_access` lui montre, hors corbeille, dans l'ordre où il les a ajoutés |
| `PUT`, `DELETE /api/v1/pages/{id}/favorite` | `read` (à la corbeille aussi) | `204` ; déjà dans cet état : `204`, rien n'est écrit |

- [ ] **Step 1 : tests, vus rouges.** `favorite_is_personal_and_listed_in_the_order_added` ; `favorite_one_no_longer_sees_is_not_listed_and_comes_back_with_the_access` (l'accès retiré : la liste ne porte plus la page, ni son identifiant ; rendu : elle revient, à sa place) ; `favorite_in_the_trash_is_not_listed_and_comes_back_restored` ; `favorites_of_another_space_are_not_listed` ; `favorite_routes_follow_the_levels` (`read` suffit) ; `foreign_page_is_indistinguishable_from_missing` gagne `PUT` et `DELETE …/favorite` ; `space_routes_by_role` gagne `GET …/favorites` (sans rôle : la réponse d'un espace introuvable) ; `favoriting_is_not_pushed_to_the_space` (canal de l'espace, ping à `NO_PING` : après un `PUT`, puis un `DELETE`, aucune trame ; prouvé par un renommage qui suit, dont la trame porte exactement le numéro qui suit celui de l'ouverture).
- [ ] **Step 2 : écrire**, relancer, régénérer les types. Commit `serveur : favoris d'un compte (R254)`. En-tête de `lib.rs` (les routes, et qu'un favori n'avance ni `rev` ni `lsn`), `CLAUDE.md` (piste « Serveur » : un favori ne change que la barre de son auteur, il ne passe pas par `bump_lsn`), registre. La conception §5.4 ne change pas.

### Task S6 : serveur, dupliquer une page

**Piste :** S. **Dépend de :** S2, S4b. **Livrable :** une route ; une copie qui ne partage rien, ne reprend aucun identifiant de bloc, et garde ses fichiers.

**Files:**
- Create: `crates/mementum-store/src/duplicate.rs`, `crates/mementum-server/src/routes/duplicate.rs`
- Modify: `crates/mementum-store/src/lib.rs`, `crates/mementum-server/src/lib.rs`, `types.rs` (`ErrorCode::SubtreeTooLarge`), `error.rs`
- Test: `crates/mementum-store/tests/duplicate.rs`, `crates/mementum-server/tests/duplicate.rs` (neufs), `tests/permissions.rs`, `tests/sync_rights.rs`, `tests/measure.rs`

**Interfaces:**
- Consumes: `space_access` et `SpaceAccess::node` (`access.rs` : les pages du sous-arbre que la personne voit, et `in_trash`), `Hub::read_blocks`, `PageDoc::new_page`, `PageDoc::write_blocks`, `PageDoc::encode_state`, `Store::attachment`. **Pas `Store::subtree_ids`**, qui rend tout le sous-arbre, corbeille comprise, sans les droits.
- Produces:
  - `pub struct NewCopy { pub space_id: Uuid, pub parent_id: Option<Uuid>, pub after: Option<Sibling>, pub grants: Vec<Grant>, pub created_by: Uuid, pub pages: Vec<CopiedPage> }` : `pages[0]` est la racine de la copie, chaque autre page vient après sa page parente ;
  - `pub struct CopiedPage { pub id: Uuid, pub parent: Option<usize>, pub title: String, pub position: Option<String>, pub doc: Vec<u8>, pub attachments: Vec<CopiedAttachment> }` (`position` : celle de l'original pour une descendante, `None` pour la racine, placée par `after`) ; `pub struct CopiedAttachment { pub id: Uuid, pub hash: [u8; 32], pub name: String }` ;
  - `Store::duplicate_pages(&self, copy: NewCopy, now: i64) -> Result<Node, StoreError>` : nœuds, accès, documents et attaches en **une** `WriteTx`, `bump_rev` une fois ; `NotFound` (espace, parent, `after` qui n'est pas un frère, hash sans ligne `files`) n'écrit rien ;
  - `POST /api/v1/pages/{id}/duplicate` : `201`, `PageNode` de la racine de la copie ; `MAX_DUPLICATED_PAGES: usize = 100`.

| Cas | Il faut | Où va la copie |
|---|---|---|
| le compte voit la page parente et peut y créer (`CreateChild`) | `read` sur la page | sous la page parente, juste après la page ; aucun accès propre |
| sinon (page de racine, page parente invisible, ou page parente où il ne peut pas créer), pour un membre | `read`, et le rôle `member` | à la racine de l'espace, en dernier, avec `(user:<lui>, full)` : dans son « Privé » |
| sinon, pour un invité | — | `403 forbidden`, par `at_least` (« réservé aux membres de l'espace ») |

Refus, dans l'ordre : `401`, `404`, `403`, `409 trashed` (la page est à la corbeille : `permit` ne le dit pas pour `Read`, la route le rend d'après `NodeAccess::in_trash`), `422 subtree_too_large`.

Déroulé (R89 : le contrôle qui compte vient après tout ce qui attend) :
1. premier contrôle, et choix de la destination ;
2. les pages à copier : celles du sous-arbre **que `space_access` montre à la personne, `in_trash` faux**, parent avant enfant ;
3. `Hub::read_blocks` de chacune ;
4. pour chaque page, dans `spawn_blocking` : blocs dont tous les `id` sont vidés, adresses de fichiers récrites, écrits dans un `PageDoc` neuf ;
5. **second contrôle, juste avant d'écrire** : la page est encore lue et hors corbeille, chaque sous-page copiée est encore visible, et la destination est rejouée (la page parente si l'on peut encore y créer, sinon « Privé » pour un membre, sinon `403`) ; c'est la destination de ce second contrôle qui est écrite ;
6. `duplicate_pages` dans `to_completion`.

Une adresse n'est récrite que si elle vaut exactement `/api/v1/pages/<la page copiée>/files/<une attache de cette page>` sur un bloc `image`, `video`, `audio` ou `file` ; toute autre est gardée telle quelle. Titre de la racine : le titre suivi de « (copie) », coupé pour tenir en `MAX_TITLE_CHARS`.

- [ ] **Step 1 : tests du store, vus rouges** : `copy_is_written_whole_or_not_at_all` (un hash inconnu sur la troisième page : aucun nœud, aucun document, `rev` inchangé) ; `copy_advances_rev_and_lsn_once` ; `copied_subtree_keeps_its_shape_and_its_order`.
- [ ] **Step 2 : tests du serveur, vus rouges.**
  - `duplicate_copies_content_with_fresh_block_ids` : le MMD de la copie est celui de l'original ; aucun `id` de bloc de l'une n'est dans l'autre (lus par `Hub::read_blocks`), à chaque profondeur.
  - `duplicate_copies_the_subtree_but_not_what_is_trashed`.
  - `duplicate_of_a_subpage_stays_beside_it_and_inherits` ; `duplicate_of_a_root_page_is_private_to_who_duplicates` (une page d'« Espace » : les autres membres ne voient pas la copie, propriétaire compris) ; `member_who_does_not_see_the_parent_gets_a_private_root_copy` (rien n'est créé sous la page parente invisible, et la réponse ne la nomme pas) ; `member_who_cannot_create_under_the_parent_gets_a_private_root_copy` (page parente lue, pas modifiable : la copie est à la racine, dans son « Privé », et rien n'est créé sous la page parente) ; `guest_cannot_duplicate_where_he_cannot_create` (page partagée avec lui à la racine, ou sous une page parente qu'il ne peut pas modifier : `403`, rien d'écrit ; témoin : sous une page parente qu'il peut modifier, la copie se pose à côté) ; `duplicate_routes_follow_the_levels` ; `foreign_page_is_indistinguishable_from_missing` gagne la route.
  - `duplicated_page_keeps_its_files_after_the_original_is_deleted` : l'adresse du bloc de la copie nomme la copie ; l'original supprimé pour de bon, elle se lit encore ; une adresse externe et celle d'un fichier d'une troisième page sont restées telles quelles.
  - `duplicate_copies_neither_comments_nor_versions_nor_favorites_nor_named_accesses`.
  - `duplicate_checks_rights_again_just_before_writing` (`sync_rights.rs`, sur le modèle de `put_mmd_checks_rights_again_once_the_page_is_loaded`) : accès retiré **pendant la construction des documents**, pas seulement pendant la lecture : `404`, aucune page créée ; page mise à la corbeille pendant elle : `409 trashed` ; droit de créer sous la page parente retiré pendant elle, pour un membre : la copie va dans son « Privé ». Témoin : sans changement, `201`.
  - `subtree_of_101_pages_is_refused` (`422 subtree_too_large`, rien d'écrit) ; 100 passent.
  - `long_title_is_cut_to_fit` (500 caractères : la copie en a 500 et finit par « (copie) »).
- [ ] **Step 3 : écrire**, puis les mesures (`tests/measure.rs`, `#[ignore]`, en release), consignées au registre : dupliquer une page de 10 000 blocs ; **100 pages de 1 000 blocs, et 10 pages de 10 000** : durée de la transaction d'écriture, et frappe d'un autre sur une autre page pendant elle (p95 sous 150 ms). L'écriture de SQLite est unique pour toute l'instance : pendant la transaction, le journal de chaque page ouverte attend, et au-delà de `busy_timeout` (5 s) une frappe échoue. **Si la transaction dépasse une seconde**, la route refuse `422 subtree_too_large` au-delà d'un poids total des états copiés (`MAX_DUPLICATED_BYTES`, fixé d'après la mesure, écrit au registre et testé à la borne) ; sinon, aucune borne de poids n'est posée, et le registre le dit. L'acteur de l'original n'est occupé que le temps de sa lecture.
- [ ] **Step 4 : relancer**, régénérer les types. Commits : `store : copier un sous-arbre de pages en une transaction`, `serveur : dupliquer une page et ses sous-pages (R252)`. Conception §5.6, en-tête de `lib.rs`, registre (dont : une page dupliquée naît sans version, son contenu de départ n'est dans aucune).

---

## Piste C

Chaque tâche qui touche `editor/` finit par : Vitest, typecheck, ses e2e, puis **les budgets du navigateur** (`corepack pnpm e2e:budgets`), avec le poids de ce que charge l'ouverture d'une page, consigné au registre. Tout e2e de la piste se lance avec `MEMENTUM_E2E_WEB_PORT=5177` et `MEMENTUM_E2E_API_PORT=8084`.

### Task C1a : banc à deux éditeurs et mesures des gestes

**Piste :** C. **Dépend de :** la Task 13a close (livrée, ou arrêtée sur son critère) : elle récrit `blockMoves.test.ts`. **Livrable :** le banc dont les tâches suivantes se servent, et ce que les gestes de ce plan écrivent dans Yjs, relu dans le document. Ni e2e, ni budgets : des tests seulement.

**Files:**
- Create: `apps/web/src/test/editors.ts`, `apps/web/src/editor/gestures.test.ts`
- Modify: `apps/web/src/editor/blockMoves.test.ts` (importe le banc, sans une assertion changée)

**Interfaces:**
- Produces: `test/editors.ts` : `peer`, `seed`, `scene`, `exchange`, `typeAtEnd`, `shown`, `stored`, et `elementIds(doc): Map<string, string>` (pour chaque `id` de bloc, l'identifiant Yjs de son conteneur et de son élément de contenu ; pas `elements`, nom d'une fonction locale de `blockMoves.test.ts`, d'une autre forme).

- [ ] **Step 1 : le banc**, sorti de `blockMoves.test.ts` ; ses tests restent verts tels quels.
- [ ] **Step 2 : mesures commitées** (`gestures.test.ts`), chaque test avec son témoin. L'en-tête du fichier dit sous quel `y-prosemirror` elles ont été prises (avec le correctif de la Task 13a, ou sans, si elle a été arrêtée), et elles restent vertes dans les deux. `inserting_blocks_creates_only_new_elements` (aucun `id` existant ne change d'élément, la frappe de Tom dans le bloc d'avant et dans celui d'après est gardée, les trois documents sont identiques) ; `changing_a_prop_keeps_the_element_and_what_another_types` (couleur et icône d'un callout). **Si l'une de ces deux est fausse : arrêt, `BLOCKED`** ; le plan repose sur elles. `changing_the_type_of_a_block_keeps_what_another_types`, marqué `it.fails` : **échec attendu**, avec ou sans correctif, qui ne joue pas à l'intérieur d'un bloc (mesuré sans : la frappe est perdue) ; la limite va au registre et à la tâche D. Si un jour la frappe est gardée, `it.fails` passe au rouge et le dit : le test devient alors un test ordinaire, et c'est consigné.
- [ ] **Step 3 : relancer** Vitest et le typecheck. Commit : `tests : le banc à deux éditeurs, et ce que les gestes de la partie B écrivent`. Registre : les mesures, et sous quel `y-prosemirror`.

### Task C1b : menu « / » des blocs riches, callout et signet

**Piste :** C. **Dépend de :** C1a. **Livrable :** deux blocs qu'on pose, règle et relit en MMD.

**Files:**
- Create: `apps/web/src/editor/slashItems.tsx`, `apps/web/src/editor/blocks/Callout.tsx`, `Bookmark.tsx`, `BlockPopover.tsx`, `urls.ts`, et leurs tests
- Modify: `apps/web/src/editor/schema.tsx` (les specs de `callout` et `bookmark` sortent dans `blocks/`, mêmes `propSchema`), `Editor.tsx` (éléments du menu « / » ; la barre de mise en forme, récrite pour « Transformer en » et sans les boutons de fichier sur un signet), `theme.css`, `icons.tsx`
- Test: `apps/web/e2e/rich-blocks.spec.ts` (neuf)

**Interfaces:**
- Produces:
  - `richSlashItems(editor: MementumEditor): DefaultReactSuggestionItem[]`, groupe « Blocs riches », dans cet ordre au fil des tâches : Callout, Équation, Équation en ligne, Signet, Table des matières, Lien vers une page ; chaque élément a ses alias en français et en anglais ;
  - `webUrl(raw: string): string | null` (`blocks/urls.ts`, sorti de `schema.tsx`) ;
  - `BlockPopover` : `{ label: string; anchor: HTMLElement; onClose: () => void; children }`, une fenêtre non modale qui prend le focus à l'ouverture, le garde (Tab n'en sort pas), se ferme sur Échap ou un clic dehors, et rend le focus à `anchor` s'il est encore là, à l'éditeur sinon ;
  - `CALLOUT_ICONS = ["💡", "⚠️", "❗", "✅", "❓", "📌", "🔥", "📝"]` ; `CALLOUT_COLORS` : les dix noms de BlockNote (`default`, `gray`, `brown`, `red`, `orange`, `yellow`, `green`, `blue`, `purple`, `pink`).

- [ ] **Step 1 : tests Vitest, vus rouges.** `slash_menu_offers_the_rich_blocks_in_order` ; `callout_icon_and_color_are_chosen_from_the_keyboard` (le bouton de l'icône ouvre la fenêtre ; flèches, Entrée ; Échap ferme et rend le focus) ; `callout_text_contrast_is_at_least_4_5` (calculé pour `#2F2E2B` sur chacun des dix fonds) ; `an_icon_from_elsewhere_is_shown_as_text` (une icône de 40 caractères, ou une balise, posée par MMD : du texte, coupé à l'affichage, jamais du HTML) ; `bookmark_takes_only_a_web_address` (`javascript:alert(1)`, `data:…`, `//hôte` : refusés avec « Une adresse commence par http:// ou https:// », rien n'est écrit ; `https://exemple.fr/a` : écrit) ; `bookmark_shows_host_and_address_as_text` (`dir="auto"`, lien `rel="noopener noreferrer"`, `target="_blank"`) ; `webUrl` : ses cas ; `bookmark_is_not_a_file_block_for_the_toolbar` (un signet sélectionné : la barre de mise en forme n'offre ni « Remplacer », ni « Télécharger », ni la légende des blocs fichier, que BlockNote montre à tout bloc qui a une prop `url` ; témoin : une image sélectionnée les montre). Première chose à faire : voir à l'écran ce que la barre offre aujourd'hui sur un signet (lu dans le code, non vérifié).
- [ ] **Step 2 : écrire.** Callout : la fenêtre de réglage s'ouvre par le bouton de l'icône (nommé « Icône et couleur du callout ») ; le callout entre dans « Transformer en » de la barre de mise en forme. Signet : inséré vide, il ouvre sa fenêtre sur un champ d'adresse ; « Modifier » la rouvre ; l'adresse écrite dans le bloc est celle que rend `webUrl` (`https://mementum.dev` devient `https://mementum.dev/`) ; rien d'autre que cette fenêtre n'écrit l'adresse d'un signet.
- [ ] **Step 3 : e2e, vus rouges.** `callout_is_inserted_set_and_read_back` (à deux : Mona pose un callout par « / », change icône et couleur, Tom les voit ; le MMD du serveur vaut exactement `<callout icon="⚠️" color="blue">`, le texte, `</callout>` ; ce MMD, récrit par `PUT /mmd`, laisse la page telle quelle) ; `bookmark_is_inserted_and_read_back` (`<bookmark url="https://mementum.dev/"/>`) ; `rich_blocks_are_read_only_for_a_reader` (ni bouton d'icône, ni « Modifier »).
- [ ] **Step 4 : relancer**, budgets. Commits : `web : le menu « / » propose les blocs riches`, `web : régler un callout`, `web : poser un signet`, `e2e : callout et signet`. Conception §4.2, `CLAUDE.md` (piste « Client »), registre.

### Task C2 : équation en bloc et en ligne

**Piste :** C. **Dépend de :** C1a (le banc), C1b (`slashItems.tsx`).

**Files:**
- Create: `apps/web/src/editor/blocks/Equation.tsx`, `LatexField.tsx`, et leurs tests
- Modify: `apps/web/src/editor/schema.tsx`, `slashItems.tsx`, `gestures.test.ts`, `theme.css`, `apps/web/e2e/rich-blocks.spec.ts`

**Interfaces:**
- Produces: `LatexField` : `{ latex: string; display: boolean; onApply: (latex: string) => void }`, un champ de 10 000 caractères au plus, l'aperçu KaTeX au-dessous (`throwOnError: false`, `trust: false`), « Terminer » ; `MAX_LATEX_CHARS = 10_000` ; le brouillon d'une équation en bloc vit par identifiant de bloc dans un magasin du module, tant que la page est ouverte ; celui d'une formule en ligne, qui n'a pas d'identifiant, vit dans l'état de sa vue, tant qu'elle est montée (le dire dans l'écran : fermer la page le perd).

- [ ] **Step 1 : mesures commitées** (`gestures.test.ts`) : `inline_math_inserted_at_the_cursor_keeps_what_another_types` (début, milieu et fin d'un texte : dire où atterrit la frappe d'un autre ; le cas du milieu est `it.fails`, échec attendu comme pour la mention : le texte d'après est recréé) ; `applying_the_latex_of_an_inline_formula_keeps_what_another_types` (« Terminer » change la prop d'un contenu en ligne : relire ce que cela écrit, et ce que devient une frappe faite au même instant avant et après la formule ; si le texte voisin est recréé, c'est une limite, écrite au registre et à la tâche D).
- [ ] **Step 2 : tests Vitest, vus rouges.** `equation_is_edited_in_its_window_and_applied_once` (rien n'est écrit dans le document tant qu'on tape ; « Terminer » ou Ctrl+Entrée écrit `latex` en une fois) ; `escape_keeps_the_typed_latex` (Échap ferme sans écrire ; rouvrir retrouve le texte tapé) ; `invalid_latex_is_shown_in_red_and_kept` ; `katex_never_follows_a_link` (`\href{javascript:alert(1)}{x}` : aucun élément `a`) ; `empty_equation_says_so` ; `inline_math_opens_its_field_when_inserted_and_on_click` ; `reader_sees_formulas_and_cannot_open_their_field`.
- [ ] **Step 3 : écrire.** « Équation » insère un bloc vide et ouvre son champ ; « Équation en ligne » insère une formule vide au curseur et ouvre le sien ; un bloc ou une formule se rouvre par Entrée quand il a le focus, ou par un clic, pour qui peut modifier.
- [ ] **Step 4 : e2e, vus rouges.** `equation_block_is_typed_and_read_back` (le MMD vaut `$$`, la formule, `$$` ; à deux) ; `inline_math_is_typed_and_read_back` (`$E = mc^2$` dans la ligne) ; `formula_that_dollars_cannot_carry_is_read_back` (une formule finissant par `\` : `<math latex="…"/>`).
- [ ] **Step 5 : relancer**, budgets. Commits : `web : écrire une équation, en bloc et en ligne`, `e2e : équations`. Conception §4.2, registre.

### Task C3 : table des matières et lien de page

**Piste :** C. **Dépend de :** C1b.

**Files:**
- Create: `apps/web/src/space/pageDirectory.ts`, `apps/web/src/editor/blocks/Toc.tsx`, `PageLink.tsx`, `PagePicker.tsx`, et leurs tests
- Modify: `apps/web/src/App.tsx` (publie les pages de l'arbre), `apps/web/src/editor/schema.tsx`, `slashItems.tsx`, `theme.css`, `apps/web/e2e/rich-blocks.spec.ts`

**Interfaces:**
- Produces:
  - `pageDirectory.ts` : `publishPages(spaceId: string | null, nodes: PageNode[] | null): void` (appelé par `App` quand l'arbre change ; `null` quand on quitte l'espace) ; `usePageTitle(id: string): string | null` (le titre à jour si le compte voit la page, `null` sinon) ; `findPages(query: string, limit: number): { id: string; title: string }[]` (sans accents ni casse, les titres qui commencent par la requête d'abord). Un magasin du module, lu par `useSyncExternalStore` : **l'éditeur n'est pas rendu de nouveau quand l'arbre change** ;
  - `PagePicker` : `{ query: string; onPick: (page: { id: string; title: string }) => void }`, une liste `role="listbox"` de huit pages au plus.

- [ ] **Step 1 : essai** (Vitest, éditeur monté) : où lire les titres dans le DOM de l'éditeur (attendu : `[data-content-type="heading"]`, le niveau dans `data-level`, l'identifiant du bloc sur l'ancêtre `[data-id]`, comme `comments/dom.ts`). **Si faux** : lire `editor.document`, une fois par recalcul, jamais par frappe.
- [ ] **Step 2 : tests Vitest, vus rouges.** `toc_lists_headings_1_to_4_in_order_with_their_depth` (un `nav` nommé « Table des matières », une liste de liens ; les niveaux 5 et 6 n'y sont pas) ; `toc_follows_the_page_without_reading_it_at_each_keystroke` (dix frappes en 100 ms : un seul recalcul, 250 ms après la dernière, une seconde au plus après la première) ; `page_without_a_toc_reads_no_heading` (aucune lecture du DOM ni de `editor.document`) ; `toc_entry_scrolls_to_its_heading_and_focuses_it` ; `empty_toc_says_so` ; `heading_text_is_shown_as_text`.
  `link_to_a_page_the_reader_sees_shows_its_current_title` ; `link_to_a_page_the_reader_cannot_see_shows_only_what_the_document_holds` (la page absente du répertoire : le `title` du bloc, jamais un titre d'ailleurs ; témoin : la même page publiée, le titre à jour paraît) ; `page_picker_offers_only_published_pages` ; `page_link_without_a_page_is_not_a_link` (`pageId` vide, ou qui n'est pas un UUID : du texte).
- [ ] **Step 3 : écrire.** La table des matières se recalcule sur `editor.onChange`, groupée (`groupedDelay` de `rereads.ts`), seulement quand un bloc `toc` est monté. « Lien vers une page » insère le bloc après le choix dans `PagePicker` ; rien n'est inséré si l'on ferme sans choisir.
- [ ] **Step 4 : e2e, vus rouges.** `toc_follows_headings_typed_by_someone_else` (à deux) ; `page_link_opens_the_page_and_reads_back` (MMD : `<page id="<uuid>">Plan</page>`) ; `renamed_page_shows_its_new_title_in_the_link` ; `guest_reads_the_link_label_of_a_page_he_cannot_see` (un invité de la seule page qui porte le lien : il lit le libellé copié, le clic mène à « Page introuvable », et aucune réponse reçue par son onglet ne porte le titre à jour ; témoin : la même recherche dans les réponses reçues par l'onglet d'un membre le trouve).
- [ ] **Step 5 : relancer**, budgets (la page de 10 000 blocs **avec** une table des matières est mesurée et consignée, sans budget à elle). Commits : `web : les pages que le compte voit, pour l'éditeur`, `web : table des matières`, `web : lien vers une page`, `e2e : table des matières et lien de page`. Conception §4.2, `CLAUDE.md`, registre.

### Task C4 : mentions et `[[`

**Piste :** C. **Dépend de :** C3.

**Files:**
- Create: `apps/web/src/mentions/MentionMenu.tsx`, `people.ts`, `dates.ts`, `Mention.tsx`, et leurs tests
- Modify: `apps/web/src/editor/schema.tsx` (le rendu de `mention`), `Editor.tsx` (deux `SuggestionMenuController` : `@` et `[[`), `gestures.test.ts`, `theme.css`
- Test: `apps/web/e2e/mentions.spec.ts` (neuf)

**Interfaces:**
- Consumes: `findPages`, `usePageTitle` (C3) ; `getMembers` (`api.ts`) ; `session.roster` ; `createPage`.
- Produces:
  - `parseDate(query: string, today: Date): Date | null` : « aujourd'hui », « demain », « hier », un jour de la semaine (le prochain, jamais aujourd'hui), `JJ/MM` (cette année), `JJ/MM/AAAA`, `AAAA-MM-JJ` ; sans accents ni casse ; une date impossible (`31/02`) : `null` ;
  - `dateLabel(date: Date): string` : « 1 déc. 2026 » ; l'`id` d'une mention de date est `AAAA-MM-JJ` ;
  - `loadPeople(spaceId: string, role: Role, roster: RosterUser[]): Promise<{ id: string; name: string }[]>` : pour un membre, `getMembers` (lu à l'ouverture du menu, gardé une minute) ; pour un invité, ou si la lecture est refusée, les comptes de `roster` ;
  - ce qui est inséré : `{ type: "mention", props: { kind: "page" | "user" | "date", id, label } }`, suivi d'une espace.

- [ ] **Step 1 : mesures commitées** (`gestures.test.ts`) : `mention_inserted_in_the_middle_keeps_what_another_types_after_it` : le texte de Tom n'est ni perdu ni doublé et les trois documents sont identiques (**exigé**) ; qu'il reste **après** la mention serait le comportement voulu : cette seconde assertion est à part, marquée `it.fails`, **échec attendu** avec ou sans correctif, qui ne joue pas à l'intérieur d'un bloc (mesuré sans : la frappe passe avant la mention) ; la limite va au registre et à la tâche D. `mention_inserted_at_the_end_keeps_what_another_types`. Et, éditeur monté : la mention est-elle un îlot `contenteditable="false"` où `useLinkInterception` suit un lien ? **Si faux** : la mention de page s'ouvre par Entrée et par clic sur un bouton, pas par un `<a>`.
- [ ] **Step 2 : tests Vitest, vus rouges.** `parseDate` : chaque forme, le 31 décembre, une année bissextile, « lundi » tapé un lundi (le suivant), des entrées quelconques (`null`). `at_menu_offers_pages_people_and_a_date` (trois groupes, huit lignes au plus chacun, `role="listbox"`, flèches et Entrée, Échap ferme sans rien retirer du texte tapé) ; `guest_is_offered_only_who_is_on_the_page` ; `double_bracket_offers_pages_and_creating_one` (« Créer la page « Compte rendu » » en dernier, absent sans le droit de modifier, absent pour une requête vide) ; `creating_from_double_bracket_creates_a_subpage_once` (`firstClick` ; un refus du serveur s'affiche tel quel et rien n'est inséré) ; `page_mention_shows_the_current_title_only_to_who_sees_the_page` ; `page_mention_without_a_page_is_not_a_link` (`id` vide, ou qui n'est pas un UUID : du texte, comme le lien de page) ; `mention_label_is_text_isolated` (`dir="auto"`, un libellé à balises reste du texte) ; `date_mention_is_formatted_from_its_id_or_shown_as_written` (`id` illisible : le libellé).
- [ ] **Step 3 : écrire.** `shouldOpen` ferme les deux menus dans un bloc de code. Aucune lecture des personnes avant l'ouverture du menu `@`.
- [ ] **Step 4 : e2e, vus rouges.** `page_mention_is_typed_seen_and_read_back` (à deux ; MMD : `<mention type="page" id="<uuid>">Plan</mention>`) ; `person_and_date_mentions_read_back` (`type="user"`, `type="date" id="2026-12-01"`) ; `double_bracket_creates_a_subpage_and_links_it` (la sous-page paraît dans la barre latérale de l'autre) ; `guest_is_offered_only_the_pages_he_sees` (un invité tape `@` et `[[` : seule sa page est proposée ; aucune réponse reçue par son onglet ne porte le titre d'une autre ; témoin : la même recherche sur l'onglet d'un membre trouve ce titre) ; `at_sign_typed_with_altgr_opens_the_menu` (AZERTY : `@` est AltGr+0).
- [ ] **Step 5 : relancer**, budgets. Commits : `web : dire une date en mots`, `web : mentionner une page, une personne, une date`, `web : [[ lie ou crée une page`, `e2e : mentions`. Conception §4.2 et §4.4, README noté au registre (« une mention ne prévient personne » ; « mentionner une page en copie le titre dans le document : qui lit la mention le lit », R251), `CLAUDE.md`, registre.

### Task C5 : dupliquer un bloc

**Piste :** C. **Dépend de :** C1a (le banc) ; après C1b dans la piste (même `Editor.tsx`).

**Files:**
- Create: `apps/web/src/editor/duplicate.ts`, `duplicate.test.ts`
- Modify: `apps/web/src/editor/Editor.tsx` (raccourci ; le menu de la poignée, `BlockMenu`, devient le nôtre dès que l'éditeur est modifiable, et « Commenter » n'y figure que si l'on peut commenter : aujourd'hui il n'existe que si `onComment` n'est pas nul), `gestures.test.ts`
- Test: `apps/web/e2e/duplicate-block.spec.ts` (neuf)

**Interfaces:**
- Produces: `isDuplicateShortcut(event: KeyboardEvent): boolean` (la lettre tapée est « d », avec Ctrl ou Cmd, sans Alt ni Maj, jamais sous AltGr) ; `duplicateBlocks(editor: MementumEditor, ids: string[]): string[]` : insère, après le dernier des blocs nommés, leurs copies dans l'ordre, sans aucun `id` à aucune profondeur, pose le curseur dans la première copie, et rend les `id` neufs.

- [ ] **Step 1 : mesures commitées** (`gestures.test.ts`) : `duplicating_touches_no_existing_element` (un bloc avec un enfant, dupliqué pendant que Tom tape dans l'original et dans le bloc suivant : rien de perdu, aucun `id` existant ne change d'élément, trois documents identiques) ; `a_copy_that_kept_its_id_is_not_a_copy` (ce que `duplicateBlocks` ne doit jamais produire : une copie insérée avec l'`id` de l'original ; le test relit ce que le document porte alors, et l'écrit tel que mesuré ; attendu, avec ou sans le correctif, qui retombe sur le code d'origine dès qu'un `id` se répète : deux conteneurs du même `id`, dont le serveur supprime le second).
- [ ] **Step 2 : tests Vitest, vus rouges.** `duplicated_block_has_fresh_ids_at_every_depth` (bloc à enfants et petits-enfants, liste de colonnes, tableau) ; `several_selected_blocks_are_copied_in_order_after_the_last` ; `cursor_goes_to_the_first_copy` ; `isDuplicateShortcut` : Ctrl+D, Cmd+D ; refusés : Ctrl+Maj+D, Ctrl+Alt+D, AltGr+D, « d » seul, une autre lettre sur la touche D d'un autre clavier.
- [ ] **Step 3 : écrire.** Le raccourci n'est pris (`preventDefault`) que le focus dans l'éditeur, modifiable ; ailleurs, le navigateur garde le sien. « Dupliquer » est dans le menu de la poignée de tout éditeur modifiable, après « Commenter » quand il y est.
- [ ] **Step 4 : e2e, vus rouges.** `ctrl_d_duplicates_and_the_server_keeps_both` (à deux : Tom voit la copie ; le MMD du serveur porte les deux blocs, encore après vingt frappes alternées de l'un et de l'autre, chacune vue par l'autre avant la suivante, sans attente fixe : le dédoublonnage n'en a retiré aucun) ; `duplicate_from_the_handle_menu` ; `ctrl_d_outside_the_editor_is_left_to_the_browser` (dans le titre : l'événement n'est pas annulé).
- [ ] **Step 5 : relancer**, budgets. Commits : `web : dupliquer un bloc (R253)`, `e2e : dupliquer un bloc`. Conception §4.5, registre.

### Task C6 : fil d'Ariane et récents

**Piste :** C. **Dépend de :** rien (ouvre la piste tant que la Task 13a n'est pas close).

**Files:**
- Create: `apps/web/src/page/Breadcrumb.tsx`, `page/ancestors.ts`, `apps/web/src/space/recents.ts`, `apps/web/src/sidebar/ShortList.tsx`, et leurs tests
- Modify: `apps/web/src/page/PageView.tsx` (le fil à la place de `mm-topbar-title`), `App.tsx`, `sidebar/Sidebar.tsx`, `sidebar/sections.ts` (les groupes repliables gagnent `recents` et `favorites`), `sidebar/SpaceSwitcher.tsx` (la déconnexion efface les récents du compte), `theme.css`
- Test: `apps/web/e2e/breadcrumb.spec.ts`, `recents.spec.ts` (neufs)

**Interfaces:**
- Produces:
  - `ancestorsOf(nodes: PageNode[], id: string): PageNode[]` : de la plus haute page visible à la page parente ; une chaîne qui boucle s'arrête ;
  - `Breadcrumb` : `{ node: PageNode; ancestors: PageNode[] }` ;
  - `recents.ts` : `RECENTS_KEPT = 20`, `RECENTS_SHOWN = 5`, `recentsKey(userId, spaceId) = "mementum.recents.<userId>.<spaceId>"`, `visit(userId, spaceId, pageId): void`, `readRecents(userId, spaceId): string[]` (un stockage refusé ou illisible : une liste vide, sans erreur), `forgetRecents(userId): void` (retire toutes les clés `mementum.recents.<userId>.*` : appelé à la déconnexion, R255) ;
  - `ShortList` : `{ label: string; group: "favorites" | "recents"; pages: PageNode[]; activeId: string | null }`, une section repliable de liens, que C7 reprend.

- [ ] **Step 1 : tests Vitest, vus rouges.** `ancestorsOf` : racine, trois niveaux, page parente invisible (le fil commence à la page), boucle. `breadcrumb_is_a_list_of_links_ending_with_the_page` (`nav` nommé « Fil d'Ariane », `ol`, la page en `aria-current="page"`, sans lien) ; `deep_breadcrumb_folds_its_middle_into_a_menu` (six niveaux : la première, « … », les deux dernières ; le menu, au clavier, liste les autres) ; `titles_are_isolated_text` (un titre en arabe, un titre à balises ; l'infobulle porte U+2068 et U+2069). `recents_keep_twenty_ids_most_recent_first_without_repeats` ; `recents_show_only_pages_of_the_tree` (un identifiant qui n'est plus dans l'arbre n'est ni montré ni retiré du stockage : il revient avec l'accès) ; `recents_of_another_account_are_not_read` ; `logging_out_forgets_the_recents_of_the_account` (toutes ses clés, dans tous ses espaces ; celles d'un autre compte restent) ; `storage_that_throws_shows_no_recents`.
- [ ] **Step 2 : écrire.** `App` calcule `ancestorsOf` (mémoïsé) et le donne à `PageView` ; il appelle `visit` quand la page ouverte est connue et hors corbeille. « Récents » se place sous « Favoris », au-dessus des trois sections, absent quand il est vide ; rien n'est lu du serveur pour lui. La déconnexion (`SpaceSwitcher.tsx`) appelle `forgetRecents` pour le compte qui part.
- [ ] **Step 3 : e2e, vus rouges.** `breadcrumb_follows_a_move_made_by_someone_else` (à deux : la page change de parent, le fil suit, sans recharger) ; `guest_breadcrumb_starts_at_the_page_he_sees` (aucun titre de page parente dans son onglet, ni à l'écran ni dans une réponse reçue ; témoin : l'onglet d'un membre porte ce titre dans son fil) ; `opened_pages_are_listed_most_recent_first` ; `recents_survive_a_reload_and_forget_a_trashed_page` ; `recents_are_gone_after_logging_out` (la clé n'est plus dans le stockage du navigateur).
- [ ] **Step 4 : relancer** Vitest, typecheck, et les e2e qui comptent les lignes ou les lectures de la barre latérale (`pages.spec.ts`, `moves.spec.ts`, `live-tree.spec.ts`, `reload.spec.ts`). Commits : `web : fil d'Ariane (R256)`, `web : pages récentes (R255)`, `e2e : fil d'Ariane et récents`. Conception §4.2 et §4.3, registre.

### Task C7 : favoris

**Piste :** C. **Dépend de :** S5, C6.

**Files:**
- Create: `apps/web/src/space/useFavorites.ts`, `useFavorites.test.ts`
- Modify: `apps/web/src/api.ts` (`getFavorites`, `setFavorite`), `space/useSpaceTree.ts` (`onRead`), `App.tsx`, `sidebar/Sidebar.tsx`, `sidebar/SectionList.tsx` (entrée du menu d'une ligne), `page/PageMenu.tsx`, `page/PageView.tsx`, `icons.tsx`, `theme.css`
- Test: `apps/web/e2e/favorites.spec.ts` (neuf)

**Interfaces:**
- Consumes: les routes de S5.
- Produces:
  - `SpaceTree.onRead(listener: (spaceId: string) => void): () => void` : appelé à la fin de chaque lecture de l'arbre qui a abouti, **même quand elle rend le même arbre** (`sameNodes` garde alors l'objet chargé, et rien d'autre ne le dirait) ;
  - `useFavorites(spaceId: string | null, onTreeRead: SpaceTree["onRead"], options: { afterLogin?: () => Promise<void> }): { pages: string[]; toggle(pageId: string): Promise<void>; error: string | null }` : relit par `scheduleRereads` après **chaque** lecture de l'arbre qui a abouti (un favori n'avance pas la version de l'espace, R254 : c'est au retour sur l'onglet, ou quand l'espace change, que l'arbre est relu, et les favoris avec lui) et après chaque `toggle` ; une lecture qui trouve la session perdue rend `SESSION_LOST` et repart au retour du compte (`waitForAccount`, comme `useComments` et `useVersions`) ; l'état est remis à zéro quand l'espace change ; plus rien après le démontage ;
  - `PageMenu` et le menu d'une ligne gagnent « Ajouter aux favoris » ou « Retirer des favoris ».

- [ ] **Step 1 : tests Vitest, vus rouges.** `favorites_are_read_after_the_tree_never_before` (la première lecture part quand l'arbre est là : rien sur le chemin de l'ouverture) ; `favorites_are_reread_after_every_tree_read` (aussi quand l'arbre rendu est le même, et que sa version n'a pas bougé : c'est le cas d'un favori posé dans un autre onglet) ; `favorites_are_read_again_once_the_account_is_back` (première lecture en `401` : relue au retour du compte, sans autre demande) ; `a_toggle_shows_at_once_and_is_put_back_if_refused` (le refus s'affiche tel quel) ; `a_reread_asked_during_a_read_is_done_once_after_it` ; `favorites_of_the_previous_space_are_not_shown` ; `favorites_show_only_pages_of_the_tree`.
- [ ] **Step 2 : écrire.** « Favoris » est la première section de la barre latérale (`ShortList`), absente quand elle est vide. Le menu « … » de la page garde « Historique » là où la partie A l'a mis (à partir de « Peut modifier ») ; le favori y est à tout niveau.
- [ ] **Step 3 : e2e, vus rouges.** `favorite_appears_in_the_other_tab_of_the_same_account_when_it_comes_back` (deux onglets du même compte : posé dans l'un, le favori paraît dans l'autre à son retour au premier plan, sans recharger, par le chemin du retour sur l'onglet, R126) ; `favoriting_makes_no_one_else_read_the_tree` (à deux comptes : pendant que l'un pose et retire dix favoris, l'onglet de l'autre ne fait aucune lecture de l'arbre ; témoin : un renommage lui en fait faire une) ; `favorite_of_one_is_not_shown_to_the_other` (à deux comptes) ; `favorite_disappears_with_the_access_and_names_nothing` (l'accès retiré : la section ne porte plus la page, et aucune réponse reçue par l'onglet après le retrait n'a son titre ; témoin : avant le retrait, l'arbre le portait) ; `favorite_from_the_page_menu_by_keyboard`.
- [ ] **Step 4 : relancer.** Commits : `web : favoris (R254)`, `e2e : favoris`. Conception §4.2, registre.

### Task C8 : fichiers, envoyer, afficher, télécharger

**Piste :** C. **Dépend de :** S4a, C1a (le banc), C1b (`schema.tsx`, `Editor.tsx`).

**Files:**
- Create: `apps/web/src/editor/files.ts`, `editor/blocks/FileBlock.tsx`, et leurs tests
- Modify: `apps/web/src/api.ts` (`getFileLimits`, `uploadFile`), `editor/Editor.tsx` (`uploadFile` pour l'éditeur de la page seul ; `resolveFileUrl` pour l'éditeur de la page **et** pour `PreviewEditor` ; refus), `editor/schema.tsx` (le bloc `file`, mêmes props, et `meta.fileBlockAccept: ["*/*"]`, `parse` et `toExternalHTML` du bloc d'origine), `editor/LazyEditor.tsx`, `page/PageView.tsx` (bandeau d'un envoi refusé), `gestures.test.ts`, `theme.css`, `apps/web/e2e/servers.ts` et `apps/web/playwright.config.ts` (`maxFileMb`, donné au seul serveur des e2e, à 1 ; c'est la configuration qui le passe à `apiServer`)
- Test: `apps/web/e2e/files.spec.ts` (neuf), `apps/web/e2e/fixtures/` (un PNG, un HTML nommé `.png`, une vidéo **WebM** en VP8 ou VP9, son Opus, d'au moins 300 Ko pour que le déplacement demande une plage ; pas de H.264 : le Chromium de Playwright sous Linux, celui de la CI, n'a pas les codecs propriétaires, lu, non vérifié). `.gitattributes` porte déjà `*.webm binary` et `*.mp4 binary`.

**Interfaces:**
- Consumes: les routes de S4a.
- Produces:
  - `api.ts` : `uploadFile(pageId: string, file: File, signal?: AbortSignal): Promise<FileInfo>` (corps brut, `?name=` encodé) ; `getFileLimits(): Promise<FileLimits>` ;
  - `files.ts` : `filePath(url: string): { pageId: string; fileId: string } | null` (seulement un chemin `/api/v1/pages/<uuid>/files/<uuid>`, sans hôte, sans requête) ; `isFilePath(url)` ; `shownUrl(url: string): string` (l'adresse telle quelle si c'est un chemin de fichier ou une adresse `http:` ou `https:` ; `about:blank` sinon), donné à BlockNote comme `resolveFileUrl` ; `uploader(options: { pageId: string; limits: () => Promise<FileLimits>; blockType: (blockId: string) => string | undefined; onRefused: (message: string) => void; signal: AbortSignal })`, donné comme `uploadFile` : il rend `{ props: { url, name } }`, ou `{ type: "file", props: { url, name } }` quand le type servi n'est pas de la famille du bloc (`image/` pour `image`, `video/` pour `video`, `audio/` pour `audio`).

- [ ] **Step 1 : essais.** (a) Acquis pour Chromium par l'essai de la relecture du plan (R246) : sous ces en-têtes, une image s'affiche, un son et une vidéo se lisent et se déplacent. L'e2e jetable le revoit **dans le bloc** de BlockNote, par le mandataire de Vite, avec la fixture WebM, et garde un **témoin** : le même fichier servi sans les en-têtes de R246 (serveur jetable) se lit ; s'il ne se lit pas non plus, c'est le codec, pas les en-têtes. **Si, témoin vert, le bloc n'affiche pas** : Ruling, et `Content-Disposition: inline` pour les seuls types `image/`, `video/` et `audio/` de la table (S4a rouvert), le reste inchangé. (b) Que voit `fetch` d'un `403` rendu pendant que le navigateur envoie encore (un fichier juste sous la limite) ? **Si c'est une coupure** : sur toute coupure d'un envoi, le client relit la page (`usePageGate.reread`) et dit le refus qu'elle révèle. (c) Mesures dans `gestures.test.ts` : `upload_sets_url_and_name_on_the_same_element` ; `image_turned_into_a_file_keeps_its_block_id` ; `file_dropped_on_an_empty_paragraph_takes_its_place` (BlockNote change alors le type du paragraphe au lieu d'insérer un bloc : relire ce que cela écrit, et ce que devient une frappe faite au même instant dans ce paragraphe ; famille du changement de type, limite portée à la tâche D).
- [ ] **Step 2 : tests Vitest, vus rouges.** `filePath` et `shownUrl` : chemin juste ; refusés : un hôte, `//hôte/api/…`, `javascript:`, `data:`, `blob:`, `/api/v1/pages/x/files/y` (pas des UUID), un chemin suivi de `?x` ou de `/../`. `a_file_larger_than_the_limit_is_refused_before_any_request` (le message donne la limite en Mio ; les limites ne sont lues qu'au premier envoi) ; `an_upload_refused_by_the_server_is_said_as_it_says_it` (`403`, `409`, `413` : `onRefused`, et la page est relue) ; `an_upload_is_cancelled_when_the_page_closes` (plus rien après le démontage) ; `a_file_that_is_not_an_image_becomes_a_file_block` ; `file_block_offers_download_to_a_reader` (un lien « Télécharger », nommé avec le nom du fichier, `download`, vers `shownUrl` ; rien quand l'adresse n'est pas permise) ; `file_name_is_isolated_text` ; `a_version_preview_resolves_its_urls_and_never_uploads` (`PreviewEditor` : ses images passent par `shownUrl`, il n'a ni `uploadFile` ni panneau de fichier) ; `the_file_panel_still_opens_on_a_file_block` (le bloc `file` récrit garde `fileBlockAccept`) ; `registry_matches_blocknote` reste vert.
- [ ] **Step 3 : écrire.** L'attente d'un envoi dit « Envoi… » (le « Loading... » de BlockNote est écrit en dur : le masquer par le style et poser le texte par `::after`). Le bandeau d'un envoi refusé est un `role="alert"` sous la barre du haut, avec « Fermer ». Rien de neuf sur le chemin de l'ouverture : ni lecture des limites, ni requête par fichier autre que celles des blocs affichés.
- [ ] **Step 4 : e2e, vus rouges.**
  - `image_dropped_in_the_page_is_seen_by_the_other_at_once` (à deux ; le MMD vaut `![](/api/v1/pages/<id>/files/<id>) {name="photo.png"}`, et se relit par `PUT /mmd`).
  - `file_is_downloaded_by_a_reader_who_cannot_upload` (un lecteur : « Télécharger » rend les octets envoyés ; ni panneau d'envoi, ni dépôt).
  - `html_named_png_never_runs` : un HTML à script nommé `photo.png` est envoyé ; le bloc devient un bloc fichier ; ouvrir son adresse dans l'onglet déclenche un téléchargement (`page.waitForEvent("download")`), aucune boîte de dialogue, aucune navigation, `window.__ran` absent. Témoin : un vrai PNG s'affiche.
  - `image_is_no_longer_served_once_access_is_withdrawn` (l'accès retiré, la requête de l'image depuis l'onglet répond `404`, cache compris).
  - `too_large_file_is_refused_with_the_limit` (2 Mio sur le serveur des e2e : « 1 Mio »).
  - `undo_and_restore_bring_the_image_back` (le bloc supprimé, annulé : l'image s'affiche ; supprimé de nouveau, une version restaurée : elle s'affiche).
  - `video_plays_and_seeks`.
- [ ] **Step 5 : relancer**, budgets, avec un budget de plus dans `e2e/budgets.spec.ts` : une page de 1 000 blocs dont 50 images (envoyées par l'API avant la mesure) s'ouvre sous 300 ms au p95, par la mesure existante (premier bloc affiché dans l'éditeur synchronisé) ; les requêtes d'images ne retardent pas la synchro ; le temps jusqu'à la dernière image est consigné, sans budget à lui. Commits : `web : envoyer un fichier dans une page (R247, R248)`, `web : télécharger un fichier, même en lecture seule`, `e2e : fichiers`. Conception §4.2, `CLAUDE.md` (« une adresse n'est chargée que par `shownUrl` »), registre.

### Task C9 : un fichier venu d'une autre page

**Piste :** C. **Dépend de :** C8, S4b (la route de recopie).

**Files:**
- Modify: `apps/web/src/api.ts` (`copyFile`), `apps/web/src/editor/files.ts`, `files.test.ts`, `editor/Editor.tsx`
- Test: `apps/web/e2e/files.spec.ts`

**Interfaces:**
- Consumes: `POST …/files/copies` (S4b), `filePath` (C8).
- Produces: `api.ts` : `copyFile(pageId: string, body: CopyFile): Promise<FileInfo>` ; `adoptForeignFiles(editor: MementumEditor, pageId: string, copy: typeof copyFile, signal: AbortSignal): Promise<number>` : pour chaque bloc `image`, `video`, `audio` ou `file` dont l'adresse est un chemin de fichier **d'une autre page**, recopie l'attache ici et récrit `url` ; un refus laisse le bloc tel quel ; rend le nombre de blocs récrits.

- [ ] **Step 1 : essai** (e2e jetable, deux pages) : copier un bloc image de A, le coller dans B : quelle adresse porte le bloc collé, et que disent `pasteHandler` et `getChanges()` (`source.type`) ? Attendu : l'adresse de A, source `paste`. **Si le collage ne passe pas par `pasteHandler`** : appeler `adoptForeignFiles` sur `editor.onChange` quand un changement a pour source `paste` ou `drop`.
- [ ] **Step 2 : tests Vitest, vus rouges.** `a_pasted_file_of_another_page_is_copied_here_and_its_url_rewritten` ; `the_document_is_scanned_only_after_a_paste_that_carries_a_file_path` (une frappe, ou un collage de texte : aucun parcours des blocs) ; `a_refused_copy_leaves_the_block_as_it_is` (`422 invalid_source`) ; `a_file_of_this_page_and_an_external_address_are_left_alone` ; `nothing_is_copied_from_a_read_only_editor`.
- [ ] **Step 3 : écrire**, dans le `pasteHandler` de l'éditeur : le collage par défaut d'abord, puis `adoptForeignFiles` si le presse-papiers porte un chemin de fichier.
- [ ] **Step 4 : e2e, vus rouges.** `pasted_image_is_seen_by_someone_who_cannot_read_its_first_page` (Mona copie une image de sa page privée dans une page d'équipe ; Tom, qui ne voit pas la page privée, voit l'image) ; `pasted_image_survives_the_deletion_of_its_first_page`.
- [ ] **Step 5 : relancer.** Commits : `web : un fichier collé d'une autre page est rattaché à celle-ci (R249)`, `e2e : image collée d'une page à l'autre`. Registre : la limite qui reste (hors collage), à porter au README.

### Task C10 : dupliquer une page

**Piste :** C. **Dépend de :** S6, C7.

**Files:**
- Modify: `apps/web/src/api.ts` (`duplicatePage`), `space/usePageActions.ts`, `usePageActions.test.ts`, `sidebar/SectionList.tsx`, `page/PageMenu.tsx`, `App.tsx`, `icons.tsx`
- Test: `apps/web/e2e/duplicate-page.spec.ts` (neuf)

**Interfaces:**
- Consumes: `POST /api/v1/pages/{id}/duplicate` (S6).
- Produces: `PageActions.duplicate(id: string): Promise<void>` : ouvre la copie ; `canDuplicate(node: PageNode, parent: PageNode | undefined, role: Role): boolean` (`sidebar/rows.ts`), la règle de R252 : vrai pour tout membre ; pour un invité, seulement une sous-page dont la page parente est visible et modifiable.

- [ ] **Step 1 : tests Vitest, vus rouges.** `duplicate_opens_the_copy_and_lists_it` (la copie s'ouvre où le serveur l'a mise : sous la page parente, ou dans « Privé ») ; `a_refused_duplicate_is_said_as_the_server_says_it` ; `duplicate_is_offered_only_where_the_server_allows_it` (`canDuplicate` : un invité sur une page partagée à la racine, non ; un invité sur une sous-page sous une page qu'il ne peut pas modifier, non ; un membre lecteur d'une sous-page sous une page qu'il ne peut pas modifier, oui) ; `duplicate_is_asked_once_on_a_double_click`.
- [ ] **Step 2 : écrire.** « Dupliquer » entre dans le menu d'une ligne et dans le menu « … » de la page ; pendant la requête, l'entrée est inactive.
- [ ] **Step 3 : e2e, vus rouges.** `duplicated_page_appears_for_everyone_and_opens_for_who_did_it` (à deux, une sous-page d'équipe : la copie paraît chez l'autre en moins de 2 s, titre « … (copie) ») ; `duplicate_of_a_team_root_page_is_private` (l'autre ne la voit pas) ; `duplicate_of_a_subpage_one_cannot_create_beside_goes_to_private` (un membre qui lit une sous-page sans pouvoir modifier sa page parente : la copie s'ouvre dans son « Privé », et rien ne paraît sous la page parente chez l'autre) ; `duplicated_page_keeps_its_images` (l'original supprimé pour de bon, l'image de la copie s'affiche) ; `subpages_come_with_the_copy`.
- [ ] **Step 4 : relancer.** Commits : `web : dupliquer une page (R252)`, `e2e : dupliquer une page`. Conception §4.2, registre.

---

## Tâches de relais

### Task X1 : reports du lot 2, côté client (M5, M9, M7, M11)

**Dépend de :** S1. **Fichiers sans partage :** ni `api.ts`, ni `theme.css` (les classes existantes suffisent), ni `e2e/helpers.ts`. Ses e2e se lancent aux ports de la piste S (`MEMENTUM_E2E_WEB_PORT=5178`, `MEMENTUM_E2E_API_PORT=8085`).

**Files:**
- Modify: `apps/web/src/session/SessionGate.tsx`, `SessionGate.test.tsx`, `SignupPage.tsx`, `InvitePage.tsx`, `InvitePage.test.tsx`, `apps/web/src/page/TitleInput.tsx`, `apps/web/src/page/TitleInput.test.tsx` (**il existe** : onze tests de R210, qui restent verts ; un test qui change de sens est dit au registre), `apps/web/src/settings/SpaceSettings.tsx`, `apps/web/src/share/ShareMenu.tsx`
- Test: `apps/web/e2e/auth.spec.ts`, `members.spec.ts`, `sharing.spec.ts`

**À lire d'abord, pour M9 :** R210 au registre du lot 3 et `TitleInput.tsx`. R210 a déjà réglé la moitié de M9 : un champ en lecture seule reprend le titre du serveur **quand celui-ci change**, et ce qui y a été tapé ne part plus (`a_read_only_field_follows_the_remote_title_even_after_typing`). Ce qui reste : quand la page passe en lecture seule **sans** que le titre change, le texte tapé reste affiché comme le titre.

- [ ] **Step 1 : M5, test vu rouge.** `first_account_screen_follows_the_instance` (`SessionGate.test.tsx`) : l'écran « Bienvenue dans Mementum » est affiché (`setup` vrai) ; le serveur répond `403 signup_closed` à l'inscription, et `GET /api/v1/info` dit désormais `setup: false` : l'écran devient celui de la connexion, avec « Un compte a déjà été créé sur cette instance. Connecte-toi, ou demande une invitation. » ; le nom, l'adresse et le mot de passe tapés ne sont envoyés nulle part d'autre. Même relecture après `409 email_taken`, et après un échec réseau du formulaire. Aujourd'hui, l'écran reste, sans lien vers la connexion : c'est le défaut.
- [ ] **Step 2 : M9, tests vus rouges** (`TitleInput.test.tsx`) : `unsaved_title_is_not_shown_as_the_title_once_read_only` (un titre tapé, le champ encore en édition, `readOnly` passe à vrai : le champ montre le titre du serveur ; un message `role="status"` dit « Ce titre n'a pas été enregistré : », suivi du texte tapé, isolé) ; `unsaved_title_is_offered_again_when_the_page_can_be_edited_again` (le message reste, et gagne un bouton « Reprendre ce titre » ; le champ garde le titre du serveur, et rien ne part : R210 tient ; le bouton pose le texte dans le champ comme une saisie, focus compris, et le message part ; « Écarter » le jette) ; `a_remote_rename_while_read_only_keeps_the_offered_text` (le champ suit le titre distant, le texte gardé reste proposé) ; témoin : `a_saved_title_shows_no_message`.
- [ ] **Step 3 : M7 et M11, tests vus rouges.** `an_invitation_that_gives_nothing_says_so_and_stays_valid` (`InvitePage.test.tsx` : `changed: false` → « Tu as déjà cet accès. Le lien reste valable pour quelqu'un d'autre. », et un lien vers l'espace ou la page ; aucune navigation d'office) ; dans les réglages et dans le menu Partager, `a_pending_page_invitation_names_its_page` (« Peut modifier sur « Plan » », le titre isolé ; « Sans titre » pour un titre vide).
- [ ] **Step 4 : écrire**, puis un e2e par report, vu rouge : `first_account_screen_gives_way_once_an_account_exists`, `owner_who_tries_his_own_link_does_not_kill_it` (le lien d'une invitation créée depuis le menu Partager d'une **page d'« Espace »**, où le propriétaire n'a aucune ligne à son nom : c'est le cas que le niveau effectif règle ; l'invité l'accepte ensuite), `title_typed_while_losing_the_right_is_not_shown_as_saved` (lectures de la page **et** de l'arbre retenues), `pending_page_invitation_shows_the_page_title`.
- [ ] **Step 5 : relancer** Vitest, typecheck, `auth.spec.ts`, `members.spec.ts`, `sharing.spec.ts`. Commits, un par report : `web : l'écran du premier compte suit l'état de l'instance (M5)`, `web : une invitation sans effet le dit (M7)`, `web : un titre non enregistré n'est pas montré comme enregistré (M9)`, `web : une invitation de page en attente nomme sa page (M11)`. Registre.

### Task X2 : fidélité MMD des blocs riches

**Dépend de :** C1b (la liste `CALLOUT_ICONS`, telle qu'elle est sur `main`). **Fichiers sans partage :** ceux-ci, qu'aucune autre tâche ne touche. L'export passe par un e2e : il se lance aux ports de la piste S (`MEMENTUM_E2E_WEB_PORT=5178`, `MEMENTUM_E2E_API_PORT=8085`).

**Files:**
- Create: `apps/web/src/editor/fixtures/rich-blocks.input.json`, `spec/mmd/fixtures/rich-blocks.mmd`, `spec/mmd/fixtures/yjs/rich-blocks.bin`, `spec/mmd/fixtures/blocknote/rich-blocks.json`
- Modify: `apps/web/e2e/fidelity.spec.ts`, `crates/mementum-crdt/tests/blocknote_fixtures.rs`, `crates/mementum-crdt/src/guard/tests.rs`

- [ ] **Step 1 : la fixture d'entrée**, ce que les écrans de ce plan peuvent produire et que `all-blocks` et `non-default` ne portent pas : un callout pour chacune des dix couleurs et des huit icônes de `CALLOUT_ICONS`, avec un enfant ; une équation de plusieurs lignes, une qui contient une ligne `$$`, une vide ; des formules en ligne en début, milieu et fin de texte, dont une que `$…$` ne porte pas ; un signet dont l'adresse a une requête et un fragment ; une table des matières ; un lien de page à l'identifiant UUID et au titre à caractères à échapper (`&`, `<`) ; les trois sortes de mention avec des identifiants réels (UUID, `2026-12-01`), une mention en tête de paragraphe ; une image, une vidéo, un son et un fichier dont l'adresse est un chemin de fichier de l'instance, et un nom à guillemets et à accents.
- [ ] **Step 2 : le MMD attendu, écrit à la main** d'après le §6.2, jamais recopié de la sortie du serveur.
- [ ] **Step 3 : exporter** (`EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec playwright test e2e/fidelity.spec.ts`, aux ports de la piste S), puis les tests Rust, vus rouges avant les fichiers : `reads_blocknote_fixture_rich_blocks`, `writes_like_blocknote_rich_blocks`, et `rich-blocks` ajouté à `blocknote_state_is_returned_with_the_same_tree` et aux listes de `guard/tests.rs`. **Un écart est un défaut du convertisseur ou du MMD attendu : il est porté au contrôleur, pas corrigé en retouchant la fixture.**
- [ ] **Step 4 : commits** `tests : fixture des blocs riches, de BlockNote au MMD et retour`. Registre.

### Task D : documentation et recette de la partie B

**Dépend de :** toutes.

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `docs/REPRISE.md`, `docs/specs/2026-11-09-mementum-design.md`, l'en-tête de `crates/mementum-server/src/lib.rs`
- Create: `docs/recette/2026-11-12-p1-lot3b-recette.md`

- [ ] **Step 1 : relire chaque document contre le code final**, pas contre ce plan : le tableau des routes ; les réglages (`MEMENTUM_MAX_FILE_MB`, où sont les fichiers, qu'ils font partie d'une sauvegarde avec la base) ; **sauvegardes et retour en arrière** (la copie d'avant migration ne contient pas les fichiers ; une base restaurée sans son dossier `files/` montre des fichiers indisponibles ; une base revenue en arrière ne fait rien supprimer : les contenus qu'elle ne connaît pas restent sur le disque, comptés au journal du démarrage) ; les limites connues, telles que mesurées (pas d'aperçu de signet ; SVG et PDF en téléchargement, une image HEIC ou AVIF en bloc fichier ; un fichier collé hors du collage reste lié à sa première page ; les fichiers d'une page pèsent jusqu'à sa suppression ; les récents par navigateur, effacés à la déconnexion ; un favori ne paraît dans un autre onglet qu'à son retour au premier plan ; une mention ne prévient personne ; ce que devient une frappe faite pendant qu'un autre insère une mention ou une formule, change le type du bloc, ou dépose un fichier sur un paragraphe vide ; une page dupliquée naît sans version) ; les deux phrases que le contrôleur a demandées au README : « d'ici le lot 7, n'ouvre l'instance qu'à des personnes de confiance : qui peut modifier une page peut remplir le disque, et un disque plein arrête toute écriture » (R247), et « mentionner une page privée dans une page d'équipe en montre le titre : qui lit la mention lit le titre copié, même sans voir la page » (R251) ; `CLAUDE.md` (la puce « En-têtes de l'API », la migration `0004`) ; conception §9 et §11 ; les « À porter au README » du registre.
- [ ] **Step 2 : la checklist de recette**, sur ce qu'un test ne juge pas : déposer une image, une vidéo, un PDF, à deux ; le dessin des blocs riches (direction A « Papier ») ; écrire une équation ; `@` et `[[` au clavier, sur un clavier AZERTY ; Ctrl+D ; dupliquer une page d'équipe et la retrouver dans « Privé », dupliquer une sous-page et la retrouver à côté d'elle ; les favoris d'un onglet à l'autre (au retour sur l'onglet) ; le fil d'Ariane d'un invité ; mentionner une page privée dans une page d'équipe, et lire ce qu'en voit un autre membre (R251) ; essayer soi-même le lien d'une invitation de page, puis le faire accepter par son destinataire (M7) ; les quatre reports du lot 2 qui se voient. La checklist rappelle en tête les cinq choix de produit pris par le contrôleur le 2026-11-13 (encadré en tête de ce plan), pour que le propriétaire les confirme ou les défasse.
- [ ] **Step 3 : commit** `docs : partie B du lot 3`.

## Fin de la partie B

1. Relecture finale de tout ce qui a changé depuis l'ouverture de la partie B, en trois passes, chaque constat prouvé par un test jetable : **sécurité** (les fichiers d'abord : chaque route relue comme une attaque, chemins, en-têtes, droits pendant l'envoi, ménage ; puis les titres qui pourraient sortir par les favoris, le fil d'Ariane, les sélecteurs) ; **intégration client et serveur** (chaque fonction d'`api.ts` a sa route, chaque refus son affichage) ; **documentation contre le code**. Vague de correction, re-relectures ciblées. La liste des constats mineurs des relectures de tâche lui est donnée.
2. Sur un clone neuf, étape par étape comme la CI : `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `cargo test --release -p mementum-sync -- --ignored`, `cargo test --release -p mementum-server -- --ignored`, `cargo deny check advisories`, `cargo deny check licenses bans`, `corepack pnpm install --frozen-lockfile`, typecheck, Vitest, tous les e2e, les budgets du navigateur, les licences npm.
3. Essai de migration sur une copie d'une base de la partie A (`0001` à `0003`) et sur une copie d'une base du lot 2 (jamais `data/instance`) : les tables des deux parties, la copie d'avant migration (`avant-0004.bak`, `avant-0003.bak`), un fichier envoyé puis relu après un redémarrage ; puis la copie d'avant migration remise en place : le serveur redémarre, et aucun fichier n'a quitté le disque.
4. L'atelier du client est retiré, son `target` supprimé.
5. Registre : mesures de clôture, reports (au lot 7 : quotas, envois simultanés, ménage des fichiers sans bloc et des contenus que la base ne connaît pas, miniatures ; à placer : les lignes de la P1 du §4.5 sans lot), ce qui attend le propriétaire : les cinq choix pris par le contrôleur le 2026-11-13 (R247 pour l'absence de quota, R251 pour le titre copié par une mention, R252 pour la place de la copie, R254 pour le favori qui n'avance pas la version, R257 pour M7), et R246 pour le SVG et le PDF, R250, R255. `docs/REPRISE.md` à jour : le lot 3 est clos.
