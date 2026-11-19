# Mementum — Phase 1, lot 4 « Recherche » : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **État de ce plan (2026-11-19)** : écrit d'après la conception et le code de `main` à `5e7514a`, **pas encore relu par un relecteur indépendant, ni par le propriétaire**. Rien n'a été compilé ni lancé pour l'écrire : ce qui est dit du code a été lu (« Ce qui a été vérifié avant d'écrire »), ce qui est supposé est dit supposé. Il ne s'exécute pas avant : (1) la clôture du lot 3 (la migration `0004` figée, R243) ; (2) la relecture du plan ; (3) les réponses de Lenny aux « Décisions à prendre par Lenny », ou son accord pour avancer sur ce que le plan suppose en attendant.

**Goal :** dans un espace, on tape quelques mots et on retrouve les pages qu'on a le droit de lire, par leur titre ou par leur texte, avec un extrait, et on arrive sur le bloc trouvé ; sous une page, on lit quelles pages mènent à elle. Personne ne trouve, ne devine ni ne compte une page qu'il ne voit pas.

**Le lot s'exécute en 14 tâches** : piste serveur S0 à S7 (8), piste client C1 à C3 (3), transverses X1, X2 et D (3).

**Architecture (en bref ; le détail est plus bas) :**
- `mementum-store` : une migration `0005_search.sql` crée `blocks_index` (la projection du document, conception §5.3), `page_links` (les rétroliens), `search_state` et `search_outbox` (où en est l'indexation) ; une voie neuve de la file d'écriture, `Lane::Index`.
- `mementum-search` (crate neuve, conception §8) : la projection d'une page en lignes d'index (pure), la lecture bornée d'une requête, et l'index plein texte Tantivy (conception §5.2, §5.7), derrière une interface étroite.
- `mementum-sync` : le hub dit quelles pages ont été écrites (un ensemble, sans attente) ; une lecture seule d'une page gardée, hors de son acteur.
- `mementum-server` : l'indexeur (une tâche de fond, jamais sur le chemin d'une frappe), deux routes (`GET /api/v1/spaces/{id}/search`, `GET /api/v1/pages/{id}/backlinks`), un débit par compte.
- `mementum-cli` : `mementum reindex`.
- `apps/web` : la fenêtre de recherche, l'arrivée sur un bloc, les rétroliens sous la page.

## Périmètre

Ce que la conception met dans ce lot, et ce que ce plan en livre :

| Sujet | Conception | Ce plan livre | Renvoyé |
|---|---|---|---|
| Recherche plein texte | §9, P1 : « recherche plein texte » ; §5.7 : « Tantivy indexe les titres et les blocs […] Les résultats sont filtrés par permissions au moment de la requête » | titres et texte des blocs d'un espace, filtrés par `access.rs` à chaque requête, extrait et bloc trouvé | recherche hybride, vecteurs, rerank : P4 (§5.7) |
| Projection du document | §5.3 : `blocks_index(block_id, node_id, type, text, path, hash)`, « recherche, rétroliens, ancres agents » | `blocks_index` et `page_links` | les ancres des agents (`^bN`, §6.2) : lot 5 |
| Reconstruction | §5.7 : « Les deux index sont reconstructibles : `mementum reindex` » | la commande, et la reconstruction d'elle-même au démarrage quand l'index ne correspond plus à la base | l'index vectoriel : P4 |
| Rétroliens | §4.2 (« Page : […] rétroliens »), §4.5 (« rétroliens au lot 4 ») | les pages que le lecteur voit et qui mènent à la page ouverte | vue graphe : P3 (§4.5) ; `[[Page#Titre]]` : dans aucun lot (décision L11) |
| Entrée dans la barre latérale | §4.2 : « sélecteur d'espace, recherche, Accueil… » | l'entrée « Rechercher » et son raccourci | Accueil, Boîte de réception, Agents, Modèles : autres lots |
| Palette de commandes | §4.2 : « Palette de commandes (Ctrl+K) : navigation, actions, et questions à l'IA avec citations » | la navigation par la recherche seulement | les actions : dans aucun lot ; les questions à l'IA : P4 (écart E5) |
| Outil `search` des agents | §6.5, outil 1 : « Recherche hybride. Filtres : espace, type, date, auteur » ; §9 : MCP v1 en P1 | la route REST que l'outil appellera | l'outil MCP, les jetons, les filtres : lot 5 (écart E1, décision L7) |

**Hors de ce plan :** la recherche dans les commentaires, dans le contenu des fichiers (PDF), dans l'historique et dans la corbeille (décisions L3, L4) ; la recherche à l'intérieur d'une base (§4.2, P2) ; les `#tags` (§4.6, lot 6 et P2) ; la tolérance aux fautes de frappe ; le surlignage des mots dans la page ouverte ; les plafonds généraux par compte et par adresse IP (lot 7 : ce plan ne pose que le débit de ses propres routes) ; un quota de disque pour l'index (lot 7, comme R247).

---

## Ce qui a été vérifié avant d'écrire

**Lu dans le code de `5e7514a`** (chemin et nom réels) :

