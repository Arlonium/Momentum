# Mementum — Phase 1, lot 3 « Collaboration et blocs riches », partie A « Collaboration » : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** à plusieurs sur un espace, on voit tout de suite ce que font les autres (pages créées, renommées, déplacées ; qui regarde la page), on commente une page ou un bloc, on revient à une version d'avant, et ce que le serveur garde d'une page est toujours ce que les navigateurs affichent.

**Le lot 3 est mené en deux plans** (Ruling R132). Celui-ci est la partie A, « Collaboration ». La partie B, « Blocs riches et organisation », aura son plan à la clôture de A : fichiers envoyés (§5.8), blocs riches utilisables depuis l'éditeur (callout, équation, signet, table des matières, lien de page), mentions et `[[`, duplication, favoris, récents, fil d'Ariane, et les reports mineurs du lot 2 (M5, M7, M9, M11, m1).

**Architecture :**
- `mementum-crdt` et `mementum-sync` : l'intégrité d'abord. Une mise à jour qui laisserait un trou d'horloge est retenue jusqu'à ce que sa suite arrive (R131) ; un tableau mal formé se lit toujours en MMD relisible (R54) ; une édition faite pendant qu'un autre déplace le bloc est reportée sur la copie (R39).
- `mementum-store` : la migration `0003` ajoute la version de l'espace (`spaces.lsn`), les commentaires, les versions de page et les déplacements de blocs.
- `mementum-server` : une WebSocket par espace, `/events/{spaceId}`, qui ne porte que la version de l'espace ; les routes des commentaires et de l'historique.
- `mementum-sync` : le nom affiché près d'un curseur est celui du compte, posé par le serveur ; une trame à part dit qui a la page ouverte, et qu'un commentaire ou une version a changé.
- `apps/web` : barre latérale en direct, avatars des présents, panneau des commentaires, historique.

**Tech Stack :** celle du lot 2. Aucune crate ni aucun paquet npm nouveau.

**Spec :** `docs/specs/2026-11-09-mementum-design.md`. Sections utiles : §4.1 (niveaux), §4.2 (écran « Page » : commentaires dans la marge, avatars des présents ; « Historique »), §4.3 (barre du haut), §4.5 (« Collaboration et organisation », lignes de la P1), §5.3 (`comments`, `ops_log`), §5.4 (canal `space:<id>`), §5.6 (présence, synchronisation), §9, §11 (déplacement concurrent d'un bloc). Lire aussi les pièges de `CLAUDE.md`, et au registre : R39 (lot 1), R52, R54, R97, R101 à R125, R128 (lot 2), R129 à R131 (ce lot).

**Hors de ce plan :**
- partie B du lot 3 : voir plus haut ;
- lot 4 : recherche ; lot 5 : jetons d'API, REST complet, MCP, écriture MMD par différence (R40), journal d'opérations détaillé s'il lui faut (voir R133) ; lot 6 : imports et export ; lot 7 : plafonds et limites de débit (R54, R121), relecture périodique des sessions (R125) ;
- P3 : suggestions, boîte de réception et notifications (une mention dans un commentaire ne prévient personne), frise complète de l'historique, comparaison de deux versions ;
- les suppressions concurrentes d'un déplacement de bloc, et les éditions concurrentes dans une cellule de tableau déplacée (Task 13, limites dites).

## Global Constraints

- **Langue** : documentation, commentaires et messages de commit en français, accents compris. Identifiants de code en anglais.
- **Commits** : petits, un sujet chacun, **sans aucune ligne d'attribution d'outil** (ni `Co-Authored-By`, ni `Claude-Session`, ni « Generated with »). Rien n'est poussé sans une demande explicite du propriétaire.
- **Contraintes des lots 1 et 2** (licences, versions, Yjs 13, `OffsetKind::Utf16`, identifiants UUIDv7, SQLite, durabilité, design A « Papier », sécurité, erreurs JSON, types partagés par `ts-rs`) : celles des plans des lots 1 et 2, « Global Constraints », restent valables telles quelles.
- **Dépendances** : aucune ne s'ajoute. Toute dépendance nouvelle passe par un Ruling. `cargo deny check licenses bans` et `node tools/check-npm-licenses.mjs` restent au vert.
- **Droits** : dans les routes, tout passe par `crates/mementum-server/src/access.rs`. Une page qu'on ne voit pas répond `404 not_found`, exactement comme une page qui n'existe pas, sur toute route nouvelle. **Aucun identifiant ni titre d'une page ne sort vers quelqu'un qui ne la voit pas**, par aucun canal nouveau.
- **Octets d'un client** : jamais écrits ni relayés tels que reçus (R123, R130). Ce que le serveur envoie de neuf (trame de page, version de l'espace) est écrit par lui.
- **Budgets** (§5.10, tels quels) : ouverture d'une page jamais vue sous 300 ms jusqu'à 1 000 blocs, sous 3 s pour 10 000 ; frappe visible chez un autre sous 150 ms. **Un budget ne se relâche jamais pour faire passer un test.**
- **Migration** : une seule pour le lot, `0003_collaboration.sql` ; elle peut changer jusqu'à la clôture (Ruling R139). **Aucun binaire de ce lot n'ouvre les données d'une instance que quelqu'un regarde** avant la clôture.
- **Schéma des blocs** : ce plan n'ajoute ni bloc ni marque. `SCHEMA_VERSION` reste à 1.
- **Postes et ports** : les e2e prennent 5173 et 8080, ou `MEMENTUM_E2E_WEB_PORT` et `MEMENTUM_E2E_API_PORT`. On n'arrête jamais un processus qu'on n'a pas lancé.
- **Registre des écarts** : `docs/superpowers/logs/2026-11-12-p1-lot3-ledger.md`, sous la forme `Ruling RNN: <décision> — <pourquoi> — <coût si faux>`. Les décisions de ce plan sont R132 à R139 ; l'exécution continue à R140.
- **Relectures** : une par groupe de tâches (1-2, 3-5, 6-7, 8-9, 10-11, 12-13), puis la relecture finale de tout le lot en trois passes (sécurité, intégration client et serveur, documentation contre le code), et toutes les suites sur un clone neuf.

## Review Focus