- Migrations : `crates/mementum-store/migrations/0001` à `0004`. `nodes` a `kind IN ('folder','page')`, `title`, `updated_at`, `trashed_at` ; `doc_updates(node_id, seq, update_blob, created_at, actor_id)`, `doc_snapshots(node_id, seq, state_blob, created_at)`. Aucune table d'index.
- `nodes.updated_at` n'est écrit qu'à la création et par `update_node` (titre, place : `nodes.rs`, l'`UPDATE` de la ligne 750) : **il ne bouge pas quand le contenu change**. Il ne peut donc servir ni de date de modification, ni de signe qu'une page est à réindexer.
- File d'écriture : `crates/mementum-store/src/queue.rs`, `enum Lane { Page(Uuid), Account(Uuid), Files, Copies, Common }` ; `Store::write_in(Lane)` et `write_conn_in(Lane)` (`docs.rs`). Le journal d'une page s'écrit par `Store::append_updates` (voie de la page), l'instantané par `put_snapshot`, qui retire le journal qu'il couvre.
- Le contenu d'une page ne change que par : l'acteur (`crates/mementum-sync/src/actor.rs` : `client_update`, `write`, `server_write`, qui passent par `Actor::persist` et `Actor::persist_restore`) ; la création (`Hub::create_page_doc`, qui charge la page, et `load`, qui écrit l'instantané de `seq` 0 d'une page sans document) ; la duplication (`Store::duplicate_pages`, `crates/mementum-store/src/duplicate.rs`, sans acteur) ; la suppression (`Store::delete_node`, qui retire à la main les lignes des tables liées).
- La compaction se fait hors de l'acteur (`compact`, `actor.rs`), sur un fil bloquant, puis dans la voie de la page (R826).
- `load` et `replay` (`actor.rs`) relisent une page gardée en écartant des entrées du journal, avec une ligne `error` par entrée écartée ; ils sont privés au module, et `load` **écrit** (l'instantané d'une page sans document).
- `Hub` (`crates/mementum-sync/src/hub.rs`) : `read_blocks` (par l'acteur), `load_page`, `forget`, `shutdown`, `live_pages`. Rien n'y dit aujourd'hui qu'une page vient d'être écrite.
- Droits : `AppState::space_access` et `find_space_access` (`crates/mementum-server/src/access.rs`) rendent un `Arc<SpaceAccess>` ; `SpaceAccess::visible()` (`crates/mementum-acl/src/access.rs`) donne chaque nœud vu avec `NodeAccess { level, section, in_trash, visible_parent }`. `Store::node_titles` existe.
- Annonces : `Store::changes()` (`spaces.rs`) annonce l'espace dont `lsn` a avancé (titre, place, corbeille, création, suppression), pour les écritures de ce `Store` seulement.
- Débits existants, modèles de celui de ce lot : `crates/mementum-server/src/mmd_reads.rs` (quatre calculs de front sur le fil bloquant, les comptes tour à tour, le tour rendu à la fin du calcul) et `duplications.rs` (une à la fois par compte, budget par fenêtre, `429` avec `Retry-After`).
- `serve` (`crates/mementum-server/src/lib.rs`) finit par `hub.shutdown()` puis `store.close()` ; `to_completion` garde la portée `writes_by`.
- La CLI (`crates/mementum-cli/src/main.rs`) a `serve` et `user set-password`, pas de `reindex`.
- Le modèle lu d'une page : `mementum_md::Block { id, kind, props, content, children }`, `Inline::{Text, Link { href }, Mention { kind, id, label }, Math { latex }, HardBreak }` (`crates/mementum-md/src/model.rs`). Le registre (`spec/schema/blocks.json`) : 24 types de blocs ; portent du texte en ligne `paragraph`, `heading`, les quatre listes, `quote`, `codeBlock`, `callout` ; `table` ; portent du texte dans des props `image`, `video`, `audio`, `file` (`caption`, `name`), `bookmark` (`url`), `equation` (`latex`), `pageLink` (`pageId`, `title`).
- Client : `apps/web/src/space/pageDirectory.ts` (`findPages`, les titres de l'arbre publié), `editor/blocks/PagePicker.tsx`, `sidebar/Sidebar.tsx` (un `nav` : `SpaceSwitcher`, `ShortList`, `SectionList`), `router.ts` (`/p/:id`, sans fragment de bloc), `editor/blocks/Toc.tsx` (`goToHeading` : mener à un bloc, éditeur en lecture seule compris). Aucun raccourci Ctrl+K dans `apps/web/src`.
- SQLite : `libsqlite3-sys` 0.37.0, tel que compilé par sqlx, pose `-DSQLITE_ENABLE_FTS5` (son `build.rs`, lu dans le registre de cargo du poste) : FTS5 est disponible sans dépendance de plus (utile à la décision L1).
- `spike/RESULTS.md` **ne dit rien de la recherche** : la phase 0 n'a mesuré ni Tantivy ni FTS5 (écart E4).
- Le dernier Ruling du lot 3 est R889.

**Supposé, sans l'avoir lu ni essayé** (chaque point est repris par la tâche qui le vérifie) :

- Tantivy : sa version courante, sa licence (MIT) et celles de ses dépendances contre `deny.toml`, sa mémoire au repos, son temps d'ouverture, le coût d'un filtre sur des dizaines de milliers d'identifiants, la charge utile d'une validation (`payload`), le verrou de son écrivain, ses tokeniseurs (pliage des accents, racinisation française). Tout cela est l'objet de **S0**, avant toute autre tâche.
- BlockNote 0.57 prend Ctrl+K pour créer un lien : cherché dans `apps/web/src` (absent) et dans le `dist` de `@blocknote/core` (rien trouvé sous la forme `Mod-k`), sans conclure. C1 le vérifie dans Chromium.
- La sorte d'une mention de page s'écrit `page` dans `Inline::Mention::kind` : à lire dans `mementum-schema` en S2.

---

## Global Constraints

Celles des plans du lot 3 restent valables telles quelles (langue, commits sans aucune ligne d'attribution d'outil, ne jamais arrêter un processus qu'on n'a pas lancé, `data/instance` jamais ouvert par un binaire d'un lot en cours, aucun budget relâché, aucun test existant affaibli, chaque test vu rouge d'abord pour la bonne raison, pas d'attente fixe, un témoin vert pour chaque test qui prouve une absence). Ce qui suit vaut pour toute tâche de ce plan.

**Droits**
- Les droits passent par `access.rs`, et nulle part ailleurs. L'index ne porte **aucun** droit, aucun niveau, aucune liste de lecteurs, et pas l'état de corbeille : ce qui est permis est calculé à chaque requête, pour le compte qui demande, sur `SpaceAccess`.
- **Rien d'une page que `access.rs` ne montre pas ne sort** : ni résultat, ni titre, ni extrait, ni identifiant, ni fil d'Ariane, ni nombre de résultats, ni suggestion. La corbeille ne sort jamais. Le temps de réponse ne doit pas dépendre de ce que portent les pages non vues plus que ne le fait déjà une lecture de l'arbre (voir « La requête et les droits »).
- Ordre des refus : `401`, `409 account_changed`, `404` d'un espace où l'on n'a aucun rôle (la réponse d'un espace inconnu) ou d'une page qu'on ne voit pas (**exactement** la réponse d'une page inconnue), puis `422`, puis `429`. Les routes neuves entrent dans `space_routes_by_role` (`tests/spaces.rs`) et `foreign_page_is_indistinguishable_from_missing` (`tests/permissions.rs`, par `page_routes`), et dans la matrice de `tests/expected_account.rs`.
- Un message de refus tutoie. La requête d'une personne n'est **jamais journalisée**, à aucun niveau, ni recopiée entière dans un message d'erreur (`mementum_md::excerpt`, R434).

**Indexation**
- **Aucun travail d'indexation sur le chemin d'une frappe** : l'acteur d'une page ne fait qu'ajouter son identifiant à un ensemble en mémoire, sans attente ni allocation proportionnelle à la page. Il ne lit pas ses blocs pour l'index, n'encode rien pour lui, n'attend aucun tour d'écriture pour lui.
- Les écritures de l'index dans SQLite passent par la file d'écriture, dans une voie à elles (`Lane::Index`), une transaction bornée à la fois ; jamais par `&self.pool`, jamais dans la voie d'une page ni d'un compte. Une transaction de l'index n'en ouvre pas une seconde, et l'indexeur ne mène jamais deux écritures du store ensemble dans une tâche (`join!`, `select!` : la garde de R518 paniquerait en debug).
- L'indexeur ne passe par aucun acteur : il relit ce que le store garde. Tout calcul dont la durée dépend d'un contenu (relire une page, la projeter, écrire dans Tantivy, chercher) tourne sur le fil bloquant, jamais sur un fil du moteur (R863).
- L'index est **dérivé** : rien n'y est la seule copie de quoi que ce soit. Perdu, abîmé ou d'une autre base, il se reconstruit. Aucune écriture de l'index n'avance `rev` ni `lsn`, et n'annonce rien sur `Store::changes()`.
- Toute lecture d'un contenu venu d'un client croît comme lui (R380) : la projection et la lecture d'une requête ont leur forme dans les tests de linéarité, sous les bornes absolues de R433.
- **Aucun bloc, aucune prop, aucune marque ne s'ajoute au schéma** : `SCHEMA_VERSION` reste à 1. Rien n'est écrit dans un document par ce lot.

**Client**
- Rien sur le chemin de l'ouverture d'une page ni de l'application : la fenêtre de recherche et les rétroliens sont chargés à la demande ; les rétroliens ne sont lus qu'après la première synchro de la page (la règle des commentaires, R175).
- Tout appel passe par `send` d'`api.ts` (compte attendu, rejeu d'un `401` tardif). Les relectures passent par `scheduleRereads` et `afterLogin` (`rereads.ts`).
- Texte venu d'ailleurs (titres, extraits) : toujours un nœud de texte, jamais du HTML ni un `style` ; `dir="auto"`, `unicode-bidi: isolate`. **Le serveur ne rend jamais de HTML d'extrait** : le texte, et des plages à surligner.
- Clavier : tout se fait au clavier ; un raccourci se reconnaît à `event.key`, jamais à `event.code`, sans avaler un caractère tapé avec AltGr ; le focus ne tombe jamais sur `body` ; Échap ne détruit jamais un texte tapé ; rien ne bouge sous le pointeur ; une ligne active se tient par son identifiant, jamais par son rang (R572).

**Exécution**
- **Dépendances** : Tantivy et ce qu'il tire, rien d'autre ; `cargo deny check licenses bans` et `node tools/check-npm-licenses.mjs` restent verts. Aucune dépendance npm neuve.
- **Migration** : une pour le lot, `0005_search.sql` ; elle peut changer jusqu'à la clôture du lot (R139, R243) : aucun binaire du lot 4 n'ouvre une instance que quelqu'un regarde avant la clôture. Après l'avoir ajoutée, toucher `crates/mementum-store/src/lib.rs` (R269).
- **Ports des e2e** : piste C, `MEMENTUM_E2E_WEB_PORT=5177` et `MEMENTUM_E2E_API_PORT=8084` ; piste S et transverses, `5178` et `8085`. Jamais 5173, 5175, 8080, 8090.
- **Pousser** : l'accord de Lenny valait pour le lot 3 seulement (`docs/REPRISE.md`). Rien ne se pousse au lot 4 sans une demande neuve (décision L12).
- **Registre** : `docs/superpowers/logs/2026-11-19-p1-lot4-ledger.md`, forme `Ruling RNN: <décision> — <pourquoi> — <coût si faux>`. **Le lot 4 numérote à partir de R900** : R890 à R899 restent à la clôture du lot 3. Les décisions de ce plan sont R900 à R915 ; l'exécution suit.
- **Mémoire et disque du poste** : une compilation à la fois, `CARGO_INCREMENTAL=0`, `TMP` et `TEMP` sur un dossier de la tâche, le `target/` d'un atelier supprimé dès qu'il a rendu. Tantivy allonge la compilation en release : le mesurer en S0.

## Review Focus

1. **Une page qu'on ne voit pas ne se trouve ni ne se devine.** Un invité, un membre devant une page privée d'un autre, un compte d'un autre espace : aucun résultat, aucun titre dans un fil d'Ariane, aucun rétrolien, et la même réponse (corps et forme) qu'on cherche un mot qui n'existe nulle part ou un mot que seule une page non vue porte. Tests : S5, `a_word_only_an_unseen_page_holds_answers_like_a_word_nobody_wrote` ; S6, `a_backlink_from_an_unseen_page_is_not_listed` ; X1, e2e à trois comptes.
2. **Un droit retiré vaut à la requête suivante**, sans attendre l'indexeur : accès retiré, page fermée aux membres, page mise à la corbeille, compte sorti de l'espace. Test : S5, `withdrawn_access_hides_the_page_at_the_next_query` (l'indexeur arrêté pendant le test).
3. **L'indexeur ne retarde aucune frappe**, ni par l'acteur, ni par la file d'écriture, ni par le processeur. Tests : S4, `the_actor_only_names_the_page_it_wrote` (compteur, sans minutage) ; X2, la mesure de la frappe pendant l'indexation d'un espace entier.
4. **Une entrée hostile** : une requête faite pour coûter (longue, mille termes, opérateurs, préfixe d'une lettre), une page faite pour coûter à l'index (un bloc de plusieurs Mio, cent mille liens, une écriture en boucle), une boucle de recherches d'un compte. Tests : S2, linéarité ; S5, `a_burst_of_searches_of_one_account_does_not_delay_another`.
5. **Un index qui n'est pas celui de la base** (base restaurée, dossier recopié d'ailleurs, arrêt brutal entre deux validations) : aucun extrait d'un texte que la base n'a pas. Tests : S3, S7.
6. **Ce qui a été effacé ne reste pas trouvable** au-delà du délai d'indexation, et ce que seule une version de l'historique garde n'est jamais trouvable (l'historique demande « Peut modifier », R203 ; la recherche, « Peut lire »). Test : S4, `erased_text_leaves_the_index`.

---

## Décisions de ce plan

Prises pour écrire le plan ; celles qui sont des choix de produit sont reprises dans « Décisions à prendre par Lenny ».

- **R900** : le moteur est **Tantivy**, comme le dit la conception (§5.2, §5.7, §12 « Mes choix » n° 7), derrière une interface étroite de `mementum-search` (`SearchIndex`) ; S0 le mesure d'abord, et s'arrête si un budget du §5.10 ne tient pas (décision L1).
- **R901** : `blocks_index` vit dans SQLite, clé `(node_id, block_id)` ; la colonne `path` de la conception n'est pas créée (écart E3). Tantivy est nourri **depuis `blocks_index`**, jamais depuis les documents : la base reste la source, l'index plein texte une copie qu'on refait.
- **R902** : l'unité de l'index plein texte est le **bloc**, et chaque bloc y porte aussi le titre de sa page, dans un champ à part, mieux noté ; une page a en plus une entrée pour son titre seul. Une requête de plusieurs mots les veut tous, dans le titre ou dans un même bloc. Ce qu'on perd : deux mots écrits dans deux blocs différents d'une page ne la trouvent pas (limite écrite ; la P4 la lève par la recherche hybride).
- **R903** : le fil d'Ariane d'un résultat est **calculé à la requête**, d'après ce que le compte voit (`visible_parent`), jamais lu dans l'index, et les titres des pages parentes ne sont pas un texte cherché (écart E2).
- **R904** : le titre d'un résultat est lu dans `nodes` à la requête, jamais dans l'index.
- **R905** : la réponse ne porte **aucun total**, seulement « il y en a d'autres ».
- **R906** : une page est réindexée 2 s après sa dernière écriture, 15 s au plus après la première écriture non indexée (`Config::index_after`, `index_at_most`, réglables pour les tests ; décision L9).
- **R907** : l'indexeur lit ce que le store garde, par une lecture seule neuve de `mementum-sync`, pas par l'acteur de la page.
- **R908** : les écritures de l'index dans SQLite ont leur voie, `Lane::Index`, sans priorité, comme `Lane::Copies` (R607) : une frappe n'attend jamais plus d'une transaction de l'index.
- **R909** : la cohérence entre la base et Tantivy tient à une **époque** : un identifiant tiré une fois par base et un compteur, gardés dans la base et dans la charge utile de chaque validation de Tantivy ; au démarrage, un écart autre que « une validation d'avance » fait tout reconstruire.
- **R910** : débit de la recherche, par compte, en mémoire : un calcul en cours et une requête en attente au plus ; une requête neuve prend la place de celle qui attendait, qui reçoit `429 search_busy` ; quatre calculs de front pour l'instance, les comptes tour à tour.
- **R911** : bornes d'une requête : 200 caractères, 8 termes, pas de syntaxe (ni opérateur, ni joker, ni expression rationnelle : la requête est bâtie par programme, jamais donnée à un analyseur de requêtes) ; préfixe sur le dernier terme seulement, à partir de 2 caractères ; 20 résultats par réponse, 5 réponses à la suite au plus.
- **R912** : bornes de la projection (valeurs de départ, à mesurer en S2) : 64 Kio de texte par bloc et 8 Mio par page, coupés entre deux caractères, la page notée `truncated` ; 10 000 liens par page.
- **R913** : `mementum reindex` tourne **serveur arrêté** ; serveur lancé, il refuse en le disant (écart E7).
- **R914** : la recherche porte sur un espace, celui qu'on affiche (décision L2).
- **R915** : une instance existante est indexée d'elle-même, en fond, après son premier démarrage sous ce lot ; la réponse d'une recherche dit tant que l'espace n'est pas à jour (décision L13).

---

## Architecture

### Quoi indexer

Une ligne de `blocks_index` par bloc qui porte du texte, pour chaque page (`kind = 'page'`) hors des pages supprimées. La corbeille **est** indexée (une page restaurée se retrouve sans attendre), et n'est jamais rendue.

| Bloc | Texte indexé |
|---|---|
| `paragraph`, `heading`, les quatre listes, `quote`, `callout`, `codeBlock` | le texte de son contenu en ligne, marques retirées ; le libellé d'une mention ; le texte d'un lien (pas son adresse) ; le LaTeX d'une formule en ligne |
| `table` | le texte de ses cellules, une espace entre deux cellules |
| `image`, `video`, `audio`, `file` | `caption` et `name` |
| `bookmark` | `url` |
| `equation` | `latex` |
| `pageLink` | `title` (le titre copié, R251) |
| `divider`, `toc`, `columnList`, `column`, `databaseRef`, `syncedBlock`, `unknown` | rien (leurs enfants sont parcourus) |

Les enfants d'un bloc ont chacun leur ligne. Un bloc sans `id`, ou dont l'`id` paraît deux fois dans la page lue, est indexé sous une clé de remplacement (`#<rang>`), que la réponse ne donne pas comme bloc à atteindre. Le titre de la page n'est pas dans `blocks_index` : il est dans `nodes`.

`page_links(node_id, block_id, target_id)` : une ligne par bloc et par page visée, pour un bloc `pageLink` (`pageId`), une mention de page, et un lien dont l'adresse est le chemin exact d'une page de l'instance (`/p/<uuid>`, avec ou sans l'origine publique du serveur). `target_id` est un UUID lu, sans clé étrangère : la page visée peut être d'un autre espace, ou ne plus exister.

### Où

```sql
-- 0005_search.sql (forme de départ ; S1 l'arrête)
CREATE TABLE blocks_index (
    node_id  TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    block_id TEXT NOT NULL,
    type     TEXT NOT NULL,
    text     TEXT NOT NULL,
    hash     BLOB NOT NULL,          -- empreinte de (type, text, liens) : ce qui n'a pas changé n'est pas récrit
    PRIMARY KEY (node_id, block_id)
) WITHOUT ROWID;

CREATE TABLE page_links (
    node_id   TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    block_id  TEXT NOT NULL,
    target_id TEXT NOT NULL,
    PRIMARY KEY (node_id, block_id, target_id)
) WITHOUT ROWID;
CREATE INDEX page_links_by_target ON page_links (target_id, node_id);

CREATE TABLE search_state (
    node_id     TEXT PRIMARY KEY NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    indexed_seq INTEGER NOT NULL,    -- le `seq` de la page que blocks_index reflète
    failed_seq  INTEGER,             -- le `seq` où la lecture a échoué : pas de nouvel essai avant qu'il change
    truncated   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE search_outbox (         -- pages dont blocks_index ou le titre a changé, pas encore validées dans Tantivy
    node_id TEXT PRIMARY KEY NOT NULL,
    gen     INTEGER NOT NULL         -- avance à chaque changement : une ligne n'est retirée que si elle n'a pas bougé
);
```

L'époque (R909) va dans `instance_settings`. Les trois premières tables partent avec leur page par la cascade, comme `favorites` (`0004`) ; S1 vérifie que `Store::delete_node`, qui retire les autres tables à la main, n'a rien à y ajouter, et met la page supprimée dans `search_outbox` dans sa transaction.

L'index Tantivy vit dans `<données>/search/`, à côté de `files/`. Champs : `space_id`, `node_id` (termes exacts, pour filtrer et supprimer), `block_id` (gardé), `title` et `text` (cherchés, gardés pour l'extrait). Un seul écrivain, tenu par l'indexeur.

### Quand

```
frappe ──► acteur ──► journal (voie de la page) ──► relais           (inchangé)
                └──► Hub::touched : insère l'id de la page dans un ensemble (sans attente)

indexeur (une tâche de fond du serveur, une page à la fois, les pages tour à tour)
  1. attend 2 s de calme sur la page (15 s au plus)                           R906
  2. relit la page gardée, hors acteur, sur le fil bloquant                   R907
  3. la projette en lignes (pur)                                              S2
  4. écrit la différence dans blocks_index / page_links / search_state,
     et la page dans search_outbox : une transaction, voie Lane::Index        R908
  5. par lots : pour chaque page de l'outbox, retire ses entrées de Tantivy
     et récrit celles de blocks_index ; valide Tantivy (époque + 1) ;
     puis, dans la base, retire de l'outbox les lignes restées au même `gen`
     et avance l'époque                                                       R909
```

Ce qui met une page à l'ordre du jour, sans que personne puisse l'oublier :
- **le contenu** : `Actor::persist` et `persist_restore`, les deux seuls points par où l'acteur écrit, nomment la page au hub après une écriture réussie ;
- **le titre, la création, la duplication, la restauration, la suppression** : l'indexeur est abonné à `Store::changes()` ; à chaque annonce d'un espace (groupées, 250 ms), il compare les pages de l'espace à `search_state` et à ce qu'il sait des titres ; une route neuve qui change un titre n'a donc rien à appeler ;
- **le démarrage** : un rattrapage compare, pour toutes les pages, le dernier `seq` gardé (instantané ou journal) à `search_state.indexed_seq`. C'est lui qui indexe une instance existante (toutes ses pages sont en retard), qui rattrape un arrêt brutal, et une écriture faite par un autre `Store` (les tests, la CLI). Il tourne en fond, après que le serveur écoute : le démarrage à froid ne l'attend pas.

Une page dont la lecture échoue est notée (`failed_seq`), journalisée une fois, et n'est pas réessayée tant que son `seq` n'a pas changé. Une transaction de l'index porte au plus `MAX_INDEX_TX_BYTES` de lignes changées (valeur de départ : 4 Mio ; la duplication écrit 6 ms par Mio en release, R608) ; au-delà, la page s'écrit en plusieurs transactions, et seule la dernière pose `indexed_seq` et l'outbox.

À l'arrêt : `hub.shutdown()`, puis l'indexeur finit sa transaction en cours et valide Tantivy, puis `store.close()`. Ce qui restait à indexer l'est au démarrage suivant.

### La requête et les droits

`GET /api/v1/spaces/{id}/search?q=…&after=…`, pour qui a un rôle dans l'espace, invité compris.

1. `caller`, puis `space_access` : `404` sans rôle, avant toute lecture de l'index.
2. La requête est lue et bornée (R911). Vide une fois normalisée : `200`, aucun résultat, sans calcul.
3. L'ensemble permis est bâti de `SpaceAccess::visible()` : les pages vues, **hors corbeille** (`in_trash` faux). Il est donné à l'index comme un **filtre de la requête** (les seules entrées de ces `node_id`), pas appliqué après coup : ce que les pages non vues contiennent ne change ni les résultats, ni leur ordre, ni le nombre de pages lues pour remplir une réponse.
4. Le calcul part sur le fil bloquant, sous le débit du compte (R910). La garde suit le calcul, pas la requête (R630).
5. Chaque résultat est **revérifié** contre `SpaceAccess` (filet : une entrée d'une page hors de l'ensemble ferait une ligne `error` et serait jetée), groupé par page (le meilleur bloc de chaque page), puis reçoit son titre (`nodes`), son fil d'Ariane (les ancêtres **vus**), son extrait.
6. Réponse : `SearchResults { results: SearchHit[], more: bool, indexing: bool }`, `SearchHit { pageId, title, path: { id, title }[], blockId: string | null, excerpt: string, marks: [number, number][] }`. Les plages de `marks` comptent en unités UTF-16, comme une chaîne JavaScript. `excerpt` fait 240 caractères au plus. `indexing` : l'indexeur sait une page **vue par ce compte** en retard dans cet espace (jamais un nombre).

Ce que le filtre laisse dépendre des pages non vues : la note de pertinence (les statistiques de l'index portent sur tout l'espace, donc l'ordre de deux résultats vus peut bouger selon ce qu'écrivent les autres). C'est écrit comme limite (L-b, plus bas) ; S0 mesure s'il est simple de noter sans statistiques globales.

`GET /api/v1/pages/{id}/backlinks`, pour qui lit la page (`read`, à la corbeille aussi : la route ne rend alors rien de plus qu'ailleurs). Les sources sont lues dans `page_links` par `target_id`, puis **triées par `space_access` de l'espace de la page** : seules les pages vues, hors corbeille, de cet espace (une source d'un autre espace n'est pas rendue au lot 4, comme le lien inverse garde son titre copié, R574). Réponse : `Backlinks { pages: { pageId, title, blockId, excerpt }[] }`, 200 au plus, sans total.

### Bornes devant une entrée hostile

| Ce qu'un compte peut faire | Borne | Où |
|---|---|---|
| Une requête longue, à beaucoup de termes, à opérateurs | 200 caractères, 8 termes, aucune syntaxe ; au-delà `422 invalid_query` | S2, S5 |
| Un préfixe d'une lettre sur tout l'espace | préfixe à partir de 2 caractères, sur le dernier terme seulement | S2, S3 |
| Une boucle de recherches | un calcul et une attente par compte, quatre calculs pour l'instance, les comptes tour à tour ; `429 search_busy` | S5 |
| Paginer sans fin | 20 résultats par réponse, 5 réponses à la suite | S5 |
| Une page d'un bloc de plusieurs Mio, ou de cent mille liens | 64 Kio par bloc, 8 Mio par page, 10 000 liens ; le reste n'est pas indexé | S2 |
| Écrire sans relâche dans une page pour occuper l'indexeur | une indexation par page toutes les 15 s au plus, une page à la fois, les pages tour à tour ; rien n'est récrit de ce qui n'a pas changé | S4 |
| Écrire sans relâche dans N pages | l'indexeur prend du retard (dit par `indexing`), sans coût pour la frappe des autres : une seule voie, une transaction bornée à la fois | S4, X2 |
| Remplir le disque par l'index | rien avant le lot 7 (l'index pèse en proportion des pages, que R247 ne borne pas non plus) | limite écrite |
| Demander les rétroliens en boucle | lecture indexée, 200 lignes au plus ; pas de débit propre (une lecture de la base, comme les favoris) | S6 |

### Reconstruction, sauvegardes

- `<données>/search/` ne fait pas partie d'une sauvegarde : il se refait. La documentation le dit (§7.4, README).
- Au démarrage, l'époque de Tantivy est comparée à celle de la base : égale, rien ; en avance d'une validation, l'outbox est rejouée ; tout autre écart (base restaurée, copie d'avant migration remise en place, dossier d'une autre instance, dossier absent ou illisible, version du schéma de l'index différente), le dossier est vidé et toutes les pages passent dans l'outbox. Tant que cette reconstruction dure, **la recherche ne rend que ce qui est déjà reconstruit**, et le dit (`indexing`).
- `mementum reindex` (serveur arrêté) refait tout : `blocks_index` et `page_links` depuis les documents, puis Tantivy depuis `blocks_index`. `mementum reindex --text-only` ne refait que Tantivy.
- La migration `0005` ne remplit rien : elle crée des tables vides, et reste rapide sur une grande base.

---

## Budgets de performance

**Ceux de la conception (§5.10), tous à tenir, aucun relâché :**

| Budget | Ce que le lot risque | Mesure |
|---|---|---|
| Frappe vue chez un autre < 150 ms | l'indexeur dans la file d'écriture, sur le processeur, les fusions de segments de Tantivy | X2 : la mesure existante de `crates/mementum-sync/tests/budgets.rs`, rejouée pendant qu'un espace de 10 000 pages est indexé |
| Ouvrir une page jamais vue < 300 ms (1 000 blocs), < 3 s (10 000) | les rétroliens, le code de la recherche dans le lot de départ | `e2e/budgets.spec.ts` inchangé et vert ; un e2e compte les requêtes d'une ouverture (aucune vers `/search` ni `/backlinks` avant la première synchro) |
| Charger une page de 10 000 blocs < 300 ms (serveur) | l'indexeur qui relit la même page au même moment | X2 |
| Mémoire du serveur au repos < 150 Mo | l'écrivain de Tantivy (tampon), l'index ouvert | S0 puis X2 : la mesure existante de `mementum-cli`, avec un index de 10 000 pages |
| Démarrage à froid < 1 s | l'ouverture de l'index, le rattrapage | S0 puis X2 : la mesure existante de `mementum-cli`, avec le même index ; le rattrapage n'est pas attendu |
| Pi 4 : ~25 personnes ; VPS 2 vCPU : ~200 | les fils de Tantivy | non mesuré par ce lot (les mesures ARM sont repoussées avant la P2) ; un seul fil d'indexation, un seul de fusion |