1. **Rôle retiré pendant que le canal de l'espace est ouvert** : la personne ne reçoit plus rien, sans attendre qu'elle recharge. Test : Task 4, `events_stop_for_a_removed_member`.
2. **Ce que le canal de l'espace laisse voir à un invité** : rien d'autre qu'un numéro, jamais l'identifiant ni le titre d'une page qu'il ne voit pas. Test : Task 4, `space_events_carry_only_the_version`.
3. **Commenter sans en avoir le droit, ou sur une page à la corbeille** : `403` sous « Peut commenter », `409 trashed` à la corbeille, `404` sur une page qu'on ne voit pas, pour chaque route des commentaires. Test : Task 8, `comment_routes_follow_the_levels`.
4. **Commentaire dont le bloc a disparu** (supprimé, ou page revenue à une version d'avant) : le fil reste, lisible, rangé à part. Tests : Task 8, `thread_survives_its_block` ; Task 9, `orphan_thread_is_listed_apart`.
5. **Restaurer une version pendant que quelqu'un écrit** : ce que la restauration remplace est lui-même une version, qu'on peut restaurer à son tour. Test : Task 10, `restore_cuts_a_version_of_what_it_replaces`.

---

## Décisions de ce plan

Elles traduisent la conception pour ce lot. Chacune est un Ruling du registre.

**Canal de l'espace (R133).** La conception (§5.4) prévoit une WebSocket multiplexée, où le serveur pousse les opérations filtrées par les droits. Ce lot livre moins, et plus sûr : chaque écriture qui change ce que montre la barre latérale avance une version de l'espace, `spaces.lsn` ; le canal `/events/{spaceId}` pousse ce numéro, et rien d'autre ; le client relit l'arbre par la route qui existe, déjà filtrée par `access.rs`. Aucun second calcul de « qui voit quoi » n'est écrit pour un flux poussé. Pas de table `ops_log` : elle n'aurait pas de lecteur. Le canal est une WebSocket, pas un flux SSE : un navigateur n'ouvre que six connexions HTTP/1.1 par hôte, et six onglets en SSE bloqueraient toute l'application. `spaces.rev` garde son rôle (clé des caches d'accès) ; `lsn` avance aussi pour un renommage et un changement de place, que `rev` ignore exprès (R83).

**Trame de page (R136).** Ce qui ne doit aller qu'aux personnes qui lisent une page passe par sa connexion de synchro, dans une trame que le serveur écrit : type 100, puis un JSON. Trois sortes : `{"type":"roster","users":[{"id","name","canWrite"}]}` (qui a la page ouverte, une ligne par compte), `{"type":"comments"}` et `{"type":"versions"}` (relire, par l'API). Le client la retire avant y-websocket. Un client n'en envoie jamais : reçue d'un client, elle rend le message illisible, comme tout type inconnu.

**Présence vérifiée (R136).** Le nom d'un état de présence est celui du compte de la connexion qui l'annonce : le serveur l'écrit, quoi que le client ait mis. Un identifiant de client reste au compte qui l'a tenu tant que l'acteur de la page vit : personne ne le prend pendant une coupure (R109, cas a et c). La couleur reste choisie par le client, dans la forme `#RRGGBB`.

**Commentaires (R134).** Un fil est attaché à une page, et au plus à un bloc, par l'identifiant du bloc (§5.3). Rien n'est écrit dans le document : quelqu'un qui peut commenter sans pouvoir modifier commente, et le MMD n'en porte rien. Les commentaires de BlockNote, eux, posent une marque dans le texte : il faudrait le droit d'écrire, un schéma neuf et une forme MMD.

| Action | Niveau minimal | À la corbeille |
|---|---|---|
| Lire les fils d'une page | `read` | oui |
| Ouvrir un fil, répondre, réagir | `comment` | `409 trashed` |
| Modifier son message | `comment`, et en être l'auteur | `409 trashed` |
| Supprimer un message | en être l'auteur avec `comment`, ou `full` | `409 trashed` |
| Résoudre ou rouvrir un fil | l'avoir ouvert avec `comment`, ou `edit` | `409 trashed` |

**Historique (R135).** Une version est l'état Yjs complet d'une page à un instant, avec les comptes qui l'ont modifiée depuis la version d'avant. Elle est coupée par l'acteur de la page : juste avant d'appliquer une écriture, quand la première écriture non versionnée a plus de dix minutes ; quand l'acteur s'arrête, s'il reste des écritures non versionnées ; et toujours avant une restauration. Une page garde ses 100 dernières versions. Restaurer écrit les blocs de la version par-dessus le contenu : c'est une écriture comme une autre, les identifiants des blocs sont gardés, et les fils de commentaires retrouvent leurs blocs.

**Tableaux mal formés (R137).** Le serveur ne récrit pas un tableau qu'un client a mal formé. Il le lit toujours comme une grille : `read_blocks` rend un tableau que `parse` accepte, quoi que porte le document.

**Bloc déplacé (R138).** Piste (b) du Ruling R39 : rien ne change dans le client ; le serveur reconnaît un déplacement dans une mise à jour reçue, et reporte sur la copie le texte que d'autres ont inséré dans l'original sans que l'auteur du déplacement l'ait vu. Dans les deux ordres d'arrivée.

---

## Structure des fichiers

| Fichier | Rôle |
|---|---|
| `crates/mementum-crdt/src/guard.rs` | `Shape::leaves_gap` ; lecture des identifiants de blocs insérés |
| `crates/mementum-crdt/src/table.rs` | lecture d'un tableau en grille |
| `crates/mementum-crdt/src/moves.rs` (neuf) | reconnaître un déplacement, reporter les insertions |
| `crates/mementum-store/migrations/0003_collaboration.sql` (neuf) | `spaces.lsn`, commentaires, versions, déplacements |
| `crates/mementum-store/src/comments.rs`, `versions.rs`, `moves.rs` (neufs) | accès à ces tables |
| `crates/mementum-store/src/spaces.rs` | `bump_lsn`, `space_lsn`, annonce des changements |
| `crates/mementum-sync/src/actor.rs` | mises à jour retenues, trame de page, présence, versions |
| `crates/mementum-sync/src/protocol.rs`, `presence.rs` | trame 100, nom posé |
| `crates/mementum-server/src/events.rs`, `routes/events.rs` (neufs) | canal de l'espace |
| `crates/mementum-server/src/routes/comments.rs`, `routes/versions.rs` (neufs) | routes |
| `apps/web/src/space/useSpaceEvents.ts` (neuf) | canal de l'espace côté client |
| `apps/web/src/editor/collab.ts`, `schemaGate.ts` | trame de page |
| `apps/web/src/page/Presence.tsx`, `PageMenu.tsx` (neufs) | avatars, menu « … » |
| `apps/web/src/comments/` (neuf) | panneau, fil, rédaction, marque dans la marge |
| `apps/web/src/history/` (neuf) | liste des versions, aperçu |

---

### Task 1 : une mise à jour qui laisserait un trou est retenue (R131)

**Files:**
- Modify: `crates/mementum-crdt/src/guard.rs`, `crates/mementum-crdt/src/page.rs`
- Modify: `crates/mementum-sync/src/actor.rs`, `crates/mementum-sync/src/lib.rs` (en-tête)
- Test: `crates/mementum-crdt/tests/updates.rs`, `crates/mementum-sync/tests/sync.rs`, `apps/web/e2e/offline.spec.ts` (neuf)

**Interfaces:**
- Produces: `PageDoc::leaves_gap(&self, update: &[u8]) -> Result<bool, CrdtError>` ; dans l'acteur, `MAX_HELD_UPDATES: usize = 32` et `MAX_HELD_BYTES: usize = 1 << 20`, par connexion.

- [ ] **Step 1 : test de la crate.** `update_ahead_of_its_client_leaves_a_gap` : un client fait deux éditions `a1` puis `a2` qui ne dépendent pas l'une de l'autre. Sur une page à l'état de départ : `leaves_gap(&a2) == Ok(true)`, `leaves_gap(&a1) == Ok(false)` ; après `apply_client_update(&a1)`, `leaves_gap(&a2) == Ok(false)`. Une mise à jour dont tous les blocs sont déjà connus : `false`. Une mise à jour qui porte un `Skip` avant un bloc inconnu : `true`. La page n'a changé dans aucun de ces appels (`revision()` égale).
- [ ] **Step 2 : le lancer, le voir échouer** (`cargo test -p mementum-crdt --test updates leaves_a_gap`).
- [ ] **Step 3 : `Shape::leaves_gap(&self, state: &StateVector) -> bool`.** Pour chaque client de la mise à jour, dans l'ordre des horloges : `covered` part de `state.get(client)` ; un bloc `Insert` ou `Gc` dont l'horloge dépasse `covered` est un trou ; sinon `covered = max(covered, fin du bloc)` ; un `Skip` n'avance rien. `PageDoc::leaves_gap` lit la mise à jour par `Shape::read` et compare, sans transaction d'écriture.
- [ ] **Step 4 : tests de synchro, vus rouges avant l'acteur.**
  - `update_that_leaves_a_gap_is_held_until_the_gap_is_filled` : `a2` envoyée seule ; le serveur lit encore l'ancien texte, le journal n'a rien de plus, l'observatrice (socket brute de `tests/wire.rs`) n'a rien reçu ; puis `a1` : le serveur lit les deux éditions, le journal porte `a1` puis `a2`, l'observatrice reçoit les deux, dans cet ordre.
  - `held_updates_are_bounded` : 33 mises à jour à trou d'une même connexion la ferment ; rien n'a été écrit.
  - `held_update_is_dropped_when_its_connection_leaves` : la connexion part, `a1` arrive d'une autre : seule `a1` est appliquée.
- [ ] **Step 5 : l'acteur.** Une mise à jour de client pour laquelle `leaves_gap` est vrai n'est ni appliquée, ni écrite, ni relayée : elle attend dans la file de sa connexion. Après chaque mise à jour appliquée, d'où qu'elle vienne, les files de toutes les connexions sont réessayées, dans l'ordre, jusqu'à ce que plus rien ne passe. Au-delà d'une borne, la connexion est fermée. `PageDoc::apply_update` (relecture du stockage) ne change pas : `revision_is_canonical` reste vert.
- [ ] **Step 6 : e2e `typing_right_after_coming_back_online_loses_nothing`** (`offline.spec.ts`) : une personne hors ligne (`context.setOffline(true)`) tape « hors ligne » ; elle revient et tape aussitôt « en ligne », sans attendre la resynchro ; le MMD du serveur finit par porter les deux, et sa connexion n'a été fermée qu'une fois au plus (compter les ouvertures de `/sync` par `page.on("websocket")`).
- [ ] **Step 7 : tout relancer** (`cargo test -p mementum-crdt -p mementum-sync`, l'e2e), puis commits : `crdt : dire si une mise à jour laisserait un trou d'horloge (R131)`, `synchro : une mise à jour qui laisserait un trou attend sa suite (R131)`, `e2e : taper au retour du réseau ne perd rien`.
- [ ] **Step 8 : documentation.** `CLAUDE.md` (le piège R131 passe au présent), conception §5.6, registre.

### Task 2 : un tableau mal formé se lit toujours en MMD relisible (R54, R137)

**Files:**
- Modify: `crates/mementum-crdt/src/table.rs`
- Test: `crates/mementum-crdt/tests/roundtrip.rs`, `crates/mementum-crdt/tests/unknown.rs`

**Interfaces:**
- Produces: `read_table` rend toujours un `Table` que `mementum_md::parse(render(…))` accepte.

- [ ] **Step 1 : test de propriétés `any_yjs_table_reads_as_mmd_that_parses`** (`roundtrip.rs`, proptest) : un tableau écrit directement dans le document Yjs, comme le ferait un client, avec des lignes de longueurs inégales, des lignes sans cellule, des `colspan` et `rowspan` de 0 à 200, des cellules qui se recouvrent. Pour chacun : `parse(&render(&page.read_blocks()).markdown)` est `Ok`, et redonne `page.read_blocks()`.
- [ ] **Step 2 : trois cas nommés** (`unknown.rs`) : `short_row_is_read_padded` (une ligne plus courte reçoit des cellules vides à droite), `span_beyond_the_grid_is_read_clamped` (`colspan` ramené à ce qui reste de la ligne, `rowspan` au nombre de lignes restantes), `row_covered_by_rowspans_is_read_with_its_cells` (une ligne entièrement couverte rend le `rowspan` qui la couvre plus court, pour qu'elle garde ses cellules).
- [ ] **Step 3 : les voir échouer**, puis écrire la lecture en grille dans `read_table`. La règle est celle de `check_grid` (`crates/mementum-md/src/parse.rs`, R37), lue à l'envers : ce qu'il refuserait est ramené à ce qu'il accepte, sans rien écrire dans le document. `header_rows`, `header_cols` et les largeurs se calculent sur la grille obtenue.
- [ ] **Step 4 : relancer** `cargo test -p mementum-crdt -p mementum-md` ; les fixtures de BlockNote (`blocknote_fixtures.rs`) ne changent pas d'un octet.
- [ ] **Step 5 : commit** `crdt : un tableau mal formé par un client se lit en grille (R54, R137)`, puis la conception (§6.2, « Tableaux ») et le registre.

### Task 3 : store, migration 0003 et version de l'espace

**Files:**
- Create: `crates/mementum-store/migrations/0003_collaboration.sql`
- Modify: `crates/mementum-store/src/spaces.rs`, `nodes.rs`, `acl.rs`, `members.rs`, `invitations.rs`, `signup.rs`, `lib.rs`
- Test: `crates/mementum-store/tests/migration.rs`, `crates/mementum-store/tests/lsn.rs` (neuf)

**Interfaces:**
- Produces: `Store::space_lsn(&self, id: Uuid) -> Result<Option<i64>, StoreError>` ; `Store::changes(&self) -> tokio::sync::broadcast::Receiver<Uuid>` (l'espace dont la version vient d'avancer, envoyé après le commit) ; les tables des tâches 8, 10 et 13.

```sql
ALTER TABLE spaces ADD COLUMN lsn INTEGER NOT NULL DEFAULT 0;

CREATE TABLE comment_threads (
    id TEXT PRIMARY KEY NOT NULL,
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    block_id TEXT,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    resolved_at INTEGER,
    resolved_by TEXT REFERENCES users(id)
);
CREATE INDEX comment_threads_by_node ON comment_threads (node_id, created_at);

CREATE TABLE comments (
    id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES comment_threads(id) ON DELETE CASCADE,
    author_id TEXT NOT NULL REFERENCES users(id),
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    edited_at INTEGER
);
CREATE INDEX comments_by_thread ON comments (thread_id, created_at);

CREATE TABLE comment_reactions (
    comment_id TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    emoji TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (comment_id, user_id, emoji)
);

CREATE TABLE doc_versions (
    id TEXT PRIMARY KEY NOT NULL,
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    state_blob BLOB NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('periodic', 'closing', 'before_restore')),
    authors TEXT NOT NULL,          -- JSON : identifiants des comptes, triés
    created_at INTEGER NOT NULL
);
CREATE INDEX doc_versions_by_node ON doc_versions (node_id, created_at);

CREATE TABLE block_moves (
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,           -- seq de la mise à jour qui a déplacé
    record BLOB NOT NULL,           -- écrit par `mementum-crdt` (Task 13)
    created_at INTEGER NOT NULL,
    PRIMARY KEY (node_id, seq)
);
```

- [ ] **Step 1 : tests de la migration** (`migration.rs`) : `lot2_database_gains_the_collaboration_tables` (une base du lot 2, bâtie avec `0001` et `0002` seuls comme `lot1_database` le fait pour `0001`, puis migrée : ses pages, accès et documents sont inchangés, `spaces.lsn` vaut 0, les cinq tables existent et sont vides) ; `deleting_a_page_deletes_its_threads_versions_and_moves`.
- [ ] **Step 2 : tests de la version** (`lsn.rs`) : `every_structural_write_advances_lsn`, un tableau des écritures : création, renommage, place parmi les mêmes frères, changement de parent, corbeille, restauration, suppression, accès posé, retiré, changé, membre ajouté, rôle changé, membre retiré, invitation acceptée. Chacune avance `lsn` d'au moins 1, et envoie l'espace sur `changes()` **après** le commit (un lecteur qui relit `space_lsn` à la réception voit la nouvelle valeur). `content_update_does_not_advance_lsn` (`append_updates`). `refused_write_announces_nothing` (une écriture refusée, `Stale` ou autre : ni `lsn`, ni annonce).
- [ ] **Step 3 : les voir échouer**, puis écrire. `bump_rev` avance aussi `lsn` ; `rename_node`, et `update_node` quand rien ne change que la place ou le titre, avancent `lsn` seul. L'annonce part d'un seul endroit, après le commit, pour qu'aucune écriture future ne l'oublie : une transaction du store qui a avancé `lsn` le dit en se validant.
- [ ] **Step 4 : relancer** `cargo test -p mementum-store`, et la copie avant migration (`backup.rs`) : `mementum.db.avant-0003.bak` est écrite pour une base du lot 2.
- [ ] **Step 5 : commit** `store : migration 0003, version de l'espace et tables de la collaboration (R133, R139)`.

### Task 4 : serveur, canal de l'espace

**Files:**
- Create: `crates/mementum-server/src/events.rs`, `crates/mementum-server/src/routes/events.rs`
- Modify: `crates/mementum-server/src/lib.rs` (route, tableau de l'en-tête), `routes/sync.rs` (les contrôles d'avant la poignée de main deviennent communs)
- Test: `crates/mementum-server/tests/events.rs` (neuf)

**Interfaces:**
- Consumes: `Store::space_lsn`, `Store::changes` (Task 3).
- Produces: `GET /events/{spaceId}`, WebSocket. Le serveur y écrit des trames texte `{"lsn":<entier>}` : une à l'ouverture, puis une chaque fois que la version avance. Il n'en lit aucune : toute trame de données reçue (texte ou binaire) ferme la connexion. Refus avant la poignée de main, dans l'ordre de `/sync` : `403 bad_origin`, `401`, `404 not_found` (identifiant mal formé, espace inconnu, ou aucun rôle), `400 invalid_request`.

- [ ] **Step 1 : tests, vus rouges.**
  - `space_events_carry_only_the_version` : une membre et un invité (qui ne voit qu'une page) ouvrent le canal ; la membre crée une page privée, la renomme, la met à la corbeille. Chacun reçoit des trames, toutes de la forme exacte `{"lsn":N}`, croissantes ; aucune ne contient l'identifiant ni le titre de la page.
  - `first_frame_is_the_current_version`.
  - `events_stop_for_a_removed_member` : la personne est retirée de l'espace ; son canal est fermé par le serveur, et une écriture faite ensuite ne lui envoie rien.
  - `events_stop_when_the_session_ends` : déconnexion, changement de mot de passe.
  - `events_refusals_match_sync` : les quatre refus, mêmes statuts et mêmes corps que `/sync`.
  - `idle_events_connection_is_kept_alive` : un ping WebSocket toutes les 20 s.
- [ ] **Step 2 : `events.rs`.** Une tâche par connexion : elle attend `Store::changes()` pour son espace, relit `space_lsn`, et n'écrit que si le numéro a monté. Avant chaque écriture et à chaque ping, elle revérifie que la session est encore celle du compte et que le compte a encore un rôle (`role_at_least`) : sinon elle ferme. Une connexion qui ne lit pas est fermée quand sa file dépasse 64 trames.
- [ ] **Step 3 : relancer** `cargo test -p mementum-server`, puis commit `serveur : canal de l'espace, qui ne porte que sa version (R133)`.

### Task 5 : client, barre latérale en direct

**Files:**
- Create: `apps/web/src/space/useSpaceEvents.ts`, `apps/web/src/space/useSpaceEvents.test.ts`
- Modify: `apps/web/src/space/useSpaceTree.ts`, `apps/web/src/App.tsx`, `apps/web/vite.config.ts` (mandataire de `/events`, WebSocket comprise, sans `changeOrigin`)
- Test: `apps/web/e2e/live-tree.spec.ts` (neuf)

**Interfaces:**
- Consumes: `GET /events/{spaceId}` (Task 4).
- Produces: `useSpaceEvents(spaceId: string | null, onChange: () => void): { live: boolean }` ; `EVENTS_DEBOUNCE_MS = 250` ; reconnexion après 1 s, puis le double à chaque échec, 30 s au plus.

- [ ] **Step 1 : tests Vitest, vus rouges** (WebSocket simulée, comme `collab.test.ts`) : `a_higher_version_reloads_the_tree_once` (trois trames en 100 ms : un seul `onChange`, 250 ms après la dernière) ; `the_first_frame_reloads_nothing_when_the_tree_is_as_recent` ; `a_lower_or_equal_version_is_ignored` ; `a_closed_channel_comes_back` ; `live_is_false_while_the_channel_is_down`.
- [ ] **Step 2 : `useSpaceEvents`**, puis `useSpaceTree` : tant que `live` est vrai, le minuteur de dix secondes (R128) ne tourne pas ; il reprend dès que le canal tombe. La relecture au retour sur l'onglet (R97) et celle du compte (R126) restent.
- [ ] **Step 3 : e2e, vus rouges avant le branchement.** `page_created_by_someone_else_appears_at_once` (elle apparaît en moins de 2 s, sans que l'onglet perde le premier plan : la borne est large, le minuteur de dix secondes ne peut pas la faire passer à tort) ; `rename_and_move_by_someone_else_appear_at_once` ; `guest_sees_nothing_new_when_the_space_changes` (l'invité ne voit toujours que sa page) ; `tree_falls_back_to_polling_when_the_channel_is_down` (`page.routeWebSocket("**/events/**", …)` ferme le canal : la page créée arrive par le minuteur).
- [ ] **Step 4 : relancer** Vitest, typecheck, les e2e de `tree`, `reload` et `pages` (ceux qui comptent les lectures de l'arbre : voir le piège R128 de `CLAUDE.md`), puis commits `web : le canal de l'espace relit la barre latérale dès qu'il change (R133)`, `e2e : la barre latérale suit en direct`.
- [ ] **Step 5 : documentation.** README (la limite R97 et R128 devient un repli), conception §4.2 et §9, `CLAUDE.md` (piège « Barre latérale relue toutes les dix secondes »), registre.

### Task 6 : synchro, trame de page et présence vérifiée

**Files:**
- Modify: `crates/mementum-sync/src/protocol.rs`, `presence.rs`, `actor.rs`, `hub.rs`, `lib.rs`
- Modify: `crates/mementum-server/src/routes/sync.rs` (le nom du compte dans `ConnInfo`)
- Test: `crates/mementum-sync/tests/presence.rs`, `crates/mementum-sync/tests/wire.rs`, `crates/mementum-sync/tests/roster.rs` (neuf)

**Interfaces:**
- Produces:
  - `ConnInfo.name: Option<Arc<str>>`, le nom du compte, lu avec la session ;
  - `pub enum PageEvent { Comments, Versions }` et `Hub::page_event(&self, node_id: Uuid, event: PageEvent)` : sans effet si la page n'a pas d'acteur vivant ;
  - `protocol::MESSAGE_PAGE_EVENT: u8 = 100` ; trame : `100`, longueur en entier variable, JSON en UTF-8 ;
  - `presence::display_name(name: &str) -> String` : le nom sans caractère de contrôle, coupé à 100 caractères, « Sans nom » s'il n'en reste rien ;
  - `MAX_REMEMBERED_CLIENTS: usize = 4096`.

- [ ] **Step 1 : tests, vus rouges.**
  - `presence_name_is_the_account_name` (`wire.rs`, lu à la manière de lib0) : Mallory annonce `{"user":{"name":"Alice"}}` ; l'observatrice reçoit le nom du compte de Mallory. Un état sans `user` en reçoit un. Une connexion sans compte (tests) garde le nom annoncé.
  - `account_name_with_control_characters_has_a_presence` : la conséquence connue de R119 est levée.
  - `client_id_stays_with_its_account_while_the_actor_lives` : Alice part ; Mallory annonce le client d'Alice, à une horloge plus haute : ignoré ; Alice revient et l'annonce : gardé, relayé. Les cas (a) et (c) de R109.
  - `presence_returns_at_once_after_a_reconnect` : après une coupure, le premier état annoncé par la connexion qui revient est relayé, sans attendre un second.
  - `roster_names_everyone_on_the_page` (`roster.rs`) : une trame `roster` part à tous à chaque arrivée et à chaque départ qui change la liste ; une ligne par compte, même avec deux onglets ; une lectrice y figure avec `"canWrite":false` ; un arrivant la reçoit dans son accueil.
  - `page_event_reaches_every_connection_of_the_page`, lecteurs compris, et aucune autre page.
  - `frame_100_from_a_client_is_illegible` : la connexion est fermée, rien n'est relayé.
- [ ] **Step 2 : écrire.** L'état gardé d'une connexion est réécrit avec `display_name` du compte avant d'être appliqué et relayé (R123 : toujours ce qui a été lu, réécrit). La table des titulaires survit aux départs, bornée. Pour le retour de la présence, suivre la piste du registre du lot 2 : retirer l'état d'une connexion qui part sans avancer l'horloge de son client.
- [ ] **Step 3 : relancer** `cargo test -p mementum-sync -p mementum-server`, dont les tests de présence du lot 2, qui ne doivent pas changer de sens. Commits : `synchro : trame de page, qui dit qui a la page ouverte (R136)`, `synchro : le nom d'une présence est celui du compte, et un client reste à son compte (R136, R109)`.

### Task 7 : client, qui regarde la page

**Files:**
- Create: `apps/web/src/page/Presence.tsx`, `apps/web/src/page/Presence.test.tsx`
- Modify: `apps/web/src/editor/schemaGate.ts`, `collab.ts`, `collab.test.ts`, `apps/web/src/page/PageView.tsx`, `apps/web/src/theme.css`
- Test: `apps/web/e2e/presence.spec.ts` (neuf)

**Interfaces:**
- Consumes: la trame de page (Task 6).
- Produces: dans `CollabSession`, `roster: RosterUser[]` (`{ id: string; name: string; canWrite: boolean }`), annoncé par `subscribe`, et `onPageEvent(listener: (event: "comments" | "versions") => void): () => void`. `gatedWebSocket(check, beforeMessage)` : `beforeMessage` rend `true` quand il a pris le message, qui n'est alors pas donné à y-websocket.

- [ ] **Step 1 : tests Vitest, vus rouges** (`collab.test.ts`) : `page_frame_never_reaches_y_websocket` (ni en écriture, ni en lecture seule) ; `roster_frame_updates_the_session` ; `unknown_page_frame_is_dropped_without_an_error` (un `type` inconnu, un JSON illisible, un type de message 101) ; `page_frame_before_the_first_sync_is_kept`.
- [ ] **Step 2 : `Presence.tsx`** : les avatars des comptes de `roster`, hors soi, dans la barre du haut, à gauche de « Partager » : cinq au plus, puis « +N » ; initiale sur la couleur du compte (`colorOf`, `session/session.ts`) ; le nom en infobulle et en `aria-label`, suivi de « (lecture) » pour qui ne peut pas écrire. Design A « Papier » : pastilles de 24 px, bord blanc de 2 px, qui se chevauchent de 6 px.
- [ ] **Step 3 : la présence au retour.** À chaque connexion rétablie, la session annonce de nouveau son état (`awareness.setLocalState(awareness.getLocalState())`).
- [ ] **Step 4 : e2e, vus rouges.** `everyone_on_the_page_is_shown_in_the_top_bar` (une lectrice aussi) ; `leaving_the_page_removes_the_avatar` ; `cursor_label_is_the_account_name` (un client brut annonce un autre nom : l'étiquette du curseur porte celui de son compte) ; `cursor_returns_at_once_after_a_reconnect` (moins de 3 s, pour quelqu'un qui ne tape pas : le piège « e2e qui change des droits » de `CLAUDE.md` est levé).
- [ ] **Step 5 : relancer** Vitest, typecheck, `presence-guard.spec.ts`, `keepalive.spec.ts`. Commits : `web : la trame de page ne va pas à y-websocket`, `web : les présents dans la barre du haut`. Puis README (la limite R109 se réduit à ce qui en reste), conception §4.3 et §5.6, `CLAUDE.md`, registre.

### Task 8 : serveur, commentaires

**Files:**
- Create: `crates/mementum-store/src/comments.rs`, `crates/mementum-server/src/routes/comments.rs`
- Modify: `crates/mementum-server/src/access.rs` (`writes(Action::Comment)` devient vrai ; `level_name`), `types.rs`, `lib.rs`, `error.rs`
- Test: `crates/mementum-store/tests/comments.rs`, `crates/mementum-server/tests/comments.rs` (neufs), `tests/permissions.rs`

**Interfaces:**
- Consumes: `Hub::page_event(id, PageEvent::Comments)` (Task 6), après chaque écriture validée.
- Produces (types `ts-rs`, ajoutés à `bindings()`) :
  - `Thread { id, blockId: Option<String>, createdAt, resolvedAt: Option<i64>, resolvedBy: Option<Author>, comments: Vec<Comment> }` ;
  - `Comment { id, author: Author, body, createdAt, editedAt: Option<i64>, reactions: Vec<Reaction> }` ;
  - `Reaction { emoji, users: Vec<Author> }` ; `Author { id, name }` (pas l'adresse e-mail : un invité ne lit pas celle des autres) ;
  - `NewThread { blockId: Option<String>, body }`, `NewComment { body }`, `ResolveThread { resolved: bool }`.

| Route | Niveau | Réponse |
|---|---|---|
| `GET /api/v1/pages/{id}/comments` | `read` | `Thread[]`, fils résolus compris, du plus ancien au plus récent |
| `POST /api/v1/pages/{id}/comments` | `comment` | `201`, `Thread` ; `422 invalid_body` ; `422 invalid_block` si `blockId` ne nomme aucun bloc de la page |
| `POST /api/v1/pages/{id}/comments/{threadId}/replies` | `comment` | `201`, `Comment` |
| `PATCH /api/v1/pages/{id}/comments/{threadId}` | voir « Décisions » | `Thread` |
| `PATCH /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}` | son auteur, `comment` | `Comment` ; un autre que l'auteur : `403` |
| `DELETE /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}` | son auteur avec `comment`, ou `full` | `204` ; le fil part avec son dernier message |
| `PUT` et `DELETE /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}/reactions/{emoji}` | `comment` | `Comment` ; `422 invalid_emoji` |

Un fil ou un message qui n'est pas de cette page répond `404 not_found`. Le corps d'un message : 1 à 10 000 caractères une fois les espaces de bord retirés, sans caractère de contrôle autre que le saut de ligne. Un emoji : 1 à 16 octets, sans espace ni caractère de contrôle.

- [ ] **Step 1 : tests du store, vus rouges** : `thread_with_replies_and_reactions_reads_back_in_order` ; `deleting_the_last_message_deletes_the_thread` ; `same_reaction_twice_is_one_reaction` ; `threads_of_a_deleted_page_are_gone`.
- [ ] **Step 2 : tests du serveur, vus rouges.**
  - `comment_routes_follow_the_levels` : la matrice du tableau « Décisions », pour `read`, `comment`, `edit`, `full`, l'auteur et un autre, sur chaque route ; à la corbeille, toute écriture répond `409 trashed` et la lecture passe.
  - `foreign_page_is_indistinguishable_from_missing` (`permissions.rs`) gagne les routes des commentaires.
  - `thread_survives_its_block` : le bloc est supprimé par `PUT /mmd` ; le fil est toujours lu, avec son `blockId`.
  - `comment_write_tells_the_page` : une connexion de synchro de la page reçoit la trame `{"type":"comments"}` ; une connexion d'une autre page, rien.
  - `invalid_block_is_refused`, `body_bounds`, `emoji_bounds`.
- [ ] **Step 3 : écrire le store et les routes**, puis régénérer les types (`UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed`).
- [ ] **Step 4 : relancer** `cargo test --workspace`. Commits : `store : fils de commentaires, messages et réactions`, `serveur : commentaires d'une page et de ses blocs (R134)`.

### Task 9 : client, commentaires

**Files:**
- Create: `apps/web/src/comments/CommentsPanel.tsx`, `ThreadView.tsx`, `Composer.tsx`, `useComments.ts`, `BlockMarks.tsx`, et leurs tests
- Modify: `apps/web/src/api.ts`, `apps/web/src/page/PageView.tsx`, `apps/web/src/editor/Editor.tsx` (entrée « Commenter » du menu d'un bloc), `apps/web/src/share/levels.ts` (`canComment`), `apps/web/src/theme.css`
- Test: `apps/web/e2e/comments.spec.ts` (neuf)

**Interfaces:**
- Consumes: les routes de la Task 8 ; `session.onPageEvent` (Task 7).
- Produces: `useComments(pageId, session): { threads, state, open(blockId | null, body), reply, edit, remove, resolve, react }` ; il relit les fils à chaque trame `comments`, et après chacune de ses écritures.

- [ ] **Step 1 : tests Vitest, vus rouges** : `threads_are_reread_on_a_page_frame` ; `orphan_thread_is_listed_apart` (un fil dont le bloc n'est plus dans le document est rangé sous « Bloc supprimé ») ; `resolved_threads_are_hidden_until_asked` ; `composer_is_absent_below_comment_level` ; `a_refusal_is_shown_as_the_server_says_it`.
- [ ] **Step 2 : l'écran.** Un bouton « Commentaires » dans la barre du haut, avec le nombre de fils ouverts, ouvre un panneau à droite de la page (320 px ; sous 1 100 px de large, par-dessus la page). De haut en bas : rédiger un commentaire de page ; les fils ouverts, dans l'ordre des blocs dans la page, puis les fils de page ; « Bloc supprimé » ; un lien « Fils résolus (N) ». Un fil montre ses messages (auteur, « il y a… », « modifié »), les réactions (👍 ❤️ 😄 🎉 👀 ✅), « Répondre », « Résoudre ». Chaque bloc qui a un fil ouvert porte une pastille dans la marge droite ; un clic ouvre le panneau sur ce fil, et un clic sur un fil fait défiler la page jusqu'à son bloc. Le menu d'un bloc (poignée de BlockNote) gagne « Commenter ». Tout se fait au clavier ; Échap ferme le panneau.
- [ ] **Step 3 : e2e, vus rouges.** `comment_on_a_block_reaches_the_other_reader_at_once` ; `commenter_who_cannot_edit_comments` (niveau « Peut commenter » : l'éditeur est en lecture seule, le commentaire part) ; `reader_reads_comments_and_cannot_write_one` ; `resolve_hides_the_thread_and_reopen_brings_it_back` ; `comment_of_a_deleted_block_is_kept_apart` ; `comments_are_read_only_in_the_trash`.
- [ ] **Step 4 : relancer** Vitest, typecheck, les e2e. Commits : `web : panneau des commentaires`, `web : commenter un bloc depuis son menu`, `e2e : commentaires`. Puis README, conception §4.1 (le niveau « Peut commenter » sert), §4.2, registre.

### Task 10 : serveur, historique des versions

**Files:**
- Create: `crates/mementum-store/src/versions.rs`, `crates/mementum-server/src/routes/versions.rs`
- Modify: `crates/mementum-sync/src/actor.rs`, `hub.rs`, `lib.rs` (`SyncConfig.version_every: Duration`, dix minutes par défaut), `crates/mementum-server/src/types.rs`, `lib.rs`, `config.rs` (réglage `MEMENTUM_VERSION_EVERY_SECS`, option `serve --version-every-secs`, 600 par défaut, 1 au moins ; une valeur invalide arrête le démarrage en la nommant, comme les autres réglages)
- Test: `crates/mementum-sync/tests/versions.rs`, `crates/mementum-server/tests/versions.rs` (neufs)

**Interfaces:**
- Consumes: `doc_versions` (Task 3), `PageEvent::Versions` (Task 6).
- Produces:
  - `Store::put_version(&self, node_id: Uuid, seq: i64, state: &[u8], authors: &[Uuid], reason: VersionReason) -> Result<Uuid, StoreError>` (garde les 100 dernières de la page, dans la même transaction), `list_versions`, `get_version` ;
  - `Hub::restore_version(&self, node_id: Uuid, version: Uuid, actor: Option<Uuid>) -> Result<(), SyncError>` ;
  - `Version { id, createdAt, reason, authors: Vec<Author> }` (`ts-rs`).

| Route | Niveau | Réponse |
|---|---|---|
| `GET /api/v1/pages/{id}/versions` | `read` | `Version[]`, la plus récente d'abord |
| `GET /api/v1/pages/{id}/versions/{versionId}/state` | `read` | `application/octet-stream` : l'état Yjs (v1) |
| `POST /api/v1/pages/{id}/versions/{versionId}/restore` | `edit` | `204` ; `409 trashed` ; droits revérifiés une fois la page chargée, comme `PUT /mmd` (R89) |

- [ ] **Step 1 : tests de synchro, vus rouges** (l'horloge de tokio en pause, `version_every` à dix minutes) :
  - `burst_shorter_than_the_interval_is_one_version_at_stop` : des frappes pendant deux minutes, puis l'acteur s'arrête : une version, `closing`, dont l'état est celui de la fin et les auteurs ceux qui ont tapé ;
  - `long_session_cuts_a_version_before_the_write_that_follows_the_interval` : la version porte l'état d'avant cette écriture ;
  - `page_that_did_not_change_cuts_nothing` ;
  - `server_write_counts_like_a_client_write` (`PUT /mmd`) ;
  - `restore_cuts_a_version_of_what_it_replaces` : après la restauration, la page a le contenu de la version, avec les mêmes identifiants de blocs ; une version `before_restore` porte ce qui a été remplacé, et se restaure à son tour ;
  - `versions_are_capped_at_100`, les plus anciennes parties ;
  - `version_survives_compaction` (la compaction purge le journal, pas `doc_versions`).
- [ ] **Step 2 : tests du serveur, vus rouges** : `version_routes_follow_the_levels` ; `foreign_page_is_indistinguishable_from_missing` gagne ces routes ; `version_of_another_page_is_not_found` ; `restore_tells_the_page` (trame `versions`, et le contenu arrive aux connexions par la synchro).
- [ ] **Step 3 : écrire.** L'acteur tient l'instant de la première écriture non versionnée et les comptes qui ont écrit depuis. Après un rechargement, ces comptes se relisent au journal (`doc_updates.actor_id` au-delà du `seq` de la dernière version), pour ce qu'il en reste.
- [ ] **Step 4 : la part du serveur des budgets** (`cargo test --release -p mementum-sync -- --ignored`) : couper une version d'une page de 10 000 blocs est mesuré et consigné ; le chargement reste sous 300 ms.
- [ ] **Step 5 : commits** `store : versions d'une page`, `synchro : l'acteur coupe les versions d'une page (R135)`, `serveur : lire et restaurer une version`.

### Task 11 : client, historique

**Files:**
- Create: `apps/web/src/history/HistoryDialog.tsx`, `VersionPreview.tsx`, `useVersions.ts`, `apps/web/src/page/PageMenu.tsx`, et leurs tests
- Modify: `apps/web/src/api.ts`, `apps/web/src/page/PageView.tsx`, `apps/web/src/editor/Editor.tsx` (un éditeur lié à un document local, sans fournisseur), `apps/web/src/theme.css`
- Test: `apps/web/e2e/history.spec.ts` (neuf)

**Interfaces:**
- Consumes: les routes de la Task 10 ; `session.onPageEvent`.
- Produces: le menu « … » de la barre du haut (`PageMenu`), avec « Historique » ; la partie B y ajoutera ses entrées.

- [ ] **Step 1 : tests Vitest, vus rouges** : `versions_are_listed_most_recent_first_with_their_authors` ; `preview_never_writes` (l'aperçu est lié à un `Y.Doc` neuf, rempli par l'état de la version, sans fournisseur : aucune WebSocket n'est ouverte, l'éditeur n'est pas modifiable) ; `restore_is_absent_below_edit_level`.
- [ ] **Step 2 : l'écran.** « Historique » ouvre une fenêtre : à gauche la liste (« Version actuelle », puis chaque version : date, « il y a… », avatars des auteurs, « avant une restauration » le cas échéant) ; à droite l'aperçu de la version choisie, en lecture seule, dans le même éditeur que la page ; en bas « Restaurer cette version », qui demande confirmation (« Le contenu actuel sera remplacé. Il restera dans l'historique. »).
- [ ] **Step 3 : e2e, vus rouges** (le serveur des e2e reçoit `MEMENTUM_VERSION_EVERY_SECS=1`, dans `apiServer` de `e2e/servers.ts`) : `version_is_listed_and_previewed` ; `restore_brings_the_old_content_back_for_everyone` ; `restored_content_is_itself_in_the_history` ; `reader_sees_the_history_and_cannot_restore`.
- [ ] **Step 4 : relancer** Vitest, typecheck, les e2e, et les budgets du navigateur (`corepack pnpm e2e:budgets`) : la page s'ouvre aussi vite qu'avant. Commits : `web : menu de la page`, `web : historique et restauration d'une version`, `e2e : historique`. Puis README, conception §4.2, §4.5, registre.

### Task 12 : la limite R39 mise en test, des deux côtés

**Files:**
- Test: `crates/mementum-sync/tests/moves.rs` (neuf), `apps/web/e2e/block-move.spec.ts` (neuf)

Cette tâche n'écrit que des tests, ignorés, qui disent la perte telle qu'elle est aujourd'hui dans un vrai navigateur. La Task 13 les rend verts. Ils existent à part pour que la relecture juge la preuve avant la correction.

- [ ] **Step 1 : e2e `text_typed_while_someone_else_moves_the_block_survives`** : Mona et Tom ont la page ouverte ; Tom est coupé du réseau et tape « de Tom » dans le bloc X ; Mona déplace X sous Z (glisser par la poignée) ; Tom revient. Attendu : X est sous Z une seule fois, et porte « de Tom », chez Mona, chez Tom et dans le MMD du serveur. L'ordre inverse (Mona coupée déplace, Tom tape en ligne) est un second test.
- [ ] **Step 2 : les mêmes scénarios par l'acteur** (`moves.rs`), avec des mises à jour bâties par `move_block` et `replace_text`, dans les deux ordres d'arrivée, journal relu.
- [ ] **Step 3 : les voir échouer**, noter au registre ce que chacun montre (texte absent, où), puis les marquer `#[ignore = "perte connue R39, levée par la Task 13"]` et `test.fixme`. Commit `tests : l'édition faite pendant le déplacement d'un bloc, dans un navigateur et par l'acteur (R39)`.

### Task 13 : une édition faite pendant le déplacement d'un bloc est gardée (R39, R138)

**Files:**
- Create: `crates/mementum-crdt/src/moves.rs`, `crates/mementum-store/src/moves.rs`
- Modify: `crates/mementum-crdt/src/guard.rs`, `page.rs`, `lib.rs`, `crates/mementum-sync/src/actor.rs`
- Test: `crates/mementum-crdt/tests/move_edit.rs`, `crates/mementum-sync/tests/moves.rs`, `apps/web/e2e/block-move.spec.ts`

**Interfaces:**
- Consumes: `block_moves` (Task 3).
- Produces: `PageDoc::apply_client_update` reporte lui-même, et rend, avec la mise à jour lue, un correctif à écrire et à diffuser avec elle, comme celui du dédoublonnage : `pub struct Applied { pub read: Vec<u8>, pub carried: Option<Vec<u8>>, pub moves: Vec<MoveRecord> }`. `MoveRecord` se sérialise en octets (`to_bytes`, `from_bytes`) pour `block_moves.record` ; `PageDoc::remember_moves(&self, records: Vec<MoveRecord>)` les rend à une page rechargée.

**Ce qu'est un déplacement, pour le serveur.** Une mise à jour d'un client qui insère un bloc dont l'identifiant est celui d'un bloc vivant de la page, et dont le jeu de suppressions couvre ce bloc vivant. Les identifiants des blocs insérés se lisent dans la mise à jour, avant de l'appliquer (`Shape`, attribut `id` des éléments insérés).

**Ce qui est reporté.** Dans l'original, juste avant d'appliquer : les caractères vivants du contenu en ligne du bloc que le jeu de suppressions de la mise à jour ne couvre pas. L'auteur du déplacement a supprimé tout ce qu'il connaissait de l'original ; ce qui lui échappe est ce que d'autres ont inséré sans qu'il le voie. Ces caractères sont réinsérés dans la copie, avec leurs marques, après le caractère de la copie qui correspond à leur voisin de gauche dans l'original. Il en va de même des blocs enfants ajoutés sous l'original, recopiés sous la copie (`copy_tree`).

**Édition arrivée après le déplacement.** Chaque déplacement laisse un `MoveRecord` : pour chaque bloc déplacé, les plages d'éléments de l'original et celles de la copie, caractère pour caractère. Une mise à jour qui insère du texte dont le parent ou l'origine tombe dans les plages d'un original est reconnue avant d'être appliquée ; son texte est réinséré dans la copie, à la position que donne la correspondance, résolue dans l'état courant de la copie. Les enregistrements sont écrits dans `block_moves` avec la mise à jour, relus au chargement, et une page en garde 256 au plus, pendant 30 jours.

**Limites, à écrire dans la conception et le README :**
- une suppression faite par quelqu'un d'autre pendant le déplacement n'est pas reportée : le texte supprimé reparaît dans la copie (comme aujourd'hui) ; jamais un texte tapé n'est supprimé par ce report ;
- le contenu d'une cellule de tableau, et un bloc enfant ajouté après le déplacement, ne sont pas reportés ;
- un bloc déplacé deux fois avant l'arrivée de l'édition suit la chaîne des enregistrements, tant qu'ils sont gardés.

- [ ] **Step 1 : essai borné à une demi-journée, jetable, consigné au registre.** Trois choses sont à prouver sur yrs 0.28.1 avant d'écrire : (1) le jeu de suppressions d'une mise à jour se lit avant de l'appliquer ; (2) les éléments vivants d'un `XmlText` s'énumèrent avec leur identifiant, leur longueur et leurs marques ; (3) un identifiant d'élément se résout en position courante dans son texte. Si l'une des trois est fausse et n'a pas de contournement : Ruling, les tests de la Task 12 restent ignorés, la limite R39 reste dite, et la tâche s'arrête là. C'est la clause de sortie que le lot 1 s'était donnée.
- [ ] **Step 2 : tests de la crate, vus rouges.** `edit_during_concurrent_move_survives` perd son `#[ignore]`, dans ses deux ordres. S'y ajoutent : `carried_text_keeps_its_marks` ; `text_typed_at_the_start_the_middle_and_the_end_lands_in_place` ; `child_block_added_under_the_original_follows_it` ; `concurrent_delete_is_not_carried_and_removes_nothing_typed` ; `mover_own_offline_edits_are_not_doubled` (l'auteur du déplacement a tapé hors ligne dans le bloc, puis l'a déplacé, le tout en une mise à jour : son texte n'est présent qu'une fois) ; `late_edit_after_a_reload_is_carried` (enregistrements relus) ; `carry_is_applied_once` (la même mise à jour tardive reçue deux fois).
- [ ] **Step 3 : test de propriétés `moves_and_edits_converge_and_lose_no_insert`** : des insertions de texte par deux clients et des déplacements par un troisième, dans un ordre d'arrivée quelconque ; à la fin, chaque texte inséré est présent exactement une fois, et la page se lit en MMD.
- [ ] **Step 4 : écrire `moves.rs`**, puis l'acteur : le correctif est écrit avec la mise à jour et le correctif du dédoublonnage, tous ou aucun, puis diffusé à toutes les connexions, l'expéditeur compris ; les enregistrements sont écrits dans la même transaction.
- [ ] **Step 5 : les tests de la Task 12 perdent leur `ignore`** et passent, dans un vrai navigateur.
- [ ] **Step 6 : budgets.** `cargo test --release -p mementum-sync -- --ignored`, et la latence d'une frappe (`sync.spec.ts`) : une frappe ordinaire ne paie rien de mesurable ; le coût d'une mise à jour qui insère des blocs est mesuré sur une page de 10 000 blocs et consigné.
- [ ] **Step 7 : commits** `crdt : reconnaître le déplacement d'un bloc dans une mise à jour`, `crdt : reporter sur la copie le texte inséré dans l'original (R39, R138)`, `store : déplacements de blocs d'une page`, `synchro : le report d'un déplacement est écrit et diffusé avec lui`. Puis conception §11 (la ligne du risque), §9, README, `CLAUDE.md` (le piège « Déplacements concurrents »), registre.

### Task 14 : documentation et recette de la partie A

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `docs/REPRISE.md`, `docs/specs/2026-11-09-mementum-design.md`, l'en-tête de `crates/mementum-server/src/lib.rs`
- Create: `docs/recette/2026-11-12-p1-lot3a-recette.md`

- [ ] **Step 1 : relire chaque document contre le code final**, pas contre ce plan : le tableau des routes, les limites connues (ce qui est levé : R97 et R128 en repli, R109 en partie, R131, R54 pour la lecture, R39 dans ses limites ; ce qui reste), les pièges.
- [ ] **Step 2 : la checklist de recette**, sur ce qu'un test ne juge pas : la barre latérale à deux, les avatars, écrire un commentaire, le retrouver, résoudre, l'historique et son aperçu, restaurer, déplacer un bloc pendant que l'autre écrit ; et le dessin, direction A « Papier ».
- [ ] **Step 3 : commit** `docs : partie A du lot 3`.

## Fin de la partie A

1. Relecture finale de tout ce qui a changé depuis `6b6f104`, en trois passes (sécurité, intégration client et serveur, documentation contre le code), chaque constat prouvé par un test jetable ; vague de correction ; re-relectures ciblées.
2. Sur un clone neuf, étape par étape comme la CI : `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `cargo test --release -p mementum-sync -- --ignored`, `cargo deny check advisories`, `corepack pnpm install --frozen-lockfile`, typecheck, Vitest, tous les e2e, les budgets du navigateur, les licences npm.
3. Essai de migration sur une copie d'une base du lot 2.
4. Registre : mesures, reports, ce qui attend le propriétaire. `docs/REPRISE.md` à jour.
5. Le plan de la partie B.