**La conception ne donne aucun budget à la recherche elle-même** (écart E8). Ce plan en propose trois, **qui ne sont pas de la conception** et attendent Lenny (décision L10) ; d'ici là ils sont des mesures, pas des critères de refus :

- une recherche répond en moins de 100 ms au p95, côté serveur, en release, sur un espace de 10 000 pages et un million de blocs, pour un membre qui voit tout comme pour un invité qui voit dix pages ;
- du dernier caractère tapé aux résultats affichés : moins de 300 ms au p95 (dont 150 ms d'anti-rebond), build de production ;
- une page modifiée est trouvable par son nouveau texte en moins de 5 s après la dernière frappe, l'indexeur au repos.

Toute mesure neuve suit les règles de la CI : `#[ignore]` ne marque que des mesures, en release, une à la fois (`alone`), avec leur ligne `MESURE` ; un budget du navigateur se vérifie aussi sous `MEMENTUM_BUDGETS_CPU_SLOWDOWN=1.5`.

---

## Structure des fichiers

| Fichier | Rôle | Tâche |
|---|---|---|
| `crates/mementum-store/migrations/0005_search.sql` | tables de l'index | S1 |
| `crates/mementum-store/src/search.rs` (neuf), `queue.rs`, `nodes.rs`, `lib.rs` | écritures et lectures de l'index, `Lane::Index` | S1 |
| `crates/mementum-search/` (crate neuve) : `src/project.rs`, `query.rs`, `index.rs`, `lib.rs` | projection, requête, Tantivy | S2, S3 |
| `crates/mementum-sync/src/hub.rs`, `actor.rs`, `lib.rs` | pages écrites, lecture seule d'une page gardée | S4 |
| `crates/mementum-server/src/indexer.rs` (neuf), `searches.rs` (neuf), `routes/search.rs` (neuf), `routes/backlinks.rs` (neuf), `lib.rs`, `types.rs`, `config.rs` | indexeur, débit, routes | S4 à S6 |
| `crates/mementum-cli/src/main.rs` | `reindex` | S7 |
| `packages/protocol/src/generated/` | `SearchResults`, `SearchHit`, `Backlinks` (régénérés) | S5, S6 |
| `apps/web/src/search/` (neuf) : `SearchDialog.tsx`, `useSearch.ts`, `shortcut.ts`, `marks.ts` | fenêtre de recherche | C1 |
| `apps/web/src/api.ts`, `router.ts`, `sidebar/Sidebar.tsx`, `App.tsx`, `page/PageView.tsx` | appels, adresse d'un bloc, entrée de la barre | C1, C2 |
| `apps/web/src/page/Backlinks.tsx`, `useBacklinks.ts` (neufs) | rétroliens | C3 |
| `apps/web/e2e/search.spec.ts`, `backlinks.spec.ts`, `helpers.ts`, `servers.ts`, `budgets.spec.ts` | parcours, budgets | C1 à C3, X1, X2 |

---

## Piste S

### Task S0 : mesurer le moteur avant de bâtir dessus

**Dépend de :** rien. **Livrable :** une section du registre, des chiffres, et un verdict ; aucun code du produit. Le banc est jetable, hors du dépôt (un dossier de la tâche), sur le modèle des essais de la phase 0.

Pourquoi : la conception choisit Tantivy (§5.2) sans que la phase 0 l'ait mesuré (`spike/RESULTS.md` n'en dit rien), et deux budgets du §5.10 (mémoire au repos, démarrage à froid) en dépendent.

- [ ] **Step 1 : épingler.** La version courante de Tantivy, sa licence et celles de tout ce qu'elle tire, contre `deny.toml` (`cargo deny check licenses bans` sur un `Cargo.toml` d'essai) ; la version de Rust qu'elle demande contre `rust-version = "1.96"` ; ce qu'elle ajoute à la taille du binaire et à la durée d'une compilation en release.
- [ ] **Step 2 : mesurer**, en release, sur un corpus fait par programme (10 000 pages, un million de blocs, en français, accents compris) : mémoire d'un processus qui tient l'index ouvert et un écrivain au repos, avec le plus petit tampon admis, un fil d'indexation, un fil de fusion ; temps d'ouverture de l'index ; débit d'indexation ; durée et coût d'une validation ; latence d'une requête de un, trois et huit termes, avec préfixe, filtrée par 10, 10 000 puis 100 000 identifiants de page ; taille de l'index sur le disque.
- [ ] **Step 3 : répondre à cinq questions**, chacune par un essai : (a) le filtre par un ensemble de pages se pose-t-il dans la requête, et à quel coût quand l'ensemble est grand ; (b) une validation porte-t-elle une charge utile relue à l'ouverture (l'époque, R909) ; (c) quel tokeniseur plie la casse et les accents (« élève » trouvé par « eleve »), et que fait-il d'un mot de plus de 40 octets, du chinois, d'un identifiant `snake_case` ; (d) la racinisation française : ce qu'elle trouve en plus, ce qu'elle trouve à tort dans un texte anglais (décision L6) ; (e) noter sans statistiques d'ensemble est-il possible simplement (limite L-b).
- [ ] **Step 4 : la même mesure avec FTS5** (disponible : `-DSQLITE_ENABLE_FTS5`), sur le même corpus, table `fts5` à contenu externe sur `blocks_index`, tokeniseur `unicode61 remove_diacritics 2` : latence, taille, et **durée d'une écriture d'index dans le tour d'écriture de SQLite**, qui est ce que FTS5 coûterait à la frappe des autres. C'est la matière de la décision L1, pas un changement de plan.
- [ ] **Step 5 : verdict au registre.** Si la mémoire au repos ou le démarrage à froid ne tiennent pas avec Tantivy, ou si une licence est refusée : la tâche rend `BLOCKED`, avec ses chiffres, et la décision revient à Lenny. Sinon, la version est épinglée et S3 part.

**Pièges de `CLAUDE.md` :** un budget de temps ne s'affirme que dans une mesure, en release, une à la fois (R341) ; la CI est plus lente que le poste ; `TMP`/`TEMP` de la tâche, à supprimer en partant (les mesures en release sont ce qui pèse le plus, R344).

### Task S1 : store, les tables de l'index

**Dépend de :** la clôture du lot 3 (`0004` figée). **Livrable :** la migration, la voie, les fonctions du store ; rien ne les appelle encore.

**Files:**
- Create: `crates/mementum-store/migrations/0005_search.sql`, `crates/mementum-store/src/search.rs`, `crates/mementum-store/tests/search.rs`
- Modify: `crates/mementum-store/src/queue.rs` (`Lane::Index`), `lib.rs`, `nodes.rs` (`delete_node`), `backup.rs` si la liste des migrations y est nommée
- Test: `crates/mementum-store/tests/writes.rs`, `tests/migrations.rs` (ou son équivalent, à lire)

**Interfaces (produites) :**
- `Store::index_page(node_id, seq, rows: &[IndexedBlock], truncated) -> Result<Indexed, StoreError>` : écrit la différence, dans `Lane::Index` ; `Indexed { changed: usize }` ; ne fait rien si `seq < indexed_seq`.
- `Store::index_failed(node_id, seq)`, `Store::stale_pages(after, limit)` (les pages dont le dernier `seq` gardé dépasse `indexed_seq`, par lots, lue sans tour d'écriture), `Store::stale_in_space(space_id)`.
- `Store::outbox(limit)`, `Store::outbox_done(&[(node_id, gen)], epoch)`, `Store::outbox_all()` (tout reconstruire), `Store::indexed_blocks(node_id)`, `Store::search_epoch()`.
- `Store::backlinks(target_id, limit) -> Vec<Backlink { node_id, block_id }>`.

- [ ] **Step 1 : tests, vus rouges.** `index_page_writes_only_what_changed` (un déclencheur témoin compte les lignes écrites : une frappe dans un bloc d'une page de 1 000 blocs en écrit une) ; `index_page_removes_the_blocks_that_left` ; `an_older_read_never_overwrites_a_newer_index` ; `the_index_leaves_with_its_page` (suppression définitive : plus aucune ligne des trois tables, et la page dans l'outbox) ; `index_writes_share_one_lane` (sur le modèle de `page_copies_share_one_lane_whoever_asks` : tour tenu par `hold_writes`, ordre des validations lu dans la base) ; `indexing_bumps_neither_rev_nor_lsn_and_announces_nothing` (témoin : un renommage qui suit est annoncé) ; `stale_pages_names_a_page_written_by_another_store`, `…a_duplicated_page`, `…a_page_never_indexed` ; `stale_pages_takes_no_write_turn` (tour tenu, la lecture rend) ; `outbox_done_keeps_a_page_changed_meanwhile` ; `backlinks_are_read_from_the_index` (`EXPLAIN QUERY PLAN` : `page_links_by_target`) ; `a_lot_3_database_migrates_and_starts_with_everything_stale`.
- [ ] **Step 2 : écrire**, toucher `lib.rs` (R269), relancer. Commit `store : tables de l'index et voie de l'indexation (R901, R908)`.

**Preuves (mutations) :** écrire toutes les lignes au lieu de la différence ; oublier la comparaison de `seq` ; `Lane::Common` à la place de `Lane::Index` ; retirer de l'outbox sans comparer `gen` ; `bump_lsn` ajouté par mégarde.

**Pièges :** une migration neuve n'entre dans le binaire qu'à la recompilation du store (R269) ; une requête reprend l'expression exacte de son index ; une écriture qui parcourt une grande table lit d'abord sans tour, et ne prend le tour que pour écrire (R270) ; `TestDir`, pas `tempfile::tempdir()` (R344) ; pour prouver un ordre de service, tenir un tour et lire l'ordre dans la base, jamais à l'horloge ; SQL dynamique sous `AssertSqlSafe`, valeurs liées.

### Task S2 : `mementum-search`, la projection et la requête (sans moteur)

**Dépend de :** rien (peut partir avec S0). **Livrable :** deux fonctions pures, leurs fixtures, leur linéarité.

**Files:**
- Create: `crates/mementum-search/Cargo.toml`, `src/lib.rs`, `src/project.rs`, `src/query.rs`, `tests/project.rs`, `tests/linear.rs`
- Create: `spec/search/projection.json` (table de cas : un bloc, le texte et les liens attendus), `spec/search/queries.json`
- Modify: `Cargo.toml` (racine)

**Interfaces (produites) :**
- `project(blocks: &[Block], origin: Option<&str>) -> Projection { rows: Vec<IndexedBlock>, truncated: bool }`, `IndexedBlock { block_id, kind, text, hash, links: Vec<Uuid> }`.
- `read_query(raw: &str) -> Result<Query, QueryRefusal>`, `Query { terms: Vec<String>, prefix: Option<String> }`.

- [ ] **Step 1 : tests, vus rouges.** `every_block_of_the_registry_has_its_rule` (échoue si `spec/schema/blocks.json` gagne un type que la table ne connaît pas) ; `the_fixtures_project_as_written` (`all-blocks`, `non-default`, `rich-blocks` de `spec/mmd/fixtures/` : le texte attendu est écrit à la main, jamais recopié de la sortie) ; `marks_are_dropped_and_text_is_kept` ; `a_mention_is_read_by_its_label_and_linked_by_its_id` ; `a_link_to_a_page_of_the_instance_is_a_link_and_an_outside_address_is_not` (table de cas : `/p/<uuid>`, la même sous l'origine publique, un autre hôte, `/p/<uuid>/x`, un UUID sans tirets) ; `a_block_without_id_or_with_a_repeated_id_gets_a_stand_in_key` ; `a_block_over_the_bound_is_cut_between_characters`, `a_page_over_the_bound_is_marked_truncated` ; `the_hash_changes_with_the_text_the_type_and_the_links_and_with_nothing_else` ; requête : `a_query_is_folded_and_split`, `operators_are_only_characters` (`AND`, `"`, `*`, `-`, `title:`, `~`), `a_query_over_the_bounds_is_refused`, `a_prefix_under_two_characters_is_dropped`, `an_empty_query_reads_empty`. Linéarité : chaque forme (un long texte, mille plages marquées, un tableau large, des listes imbriquées, cent mille mentions, une requête longue) à une taille et à huit fois cette taille, sous les bornes de R433.
- [ ] **Step 2 : écrire**, relancer. Commit `recherche : projection d'une page et lecture d'une requête (R911, R912)`.

**Preuves (mutations) :** indexer l'adresse d'un lien ; oublier les enfants d'un bloc sans texte (`column`) ; couper au milieu d'un caractère ; laisser passer un `*` ; reconnaître une page par `Uuid::parse_str` seul (les formes sans tirets, la leçon de R754).

**Pièges :** toute lecture d'une entrée venue d'un client croît comme elle, avec des bornes absolues (R380, R433) ; le rapport de temps se calcule en nanosecondes ; un identifiant de page se reconnaît par une règle stricte, table de cas partagée (`spec/files/addresses.json` est le modèle) ; un message d'erreur ne recopie jamais l'entrée (R434) ; fins de ligne LF pour les fixtures.

### Task S3 : `mementum-search`, l'index plein texte

**Dépend de :** S0 (verdict), S2. **Livrable :** l'index derrière `SearchIndex`, sans rien savoir du store ni des droits.

**Files:**
- Create: `crates/mementum-search/src/index.rs`, `tests/index.rs`, `tests/linear_index.rs`
- Modify: `Cargo.toml` (racine : `tantivy`, version de S0), `deny.toml` si une licence déjà compatible manque à la liste (sinon `BLOCKED`)

**Interfaces (produites) :**
- `SearchIndex::open(dir, expected: Epoch) -> Opened { index, state: InStep | OneAhead | Rebuilt }`
- `put_page(space_id, node_id, title, rows)`, `remove_page(node_id)`, `commit(epoch)` : synchrones, pour le fil bloquant.
- `search(space_id, query: &Query, allowed: &PageSet, skip, limit) -> Found { hits: Vec<Hit { node_id, block_id, excerpt, marks }>, more }`.

- [ ] **Step 1 : tests, vus rouges.** `a_word_of_a_block_finds_its_page_and_its_block` ; `accents_and_case_do_not_matter` ; `every_term_is_wanted` ; `the_last_term_matches_as_a_prefix` ; `a_title_outranks_a_block` ; `only_allowed_pages_are_found` et son témoin (la même requête, l'ensemble élargi, trouve) ; `what_unseen_pages_hold_changes_neither_the_hits_nor_more` (deux index : l'un avec mille pages non permises qui portent le mot, l'autre sans ; mêmes résultats, même `more`) ; `another_space_is_never_found` ; `a_removed_page_is_no_longer_found` ; `putting_a_page_again_replaces_it` ; `the_excerpt_is_text_with_utf16_marks` (un emoji avant le mot : les plages tombent juste dans une chaîne JavaScript ; aucune balise dans `excerpt`) ; `the_excerpt_is_bounded` ; époque : `an_index_one_commit_ahead_is_kept`, `an_index_of_another_database_is_rebuilt`, `a_missing_or_unreadable_directory_is_rebuilt`, `an_index_of_another_schema_is_rebuilt` ; `a_second_writer_is_refused_with_a_message` (le verrou, pour `mementum reindex`). Linéarité : `search` sur un index d'une taille et de huit fois cette taille.
- [ ] **Step 2 : écrire**, relancer. Commit `recherche : index plein texte (R900, R902, R909)`.

**Preuves (mutations) :** appliquer le filtre après la recherche (le test des mille pages rougit sur `more`) ; oublier `space_id` ; compter les plages en octets ; rendre le HTML d'extrait du moteur ; garder un index d'une autre époque.

**Pièges :** jamais d'analyseur de requêtes du moteur sur le texte d'une personne ; un test qui affirme une absence a son témoin ; sous Windows, un fichier ouvert ne se supprime pas (vider le dossier demande d'avoir lâché l'index : `TestDir`) ; un dossier temporaire par tâche.

### Task S4 : l'indexeur

**Dépend de :** S1, S2, S3. **Livrable :** une page écrite devient trouvable, sans que la frappe le paie.

**Files:**
- Create: `crates/mementum-server/src/indexer.rs`, `crates/mementum-server/tests/indexer.rs`
- Modify: `crates/mementum-sync/src/hub.rs` (`Hub::touched`, `Hub::take_touched`), `actor.rs` (`persist`, `persist_restore` ; `stored_page`), `lib.rs` ; `crates/mementum-server/src/lib.rs` (`build`, `serve`, `AppState`), `config.rs` (`index_after`, `index_at_most`, `MEMENTUM_INDEX_AFTER_MS` pour les e2e)
- Test: `crates/mementum-sync/tests/` (un fichier neuf), `crates/mementum-server/tests/journal.rs`

**Interfaces :**
- Consumes : S1, S2, S3 ; `Store::changes()`.
- Produces : `mementum_sync::stored_page(store, node_id) -> Result<Option<(PageDoc, i64)>, SyncError>` : la page que le store garde et son dernier `seq`, relue comme `load` la relit (mêmes entrées écartées), **sans rien écrire** (`None` pour une page sans document), sans ligne `error` (niveau `debug` : l'acteur a déjà dit ces entrées) ; `Indexer::spawn(state) -> IndexerHandle`, `IndexerHandle::behind(space_id, seen: &PageSet) -> bool`, `IndexerHandle::settled()` (pour les tests), `stop()`.

- [ ] **Step 1 : tests, vus rouges.** Dans `mementum-sync` : `the_actor_only_names_the_page_it_wrote` (après mille frappes, l'ensemble porte un identifiant ; `Hub::block_reads` n'a pas bougé ; aucun encodage de plus, compté) ; `a_write_of_the_server_and_a_restore_name_the_page` ; `a_refused_update_names_nothing` ; `stored_page_reads_what_the_actor_serves` (propriété : après chaque étape d'un scénario de `tests/reload.rs`, mêmes blocs) ; `stored_page_writes_nothing` (une page sans document reste sans instantané) ; `stored_page_sets_aside_what_load_sets_aside`. Dans `mementum-server` : `a_typed_word_becomes_findable` ; `erased_text_leaves_the_index` ; `a_page_written_by_put_mmd_is_indexed`, `…restored…`, `…duplicated…` (avec ses sous-pages), `…renamed…` (trouvée par son nouveau titre, plus par l'ancien) ; `a_deleted_page_leaves_the_index` ; `a_trashed_page_stays_indexed` (restaurée, elle est trouvée sans nouvelle indexation : compteur) ; `an_existing_instance_is_indexed_after_startup` (base du lot 3 remplie par un autre `Store`) ; `a_crash_between_the_two_commits_is_caught_up` (l'indexeur arrêté après la validation de Tantivy, avant celle de la base) ; `a_page_that_cannot_be_read_is_tried_once` (compte des lectures) ; `a_page_written_without_pause_is_indexed_at_most_every_bound` (horloge de tokio arrêtée, `StoppedClock`) ; `pages_take_turns` (une page écrite sans relâche n'empêche pas une autre d'être indexée) ; `indexing_leaves_the_engine_free` (sur le modèle d'`engine_stays_free`) ; `the_indexer_stops_before_the_store_closes` ; `entries_set_aside_are_logged_once` (le test de `journal.rs` qui compte les lignes `error` garde son compte avec l'indexeur lancé).
- [ ] **Step 2 : écrire**, relancer les suites de `mementum-sync` et de `mementum-server`. Commit `serveur : indexeur des pages (R906, R907)`.
- [ ] **Step 3 : mesures** (`#[ignore]`, release, `alone`) : `measure_indexing_a_10000_block_page` (lecture, projection, transaction, Tantivy) ; `measure_the_write_turn_held_by_an_index_transaction` (p95, pour arrêter `MAX_INDEX_TX_BYTES`) ; `measure_first_indexing_of_10000_pages`.

**Preuves (mutations) :** nommer la page avant l'écriture au journal ; lire les blocs par l'acteur (`Hub::read_blocks`) ; indexer sans attendre le calme ; poser `indexed_seq` au `seq` courant plutôt qu'au `seq` lu ; oublier `Store::changes()` (le renommage) ; oublier le rattrapage (l'instance existante).

**Pièges :** une tâche lancée par `tokio::spawn` perd la portée `writes_by` (l'indexeur écrit dans `Lane::Index`, nommée par le store, pas par la portée) ; ne jamais mener deux écritures du store ensemble dans une tâche (R518) ; un calcul long rend son fil au moteur, et `block_in_place` panique sur un moteur à un fil (R863) ; `Store::changes()` : s'abonner avant de lire, relire après `Lagged`, une annonce de trop est possible ; une écriture faite par un autre `Store` n'est pas annoncée (R133) ; test qui lit le journal : `Logs::capture` ; test qui suppose la durée d'une écriture : tenir le tour (`hold_writes`), ne pas supposer (R886) ; `StoppedClock` et ses règles (R179) ; un test qui lit le store après une compaction l'attend (`Hub::compacted`) ; une rafale d'écritures prend le temps du disque (R441).

### Task S5 : la route de recherche

**Dépend de :** S3, S4. **Livrable :** `GET /api/v1/spaces/{id}/search`.

**Files:**
- Create: `crates/mementum-server/src/routes/search.rs`, `src/searches.rs`, `tests/search.rs`
- Modify: `crates/mementum-server/src/lib.rs` (routeur, tableau des routes), `types.rs`, `tests/common/mod.rs`, `tests/spaces.rs`, `tests/expected_account.rs`

| Route | Il faut | Réponse |
|---|---|---|
| `GET /api/v1/spaces/{id}/search?q=…&after=N` | un rôle | `SearchResults` ; `422 invalid_query` ; `429 search_busy` (`Retry-After: 1`) |

- [ ] **Step 1 : tests, vus rouges.** Droits : `a_word_only_an_unseen_page_holds_answers_like_a_word_nobody_wrote` (corps égaux, octet pour octet, `indexing` compris) ; `a_guest_finds_only_the_pages_he_was_invited_on` ; `a_private_page_is_found_by_its_owner_only` ; `withdrawn_access_hides_the_page_at_the_next_query`, `a_page_closed_to_members…`, `a_member_removed_from_the_space_gets_404` (l'indexeur arrêté : c'est la requête qui juge) ; `a_trashed_page_is_not_found_and_comes_back_restored` ; `a_subpage_of_a_trashed_page_is_not_found` ; `the_breadcrumb_names_only_seen_ancestors` (un invité sur une sous-page : le fil commence à elle) ; `the_title_is_the_current_one` (renommée, indexeur arrêté) ; `indexing_says_nothing_of_unseen_pages` (une page non vue en retard : `indexing` faux pour l'invité, vrai pour le membre) ; `space_routes_by_role` gagne la route ; la matrice du compte attendu aussi. Bornes : `a_query_over_the_bounds_is_422`, `an_empty_query_answers_empty_without_computing` (compteur de calculs) ; `paging_stops_at_the_bound` ; `the_query_is_never_logged` (`all_logs`, la requête porte un mot témoin ; son témoin : un autre point de journal de la requête est vu). Débit : `a_newer_search_takes_the_place_of_the_one_waiting` ; `a_burst_of_searches_of_one_account_does_not_delay_another` (une place sur quatre laissée libre, l'ordre de service lu, sans minutage) ; `an_abandoned_search_keeps_its_place_until_its_computation_ends` (R630). En-têtes : `no-store`, `nosniff`.
- [ ] **Step 2 : écrire**, régénérer les types (`packages/protocol`), relancer. Commit `serveur : recherche dans un espace (R903 à R905, R910, R914)`. En-tête de `lib.rs`, registre.
- [ ] **Step 3 : mesure** (`#[ignore]`, release) : `measure_search_on_10000_pages` (membre qui voit tout, invité qui voit dix pages ; un, trois, huit termes ; p95), ligne `MESURE`.

**Preuves (mutations) :** filtrer sans `in_trash` ; bâtir le fil d'Ariane de `parent_id` au lieu de `visible_parent` ; lire le titre dans l'index ; calculer `indexing` sur tout l'espace ; rendre la garde du débit à l'abandon de la requête ; appeler l'index avant `space_access`.

**Pièges :** les droits par `access.rs` seulement ; qui n'a aucun rôle reçoit sa réponse sans que l'espace soit lu (R99) ; toute garde de débit suit le travail qu'elle borne (R630) ; un test de débit abandonne la requête pendant un calcul qui tourne ; compter les requêtes SQL d'une route demande un binaire à lui (R266) ; messages du serveur affichés tels quels, ils tutoient ; `StoreError::Stale` n'a pas de traduction générique (aucune écriture ici : ne pas en introduire) ; un seuil « N pendant M » finit par échouer : attendre une condition.

### Task S6 : la route des rétroliens

**Dépend de :** S1, S4. **Livrable :** `GET /api/v1/pages/{id}/backlinks`.

**Files:**
- Create: `crates/mementum-server/src/routes/backlinks.rs`, `tests/backlinks.rs`
- Modify: `lib.rs`, `types.rs`, `tests/common/mod.rs` (`page_routes`), `tests/permissions.rs`

- [ ] **Step 1 : tests, vus rouges.** `a_page_link_a_mention_and_a_link_in_a_text_are_backlinks` ; `a_backlink_from_an_unseen_page_is_not_listed` et son témoin (l'accès donné, elle paraît) ; `a_backlink_from_a_trashed_page_is_not_listed` ; `a_backlink_from_another_space_is_not_listed` ; `a_page_linking_twice_is_listed_once` (le premier bloc) ; `a_link_removed_leaves_the_backlinks` ; `a_page_does_not_backlink_itself` ; `backlinks_are_bounded` (201 sources : 200 rendues, rien ne dit qu'il en reste d'autres que la personne ne voit pas) ; `the_excerpt_is_the_block_as_indexed_and_bounded` ; `foreign_page_is_indistinguishable_from_missing` gagne la route ; `a_reader_gets_the_backlinks`, `…in_the_trash_too`.
- [ ] **Step 2 : écrire**, régénérer les types, relancer. Commit `serveur : rétroliens d'une page`. En-tête de `lib.rs`.

**Preuves (mutations) :** rendre les sources sans les trier par `space_access` ; trier par l'accès à la page visée seulement ; compter avant de trier (une borne qui dit combien de sources existent).

**Pièges :** « ne jamais lister sans ce tri » (la leçon des favoris, R280) ; un id qu'on ne voit pas ne sort par aucune route neuve ; l'extrait vient de l'index, qui peut porter un titre copié (R251) : c'est le texte de la page source, que le lecteur lit déjà.

### Task S7 : `mementum reindex`, et la reconstruction au démarrage

**Dépend de :** S4. **Livrable :** la commande de la conception (§5.7), et un serveur qui se répare seul.

**Files:**
- Modify: `crates/mementum-cli/src/main.rs`, `crates/mementum-server/src/lib.rs` (`build`), `indexer.rs`
- Test: `crates/mementum-cli/tests/` (un fichier neuf), `crates/mementum-server/tests/indexer.rs`

- [ ] **Step 1 : tests, vus rouges.** `reindex_rebuilds_everything_from_the_documents` (tables vidées à la main, dossier supprimé : après la commande, mêmes résultats qu'avant) ; `reindex_text_only_rebuilds_the_text_index_from_the_tables` ; `reindex_refuses_while_a_server_holds_the_index` (message en français, base et index intacts) ; `reindex_never_opens_a_database_it_would_have_to_migrate_without_the_backup_rule` (la commande passe par `Store::open`, donc par `backup.rs`) ; au démarrage du serveur : `a_restored_database_rebuilds_its_text_index` (copie de la base d'avant des écritures remise en place, dossier `search/` gardé : aucun extrait d'un texte que la base n'a pas, témoin avant) ; `a_backup_put_back_before_0005_rebuilds_after_migrating_again` ; `searching_during_a_rebuild_says_indexing`.
- [ ] **Step 2 : écrire**, relancer. Commit `cli : mementum reindex (R913)`.

**Preuves (mutations) :** garder le dossier quand l'époque diffère ; reconstruire depuis les documents quand `--text-only` ; ouvrir un second écrivain.

**Pièges :** toute lecture neuve d'une base avant de l'accepter passe par une connexion sans réglage qui écrive (R840, R845) : passer par `Store::open` ; une commande dans un autre processus dispute le verrou de SQLite par `busy_timeout` ; démarrage à froid < 1 s : la reconstruction n'est jamais attendue par `serve`.

---

## Piste C

### Task C1 : la fenêtre de recherche

**Dépend de :** S5. **Livrable :** on ouvre la recherche, on tape, on choisit au clavier, on arrive sur la page.

**Files:**
- Create: `apps/web/src/search/SearchDialog.tsx`, `useSearch.ts`, `shortcut.ts`, `marks.ts`, leurs tests ; `apps/web/e2e/search.spec.ts`
- Modify: `apps/web/src/api.ts` (`searchSpace`), `App.tsx`, `sidebar/Sidebar.tsx`, `theme.css`, `e2e/helpers.ts` (`waitFindable`), `e2e/servers.ts` (`MEMENTUM_INDEX_AFTER_MS`)

Comportement :
- Une entrée « Rechercher » sous le sélecteur d'espace, avec son raccourci écrit ; le raccourci l'ouvre de partout (décision L5 ; le plan suppose Ctrl+K, laissé à l'éditeur quand sa sélection n'est pas vide).
- La fenêtre (`<dialog>`) est chargée à la demande. Champ vide : les pages récentes (ce que la barre montre déjà). Dès la frappe : d'abord les pages de l'arbre dont le titre contient la requête (`findPages`, sans réseau), puis, 150 ms après la dernière frappe, les résultats du serveur ; une réponse d'une requête qui n'est plus la courante est jetée, une requête en route est abandonnée (`AbortSignal`).
- Une page trouvée des deux côtés n'a qu'une ligne. La ligne active est tenue par l'identifiant de la page ; l'arrivée des résultats du serveur ne la change pas, et ne déplace rien sous le pointeur (les lignes déjà montrées gardent leur rang, les autres s'ajoutent dessous).
- Chaque ligne : le titre, le fil d'Ariane, l'extrait avec ses mots surlignés (`<mark>` autour de nœuds de texte, d'après `marks`). Flèches, Entrée (ouvre), Ctrl+Entrée (nouvel onglet : chaque ligne est un vrai `<a href>`), Échap (ferme, le focus revient d'où il venait).
- États dits : « Aucun résultat », « L'indexation est en cours : des pages peuvent manquer » (`indexing`), un refus du serveur tel qu'il le dit, la session perdue (la connexion est demandée, la requête repart au retour du compte).

- [ ] **Step 1 : tests, vus rouges.** Vitest : `titles_of_the_tree_come_first_and_at_once` ; `a_late_answer_of_an_older_query_is_dropped` ; `the_active_line_is_held_by_its_id_when_results_arrive` ; `nothing_moves_under_the_pointer_when_results_arrive` ; `a_page_found_twice_has_one_line` ; `marks_wrap_text_nodes_only` (un extrait `<img onerror=…>` s'affiche en texte ; une plage hors du texte ou qui coupe une paire UTF-16 est ignorée) ; `escape_closes_and_gives_the_focus_back`, `escape_in_the_window_closes_nothing_under_it` (`event.target === event.currentTarget`) ; `the_shortcut_is_read_from_the_key_typed` (AZERTY, AltGr, sous une autre `dialog`, dans un champ) ; `search_busy_is_not_shown_for_a_replaced_query` ; `a_lost_session_asks_to_log_in_and_searches_again`. E2e : `typed_word_is_found_and_opened` ; `searching_at_the_keyboard_only` ; `the_shortcut_in_the_editor_with_a_selection_is_left_to_the_editor` (témoin : sans sélection, la fenêtre s'ouvre) ; `opening_the_application_asks_nothing_of_search` (compte des requêtes).
- [ ] **Step 2 : écrire**, relancer Vitest, types, e2e (ports de la piste C). Commit `client : fenêtre de recherche`.

**Preuves (mutations) :** garder la ligne active par son rang ; afficher l'extrait par `innerHTML` ; reconnaître le raccourci par `event.code` ; ne pas jeter la réponse en retard ; charger la fenêtre au démarrage.

**Pièges :** `onCancel` et `onClose` d'un `<dialog>` remontent dans l'arbre de React ; le focus ne tombe jamais sur `body` (un élément retiré sous le focus, R339) ; une liste qui suit le réseau tient sa ligne par l'id (R572) ; rien ne bouge sous le pointeur (R296, R411 : le suivre au niveau du document) ; un raccourci se lit à `event.key`, se tait sous une `dialog` ouverte, ne prend jamais un caractère tapé avec AltGr (R176, R214) ; un raccourci qui double celui de l'éditeur ou du navigateur (R634) ; toute fonction neuve d'`api.ts` passe par `send` ; une liste qui trouve la session perdue rend `SESSION_LOST` et prend `afterLogin` (R212) ; clés React à soi dans la barre (R290) ; un e2e qui cherche une ligne de la barre prend un compte neuf ; les pages d'un e2e se créent avant d'ouvrir les onglets ; un e2e attend que la page soit trouvable par une condition (`waitFindable`), pas par une durée ; l'outil d'écriture change `⁨` en caractère réel (`String.fromCharCode`) ; ne pas éditer `apps/web/src` pendant une suite e2e.

### Task C2 : arriver sur le bloc trouvé

**Dépend de :** C1. **Livrable :** ouvrir un résultat mène au bloc, pour qui modifie comme pour qui lit.

**Files:**
- Modify: `apps/web/src/router.ts` (un fragment `#b=<id>` sur `/p/:id`), `page/PageView.tsx`, `editor/Editor.tsx` ou un module neuf `editor/goToBlock.ts` (tiré de `goToHeading`, `Toc.tsx`), `search/SearchDialog.tsx`
- Test: `router.test.ts`, `page/PageView.test.tsx`, `e2e/search.spec.ts`

- [ ] **Step 1 : tests, vus rouges.** `a_block_fragment_is_parsed_and_kept_out_of_the_page_id` ; `the_block_is_reached_after_the_first_sync_only` (jamais sur un document encore vide) ; `a_block_that_left_opens_the_page_at_its_top_and_says_nothing` ; `a_reader_is_taken_to_the_block_too` (éditeur en lecture seule : la sélection du document, comme `goToHeading`) ; `the_fragment_is_followed_once` (une relecture, une session recréée ne ramènent pas au bloc) ; `typing_after_arriving_is_not_lost` ; e2e `a_result_opens_on_its_block_in_a_long_page` (page de 1 000 blocs, le bloc trouvé est à l'écran) ; budgets : `open_1000_block_page_under_300ms` reste vert.
- [ ] **Step 2 : écrire**, relancer. Commit `client : un résultat mène à son bloc`.

**Preuves (mutations) :** mener au bloc avant la première synchro ; le refaire à chaque rendu ; `editor.focus()` seul dans un éditeur en lecture seule.

**Pièges :** `setTextCursorPosition` ne tient pas dans un éditeur en lecture seule qui prend le focus ; `editor.focus()` n'y fait rien (R470) ; ne jamais relire des blocs un par un dans une boucle (R641) ; tout branchement neuf de `PageView` prend son test dans `PageView.test.tsx` ; `PageView` est rendu sous la clé de la page ; même id n'est pas même élément (R220).

### Task C3 : les rétroliens sous la page

**Dépend de :** S6, C2. **Livrable :** sous l'éditeur, « Pages qui mènent ici », pour qui lit la page.

**Files:**
- Create: `apps/web/src/page/Backlinks.tsx`, `useBacklinks.ts`, leurs tests ; `apps/web/e2e/backlinks.spec.ts`
- Modify: `apps/web/src/api.ts` (`getBacklinks`), `page/PageView.tsx`, `theme.css`

Comportement : rien n'est lu avant la première synchro de la page ; la section n'est rendue que s'il y a au moins une page ; elle est relue au retour sur l'onglet (la règle des cinq secondes) et au retour de la connexion, jamais à la frappe ; chaque ligne est un lien vers la page source, à son bloc (`#b=`), avec le titre de l'arbre (`usePageTitle`) et l'extrait ; rien n'y bouge sous le pointeur ni sous le focus.

- [ ] **Step 1 : tests, vus rouges.** Vitest : `backlinks_are_read_after_the_first_sync_only` ; `no_section_without_a_backlink` ; `the_list_is_held_under_the_pointer_and_the_focus` ; `a_refusal_is_said_and_the_page_is_read_again` (`404` : la page n'est plus vue) ; `a_lost_session_reads_again_after_login` ; `the_section_is_a_list_of_links` (`nav`, `ul`, `li`, nom accessible). E2e : `a_mention_makes_a_backlink` (à deux onglets : l'un mentionne, l'autre revient sur l'onglet de la page visée) ; `a_guest_sees_only_the_backlinks_of_pages_he_sees` ; `opening_a_page_asks_for_backlinks_after_its_first_sync` (ordre des requêtes).
- [ ] **Step 2 : écrire**, relancer, e2e, budgets du navigateur (les 14, plus un : une page visée par 200 pages s'ouvre sous son budget). Commit `client : rétroliens d'une page`.

**Preuves (mutations) :** lire au montage ; relire à chaque frappe ; prendre le titre de la réponse plutôt que de l'arbre.

**Pièges :** un titre de page montré vient de l'arbre, et de lui seul (R570) ; la feuille de BlockNote passe après `theme.css` (deux classes, R763), et la preuve est dans Chromium (`e2e/rich-styles.spec.ts`) ; `data-page-id` est réservé aux lignes de l'arbre ; un e2e qui compte les requêtes voit passer celles du canal et des favoris ; un e2e qui change des droits les pose avant d'ouvrir les pages.

---

## Tâches transverses

### Task X1 : les droits de la recherche, de bout en bout

**Dépend de :** C1, C3. **Livrable :** les six points de « Review Focus » prouvés dans un navigateur, à plusieurs comptes.

**Files:** `apps/web/e2e/search-rights.spec.ts` (neuf)

- [ ] **Step 1 : e2e, vus rouges par une sonde** (une mutation du serveur posée le temps de la preuve, jamais commitée) : `a_guest_never_finds_a_page_he_does_not_see` (propriétaire, membre, invité ; un mot témoin par page ; l'invité cherche chacun) ; `a_private_page_is_found_by_its_owner_only` ; `access_withdrawn_while_the_window_is_open` (la ligne reste affichée ; l'ouvrir dit « Tu n'as plus accès à cette page » ; la requête suivante ne la rend plus) ; `a_trashed_page_leaves_the_results_and_comes_back` ; `another_account_logging_in_in_another_tab_gets_no_result_of_the_first` (compte attendu, R540) ; `erased_text_is_no_longer_found`.
- [ ] **Step 2 : commit** `e2e : droits de la recherche`.

**Pièges :** e2e qui change des droits ; e2e d'un onglet qui ne doit pas encore savoir (`closeEventsChannel`, `hideSyncClosures`) ; un e2e qui affirme une absence le fait derrière une barrière (une lecture demandée après le geste, et sa réponse reçue), jamais par `toHaveCount(0)` seul ; comptes neufs (`signUpAccount`), `section: "private"` pour un lecteur membre.

### Task X2 : budgets

**Dépend de :** S4, S5, C1. **Livrable :** les budgets du §5.10 mesurés avec l'indexeur, dans la CI.

**Files:** `crates/mementum-sync/tests/budgets.rs` ou `crates/mementum-server/tests/measure.rs`, `crates/mementum-cli/tests/` (mesures existantes), `apps/web/e2e/budgets.spec.ts`, `.github/workflows/ci.yml`

- [ ] **Step 1 : mesures** (`#[ignore]`, release, `alone`, lignes `MESURE`) : `keystroke_seen_under_150ms_while_a_space_is_being_indexed` (10 000 pages en retard, p95 au plus proche rang, la première frappe jugée à part, R488) ; `loading_a_10000_block_page_under_300ms_while_it_is_being_indexed` ; la mémoire au repos et le démarrage à froid de `mementum-cli`, sur un dossier de données qui porte un index de 10 000 pages ; `measure_search_on_10000_pages` (S5) ; côté navigateur, `results_shown_after_the_last_keystroke` (mesure, sous ×1 et ×1,5).
- [ ] **Step 2 : la CI** : si les mesures de `mementum-server` y entrent (elles n'y sont pas aujourd'hui), le dire au registre ; sinon, la mesure de la frappe pendant l'indexation va dans `mementum-sync` ou `mementum-cli`, que le job `budgets` lance déjà. Commit `budgets : la frappe, la mémoire et le démarrage avec l'index`.

**Pièges :** ne jamais relâcher un budget, sa statistique ni sa définition ; les mesures d'un binaire passent une à la fois (R341) ; le disque de la CI varie du simple au quintuple (R487) ; le point de contrôle du WAL tombe sur l'écriture qui le déclenche (R447) : les transactions de l'index en déclenchent, le lire dans la liste des frappes ; un fil qui partage un cœur attend par tranches (R585) ; mesurer le coût d'un ajout à binaire égal (interrupteur d'environnement le temps de la mesure, jamais commité).

### Task D : documentation et recette

**Dépend de :** toutes.

**Files:**
- Modify: `README.md`, `CLAUDE.md` (« Pièges rencontrés au lot 4 »), `docs/REPRISE.md`, `docs/specs/2026-11-09-mementum-design.md` (§4.2, §5.3, §5.7, §5.10, §6.5, §7.4, §9 : les écarts E1 à E9, tels que Lenny les aura tranchés), l'en-tête de `crates/mementum-server/src/lib.rs`
- Create: `docs/recette/2026-11-19-p1-lot4-recette.md`

- [ ] **Step 1 : relire chaque document contre le code final**, pas contre ce plan : les routes ; les réglages ; `mementum reindex` ; ce qui est indexé et ce qui ne l'est pas ; les sauvegardes (`search/` ne se sauvegarde pas) ; les limites, telles que mesurées.
- [ ] **Step 2 : la checklist de recette** (ébauche plus bas).
- [ ] **Step 3 : commit** `docs : lot 4, la recherche`.

---

## Limites connues, écrites d'avance

À porter au README et au registre telles que mesurées ; chacune a son test `known_limits` quand elle se prouve.

- **L-a** : un texte effacé reste trouvable, et lisible dans un extrait, jusqu'à la réindexation de sa page (2 à 15 s, davantage si l'indexeur est en retard), par toute personne qui lit la page à ce moment, y compris quelqu'un à qui l'accès vient d'être donné. L'historique, lui, demande « Peut modifier » (R203).
- **L-b** : l'ordre de deux résultats peut dépendre de ce qu'écrivent des pages que la personne ne voit pas (les statistiques de pertinence portent sur l'espace) ; leur présence, non.
- **L-c** : deux mots écrits dans deux blocs différents d'une page ne la trouvent pas, sauf si l'un est dans le titre (R902).
- **L-d** : un titre copié par une mention ou un lien de page (R251) est un texte de la page qui le porte : on trouve cette page en cherchant le titre de la page mentionnée, même sans voir celle-ci. Rien de neuf n'est montré (la personne lit déjà ce titre dans la page), mais la recherche le rend plus facile à trouver.
- **L-e** : les rétroliens ne viennent que du même espace ; un lien vers une page inconnue ou d'un autre espace est gardé dans `page_links` et ne se voit nulle part.
- **L-f** : pendant la première indexation d'une instance existante, ou une reconstruction, la recherche est incomplète, et le dit.
- **L-g** : rien ne borne la place de l'index sur le disque ; un compte qui peut écrire fait grossir l'index comme il fait grossir la base (lot 7).
- **L-h** : au-delà de 64 Kio par bloc et de 8 Mio par page, le texte n'est pas indexé.

## Écarts et contradictions trouvés dans la conception

Signalés, pas résolus : ce que le plan suppose en attendant est dit à chaque ligne. La tâche D ne récrit la conception qu'après la réponse de Lenny.

- **E1 — « plein texte » ou « hybride » en P1.** §9 met en P1 la « recherche plein texte » et « MCP v1 (search, …) » ; §6.5 définit l'outil `search` comme « Recherche hybride » ; §5.7 et §9 placent l'hybride en P4. *Le plan suppose* : plein texte au lot 4, et l'outil `search` du lot 5 bâti dessus.
- **E2 — le fil d'Ariane dans l'index.** §5.7 : Tantivy indexe les blocs « avec le fil d'Ariane de chaque page ». Indexé comme texte cherché, le titre d'une page parente que la personne ne voit pas (un invité sur une sous-page ; en P3, une sous-page plus ouverte que sa parente) se devinerait par la recherche ; gardé pour l'affichage, il serait montré, et périmé à chaque renommage. *Le plan suppose* : le fil est calculé à la requête, d'après ce que le compte voit (R903).
- **E3 — la forme de `blocks_index`.** §5.3 : `blocks_index(block_id, node_id, type, text, path, hash)`. Un `block_id` n'est unique que dans sa page ; `path` n'est défini nulle part (chemin du bloc dans la page, ou fil d'Ariane de la page ?) ; les rétroliens demandent une table que le §5.3 ne porte pas. *Le plan suppose* : clé `(node_id, block_id)`, pas de `path`, et trois tables de plus (`page_links`, `search_state`, `search_outbox`).
- **E4 — Tantivy n'a pas été mesuré.** §9 dit de la P0 que « les choix du §5.2 [sont] confirmés ou corrigés » ; `spike/RESULTS.md` ne dit rien de la recherche. §5.2 justifie Tantivy par « même comportement sur SQLite et Postgres » et des « index reconstructibles depuis la base », sans dire le coût : deux stockages du texte, deux validations à tenir cohérentes, un dossier de plus, des fils de fusion, et deux budgets du §5.10 (mémoire au repos, démarrage) qu'il peut toucher. FTS5 est déjà compilé dans le SQLite du serveur. *Le plan suppose* : Tantivy, mesuré d'abord (S0) ; décision L1.
- **E5 — la palette de commandes.** §4.2 : « Palette de commandes (Ctrl+K) : navigation, actions, et questions à l'IA ». Aucun lot ne porte les « actions » ; Ctrl+K est, à vérifier, le raccourci de lien de l'éditeur. *Le plan suppose* : une fenêtre de recherche, pas une palette ; décision L5.
- **E6 — `[[Page#Titre]]`.** §4.2 le range avec les rétroliens et la vue graphe ; §4.5 place les rétroliens au lot 4 et la vue graphe en P3, et ne dit rien de `[[Page#Titre]]`, que le README donne pour « pas encore ». *Le plan suppose* : hors du lot ; décision L11.
- **E7 — `mementum reindex`.** §5.7 ne dit pas s'il tourne pendant que le serveur tourne. *Le plan suppose* : serveur arrêté (R913).
- **E8 — aucun budget pour la recherche.** §5.10 n'en donne pas. *Le plan en propose trois*, à valider ; décision L10.
- **E9 — sauvegardes.** §7.4 ne dit rien d'un index dérivé. *Le plan suppose* : `search/` ne se sauvegarde pas et se refait.
- **E10 — la date de modification.** §5.3 donne `updated_at` et `updated_by` à `nodes` ; dans le code ils ne suivent que le titre et la place, pas le contenu. Le filtre « date » de l'outil `search` (§6.5) n'a donc pas de donnée aujourd'hui. *Le plan n'y touche pas* (lot 5, décision L7) ; il le signale parce que « Modifié il y a… » (ligne de la P1 sans lot) en dépend aussi.

## Décisions à prendre par Lenny

Rien de ceci ne se décide à sa place. Pour chacune : la question, la recommandation, ce que le plan suppose en attendant.

- **L1 — Le moteur : Tantivy, ou FTS5 de SQLite ?** La conception dit Tantivy (§5.2, §12 « Mes choix (à relire) » n° 7), sans mesure (E4). Tantivy : meilleur classement, racinisation, base de la recherche hybride de la P4, indépendant de SQLite et de Postgres ; coûte une dépendance lourde, un second stockage et sa cohérence. FTS5 : déjà là, dans la même transaction que `blocks_index`, rien à tenir cohérent ni à reconstruire à part ; mais ses écritures passent par le tour d'écriture unique (donc devant la frappe des autres), pas de racinisation française, et il faudrait autre chose le jour de Postgres. **Recommandation : garder Tantivy, comme la conception, si S0 tient les budgets ; sinon FTS5.** *Le plan suppose* Tantivy ; seules S0, S3 et S7 changeraient.
- **L2 — Chercher dans l'espace affiché, ou dans tous ses espaces ?** **Recommandation : l'espace affiché**, comme Notion ; tous les espaces demanderait un calcul d'accès par espace à chaque frappe. *Le plan suppose* un espace.
- **L3 — Ce qui est cherché.** *Le plan suppose* : les titres des pages, le texte des blocs, les légendes et noms de fichiers, l'adresse d'un signet, le LaTeX. Pas les commentaires, pas le contenu des fichiers (PDF), pas les versions de l'historique, pas les dossiers. **Recommandation : tel quel** ; les commentaires seraient le premier ajout (ils ont leurs droits à eux, R134).
- **L4 — La corbeille.** **Recommandation : jamais dans les résultats** ; chercher dans la corbeille serait une fonction de l'écran de la corbeille, plus tard. *Le plan suppose* cela.
- **L5 — Le raccourci et la forme.** La conception annonce une palette Ctrl+K (navigation, actions, IA). **Recommandation : au lot 4, une fenêtre de recherche**, ouverte par « Rechercher » dans la barre latérale et par Ctrl+K (Cmd+K sur Mac), sauf quand du texte est sélectionné dans l'éditeur, où Ctrl+K reste « créer un lien », comme dans Notion ; les actions et l'IA plus tard, dans la même fenêtre. *Le plan suppose* cela ; changer de touche est un changement d'une ligne.
- **L6 — Tolérance de la recherche.** *Le plan suppose* : sans casse ni accents, tous les mots voulus, le dernier complété (« proj » trouve « projet »), **sans racinisation** (« pages » ne trouve pas « page ») et sans tolérance aux fautes. **Recommandation : tel quel au lot 4** : une racinisation française fausse les textes anglais d'un même espace ; S0 chiffre ce qu'elle apporterait.
- **L7 — Filtres et tri.** L'outil `search` des agents annonce des filtres (type, date, auteur). **Recommandation : aucun filtre à l'écran au lot 4, tri par pertinence seul ; les filtres au lot 5**, avec l'outil, une fois décidé ce qu'est la date de modification d'une page (E10). *Le plan suppose* cela.
- **L8 — Les rétroliens : où, et lesquels.** *Le plan suppose* : une section « Pages qui mènent ici » sous le texte de la page, absente quand il n'y en a pas ; y comptent un lien de page, une mention de page, un lien du texte vers une page de l'instance ; seules les pages que le lecteur voit, du même espace. **Recommandation : tel quel.** Autre place possible : un compteur dans la barre du haut, qui ouvre un panneau.
- **L9 — Le délai d'indexation, et ce qu'il laisse voir.** Une page est trouvable 2 s après la dernière frappe (15 s pendant une frappe continue). Pendant ce délai, un texte effacé reste trouvable par qui lit la page (L-a). **Recommandation : accepter**, c'est le prix de ne rien mettre sur le chemin d'une frappe. *Le plan suppose* cela.
- **L10 — Les budgets de la recherche.** La conception n'en a pas (E8). Proposés : réponse du serveur < 100 ms au p95 sur 10 000 pages ; résultats affichés < 300 ms après la dernière frappe ; page trouvable < 5 s après la dernière frappe. **Recommandation : les adopter au §5.10 après S0**, aux chiffres mesurés s'ils sont meilleurs. *En attendant*, ce sont des mesures, pas des critères de refus.
- **L11 — `[[Page#Titre]]`** (lien vers un titre d'une page). Dans aucun lot (E6). **Recommandation : pas au lot 4** ; il demande un choix de schéma (une prop ou une mention neuve, donc une version de schéma), à faire avec le lot 5 ou le lot 6 (l'import d'Obsidian en a besoin). *Le plan suppose* qu'il n'y est pas.
- **L12 — Pousser.** L'accord de pousser au fil du travail valait pour le lot 3. **Recommandation : le redonner pour le lot 4**, aux mêmes conditions. *En attendant*, rien n'est poussé.
- **L13 — Indexer une instance existante.** **Recommandation : toute seule, en fond, au premier démarrage**, la recherche disant qu'elle est incomplète tant que ce n'est pas fini ; l'autre choix est une commande à lancer par l'exploitant. *Le plan suppose* l'indexation automatique.
- **L14 — Un titre mentionné devient trouvable** (L-d). Conséquence de R251, que Lenny doit encore confirmer : qui cherche le titre d'une page privée trouve les pages qui la mentionnent. **Recommandation : l'accepter avec R251** ; si R251 est défait, la projection suit sans autre travail. *Le plan suppose* R251.

Restent à lui, sans rapport direct avec ce plan mais à ne pas perdre : les lignes de la P1 du §4.5 sans lot, dont « Modifié il y a… » (E10).

## Ébauche de la checklist de recette

Sur ce qu'un test ne juge pas ; la tâche D l'écrit d'après le code final, et rappelle en tête les décisions L1 à L14 telles qu'elles auront été prises ou supposées.

**Chercher**
- [ ] Taper un mot d'un titre, puis un mot du milieu d'une longue page : les résultats viennent-ils assez vite pour qu'on continue de taper sans y penser ?
- [ ] Le bon résultat est-il en tête ? Essayer dix recherches réelles, noter celles où il ne l'est pas.
- [ ] Sans accents (« resume » pour « résumé »), en majuscules, avec un mot inachevé.
- [ ] L'extrait dit-il pourquoi la page est trouvée ? Le mot surligné se lit-il (direction A « Papier », contraste) ?
- [ ] Ouvrir un résultat : arrive-t-on au bon endroit de la page, sans que la page saute ensuite ?
- [ ] Tout au clavier : ouvrir, taper, flèches, Entrée, Échap ; sur un clavier AZERTY ; Ctrl+K avec du texte sélectionné dans la page (un lien, pas la recherche).
- [ ] « Aucun résultat » et « indexation en cours » se comprennent-ils ?

**À plusieurs**
- [ ] Un invité sur une seule page cherche un mot d'une page d'équipe : rien. Le fil d'Ariane de ses résultats ne nomme aucune page qu'il ne voit pas.
- [ ] Une page privée n'est trouvée que par son propriétaire.
- [ ] Retirer l'accès d'une personne pendant que sa fenêtre de recherche est ouverte.
- [ ] Mettre une page à la corbeille, la restaurer.
- [ ] Écrire un mot rare, attendre quelques secondes, le chercher depuis un autre compte ; l'effacer, le rechercher (limite L-a : combien de temps reste-t-il ?).

**Rétroliens**
- [ ] Mentionner une page depuis deux autres : la page visée les nomme-t-elle, à la bonne place, sans gêner la lecture ?
- [ ] Le nom de la section et sa place conviennent-ils (décision L8) ?
- [ ] Un lecteur qui ne voit qu'une des deux pages n'en voit qu'une.

**Exploitation**
- [ ] Mettre à jour une instance qui a déjà des pages : la recherche se remplit-elle seule, et le dit-elle pendant ce temps ?
- [ ] Arrêter le serveur, supprimer `search/`, relancer : tout revient.
- [ ] `mementum reindex`, serveur arrêté ; puis serveur lancé (il refuse, et le dit clairement).
- [ ] Pendant qu'un grand espace s'indexe, taper à deux dans une page : rien ne se sent.

**Pas encore** (ne pas le signaler comme un défaut) : chercher dans les commentaires, les fichiers, la corbeille, l'historique ; les filtres ; tous les espaces à la fois ; les fautes de frappe ; la palette de commandes (actions, IA) ; la vue graphe ; `[[Page#Titre]]`.

## Fin du lot

1. **Relecture finale** de tout ce qui a changé, en trois passes, chaque constat prouvé par un test jetable : **sécurité** (chaque route relue comme une attaque : ce qui sort d'une page non vue par les résultats, l'ordre, `more`, `indexing`, le temps de réponse, les rétroliens, les messages d'erreur, le journal ; puis l'indexeur comme cible : ce qu'un compte qui écrit peut lui faire coûter) ; **intégration client et serveur** ; **documentation contre le code**. Vague de correction, re-relectures ciblées.
2. **Sur un clone neuf**, étape par étape comme la CI, avec les mesures de `mementum-server` en plus.
3. **Essai de migration** sur des copies (jamais `data/instance`) : une base du lot 3 passe `0005`, avec sa copie `avant-0005.bak` ; elle s'indexe seule ; la copie remise en place, le serveur redémarre, migre de nouveau et reconstruit ; `search/` supprimé, il reconstruit.
4. **Registre** : mesures de clôture ; reports (lot 5 : l'outil `search`, les filtres, les ancres ; lot 7 : place de l'index sur le disque, débits généraux ; P4 : hybride) ; ce qui attend le propriétaire (L1 à L14, les écarts E1 à E10). `docs/REPRISE.md` à jour ; les ateliers retirés, leur `target/` supprimé.
