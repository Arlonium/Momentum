# Mementum — Phase 1, lot 4 « Recherche » : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **État de ce plan (2026-11-19)** : écrit d'après la conception et le code de `main` à `5e7514a`, **pas encore relu par le propriétaire**. Rien n'a été compilé ni lancé pour l'écrire : ce qui est dit du code a été lu (« Ce qui a été vérifié avant d'écrire »), ce qui est supposé est dit supposé. Il ne s'exécute pas avant : (1) la clôture du lot 3 (la migration `0004` figée, R243) ; (2) une re-relecture de ce tour de correction ; (3) les réponses de Lenny aux « Décisions à prendre par Lenny », ou son accord pour avancer sur ce que le plan suppose en attendant.

> **Relu le 2026-11-19 par un relecteur indépendant, avant exécution** (« corriger d'abord ») : 3 constats bloquants, 9 importants et des mineurs, tous vérifiés dans le code puis reportés dans ce texte (tour de correction 1). Ce qui a changé : la cohérence entre la base et l'index plein texte passe par une **intention** écrite d'abord (R909 récrit) ; les résultats sont **groupés par page dans l'index**, avant toute pagination (R902 récrit) ; la note d'un résultat ne lit **aucune statistique d'ensemble** (R916) ; la recherche est un `POST` (R917) ; les titres, les créations et les suppressions ont un **marqueur durable**, écrit dans leur transaction (R918) ; seuls les blocs changés passent à l'index plein texte (R919) ; la Task S4 est scindée en trois ; cinq choix pris en silence sont portés à Lenny (L15 à L19).

> **Re-relu le même jour** (« prêt », un constat important et six mineurs, reportés ici) : les débuts de mot indexés sont bornés (R920) ; L12, retirée par erreur au tour 1, est rétablie ; l'ordre des refus de la recherche, la reconstruction par tranches, la page illisible retirée aussi de l'index plein texte, et la clôture refusée sans arrêt du processus sont écrits.

> **État au 2026-11-20 : la Task S0 est faite, le plan s'exécute.** Le moteur est mesuré ([compte rendu](../logs/2026-11-19-p1-lot4-s0-mesures.md), [relecture](../logs/2026-11-19-p1-lot4-s0-relecture.md)) : Tantivy tient. Lenny a dit le 2026-11-20 de pousser au fil du travail (L12 : oui, pour tous les lots, R934), a changé la méthode (R935 : « Méthode », dans les contraintes), et, pour les autres décisions, « tu es autonome » : elles sont retenues par défaut, tableau ci-dessous. Les décisions sorties de S0 et de sa relecture sont au [registre](../logs/2026-11-19-p1-lot4-ledger.md), R921 à R935, et portées dans ce texte. Des trois conditions du premier encadré, il reste la première pour la seule Task S1 (la migration `0004` figée) : S2, S4a et S3 n'y touchent pas et peuvent partir.

## Les dix-neuf décisions, telles que retenues (2026-11-20)

Une seule est une réponse de Lenny (L12). L1 est retenue sur les mesures de S0 et leur relecture. Les dix-sept autres sont **retenues par défaut** : la recommandation du plan, adaptée aux chiffres là où la relecture l'affaiblit (L6, L10, L18, L19). Lenny peut revenir sur chacune ; la question et les raisons sont plus bas, « Décisions L1 à L19 ».

| | Question | Retenu | Statut |
|---|---|---|---|
| L1 | Le moteur | **Tantivy 0.27.0** : 1,5 à 7 fois plus rapide qu'un FTS5 bien écrit ; avec FTS5, R916 demanderait du C | sur mesures et relecture (R933) |
| L2 | Où l'on cherche | dans l'espace affiché | par défaut |
| L3 | Ce qui est cherché | titres, texte des blocs, légendes et noms de fichiers, adresse d'un signet, LaTeX ; pas les commentaires, les fichiers, l'historique, les dossiers | par défaut |
| L4 | La corbeille | jamais dans les résultats ; elle reste indexée | par défaut |
| L5 | Raccourci et forme | une fenêtre de recherche ; « Rechercher » dans la barre et Ctrl+K, laissé à l'éditeur quand du texte y est sélectionné | par défaut |
| L6 | Tolérance | sans casse ni accents, tous les mots, le dernier complété ; **pas de racinisation**, pas de tolérance aux fautes | par défaut ; S0 n'en donne qu'une indication (petit échantillon) |
| L7 | Filtres et tri | aucun filtre à l'écran, tri par pertinence ; les filtres au lot 5 | par défaut |
| L8 | Rétroliens | « Pages qui mènent ici » sous le texte ; pages vues, du même espace | par défaut |
| L9 | Délai d'indexation | 2 s après la dernière frappe, 15 s au plus ; un texte effacé reste trouvable ce temps-là | par défaut |
| L10 | Budgets de la recherche | les trois restent des **mesures** pendant le lot ; adoptés au §5.10 après X2 (sous charge), pas après S0 | par défaut, adapté |
| L11 | `[[Page#Titre]]` | pas au lot 4 | par défaut |
| L12 | Pousser | **oui**, au fil du travail, pour tous les lots | **réponse de Lenny** (R934) |
| L13 | Instance existante | indexée seule, en fond, au premier démarrage | par défaut |
| L14 | Titre mentionné trouvable | accepté, avec R251 | par défaut |
| L15 | Nombre de résultats | cent pages au plus par recherche (20 par réponse) | par défaut |
| L16 | Forme d'une requête | 200 caractères, 8 mots, aucun opérateur | par défaut |
| L17 | Bloc `unknown` | pas cherché | par défaut |
| L18 | Texte gardé trois fois | accepté aux tailles mesurées (l'index 2,5 fois le texte, `blocks_index` 1,9 fois) ; le repli est mesuré en S3, le choix rouvert en S4c | par défaut, adapté (R931) |
| L19 | Temps de réponse comme indice | un index pour l'instance, et **un temps de réponse plancher de 50 ms** par recherche | par défaut, adapté (R932) |

Deux autres choix pris sans Lenny, hors des dix-neuf, pèsent : ce que « mémoire au repos » compte (R924 : la mémoire que le processus tient en propre, les fichiers de l'index projetés en mémoire dits à part), et le tampon de l'écrivain (R925 : 50 Mo).

**Goal :** dans un espace, on tape quelques mots et on retrouve les pages qu'on a le droit de lire, par leur titre ou par leur texte, avec un extrait, et on arrive sur le bloc trouvé ; sous une page, on lit quelles pages mènent à elle. Personne ne trouve, ne devine ni ne compte une page qu'il ne voit pas.

**Le lot s'exécute en 16 tâches** : piste serveur S0, S1, S2, S3, S4a, S4b, S4c, S5, S6, S7 (10), piste client C1 à C3 (3), transverses X1, X2 et D (3).

**Architecture (en bref ; le détail est plus bas) :**
- `mementum-store` : une migration `0005_search.sql` crée `blocks_index` (la projection du document, conception §5.3), `page_links` (les rétroliens), `search_state` et `search_outbox` (où en est l'indexation) ; une voie neuve de la file d'écriture, `Lane::Index`.
- `mementum-search` (crate neuve, conception §8) : la projection d'une page en lignes d'index (pure), la lecture bornée d'une requête, et l'index plein texte Tantivy (conception §5.2, §5.7), derrière une interface étroite.
- `mementum-sync` : le hub dit quelles pages ont été écrites (un ensemble, sans attente) ; une lecture seule d'une page gardée, hors de son acteur.
- `mementum-server` : l'indexeur (une tâche de fond, jamais sur le chemin d'une frappe), deux routes (`POST /api/v1/spaces/{id}/search`, `GET /api/v1/pages/{id}/backlinks`), un débit par compte.
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
| Outil `search` des agents | §6.5, outil 1 : « Recherche hybride. Filtres : espace, type, date, auteur » ; §9 : MCP v1 en P1 | la route de l'API que l'outil appellera | l'outil MCP, les jetons, les filtres : lot 5 (écart E1, décision L7) |

**Hors de ce plan :** la recherche dans les commentaires, dans le contenu des fichiers (PDF), dans l'historique et dans la corbeille (décisions L3, L4) ; la recherche à l'intérieur d'une base (§4.2, P2) ; les `#tags` (§4.6, lot 6 et P2) ; la tolérance aux fautes de frappe ; le surlignage des mots dans la page ouverte ; les plafonds généraux par compte et par adresse IP (lot 7 : ce plan ne pose que le débit de ses propres routes) ; un quota de disque pour l'index (lot 7, comme R247).

---

## Ce qui a été vérifié avant d'écrire

**Lu dans le code de `5e7514a`** (chemin et nom réels) :

- Migrations : `crates/mementum-store/migrations/0001` à `0004`. `nodes` a `kind IN ('folder','page')`, `title`, `updated_at`, `trashed_at` ; `doc_updates(node_id, seq, update_blob, created_at, actor_id)`, `doc_snapshots(node_id, seq, state_blob, created_at)`. Aucune table d'index.
- `nodes.updated_at` n'est écrit qu'à la création et par `update_node` (titre, place : `nodes.rs`, l'`UPDATE` de la ligne 750) : **il ne bouge pas quand le contenu change**. Il ne peut donc servir ni de date de modification, ni de signe qu'une page est à réindexer.
- File d'écriture : `crates/mementum-store/src/queue.rs`, `enum Lane { Page(Uuid), Account(Uuid), Files, Copies, Common }` ; `Store::write_in(Lane)` et `write_conn_in(Lane)` (définies dans `spaces.rs`, `pub(crate)`). Le journal d'une page s'écrit par `Store::append_updates` (voie de la page), l'instantané par `put_snapshot`, qui retire le journal qu'il couvre.
- Le contenu d'une page ne change que par : l'acteur (`crates/mementum-sync/src/actor.rs` : `client_update`, `write`, `server_write`, qui passent par `Actor::persist` et `Actor::persist_restore`) ; la création (`Store::create_node`, `nodes.rs`, écrit l'instantané de `seq` 0 du document donné dans la transaction qui crée le nœud : c'est le chemin de `POST /api/v1/pages` ; `load`, dans l'acteur, n'écrit celui d'une page restée sans document qu'en filet, R28 ; `Hub::create_page_doc` n'est appelé par aucune route, seulement par des tests) ; la duplication (`Store::duplicate_pages`, `crates/mementum-store/src/duplicate.rs`, sans acteur) ; la suppression (`Store::delete_node`, qui retire à la main les lignes des tables liées).
- La compaction se fait hors de l'acteur (`compact`, `actor.rs`), sur un fil bloquant, puis dans la voie de la page (R826).
- `load` et `replay` (`actor.rs`) relisent une page gardée en écartant des entrées du journal, avec une ligne `error` par entrée écartée ; ils sont privés au module, et `load` **écrit** (l'instantané d'une page sans document).
- `Hub` (`crates/mementum-sync/src/hub.rs`) : `read_blocks` (par l'acteur), `load_page`, `forget`, `shutdown`, `live_pages`. Rien n'y dit aujourd'hui qu'une page vient d'être écrite.
- Droits : `AppState::space_access` et `find_space_access` (`crates/mementum-server/src/access.rs`) rendent un `Arc<SpaceAccess>` ; `SpaceAccess::visible()` (`crates/mementum-acl/src/access.rs`) donne chaque nœud vu avec `NodeAccess { level, section, in_trash, visible_parent }`. `Store::node_titles` existe.
- Annonces : `Store::changes()` (`spaces.rs`) annonce l'espace dont `lsn` a avancé (titre, place, corbeille, création, suppression), pour les écritures de ce `Store` seulement.
- Débits existants, modèles de celui de ce lot : `crates/mementum-server/src/mmd_reads.rs` (quatre calculs de front sur le fil bloquant, les comptes tour à tour, le tour rendu à la fin du calcul) et `duplications.rs` (une à la fois par compte, budget par fenêtre, `429` avec `Retry-After`).
- `serve` (`crates/mementum-server/src/lib.rs`) finit par `hub.shutdown()` puis `store.close()` ; `to_completion` garde la portée `writes_by`.
- La CLI (`crates/mementum-cli/src/main.rs`) a `serve` et `user set-password`, pas de `reindex`.
- Le modèle lu d'une page : `mementum_md::Block { id, kind, props, content, children }`, `Inline::{Text, Link { href }, Mention { kind, id, label }, Math { latex }, HardBreak }` (`crates/mementum-md/src/model.rs`). Le registre (`spec/schema/blocks.json`) : 24 types de blocs ; portent du texte en ligne `paragraph`, `heading`, les quatre listes, `quote`, `codeBlock`, `callout` ; `table` ; portent du texte dans des props `image`, `video`, `audio`, `file` (`caption`, `name`), `bookmark` (`url`), `equation` (`latex`), `pageLink` (`pageId`, `title`).
- Client : `apps/web/src/space/pageDirectory.ts` (`findPages`, les titres de l'arbre publié), `editor/blocks/PagePicker.tsx`, `sidebar/Sidebar.tsx` (un `nav` : `SpaceSwitcher`, `ShortList`, `SectionList`), `editor/blocks/Toc.tsx` (`goToHeading` : mener à un bloc, éditeur en lecture seule compris). Aucun raccourci Ctrl+K dans `apps/web/src`.
- `apps/web/src/router.ts` **jette tout fragment** : `navigate` ne fait rien quand `location.pathname + location.search` est déjà le chemin demandé, `useLinkInterception` navigue vers `url.pathname + url.search`, et `usePathname` ne suit que `location.pathname`. Un lien `/p/<id>#b=<bloc>` suivi dans l'application perdrait donc son bloc, et un lien vers un autre bloc de la page ouverte ne ferait rien (C2).
- `apps/web/src/api.ts`, `silence(method, path)` : tout appel qui n'est pas un `GET` (hors connexion et déconnexion) resté sans réponse est dit « a peut-être été fait » (R757). Un `POST` qui ne fait que lire doit y être nommé comme une lecture (C1).
- Une route d'API lit la session par l'extracteur `AuthUser` (`routes/favorites.rs`, par exemple) ; `caller` (`routes/sync.rs`) ne sert qu'aux WebSockets.
- `StoppedClock` est privé à `crates/mementum-sync/tests/versions.rs`, et la fonction `test-util` de tokio n'est dev-dépendance que de `mementum-sync` (son `Cargo.toml`).
- Le test des migrations du store est `crates/mementum-store/tests/migration.rs`.
- SQLite : `libsqlite3-sys` 0.37.0, tel que compilé par sqlx, pose `-DSQLITE_ENABLE_FTS5` (son `build.rs`, lu dans le registre de cargo du poste) : FTS5 est disponible sans dépendance de plus (utile à la décision L1).
- `spike/RESULTS.md` **ne dit rien de la recherche** : la phase 0 n'a mesuré ni Tantivy ni FTS5 (écart E4).
- Le dernier Ruling du lot 3 est R889.

**Supposé, sans l'avoir lu ni essayé** (chaque point est repris par la tâche qui le vérifie) :

- Tantivy : sa version courante, sa licence (MIT) et celles de ses dépendances contre `deny.toml`, sa mémoire au repos, son temps d'ouverture, le coût d'un filtre sur des dizaines de milliers d'identifiants, la charge utile d'une validation (`payload`), le verrou de son écrivain, ses tokeniseurs (pliage des accents, racinisation française). Tout cela était l'objet de **S0**, faite : Tantivy 0.27.0, licences vertes, chiffres au compte rendu. Le verrou de l'écrivain n'a été vu que rendu après un processus tué ; le refus d'un second écrivain est un test de S3.
- BlockNote 0.57 prend Ctrl+K pour créer un lien. D'après la relecture du plan : par un écouteur de `@blocknote/react` posé sur l'élément de l'éditeur modifiable, qui appelle `preventDefault` sans arrêter la propagation. Je ne l'ai pas retrouvé par mes recherches dans son `dist` (code réduit) : C1 le prouve dans Chromium, par `defaultPrevented` et par la sélection de l'éditeur, sans supposer la forme de l'écouteur.
- La sorte d'une mention de page s'écrit `page` dans `Inline::Mention::kind` : à lire dans `mementum-schema` en S2.

---

## Global Constraints

Celles des plans du lot 3 restent valables telles quelles (langue, commits sans aucune ligne d'attribution d'outil, ne jamais arrêter un processus qu'on n'a pas lancé, `data/instance` jamais ouvert par un binaire d'un lot en cours, aucun budget relâché, aucun test existant affaibli, chaque test vu rouge d'abord pour la bonne raison, pas d'attente fixe, un témoin vert pour chaque test qui prouve une absence). Ce qui suit vaut pour toute tâche de ce plan.

**Droits**
- Les droits passent par `access.rs`, et nulle part ailleurs. L'index ne porte **aucun** droit, aucun niveau, aucune liste de lecteurs, et pas l'état de corbeille : ce qui est permis est calculé à chaque requête, pour le compte qui demande, sur `SpaceAccess`.
- **Rien d'une page que `access.rs` ne montre pas ne sort** : ni résultat, ni titre, ni extrait, ni identifiant, ni fil d'Ariane, ni nombre de résultats, ni suggestion, **ni l'ordre des résultats** (R916 : la note ne lit aucune statistique de l'index). La corbeille ne sort jamais. Reste le temps de réponse, qui dépend du nombre d'entrées de l'instance qui portent un terme : S0 l'a mesuré (16 ns par entrée non permise), un temps de réponse plancher le couvre tant que le calcul tient dessous (R932), et au-delà c'est une limite écrite (L-b), pas une promesse.
- Ordre des refus : `401`, `409 account_changed`, `404` d'un espace où l'on n'a aucun rôle (la réponse d'un espace inconnu) ou d'une page qu'on ne voit pas (**exactement** la réponse d'une page inconnue), puis `422`, puis `429`. Les routes neuves entrent dans `space_routes_by_role` (`tests/spaces.rs`) et `foreign_page_is_indistinguishable_from_missing` (`tests/permissions.rs`, par `page_routes`), et dans la matrice de `tests/expected_account.rs`.
- Un message de refus tutoie. La requête d'une personne n'est **jamais journalisée**, à aucun niveau, ni recopiée entière dans un message d'erreur (`mementum_md::excerpt`, R434), et **ne voyage jamais dans une adresse** (le journal d'accès d'un mandataire la garderait) : elle est dans le corps d'un `POST` (R917).

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
- **Méthode (décision de Lenny du 2026-11-20, R935)** : une tâche n'écrit et ne lance que **les tests de son ajout et les suites qu'elle touche directement** ; elle a **une seule relecture**, sans re-relecture. Entre les tâches : ni `cargo test --workspace`, ni Vitest entier, ni la suite de bout en bout entière, ni les quatorze budgets du navigateur, ni essai au hasard large, ni campagne de mutations. Les listes « Preuves (mutations) » des tâches ne sont pas jouées par la tâche : elles disent ce que ses tests doivent distinguer, et sont jouées une fois, à la fin du lot. Les mesures qu'une tâche nomme (`#[ignore]`, release) sont de son ajout : elle les lance. La CI de chaque poussée est lue ; un rouge de l'ajout se corrige, un rouge hors de lui se note au registre (« À reprendre à la fin du lot ») sans arrêter la tâche. « Fin du lot », plus bas, porte la passe complète unique, la liste des défauts, la correction groupée et la passe de contrôle.
- **Dépendances** : Tantivy et ce qu'il tire, rien d'autre ; `cargo deny check licenses bans` et `node tools/check-npm-licenses.mjs` restent verts. Aucune dépendance npm neuve.
- **Migration** : une pour le lot, `0005_search.sql` ; elle peut changer jusqu'à la clôture du lot (R139, R243) : aucun binaire du lot 4 n'ouvre une instance que quelqu'un regarde avant la clôture. Après l'avoir ajoutée, toucher `crates/mementum-store/src/lib.rs` (R269).
- **Ports des e2e** : piste C, `MEMENTUM_E2E_WEB_PORT=5177` et `MEMENTUM_E2E_API_PORT=8084` ; piste S et transverses, `5178` et `8085`. Jamais 5173, 5175, 8080, 8090.
- **Pousser** : `docs/REPRISE.md` (« Règles à ne pas enfreindre ») dit « Ne rien pousser sans une demande explicite d'Alex ou de Lenny. Un accord donné pour un lot ne vaut pas pour le suivant ». L'accord du 2026-11-12 valait pour le lot 3. **Lenny a répondu le 2026-11-20 (L12, R934) : les commits se poussent sur `main` au fil du travail, pour tous les lots**, et `docs/REPRISE.md` le dit désormais. Jamais de poussée forcée, jamais de secrets, jamais la branche d'un atelier.
- **Registre** : `docs/superpowers/logs/2026-11-19-p1-lot4-ledger.md`, forme `Ruling RNN: <décision> — <pourquoi> — <coût si faux>`. **Le lot 4 numérote à partir de R900** : R890 à R899 restent à la clôture du lot 3. Si elle en prend davantage, le lot 4 décale ses numéros d'autant, et son registre le dit en tête avec la table de correspondance de ce plan. Les décisions de ce plan sont R900 à R920, écrites ici ; le registre commence à R921 (R921 à R933 : la Task S0 et sa relecture ; R934 et R935 : les décisions de Lenny du 2026-11-20) ; l'exécution suit à R936.
- **Mémoire et disque du poste** : une compilation à la fois, `CARGO_INCREMENTAL=0`, `TMP` et `TEMP` sur un dossier de la tâche, le `target/` d'un atelier supprimé dès qu'il a rendu. Tantivy allonge la compilation en release : 40 s de plus sur 24 cœurs, +5,5 Mo de binaire, 65 crates neuves (S0 ; non mesuré sur les 4 cœurs de la CI).

## Review Focus

1. **Une page qu'on ne voit pas ne se trouve, ne se devine ni ne se compte.** Un invité, un membre devant une page privée d'un autre, un compte d'un autre espace : aucun résultat, aucun titre dans un fil d'Ariane, aucun rétrolien, la même réponse (corps et forme) qu'on cherche un mot qui n'existe nulle part ou un mot que seule une page non vue porte, et **le même ordre** des résultats vus quoi que portent les pages non vues, les autres espaces et la corbeille (un compte qui écrit deux blocs dosés ne doit rien lire dans leur ordre). Tests : S3, `what_unseen_pages_hold_changes_no_order` ; S5, `a_word_only_an_unseen_page_holds_answers_like_a_word_nobody_wrote` ; S6, `a_backlink_from_an_unseen_page_is_not_listed`, `unseen_sources_never_hide_seen_ones` ; X1, e2e à trois comptes.
2. **Un droit retiré vaut à la requête suivante**, sans attendre l'indexeur : accès retiré, page fermée aux membres, page mise à la corbeille, compte sorti de l'espace. Test : S5, `withdrawn_access_hides_the_page_at_the_next_query` (l'indexeur arrêté pendant le test).
3. **L'indexeur ne retarde aucune frappe**, ni par l'acteur, ni par la file d'écriture, ni par le processeur. Tests : S4a, `the_actor_only_names_the_page_it_wrote` (compteur, sans minutage) ; X2, la mesure de la frappe pendant l'indexation d'un espace entier, et pendant qu'une grande page est récrite en boucle.
4. **Une entrée hostile** : une requête faite pour coûter (longue, mille termes, opérateurs, préfixe d'une lettre), une page faite pour coûter à l'index ou pour évincer les autres des résultats (un bloc de plusieurs Mio, cent mille liens, dix mille blocs sous un titre courant, une écriture ou un renommage en boucle), une boucle de recherches d'un compte, des centaines de liens posés vers la page d'un autre. Tests : S2, linéarité ; S3, `a_title_word_of_a_10000_block_page_does_not_hide_other_pages` ; S5, `a_burst_of_searches_of_one_account_does_not_delay_another` ; S6, `unseen_sources_never_hide_seen_ones`.
5. **Un index qui n'est pas celui de la base** (base restaurée après une seule validation comme après cent, dossier recopié d'ailleurs, arrêt brutal entre deux validations) : aucun extrait d'un texte que la base n'a pas. Tests : S3, S4c (`a_copy_then_one_commit_then_the_copy_put_back_rebuilds`), S7.
6. **Ce qui a été effacé ne reste pas trouvable** au-delà du délai d'indexation, et ce que seule une version de l'historique garde n'est jamais trouvable (l'historique demande « Peut modifier », R203 ; la recherche, « Peut lire »). Test : S4b, `erased_text_leaves_the_index`.

---

## Décisions de ce plan

Prises pour écrire le plan ; celles qui sont des choix de produit sont reprises dans « Décisions à prendre par Lenny ».

- **R900** : le moteur est **Tantivy**, comme le dit la conception (§5.2, §5.7, §12 « Mes choix » n° 7), derrière une interface étroite de `mementum-search` (`SearchIndex`) ; S0 l'a mesuré d'abord : il tient (version 0.27.0 ; décision L1, R933).
- **R901** : `blocks_index` vit dans SQLite, clé `(node_id, block_id)` ; la colonne `path` de la conception n'est pas créée (écart E3). Tantivy est nourri **depuis `blocks_index`**, jamais depuis les documents : la base reste la source, l'index plein texte une copie qu'on refait.
- **R902** (récrit au tour de correction 1) : l'unité de l'index plein texte est le **bloc**, et chaque bloc y porte aussi le titre de sa page, dans un champ à part, mieux noté ; une page a en plus une entrée pour son titre seul. Une requête de plusieurs mots les veut tous, dans le titre ou dans un même bloc. **Les entrées sont groupées par page dans l'index, avant toute pagination** : un collecteur garde la meilleure entrée de chaque `node_id`, et `skip` et `limit` comptent des pages, jamais des entrées. Sans cela, un mot du titre d'une page de 10 000 blocs rendait 10 001 entrées de cette page, et les cent résultats qu'une personne peut lire n'étaient qu'elle : une page faite pour cela évinçait les autres pour tout l'espace. S0 a mesuré ce collecteur : 1,1 million d'entrées de 10 000 pages en 29,6 ms au p95 et 1,4 Mo ; le repli (ne plus recopier le titre sur chaque bloc) n'est pas nécessaire. Le collecteur lit la page de chaque entrée dans des colonnes rapides, où il pose aussi le filtre des pages permises (R921, R922). Ce qu'on perd dans les deux cas : deux mots écrits dans deux blocs différents d'une page ne la trouvent pas (la P4 le lève par la recherche hybride). Ce que coûte le titre recopié : un renommage récrit toutes les entrées de la page dans l'index plein texte (borné par R906 : une fois toutes les 15 s au plus par page ; mesuré en S4c).
- **R903** : le fil d'Ariane d'un résultat est **calculé à la requête**, d'après ce que le compte voit (`visible_parent`), jamais lu dans l'index, et les titres des pages parentes ne sont pas un texte cherché (écart E2).
- **R904** : le titre d'un résultat est lu dans `nodes` à la requête, jamais dans l'index.
- **R905** : la réponse ne porte **aucun total**, seulement « il y en a d'autres ».
- **R906** : une page est réindexée 2 s après sa dernière écriture, 15 s au plus après la première écriture non indexée (`Config::index_after`, `index_at_most`, réglables pour les tests ; décision L9).
- **R907** : l'indexeur lit ce que le store garde, par une lecture seule neuve de `mementum-sync`, pas par l'acteur de la page.
- **R908** : les écritures de l'index dans SQLite ont leur voie, `Lane::Index`, sans priorité, comme `Lane::Copies` (R607) : une frappe n'attend jamais plus d'une transaction de l'index.
- **R909** (récrit au tour de correction 1) : la cohérence entre la base et Tantivy tient à un **jeton par validation**, écrit d'abord dans la base comme une intention (« L'intention, la validation, la clôture », plus bas). Un compteur ne suffisait pas : « l'index a une validation d'avance » se lisait pareil après un arrêt brutal et après une base restaurée une validation plus tôt, dont l'outbox ne nomme pas les pages de la validation perdue.
- **R910** : débit de la recherche, par compte, en mémoire : un calcul en cours et une requête en attente au plus ; une requête neuve prend la place de celle qui attendait, qui reçoit `429 search_busy` ; quatre calculs de front pour l'instance, les comptes tour à tour.
- **R911** : bornes d'une requête : 200 caractères, 8 termes, pas de syntaxe (ni opérateur, ni joker, ni expression rationnelle : la requête est bâtie par programme, jamais donnée à un analyseur de requêtes) ; préfixe sur le dernier terme seulement, à partir de 2 caractères ; 20 pages par réponse, 5 réponses à la suite au plus, donc cent pages trouvées par requête (décisions L15, L16).
- **R912** : bornes de la projection (valeurs de départ, à mesurer en S2) : 64 Kio de texte par bloc et 8 Mio par page, coupés entre deux caractères, la page notée `truncated` ; 10 000 liens par page.
- **R913** : `mementum reindex` tourne **serveur arrêté** ; serveur lancé, il refuse en le disant (écart E7).
- **R914** : la recherche porte sur un espace, celui qu'on affiche (décision L2).
- **R915** : une instance existante est indexée d'elle-même, en fond, après son premier démarrage sous ce lot ; la réponse d'une recherche dit tant que l'espace n'est pas à jour (décision L13).
- **R916** (tour de correction 1) : **la note d'un résultat ne lit aucune statistique de l'index** (ni nombre d'entrées, ni fréquence d'un terme dans l'ensemble, ni longueur moyenne). L'index est un pour l'instance, filtré par l'ensemble des pages permises (R921) : une note à la BM25 y lirait les autres espaces, la corbeille, les pages non vues et les entrées supprimées pas encore fusionnées, et un compte qui écrit deux blocs dosés lirait dans leur ordre la fréquence d'un mot chez les autres, puis compterait leurs blocs par dichotomie. La note ne dépend que de l'entrée elle-même (le champ touché, le nombre de termes trouvés, leur fréquence dans l'entrée, sa longueur) et de la requête, ou de statistiques constantes données par programme. De même, **le préfixe ne parcourt pas le dictionnaire de l'index** (son coût et ses termes dépendraient du vocabulaire des autres) : les débuts de mot sont indexés dans un champ à eux, et un préfixe est un terme exact. C'est une exigence de S0 et de S3. S0 l'a tenue par des statistiques constantes données au moteur (R923) ; S3 doit encore prouver le départage de deux notes égales, que l'essai de S0 ne voyait pas (R922).
- **R920** (re-relecture) : **les débuts de mot indexés sont bornés à `MAX_START_CHARS` = 10 caractères** : un mot donne au plus 9 termes dans le champ des débuts (ses débuts de 2 à 10 caractères), jamais un par caractère. Sans borne, un mot de 40 octets en donnait 40, et un bloc de 64 Kio de longs mots distincts environ 60 000. Un préfixe tapé de 10 caractères au plus est un terme exact de ce champ. **Un préfixe plus long** cherche le terme exact de ses 10 premiers caractères, puis chaque entrée candidate est vérifiée sur son texte gardé (un mot du champ touché commence-t-il par tout le préfixe ?), dans le collecteur, avant le groupement par page ; au plus `MAX_VERIFIED_ENTRIES` (2 000, valeur de départ) entrées sont vérifiées par requête et `MAX_VERIFIED_PER_PAGE` (20) par page, **dans l'ordre (note décroissante, identifiant de page, clé de départage), jamais dans l'ordre de l'index**, qui dépend des fusions (R929) ; celles d'après sont écartées sans être lues (limite L-o). Le titre est gardé sur chaque entrée pour cette vérification (R929). Le nombre vérifié dépend des entrées permises seulement : la vérification vient après le filtre.
- **R917** (tour de correction 1) : la recherche est un `POST` dont le corps porte la requête (`{ "q": …, "after": … }`), jamais un `GET` avec la requête dans l'adresse.
- **R918** (tour de correction 1) : ce qui change l'index sans passer par un acteur laisse un **marqueur durable dans sa propre transaction** : un titre changé, une page supprimée (une ligne de `search_outbox`) ; une page créée ou dupliquée (une ligne de `search_state` à `indexed_seq = -1`). `Store::changes()` ne sert plus qu'à réveiller l'indexeur, qui lit ces marqueurs : rien ne tient à sa mémoire, et aucune annonce ne fait comparer toutes les pages d'un espace.
- **R919** (tour de correction 1) : seuls les blocs dont l'empreinte a changé passent à l'index plein texte ; `search_outbox` nomme des blocs, et la page entière seulement pour un titre, une suppression ou une reconstruction.

**Sorties de S0 et de sa relecture** (2026-11-20 ; le texte entier, avec le pourquoi et le coût si faux, est au [registre](../logs/2026-11-19-p1-lot4-ledger.md)) :

- **R921** : le filtre des pages permises est posé **dans le collecteur**, par l'identifiant de la page lu en colonne rapide, avant le groupement ; jamais par un ensemble de termes dans la requête (65 à 580 ms) ; l'index n'a pas de champ `space_id`.
- **R922** : deux notes égales se départagent par une valeur tirée de l'entrée seule : l'identifiant de la page entre deux pages ; dans une page, l'entrée de titre d'abord, puis les seize premiers octets du SHA-256 de la clé du bloc, en colonnes rapides. Jamais l'adresse d'une entrée dans l'index, ni le rang du bloc.
- **R923** : la note est la BM25 de Tantivy sous des statistiques constantes ; ni `Searcher::search`, ni `SnippetGenerator` dans le chemin de la recherche (test de sources) ; l'extrait est bâti par programme.
- **R924** : ce que « mémoire au repos » juge : la mesure existante sur un index de 10 000 pages à jour, et, après usage, la mémoire tenue en propre (`RssAnon` sous Linux, mémoire privée sous Windows) ; les fichiers de l'index projetés en mémoire sont dits à part.
- **R925** : un tampon d'écrivain de 50 Mo, dans le serveur comme dans `mementum reindex` ; repli à 15 Mo si X2 le demande.
- **R926** : NFC avant d'indexer et avant de chercher ; `RemoveLongFilter::limit(41)` ; le chinois, le japonais et le coréen ne sont pas découpés (limite L-p).
- **R927** : l'ensemble permis est bâti à chaque requête, sans cache à lui ; S5 mesure sa part.
- **R928** : le pire cas du champ des débuts de mot est une suite de mots courts tous différents (4,3 fois les octets reçus) ; rien ne borne la pointe de mémoire des fusions : risque ouvert RO-1.
- **R929** : le titre est gardé sur chaque entrée pour vérifier un préfixe long ; l'ordre et les deux plafonds de cette vérification.
- **R930** : le dernier terme est cherché comme mot entier ou comme début de mot ; un préfixe seul a une note constante (limite L-q).
- **R931** : L18, les trois copies gardées aux tailles mesurées. **R932** : L19, un temps de réponse plancher. **R933** : L1 et les décisions retenues par défaut. **R934** : pousser. **R935** : la méthode.

---

## Architecture

### Quoi indexer

Une ligne de `blocks_index` par bloc qui porte du texte, pour chaque page (`kind = 'page'`) hors des pages supprimées. La corbeille **est** indexée (une page restaurée se retrouve sans attendre), et n'est jamais rendue : son texte reste donc dans l'index tant que la page n'est pas supprimée pour de bon (décision L4).

| Bloc | Texte indexé |
|---|---|
| `paragraph`, `heading`, les quatre listes, `quote`, `callout`, `codeBlock` | le texte de son contenu en ligne, marques retirées ; le libellé d'une mention ; le texte d'un lien (pas son adresse) ; le LaTeX d'une formule en ligne |
| `table` | le texte de ses cellules, une espace entre deux cellules |
| `image`, `video`, `audio`, `file` | `caption` et `name` |
| `bookmark` | `url` |
| `equation` | `latex` |
| `pageLink` | `title` (le titre copié, R251) |
| `divider`, `toc`, `columnList`, `column`, `databaseRef`, `syncedBlock` | rien (leurs enfants sont parcourus) |
| `unknown` (un bloc d'un autre schéma, ou étranger, gardé en JSON dans `data`) | rien : son texte n'est pas trouvable (décision L17) ; ses enfants sont parcourus |

Les enfants d'un bloc ont chacun leur ligne. Un bloc sans `id`, ou dont l'`id` paraît deux fois dans la page lue, est indexé sous une clé de remplacement (`#<rang>`), que la réponse ne donne pas comme bloc à atteindre. Le titre de la page n'est pas dans `blocks_index` : il est dans `nodes`.

`page_links(node_id, block_id, target_id)` : une ligne par bloc et par page visée, pour un bloc `pageLink` (`pageId`), une mention de page, et un lien dont l'adresse est le chemin exact d'une page de l'instance (`/p/<uuid>`, avec ou sans l'origine publique du serveur, avec ou sans fragment de bloc). `target_id` est un UUID lu, sans clé étrangère : la page visée peut être d'un autre espace, ou ne plus exister.

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
CREATE INDEX page_links_by_target ON page_links (target_id, node_id, block_id);

CREATE TABLE search_state (
    node_id     TEXT PRIMARY KEY NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    indexed_seq INTEGER NOT NULL,    -- le `seq` de la page que blocks_index reflète ; -1 : jamais indexée (R918)
    failed_seq  INTEGER,             -- le `seq` où la lecture a échoué : pas de nouvel essai avant qu'il change
    truncated   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX search_state_never ON search_state (node_id) WHERE indexed_seq = -1;

-- Ce que l'index plein texte n'a pas encore validé (R919). `block_id = ''` : la page entière
-- (son titre a changé, elle est supprimée, ou tout est à reconstruire).
CREATE TABLE search_outbox (
    node_id  TEXT NOT NULL,          -- sans clé étrangère : la ligne survit à la page supprimée
    block_id TEXT NOT NULL,
    gen      INTEGER NOT NULL,       -- avance à chaque changement : une ligne n'est close que si elle n'a pas bougé
    token    TEXT,                   -- le jeton de l'intention qui l'a prise (R909) ; NULL : pas encore prise
    PRIMARY KEY (node_id, block_id)
) WITHOUT ROWID;
```

`instance_settings` garde deux jetons (R909) : `search.committed`, celui de la dernière validation close, et `search.intent`, celui d'une validation annoncée et pas encore close. Les trois premières tables partent avec leur page par la cascade, comme `favorites` (`0004`) ; S1 vérifie que `Store::delete_node`, qui retire les autres tables à la main, n'a rien à y ajouter.

**Les marqueurs durables (R918)**, écrits dans la transaction de l'écriture qui les cause, par le store lui-même :
- `Store::create_node` (avec un document) et `Store::duplicate_pages` : une ligne `search_state` à `indexed_seq = -1` par page née, et sa ligne d'outbox de page entière (son titre) ;
- `Store::rename_node` et `Store::update_node` quand le titre change : la ligne d'outbox de page entière ;
- `Store::delete_node` : la ligne d'outbox de page entière de chaque page supprimée (les lignes de bloc de ces pages sont retirées de l'outbox).

Un test lit les sources du store (sur le modèle de `only_the_depot_forgets_files`) : `UPDATE nodes SET` qui touche `title`, `INSERT INTO nodes` et `DELETE FROM nodes` n'y paraissent qu'aux places d'une liste fermée, chacune suivie de son marqueur ; une écriture neuve du titre est une place de plus, donc un test rouge.

L'index Tantivy vit dans `<données>/search/`, à côté de `files/`. Champs (arrêtés d'après S0 ; S3 mesure ce qui ne l'est pas) :

| Champ | Forme | Sert à |
|---|---|---|
| `node_id` | terme exact | retirer toutes les entrées d'une page ; **pas à filtrer** (R921) |
| `entry` (`<node_id>/<block_id>`) | terme exact | remplacer une entrée |
| `block_id` | gardé | le bloc rendu |
| `page_hi`, `page_lo` | colonnes rapides, l'identifiant de la page en deux entiers | le filtre des pages permises et le groupement par page, dans le collecteur (R921, R902) ; le départage entre deux pages |
| `blk_hi`, `blk_lo` | colonnes rapides : (0, 0) pour l'entrée de titre, sinon seize octets du SHA-256 de la clé du bloc | le départage dans une page (R922), lu à note égale seulement |
| `text` | cherché (fréquences, sans positions), gardé | la recherche, l'extrait, la vérification d'un préfixe long |
| `title` | cherché, **gardé** (R929) | la recherche ; la vérification d'un préfixe long. Le titre montré vient de `nodes` (R904) |
| `text_starts`, `title_starts` | les débuts de mot de 2 à 10 caractères, indexés sans fréquence, pas gardés | le préfixe (R916, R920) |

Pas de champ `space_id` (R921). Le texte et la requête passent par la même chaîne : NFC, `SimpleTokenizer`, `RemoveLongFilter::limit(41)`, `LowerCaser`, `AsciiFoldingFilter` (R926). Un seul écrivain, tenu par l'indexeur, tampon de 50 Mo, un fil d'indexation, un fil de fusion (R925). Tailles mesurées par S0 sur 262 Mo de texte : 651 Mo, dont 256 de textes gardés, 247 de débuts de mot, 58 pour `entry`, 51 pour `text`, 22 pour trois colonnes ; non mesurées : le titre gardé et la quatrième colonne (S3).

### Quand

```
frappe ──► acteur ──► journal (voie de la page) ──► relais           (inchangé)
                └──► Hub::touched : insère l'id de la page dans un ensemble (sans attente)

indexeur (une tâche de fond du serveur, une page à la fois, les pages tour à tour)
  1. attend 2 s de calme sur la page (15 s au plus)                           R906
  2. relit la page gardée, hors acteur, sur le fil bloquant                   R907
  3. la projette en lignes (pur)                                              S2
  4. écrit la différence dans blocks_index / page_links / search_state,
     et une ligne d'outbox par bloc changé ou parti :
     une transaction bornée, voie Lane::Index                                 R908, R919
  5. par lots, l'intention, la validation, la clôture                         R909
```

Ce qui met une page à l'ordre du jour :
- **le contenu écrit par un acteur** : `Actor::persist` et `persist_restore`, les deux seuls points par où l'acteur écrit, nomment la page au hub après une écriture réussie. C'est le seul signal gardé en mémoire ; perdu dans un arrêt brutal, il est rattrapé au démarrage ;
- **le titre, la création, la duplication, la suppression** : les marqueurs durables ci-dessus. L'indexeur, abonné à `Store::changes()` (toutes ces écritures avancent `rev` ou `lsn`), s'en sert comme d'un réveil, et lit alors `search_state` à `-1` (par son index partiel) et l'outbox : jamais toutes les pages d'un espace ;
- **le démarrage** : un rattrapage compare, pour toutes les pages, par lots, le dernier `seq` gardé (instantané ou journal) à `search_state.indexed_seq`. C'est lui qui indexe une instance existante (aucune de ses pages n'a de ligne), et qui rattrape un arrêt brutal ou une écriture de contenu faite par un autre `Store` (les tests, la CLI). Il tourne en fond, après que le serveur écoute : le démarrage à froid ne l'attend pas.

Une page dont la lecture échoue est notée (`failed_seq`), journalisée une fois, et n'est pas réessayée tant que son `seq` n'a pas changé ; **ses lignes d'index sont retirées** dans la même transaction, avec une ligne d'outbox de page entière qui retire ses entrées de l'index plein texte (elle n'est plus trouvable par un texte qu'on ne sait plus relire, et ses rétroliens partent), plutôt que gardées sans limite de temps.

Une transaction de l'index porte au plus `MAX_INDEX_TX_BYTES` de lignes changées. **Valeur de départ : 256 Kio.** Le point de contrôle automatique du WAL tombe toutes les 1 000 pages, soit environ 4 Mio : une transaction de cette taille en déclencherait un à chaque fois, sur le tour d'écriture, devant la frappe des autres (R447). La valeur est arrêtée par `measure_the_write_turn_held_by_an_index_transaction`, **préalable de S4b**, pas par ce plan. Au-delà de la borne, la page s'écrit en plusieurs transactions, et seule la dernière pose `indexed_seq` ; pendant ce temps, `blocks_index` et `page_links` portent un état partiel de la page (limite L-i).

À l'arrêt : `hub.shutdown()`, puis l'indexeur finit sa transaction en cours et clôt sa validation, puis `store.close()`. Ce qui restait à indexer l'est au démarrage suivant.

### L'intention, la validation, la clôture (R909)

Passer un lot de l'outbox à l'index plein texte se fait en trois temps, deux dans la base et un dans Tantivy :

1. **L'intention** (une transaction, `Lane::Index`) : un jeton `T` est tiré au hasard ; les lignes du lot le reçoivent (`token = T`) ; `search.intent = T`. Rien n'est encore écrit dans Tantivy.
2. **La validation** (fil bloquant) : pour chaque ligne du lot, l'entrée est retirée de Tantivy (`entry`, ou toutes celles du `node_id` pour une page entière) et récrite d'après `blocks_index` et `nodes` si elle existe encore ; Tantivy valide avec `T` pour charge utile.
3. **La clôture** (une transaction) : les lignes qui portent `T` **et dont `gen` n'a pas bougé** sont retirées ; celles qui ont bougé perdent leur jeton et restent ; `search.committed = T`, `search.intent` vidé.

**Une clôture refusée sans que le processus s'arrête** (la base ne répond pas, le disque est plein) laisse l'état du second cas du tableau qui suit, serveur vivant : l'index a validé `T`, la base tient encore `T` pour une intention. L'indexeur ne commence alors **aucune** intention neuve (`Store::begin_feed` refuse tant qu'une intention est posée) ; il journalise une ligne `error`, attend (1 s, puis le double, 60 s au plus), puis **rejoue** les lignes de `T` depuis `blocks_index`, valide de nouveau sous le même jeton et tente de clore, jusqu'à ce que la clôture passe. Rejouer est sans effet de bord (les mêmes entrées récrites), et ne tient jamais pour faite une ligne qui a bougé entre-temps. Une validation de Tantivy refusée, elle, défait l'intention (`undo_feed`) et réessaie de même. Pendant ce temps, la recherche répond sur le dernier index validé, et `indexing` est vrai.

Au démarrage, la charge utile `P` de l'index est comparée aux deux jetons de la base :

| Cas | Ce que c'est | Ce qui est fait |
|---|---|---|
| `P = search.committed` | en ordre ; une intention restée posée n'a jamais été validée | l'intention est défaite (les jetons des lignes retirés), rien d'autre |
| `P = search.intent` | arrêt entre la validation et la clôture, **ou** une copie de la base prise entre l'intention et la clôture puis remise en place | les lignes de l'intention **sont rejouées** depuis `blocks_index` (jamais tenues pour faites : la base remise en place peut porter un autre texte que celui validé), puis closes |
| tout autre `P`, dossier absent, illisible, d'un autre schéma d'index | base restaurée (une validation plus tôt comme cent), copie d'avant migration remise en place, dossier d'une autre instance | le dossier est vidé, toutes les pages passent dans l'outbox, tout est reconstruit |

Pourquoi le jeton ferme le trou du compteur : une base restaurée d'avant la validation `T` ne connaît `T` ni comme `committed` ni comme `intent`, quel que soit le nombre de validations faites depuis sa copie.

Vider `search/` ne suit aucun lien (`symlink_metadata`, la règle du dépôt des fichiers, R272) : un lien posé dans le dossier est retiré lui-même, jamais ce qu'il vise.

### La requête et les droits

`POST /api/v1/spaces/{id}/search`, corps `{ "q": "…", "after": N }` (R917), pour qui a un rôle dans l'espace, invité compris.

1. L'extracteur `AuthUser` (`401`, `409 account_changed`). Le corps est pris en `Result<Json<SearchRequest>, JsonRejection>` (la forme de `routes/auth.rs`), sous une borne de 4 Kio : **il est reçu avant tout refus fait à un compte** (un refus rendu sur un corps non lu se perd chez un client qui envoie sans attendre, R505), mais **jugé après** `space_access`. L'ordre des refus est donc `401`, `409`, `404` (aucun rôle : la réponse d'un espace inconnu, que le corps soit lisible ou non), puis `400` ou `422 invalid_json`, `422 invalid_query`, `429`. Rien de l'index n'est lu avant le `404`. `space_access` est lu dans son cache par `rev` : ce premier pas ne coûte pas un calcul par frappe.
2. La requête est lue et bornée (R911). Vide une fois normalisée : `200`, aucun résultat, sans calcul.
3. La requête prend sa place dans le débit du compte (R910). **Tout ce qui suit est fait sous cette garde, sur le fil bloquant**, bâtir l'ensemble permis compris : rien de proportionnel à l'espace ne tourne sur un fil du moteur, ni avant le débit.
4. L'ensemble permis est bâti de `SpaceAccess::visible()` : les pages vues, **hors corbeille** (`in_trash` faux), à chaque requête, sans cache à lui (R927). Il est donné à l'index, qui l'applique **dans son collecteur** : pour chaque entrée trouvée, l'identifiant de sa page est lu en colonne rapide, et une entrée d'une page hors de l'ensemble est écartée avant le groupement, donc avant `skip` et `limit` (R921). Jamais un ensemble de termes dans la requête (65 à 580 ms mesurés), jamais un tri après coup.
5. L'index rend des **pages** : la meilleure entrée de chaque page, groupées avant `skip` et `limit` (R902), notées sans statistique d'ensemble (R916, R923), deux notes égales départagées par l'entrée seule (R922). Le dernier terme est cherché comme mot entier ou comme début de mot (R930). `after` compte des pages ; deux réponses qui se suivent ne rendent jamais la même page (à index inchangé entre les deux).
6. Chaque page rendue est **revérifiée** contre `SpaceAccess` (filet : une entrée d'une page hors de l'ensemble ferait une ligne `error` et serait jetée), puis reçoit son titre (`nodes`), son fil d'Ariane (les ancêtres **vus**), son extrait.
7. Réponse : `SearchResults { results: SearchHit[], more: bool, indexing: bool }`, `SearchHit { pageId, title, path: { id, title }[], blockId: string | null, excerpt: string, marks: [number, number][] }`. Les plages de `marks` comptent en unités UTF-16, comme une chaîne JavaScript. `excerpt` fait 240 caractères au plus. `indexing` : l'indexeur sait une page **vue par ce compte** en retard dans cet espace (jamais un nombre).
8. **La réponse d'une recherche calculée ne part pas avant `SEARCH_FLOOR`** (50 ms, valeur de départ) après la prise de sa place au débit (R932). L'attente se fait hors du fil bloquant ; la place de l'instance est rendue à la fin du calcul, **celle du compte au départ de la réponse** : rendue plus tôt, l'heure où la requête suivante du compte commence dirait la durée du calcul. Une requête vide, un refus et un `429` ne sont pas retenus : ils ne lisent pas l'index.

Ce qui dépend encore de ce que la personne ne voit pas : **le temps de réponse**. Lire les entrées d'un terme coûte en proportion du nombre d'entrées de l'instance qui le portent, avant le filtre. S0 l'a chiffré : 1,6 ms de plus quand cent mille entrées non permises portent le terme, 16 ns par entrée, dix fois l'écart type dans le processus ; à travers un réseau ce n'est pas mesuré, c'est un calcul (quelques appels suffisent pour cent mille entrées cachées, des milliers pour mille). Le plancher du point 8 le couvre tant que le calcul tient dessous ; au-delà, c'est la limite L-b (décision L19, R932).

`GET /api/v1/pages/{id}/backlinks`, pour qui lit la page (`read`, à la corbeille aussi). Une personne ne doit ni perdre un vrai rétrolien derrière des sources qu'elle ne voit pas, ni apprendre leur nombre : `target_id` n'a pas de clé étrangère, et tout compte peut écrire, dans une page à lui, des centaines de liens vers l'UUID d'une page d'un autre. La lecture est donc faite ainsi :

- les sources sont les `node_id` **distincts** de `page_links` pour ce `target_id`, **restreints à l'espace de la page visée** par une jointure sur `nodes` (une source d'un autre espace n'est pas rendue au lot 4, comme le lien inverse garde son titre copié, R574), hors la page elle-même, dans l'ordre des identifiants ;
- elles sont lues **par lots** (500), chaque lot trié par `space_access` (vues, hors corbeille), jusqu'à **200 sources vues** ou jusqu'à un plafond de **20 000 sources parcourues** ; la réponse ne dit ni combien ont été parcourues, ni s'il en reste ;
- pour chaque source gardée, son premier bloc qui lie, et l'extrait de ce bloc **coupé dans la requête** (`substr`), jamais un texte de 64 Kio ramené pour en garder 240 caractères.

Réponse : `Backlinks { pages: { pageId, blockId, excerpt }[] }`. Au-delà du plafond de parcours, des sources vues peuvent manquer : c'est la limite L-j, à laquelle un compte hostile n'arrive qu'en créant vingt mille pages dans l'espace.

### Bornes devant une entrée hostile

| Ce qu'un compte peut faire | Borne | Où |
|---|---|---|
| Une requête longue, à beaucoup de termes, à opérateurs | 200 caractères, 8 termes, aucune syntaxe ; au-delà `422 invalid_query` | S2, S5 |
| Un préfixe d'une lettre sur tout l'espace | préfixe à partir de 2 caractères, sur le dernier terme seulement, par un terme exact du champ des débuts de mot (R916) | S2, S3 |
| Un bloc de longs mots tous différents, pour gonfler le champ des débuts de mot | 9 débuts par mot au plus (2 à 10 caractères, R920), donc un nombre de termes proportionnel au nombre de mots, pas à leur longueur ; le bloc reste borné à 64 Kio. Mesuré (mots de 40 octets) : le champ pèse 1,17 à 1,25 fois les octets reçus, l'index entier 3,3 à 3,4 fois | S0, S3 |
| Un bloc de **mots courts** tous différents (10 caractères) : le pire cas, R920 n'y borne rien (9 débuts pour 11 octets) | la taille reste proportionnelle aux octets reçus, bornés à 64 Kio par bloc et 8 Mio par page : le champ pèse 4,3 fois les octets reçus, l'index 6,7 fois, à 0,51 Mo/s (mesuré par la relecture de S0 sur 300 blocs, non rejoué ; R928) | S3, limite L-g |
| Écrire des pages entières de mots tous différents, pour faire fusionner l'index | **rien ne borne la pointe de mémoire ni la durée des fusions** : 802 Mo de mémoire de travail et vingt minutes de fusions pour 655 Mo reçus (mots de 40 octets, S0) ; 455 Mo pour 19,7 Mo reçus (mots de 10 octets, relecture) ; vraisemblablement du tas, pas des fichiers projetés ; le tampon de l'écrivain ne la borne pas. **Risque ouvert RO-1** (R928) | S3 (tas, politique de fusion), X2, sinon lot 7 |
| Une page de dix mille blocs qui partagent les dix premiers caractères d'un préfixe tapé, pour prendre toutes les vérifications | 20 entrées vérifiées par page au plus, dans un ordre tiré des entrées seules (R929) | S3 |
| Un préfixe long partagé par des milliers d'entrées, pour faire vérifier leur texte | 2 000 entrées vérifiées par requête au plus (R920) | S3 |
| Une boucle de recherches | un calcul et une attente par compte, quatre calculs pour l'instance, les comptes tour à tour ; `429 search_busy` | S5 |
| Paginer sans fin | 20 pages par réponse, 5 réponses à la suite | S5 |
| Une page de dix mille blocs sous un titre courant, pour occuper tous les résultats | une page n'a qu'un résultat : groupement par page dans l'index, avant la pagination (R902) | S3 |
| Deux blocs dosés pour lire dans leur ordre ce qu'écrivent les autres | la note ne lit aucune statistique d'ensemble (R916) | S0, S3 |
| Une page d'un bloc de plusieurs Mio, ou de cent mille liens | 64 Kio par bloc, 8 Mio par page, 10 000 liens ; le reste n'est pas indexé | S2 |
| Écrire sans relâche dans une grande page | une indexation de la page toutes les 15 s au plus ; elle la relit et la projette en entier (81 ms pour 10 000 blocs en release, sur le fil bloquant), mais **seuls les blocs changés** sont écrits, dans SQLite comme dans l'index plein texte (R919) | S4b, S4c, X2 |
| Renommer sans relâche une grande page | une fois toutes les 15 s au plus ; chaque fois, toutes les entrées de la page sont récrites dans l'index plein texte (R902), rien dans `blocks_index` | S4c, X2 |
| Écrire sans relâche dans N pages | l'indexeur prend du retard (dit par `indexing`), sans coût pour la frappe des autres : une seule voie, une transaction bornée à la fois | S4b, X2 |
| Poser des centaines de liens vers la page d'un autre | sources distinctes, de l'espace de la cible, triées par droits avant d'être comptées, plafond de parcours | S6 |
| Remplir le disque par l'index | rien avant le lot 7 (l'index pèse en proportion des pages, que R247 ne borne pas non plus) | limite écrite |
| Demander les rétroliens en boucle | lecture indexée et bornée (200 vues, 20 000 parcourues), extraits coupés en SQL ; pas de débit propre au lot 4 (mesurée en S6 ; un débit s'ajoute si elle dépasse celle des commentaires d'une page pleine) | S6 |

### Reconstruction, sauvegardes

- `<données>/search/` ne fait pas partie d'une sauvegarde : il se refait. La documentation le dit (§7.4, README).
- Au démarrage, le jeton de l'index est comparé à ceux de la base (table plus haut). Tant qu'une reconstruction dure, **la recherche ne rend que ce qui est déjà reconstruit**, et le dit (`indexing`).
- `mementum reindex` (serveur arrêté) refait tout : `blocks_index` et `page_links` depuis les documents, puis Tantivy depuis `blocks_index`. `mementum reindex --text-only` ne refait que Tantivy.
- La migration `0005` ne remplit rien : elle crée des tables vides, et reste rapide sur une grande base.
- **Le texte d'une page est gardé trois fois** : dans son document Yjs, dans `blocks_index`, dans l'index plein texte (qui le garde pour l'extrait). S0 a chiffré les deux copies neuves : 499 Mo pour `blocks_index` (1,92 fois le texte, à cause de sa clé de deux identifiants en texte) et 651 Mo pour l'index (2,48 fois), soit 1 150 Mo pour 262 Mo de texte ; S3 mesure le repli (ne pas garder le texte dans l'index), S4c la base et le dossier avant et après l'indexation d'un espace (décision L18, R931).
- Un texte effacé quitte `blocks_index` et les résultats à la réindexation de sa page, mais **reste dans les fichiers de l'index plein texte jusqu'à la fusion de son segment**, comme il reste dans les pages libres de SQLite : limite L-k, pour qui lit le disque du serveur.

---

## Budgets de performance

**Ceux de la conception (§5.10), tous à tenir, aucun relâché :**

| Budget | Ce que le lot risque | Mesure |
|---|---|---|
| Frappe vue chez un autre < 150 ms | l'indexeur dans la file d'écriture, sur le processeur, les fusions de segments de Tantivy | S4b puis X2 : la frappe d'un autre pendant la première indexation d'un espace de 10 000 pages, et pendant qu'une page de 10 000 blocs est récrite puis renommée en boucle |
| Ouvrir une page jamais vue < 300 ms (1 000 blocs), < 3 s (10 000) | les rétroliens, le code de la recherche dans le lot de départ | `e2e/budgets.spec.ts` inchangé et vert ; un e2e compte les requêtes d'une ouverture (aucune vers `/search` ni `/backlinks` avant la première synchro) |
| Charger une page de 10 000 blocs < 300 ms (serveur) | l'indexeur qui relit la même page au même moment | X2 |
| Mémoire du serveur au repos < 150 Mo | l'écrivain de Tantivy (tampon), l'index ouvert, **ses fichiers projetés en mémoire** | S0 : +4 Mo à l'ouverture d'un index de 651 Mo (processus nu, Windows, écrivain de 15 Mo) ; après 500 recherches, 20 à 24 Mo de mémoire privée et 185 à 190 Mo de mémoire de travail. X2, d'après R924 : (a) la mesure existante de `mementum-cli`, sur un dossier qui porte un index de 10 000 pages **à jour** (rattrapage fini, écrivain et lecteur tenus : c'est cela, « au repos ») ; (b) après 500 recherches et après un rattrapage qui indexe, la mémoire tenue en propre, `RssAnon` sous Linux (la CI) et mémoire privée sous Windows, sous 150 Mo ; le RSS entier, la pointe pendant le rattrapage et pendant une reconstruction sont relevés à part, sans budget de la conception, et dits au registre |
| Démarrage à froid < 1 s | l'ouverture de l'index, le rattrapage | S0 (2,4 à 2,8 ms pour ouvrir l'index et l'écrivain) puis X2 : la mesure existante de `mementum-cli`, avec le même index ; le rattrapage n'est pas attendu |
| Pi 4 : ~25 personnes ; VPS 2 vCPU : ~200 | les fils de Tantivy | non mesuré par ce lot (les mesures ARM sont repoussées avant la P2) ; un seul fil d'indexation, un seul de fusion |

**La conception ne donne aucun budget à la recherche elle-même** (écart E8). Ce plan en propose trois, **qui ne sont pas de la conception** (décision L10) ; pendant le lot ils sont des mesures, pas des critères de refus, et ne vont au §5.10 qu'après X2. Ce que S0 en dit : une recherche seule, index au repos, répond en 42,6 ms au pire p95 (non mesuré : pendant une fusion, sous quatre recherches de front, avec le terme de plus de R930, avec l'ensemble permis à bâtir) ; le plancher de R932 fait qu'aucune recherche ne répond en moins de 50 ms ; une page ordinaire est trouvable en 2,1 à 2,3 s, le renommage d'une page de 10 000 blocs en 5 à 6 s.

- une recherche répond en moins de 100 ms au p95, côté serveur, en release, sur un espace de 10 000 pages et un million de blocs, pour un membre qui voit tout comme pour un invité qui voit dix pages ;
- du dernier caractère tapé aux résultats affichés : moins de 300 ms au p95 (dont 150 ms d'anti-rebond), build de production ;
- une page modifiée est trouvable par son nouveau texte en moins de 5 s après la dernière frappe, l'indexeur au repos.

Toute mesure neuve suit les règles de la CI : `#[ignore]` ne marque que des mesures, en release, une à la fois (`alone`), avec leur ligne `MESURE` ; un budget du navigateur se vérifie aussi sous `MEMENTUM_BUDGETS_CPU_SLOWDOWN=1.5`.

## Ce qui reste à mesurer, et par qui

S0 n'a rien mesuré dans le serveur, sous Linux, à froid ni sous charge. Chaque point est une mesure de la tâche nommée (elle est de son ajout) ; aucun n'est tranché d'avance.

| Quoi | Tâche |
|---|---|
| La clé de `blocks_index` : deux identifiants en texte de 36 caractères font peser la table 1,92 fois son texte ; **question ouverte** : une forme plus courte tient-elle avec la clé étrangère vers `nodes(id)` et des identifiants de bloc qui sont des chaînes du client ? | S1 (arrête la forme, dit la taille) |
| Taille du titre gardé sur chaque entrée (R929) et des quatre colonnes rapides (R922) | S3 |
| Le repli de L18 : l'index sans texte gardé (extrait et vérification de R920 lus dans `blocks_index`), taille et latence | S3 (l'index seul), S4c (avec la base) |
| Le terme de plus de R930 (mot entier ou début) sur un mot très courant | S3, S5 |
| Le tas pendant l'indexation et les fusions des formes hostiles (mots de 40 et de 10 octets), et ce qu'une politique de fusion bornée y change (RO-1) | S3 ; X2 dans le serveur |
| La mémoire d'un écrivain de 50 Mo au repos après une validation (R925) | S3, puis X2 |
| Bâtir l'ensemble permis de 10 000 et 100 000 pages (R927) | S5 |
| La pagination prouvée par deux vraies recherches avec `skip` (la « preuve » de S0 coupait en deux une même liste) | S3, S5 |
| La base réelle d'un espace de 10 000 pages, avant et après l'indexation | S4c |
| La mémoire jugée par R924, sous Linux et sous Windows ; la valeur de `SEARCH_FLOOR` ; la latence d'une recherche pendant une fusion et sous quatre recherches de front ; la frappe pendant l'indexation | X2 |
| `cargo deny check` entier sur l'atelier du serveur avec Tantivy (S0 n'a passé que `licenses bans`, sur un `Cargo.toml` d'essai) ; la compilation sur les 4 cœurs de la CI | S3 (la CI de sa poussée) |

Pas dans ce lot : ARM et le Pi 4 ; une coupure de courant pendant une validation ; un répertoire Tantivy sans projection en mémoire (R924).

---

## Structure des fichiers

| Fichier | Rôle | Tâche |
|---|---|---|
| `crates/mementum-store/migrations/0005_search.sql` | tables de l'index | S1 |
| `crates/mementum-store/src/search.rs` (neuf), `queue.rs`, `nodes.rs`, `duplicate.rs`, `lib.rs` | écritures et lectures de l'index, marqueurs durables, `Lane::Index` | S1 |
| `crates/mementum-search/` (crate neuve) : `src/project.rs`, `query.rs`, `index.rs`, `lib.rs` | projection, requête, Tantivy | S2, S3 |
| `crates/mementum-sync/src/hub.rs`, `actor.rs`, `lib.rs`, `tests/common/clock.rs` | pages écrites, lecture seule d'une page gardée, `StoppedClock` partagé | S4a |
| `crates/mementum-server/src/indexer.rs` (neuf), `searches.rs` (neuf), `routes/search.rs` (neuf), `routes/backlinks.rs` (neuf), `lib.rs`, `types.rs`, `config.rs` | indexeur, débit, routes | S4b à S6 |
| `crates/mementum-cli/src/main.rs` | `reindex` | S7 |
| `packages/protocol/src/generated/` | `SearchResults`, `SearchHit`, `Backlinks` (régénérés) | S5, S6 |
| `apps/web/src/search/` (neuf) : `SearchDialog.tsx`, `useSearch.ts`, `shortcut.ts`, `marks.ts` | fenêtre de recherche | C1 |
| `apps/web/src/api.ts`, `router.ts`, `sidebar/Sidebar.tsx`, `App.tsx`, `page/PageView.tsx` | appels, adresse d'un bloc, entrée de la barre | C1, C2 |
| `apps/web/src/page/Backlinks.tsx`, `useBacklinks.ts` (neufs) | rétroliens | C3 |
| `apps/web/e2e/search.spec.ts`, `backlinks.spec.ts`, `helpers.ts`, `servers.ts`, `budgets.spec.ts` | parcours, budgets | C1 à C3, X1, X2 |

---

## Piste S

Ordre : S0 (**faite**) et S2 d'abord (sans dépendance), puis S1, S4a, S3, S4b, S4c, S5, S6, S7. Au 2026-11-20 : S2 et S4a peuvent partir de front (une crate neuve d'un côté, `mementum-sync` de l'autre), S3 après S2 ; S1 attend la migration `0004` figée, et tient S4b et la suite.

### Task S0 : mesurer le moteur avant de bâtir dessus — faite (2026-11-19, close le 2026-11-20)

> **Verdict : Tantivy 0.27.0 tient, la tâche ne rend pas `BLOCKED`.** [Compte rendu](../logs/2026-11-19-p1-lot4-s0-mesures.md), [relecture](../logs/2026-11-19-p1-lot4-s0-relecture.md) (partielle, portée le 2026-11-20 sans nouvelle mesure), section S0 du [registre](../logs/2026-11-19-p1-lot4-ledger.md) (R921 à R933). Le banc est resté dans le dépôt, `spike/search-bench/` (jetable, son propre atelier Cargo), avec ses sorties brutes. Écarts au texte ci-dessous : la forme hostile n'a été mesurée que pour des mots de 40 octets, qui n'est pas le pire cas (R928) ; la question (d) est répondue par un calcul, pas par une mesure à travers un réseau ; FTS5 n'a été mesuré par la tâche que dans une forme défavorable, la relecture en a mesuré une meilleure (compte rendu, Step 5). Ce qui reste à mesurer est réparti plus haut.

**Dépend de :** rien. **Livrable :** une section du registre, des chiffres, et un verdict ; aucun code du produit. Le banc est jetable, hors du dépôt (un dossier de la tâche), sur le modèle des essais de la phase 0.

Pourquoi : la conception choisit Tantivy (§5.2) sans que la phase 0 l'ait mesuré (`spike/RESULTS.md` n'en dit rien), deux budgets du §5.10 (mémoire au repos, démarrage à froid) en dépendent, et trois exigences de ce plan (R902, R909, R916) supposent des fonctions du moteur que personne n'a essayées.

- [x] **Step 1 : épingler.** La version courante de Tantivy, sa licence et celles de tout ce qu'elle tire, contre `deny.toml` (`cargo deny check licenses bans` sur un `Cargo.toml` d'essai) ; la version de Rust qu'elle demande contre `rust-version = "1.96"` ; ce qu'elle ajoute à la taille du binaire et à la durée d'une compilation en release.
- [x] **Step 2 : mesurer**, en release, sur un corpus fait par programme (10 000 pages, un million de blocs, en français, accents compris) : mémoire d'un processus qui tient l'index ouvert et un écrivain au repos, avec le plus petit tampon admis, un fil d'indexation, un fil de fusion ; temps d'ouverture de l'index ; débit d'indexation ; durée et coût d'une validation ; latence d'une requête de un, trois et huit termes, avec préfixe, filtrée par 10, 10 000 puis 100 000 identifiants de page ; taille de l'index sur le disque, avec et sans le champ des débuts de mot, sur le corpus ordinaire **et sur la forme hostile** (dix mille blocs de 64 Kio de mots de 40 octets tous différents : la taille du champ, borné à 10 caractères par R920, rapportée aux octets reçus), et taille de `blocks_index` pour le même corpus (décision L18).
- [x] **Step 3 : trois exigences, chacune prouvée par un essai, ou la tâche rend `BLOCKED`** :
  - **(R916) une note sans statistique d'ensemble.** Deux index qui ne diffèrent que par des entrées hors du filtre (un autre espace, des pages non permises, des entrées supprimées et pas encore fusionnées) rendent les mêmes pages **dans le même ordre**, pour des requêtes dosées (deux blocs qui portent `t1` et `t2` en proportions inverses). Dire comment : une note calculée par programme sur l'entrée seule, ou des statistiques constantes données au moteur. Et un préfixe servi par un terme exact du champ des débuts de mot, sans parcours du dictionnaire.
  - **(R902) le groupement par page avant la pagination.** Un collecteur qui garde la meilleure entrée par `node_id` : sa latence et sa mémoire quand un terme touche un million d'entrées de dix mille pages, et quand il touche dix mille entrées d'une seule page. S'il ne tient pas le budget proposé (L10), mesurer le repli (pas de titre recopié sur les blocs).
  - **(R909) la charge utile d'une validation**, relue à l'ouverture de l'index, et ce qu'on lit d'un index dont la dernière validation a été coupée.
- [x] **Step 4 : quatre questions**, chacune par un essai : (a) le filtre par un ensemble de pages se pose-t-il dans la requête, et à quel coût quand l'ensemble est grand ; (b) quel tokeniseur plie la casse et les accents (« élève » trouvé par « eleve »), et que fait-il d'un mot de plus de 40 octets, du chinois, d'un identifiant `snake_case` ; (c) la racinisation française : ce qu'elle trouve en plus, ce qu'elle trouve à tort dans un texte anglais (décision L6) ; (d) **le temps de réponse comme canal** : la même requête, le même ensemble permis, quand le terme est porté par zéro puis par cent mille entrées non permises ; l'écart, et s'il se distingue du bruit d'un appel par le réseau local (limite L-b, décision L19).
- [x] **Step 5 : la même mesure avec FTS5** (disponible : `-DSQLITE_ENABLE_FTS5`), sur le même corpus, table `fts5` à contenu externe sur `blocks_index`, tokeniseur `unicode61 remove_diacritics 2` : latence, taille, et **durée d'une écriture d'index dans le tour d'écriture de SQLite**, qui est ce que FTS5 coûterait à la frappe des autres ; et ce que deviennent chez lui les trois exigences du Step 3. C'est la matière de la décision L1, pas un changement de plan.
- [x] **Step 6 : verdict au registre.** Si la mémoire au repos ou le démarrage à froid ne tiennent pas avec Tantivy, si une licence est refusée, ou si une exigence du Step 3 ne se tient pas simplement : la tâche rend `BLOCKED`, avec ses chiffres, et la décision revient à Lenny. Sinon, la version est épinglée et S3 part.

**Pièges de `CLAUDE.md` :** un budget de temps ne s'affirme que dans une mesure, en release, une à la fois (R341) ; la CI est plus lente que le poste ; `TMP`/`TEMP` de la tâche, à supprimer en partant (les mesures en release sont ce qui pèse le plus, R344).

### Task S1 : store, les tables de l'index — faite (2026-11-20)

> `0004` figée par décision du coordinateur (R936). Écarts et précisions au [registre](../logs/2026-11-19-p1-lot4-ledger.md), section S1 (R937 à R939, R961 à R963) : la clé de `blocks_index` gardée (sonde : 1,77 fois le texte, coût accepté de L18) ; le store a son propre `IndexedBlock<'a>` ; l'outbox n'a pas de `gen` (une ligne touchée perd son jeton) ; toute page créée est marquée, avec ou sans document ; un échec de lecture note la page à son `seq` ; `begin_feed` rend un `FeedStart`, `intent_lines` s'ajoute, `stale_pages` rend un curseur.

**Dépend de :** la clôture du lot 3 (`0004` figée). **Livrable :** la migration, la voie, les fonctions du store, les marqueurs durables ; rien ne lit encore l'index. **Question ouverte, à trancher ici** : S0 a mesuré `blocks_index` à 1,92 fois son texte, à cause de sa clé (deux identifiants en texte de 36 caractères, insérés dans un ordre quelconque) ; la tâche dit si une clé plus courte tient (la clé étrangère vers `nodes(id)`, en texte, et un `block_id` qui est une chaîne du client), et sinon l'écrit au registre comme un coût accepté de L18.

**Files:**
- Create: `crates/mementum-store/migrations/0005_search.sql`, `crates/mementum-store/src/search.rs`, `crates/mementum-store/tests/search.rs`
- Modify: `crates/mementum-store/src/queue.rs` (`Lane::Index`), `lib.rs`, `nodes.rs` (`create_node`, `rename_node`, `update_node`, `delete_node` : R918), `duplicate.rs` (`duplicate_pages` : R918), `backup.rs` si la liste des migrations y est nommée
- Test: `crates/mementum-store/tests/writes.rs`, `tests/migration.rs`

**Interfaces (produites) :**
- `Store::index_page(node_id, seq, rows: &[IndexedBlock], truncated) -> Result<Indexed, StoreError>` : écrit la différence et une ligne d'outbox par bloc changé ou parti, dans `Lane::Index`, par tranches de `MAX_INDEX_TX_BYTES` ; `Indexed { changed: usize }` ; ne fait rien si `seq < indexed_seq`.
- `Store::index_failed(node_id, seq)` (retire aussi les lignes d'index de la page, **et pose sa ligne d'outbox de page entière**, pour que ses entrées quittent aussi l'index plein texte), `Store::never_indexed(limit)` (les lignes à `-1`, par l'index partiel), `Store::stale_pages(after, limit)` (le rattrapage : les pages dont le dernier `seq` gardé dépasse `indexed_seq`, par lots, lue sans tour d'écriture), `Store::behind_in_space(space_id, limit)`.
- `Store::begin_feed(limit) -> Feed { token, lines }`, `Store::finish_feed(token)`, `Store::undo_feed()`, `Store::feed_tokens() -> { committed, intent }`, `Store::outbox_everything(after: Option<Uuid>, limit) -> Option<Uuid>` (tout reconstruire, **par tranches** de pages dans l'ordre des identifiants, une transaction bornée par tranche dans `Lane::Index` : jamais une transaction en proportion de l'instance ; l'appelant reprend à l'identifiant rendu, et un arrêt en route se rattrape, le jeton de l'index vidé n'étant posé qu'à la première validation), `Store::indexed_block(node_id, block_id)`, `Store::indexed_blocks(node_id)`.
- `Store::backlink_sources(target_id, space_id, after: Option<Uuid>, limit) -> Vec<Uuid>` (sources distinctes, de l'espace, hors la cible, dans l'ordre des identifiants), `Store::backlink_excerpts(target_id, sources: &[Uuid], chars) -> Vec<Backlink { node_id, block_id, excerpt }>` (le premier bloc de chaque source, l'extrait coupé par `substr`).

- [x] **Step 1 : tests, vus rouges.** Différence : `index_page_writes_only_what_changed` (un déclencheur témoin compte les lignes écrites : une frappe dans un bloc d'une page de 1 000 blocs en écrit une, et **une** ligne d'outbox) ; `index_page_removes_the_blocks_that_left` (et les nomme à l'outbox) ; `an_older_read_never_overwrites_a_newer_index` ; `a_page_over_the_transaction_bound_is_written_in_slices_and_marked_at_the_last`. Marqueurs (R918) : `a_created_page_is_born_never_indexed`, `a_duplicated_page_and_its_subpages_are_born_never_indexed`, `a_rename_puts_the_page_in_the_outbox_in_its_transaction` (transaction abandonnée : aucune ligne ; témoin : validée, une ligne), `a_move_without_a_new_title_marks_nothing`, `the_index_leaves_with_its_page` (suppression définitive : plus aucune ligne des trois tables, les lignes de bloc de l'outbox parties, la ligne de page entière posée), `only_listed_places_write_a_title_or_a_node` (lit les sources du store, liste fermée). Voie : `index_writes_share_one_lane` (sur le modèle de `page_copies_share_one_lane_whoever_asks` : tour tenu par `hold_writes`, ordre des validations lu dans la base) ; `indexing_bumps_neither_rev_nor_lsn_and_announces_nothing` (témoin : un renommage qui suit est annoncé) ; `stale_pages_takes_no_write_turn` (tour tenu, la lecture rend) ; `stale_pages_names_a_page_written_by_another_store`. Intention : `outbox_everything_goes_by_slices` (un tour tenu entre deux tranches laisse passer une autre écriture : ordre lu dans la base) ; `finish_feed_keeps_a_line_changed_meanwhile` ; `undo_feed_gives_the_lines_back` ; `a_second_feed_cannot_begin_before_the_first_is_finished_or_undone`. Rétroliens : `backlink_sources_are_distinct_of_the_space_and_ordered` ; `backlink_sources_are_read_from_the_index` (`EXPLAIN QUERY PLAN` : `page_links_by_target`, aucun parcours de table) ; `backlink_excerpts_are_cut_by_the_database` (un bloc de 64 Kio : la ligne rendue fait la longueur demandée). Migration : `a_lot_3_database_migrates_and_starts_with_nothing_indexed`.
- [x] **Step 2 : écrire**, toucher `lib.rs` (R269), relancer ces tests et les deux fichiers touchés (`tests/writes.rs`, `tests/migration.rs`). Commit `store : tables de l'index, marqueurs et voie de l'indexation (R901, R908, R918, R919)`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** écrire toutes les lignes au lieu de la différence ; une ligne d'outbox de page entière à chaque indexation ; oublier la comparaison de `seq` ; `Lane::Common` à la place de `Lane::Index` ; clore une ligne sans comparer `gen` ; le marqueur du renommage écrit après la validation ; `bump_lsn` ajouté par mégarde ; `LIMIT` posé avant `DISTINCT` dans les sources d'un rétrolien.

**Pièges :** une migration neuve n'entre dans le binaire qu'à la recompilation du store (R269) ; une requête reprend l'expression exacte de son index ; une écriture qui parcourt une grande table lit d'abord sans tour, et ne prend le tour que pour écrire (R270) ; une transaction qui écrit n'en ouvre pas une seconde (les marqueurs s'écrivent **dans** la `WriteTx` de l'écriture qui les cause, pas par un second appel au store) ; `TestDir`, pas `tempfile::tempdir()` (R344) ; pour prouver un ordre de service, tenir un tour et lire l'ordre dans la base, jamais à l'horloge ; SQL dynamique sous `AssertSqlSafe`, valeurs liées ; une chaîne SQL à `\` de fin de ligne s'écrit par l'outil d'édition.

### Task S2 : `mementum-search`, la projection et la requête (sans moteur)

> **Faite le 2026-11-20** (registre, « Task S2 », R940 à R946). Écarts : le dernier mot d'un caractère reste cherché entier (R940) ; la clé de remplacement est élargie (R941) ; le découpage et le pliage des mots sont écrits ici (`nfc`, `words`, `fold`), et S3 pose son analyseur dessus plutôt que sur les filtres de Tantivy (R942) ; le texte projeté est en NFC, pas le titre (R943) ; les tests de la requête sont dans `tests/query.rs`.

**Dépend de :** rien (peut partir avec S0). **Livrable :** deux fonctions pures, leurs fixtures, leur linéarité.

**Files:**
- Create: `crates/mementum-search/Cargo.toml`, `src/lib.rs`, `src/project.rs`, `src/query.rs`, `tests/project.rs`, `tests/linear.rs`
- Create: `spec/search/projection.json` (table de cas : un bloc, le texte et les liens attendus), `spec/search/queries.json`
- Modify: `Cargo.toml` (racine)

**Interfaces (produites) :**
- `project(blocks: &[Block], origin: Option<&str>) -> Projection { rows: Vec<IndexedBlock>, truncated: bool }`, `IndexedBlock { block_id, kind, text, hash, links: Vec<Uuid> }`.
- `read_query(raw: &str) -> Result<Query, QueryRefusal>`, `Query { terms: Vec<String>, prefix: Option<String> }`.

- [x] **Step 1 : tests, vus rouges.** `every_block_of_the_registry_has_its_rule` (échoue si `spec/schema/blocks.json` gagne un type que la table ne connaît pas) ; `the_fixtures_project_as_written` (`all-blocks`, `non-default`, `rich-blocks` de `spec/mmd/fixtures/` : le texte attendu est écrit à la main, jamais recopié de la sortie) ; `marks_are_dropped_and_text_is_kept` ; `a_mention_is_read_by_its_label_and_linked_by_its_id` ; `a_link_to_a_page_of_the_instance_is_a_link_and_an_outside_address_is_not` (table de cas : `/p/<uuid>`, la même sous l'origine publique, avec un fragment `#b=…`, un autre hôte, `/p/<uuid>/x`, un UUID sans tirets) ; `a_block_without_id_or_with_a_repeated_id_gets_a_stand_in_key` ; `an_unknown_block_is_not_indexed_and_its_children_are` (décision L17) ; `a_block_over_the_bound_is_cut_between_characters`, `a_page_over_the_bound_is_marked_truncated` ; `the_hash_changes_with_the_text_the_type_and_the_links_and_with_nothing_else` ; requête : `a_query_is_folded_and_split`, `operators_are_only_characters` (`AND`, `"`, `*`, `-`, `title:`, `~`), `a_query_over_the_bounds_is_refused`, `a_prefix_under_two_characters_is_dropped`, `an_empty_query_reads_empty`. Linéarité : chaque forme (un long texte, mille plages marquées, un tableau large, des listes imbriquées, cent mille mentions, une requête longue) à une taille et à huit fois cette taille, sous les bornes de R433.
- [x] **Step 2 : écrire**, relancer. Commit `recherche : projection d'une page et lecture d'une requête (R911, R912)`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** indexer l'adresse d'un lien ; oublier les enfants d'un bloc sans texte (`column`) ; couper au milieu d'un caractère ; laisser passer un `*` ; reconnaître une page par `Uuid::parse_str` seul (les formes sans tirets, la leçon de R754).

**Pièges :** toute lecture d'une entrée venue d'un client croît comme elle, avec des bornes absolues (R380, R433) ; le rapport de temps se calcule en nanosecondes ; un identifiant de page se reconnaît par une règle stricte, table de cas partagée (`spec/files/addresses.json` est le modèle) ; un message d'erreur ne recopie jamais l'entrée (R434) ; fins de ligne LF pour les fixtures.

### Task S3 : `mementum-search`, l'index plein texte

**Dépend de :** S0 (faite : Tantivy 0.27.0), S2. **Livrable :** l'index derrière `SearchIndex`, sans rien savoir du store ni des droits, dans la forme du paragraphe « Où » (R921 à R923, R925, R926, R929, R930).

**Files:**
- Create: `crates/mementum-search/src/index.rs`, `tests/index.rs`, `tests/linear_index.rs`
- Modify: `Cargo.toml` (racine : `tantivy`, version de S0), `deny.toml` si une licence déjà compatible manque à la liste (sinon `BLOCKED`)

**Interfaces (produites) :**
- `SearchIndex::open(dir) -> Opened { index, token: Option<String> }` : le jeton de la dernière validation, ou `None` pour un index neuf ; `SearchIndex::reset(dir)` : vide le dossier sans suivre de lien, et rouvre.
- `put_entry(node_id, block_id, title, text)` (`block_id` absent : l'entrée de titre), `remove_entry(node_id, block_id)`, `remove_page(node_id)`, `commit(token)` : synchrones, pour le fil bloquant. Pas de `space_id` (R921).
- `search(query: &Query, allowed: &PageSet, skip, limit) -> Found { pages: Vec<Hit { node_id, block_id, excerpt, marks }>, more }` : **une ligne par page**, `skip` et `limit` en pages.

- [ ] **Step 1 : tests, vus rouges.** Trouver : `a_word_of_a_block_finds_its_page_and_its_block` ; `accents_and_case_do_not_matter` ; `every_term_is_wanted` ; `the_last_term_matches_as_a_prefix` ; `a_title_outranks_a_block`. Pages (R902) : `a_page_is_found_once_whatever_its_blocks` ; `a_title_word_of_a_10000_block_page_does_not_hide_other_pages` (une page de 10 000 blocs et trente pages d'un bloc sous le même mot de titre : les trente et une sortent, en deux réponses) ; `paging_never_repeats_a_page` (**deux vraies recherches**, `skip` 0 puis `skip` 20, sur un index où plus de quarante pages ont la même note : aucune page commune, et leur réunion est le début de la liste entière ; la « preuve » de S0 coupait en deux une même liste, et serait passée avec toute implémentation) et `paging_counts_pages` (toute réponse d'avant la dernière porte `limit` pages) ; `more_is_true_only_when_another_page_follows`. Droits : `only_allowed_pages_are_found` et son témoin (la même requête, l'ensemble élargi, trouve) ; `another_space_is_never_found` (par l'ensemble permis seul : l'index n'a pas de champ d'espace, R921) ; `the_filter_is_applied_in_the_collector_before_grouping` (compteur : aucune page non permise n'atteint le groupement) ; `what_unseen_pages_hold_changes_neither_the_hits_nor_more` ; **`what_unseen_pages_hold_changes_no_order`** (R916 : deux blocs permis dosés en `t1` et `t2` ; l'index reçoit tour à tour dix mille entrées non permises qui portent `t1`, puis `t2`, dans l'espace, dans un autre espace, puis supprimées sans fusion : même ordre à chaque fois ; son témoin : une note à statistiques d'ensemble, posée le temps de la preuve, inverse l'ordre) ; **`equal_notes_keep_their_order_whatever_the_index_holds`** (R922 : des entrées permises de même note, dans une page et entre pages ; l'index est bâti deux fois, les entrées permises écrites dans un autre ordre et mêlées d'entrées non permises, puis fusionné : mêmes pages, même ordre, même bloc rendu ; son témoin : un départage par adresse d'entrée change l'ordre ; c'est ce que l'essai de S0 ne voyait pas, les entrées permises y ayant les mêmes adresses partout) ; `a_block_inserted_before_another_changes_no_tie_break` (la clé de départage ne tient pas au rang du bloc) ; `the_title_entry_wins_a_tie_in_its_page` ; `no_search_path_reads_the_statistics_of_the_index` (test de sources, R923 : ni `Searcher::search`, ni `SnippetGenerator`, ni collecteur de Tantivy qui note, hors d'une liste fermée) ; `a_whole_word_outranks_a_longer_word` et `a_prefix_alone_ranks_titles_first_then_by_page` (R930, limite L-q) ; `a_prefix_costs_one_term_whatever_the_dictionary_holds` (le nombre de termes lus, compté, ne change pas quand cent mille mots d'un autre espace commencent pareil) ; `a_word_gives_at_most_nine_starts` (un mot de 40 octets : 9 termes, comptés) ; `a_prefix_of_ten_characters_is_one_exact_term` ; `a_longer_prefix_is_checked_on_the_kept_text` (« développem » ne trouve pas « développeur », qui partage ses 10 premiers caractères ; son témoin le trouve par « développe ») ; `the_check_of_a_long_prefix_is_bounded_and_counts_allowed_entries_only` (compteur : 2 000 au plus, et le même que des entrées non permises partagent le préfixe ou non) ; `a_long_prefix_in_a_title_is_checked_on_the_kept_title` (R929 : « développem » ne trouve pas une page titrée « Développeurs », la trouve titrée « Développement » ; un mot du bloc et le préfixe long dans le titre trouvent la page) ; `the_checked_entries_do_not_depend_on_the_order_of_the_index` (le même index fusionné autrement : mêmes entrées vérifiées) ; `a_page_of_10000_candidates_does_not_take_all_the_checks` (20 par page : trente autres pages qui portent le préfixe sortent). Mots (R926) : `text_in_nfd_is_found_like_text_in_nfc` (« élève » écrit en accents combinants, dans le texte puis dans la requête) ; `a_word_of_40_bytes_is_indexed_and_a_longer_one_is_not` ; `known_limits::a_run_of_cjk_is_one_word`. Écrire : `a_removed_entry_is_no_longer_found` ; `putting_an_entry_again_replaces_it` ; `removing_a_page_removes_all_its_entries`. Extrait (bâti par programme sur le texte de l'entrée, R923) : `the_excerpt_is_text_with_utf16_marks` (un emoji avant le mot : les plages tombent juste dans une chaîne JavaScript ; aucune balise dans `excerpt`) ; `the_excerpt_is_bounded`. Jeton : `the_token_of_the_last_commit_is_read_back` ; `a_commit_cut_short_reads_the_token_before_it` ; `a_missing_unreadable_or_other_schema_directory_reads_no_token` ; `reset_removes_a_link_without_following_it` (une jonction sous Windows, un lien sous Unix : ce qu'elle vise reste) ; `a_second_writer_is_refused_with_a_message` (le verrou, pour `mementum reindex`). Linéarité : `search` sur un index d'une taille et de huit fois cette taille ; **`put_entry` sur la forme hostile de R920** (un texte de mots de 40 octets tous différents, à une taille et à huit fois cette taille : temps, octets alloués et nombre de termes écrits croissent comme l'entrée, sous les bornes absolues de R433) ; un préfixe long partagé par toutes les entrées.
- [ ] **Step 2 : écrire**, relancer. Commit `recherche : index plein texte (R900, R902, R916, R920 à R923, R926, R929, R930)`.
- [ ] **Step 3 : mesures** (`#[ignore]`, release, une à la fois, lignes `MESURE` au registre ; le corpus de `spike/search-bench/src/corpus.rs` peut être repris) : `measure_the_index_by_field` (`space_usage` : le titre gardé, les quatre colonnes rapides, puis l'index sans texte gardé, le repli de L18) ; `measure_a_common_last_word` (R930 : un mot très courant cherché comme mot entier et comme début, 10 000 pages permises) ; `measure_the_heap_of_hostile_blocks` (300 blocs de 64 Kio de mots de 40 puis de 10 octets : taille par champ, débit, pointe du tas relevée à part de la mémoire de travail, sous la politique de fusion par défaut puis sous une taille maximale de segment fusionné : RO-1) ; `measure_a_resting_writer_of_50_mb` (R925 : le tas après une validation). Ce qu'elles disent va au registre ; aucune n'est un critère de refus, sauf si le titre gardé pèse plus que le titre brut (repli de R929).

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** grouper par page après `skip` et `limit` (le test des 10 000 blocs rougit) ; appliquer le filtre après le groupement ; départager par l'adresse d'une entrée ; départager par le rang du bloc ; vérifier un préfixe long dans l'ordre de l'index ; ne vérifier que `text` ; oublier la normalisation NFC ; rendre la note du moteur telle quelle (le test de l'ordre rougit) ; servir le préfixe par le dictionnaire ; indexer tous les débuts d'un mot ; rendre sans vérifier une entrée trouvée par les 10 premiers caractères d'un préfixe plus long ; vérifier avant le filtre ; compter les plages en octets ; rendre le HTML d'extrait du moteur ; suivre un lien en vidant le dossier.

> **De la Task S2 et de sa relecture (2026-11-20, R942, R943, R947)** : l'analyseur de l'index se pose sur `mementum_search::indexed_words` (les mots de 40 octets au plus tels qu'écrits, pliés, jamais vides), pas sur les filtres de Tantivy nommés plus haut ; **un mot plié peut dépasser 40 octets** (« Ⱥ », deux octets, se plie en trois : 60 octets pour un mot de 40), et les débuts de mot se comptent en caractères du mot plié ; les termes d'une `Query` sont déjà pliés, sans répétition et jamais vides, et se donnent tels quels ; un titre lu dans `nodes` passe par `nfc` avant `put_entry` ; une ligne de `blocks_index` au texte vide (des liens seuls, R944) n'a pas d'entrée.

**Pièges :** trouvés par S0 en lisant les sources de Tantivy 0.27 : `SnippetGenerator` lit la fréquence des termes dans l'index, `Searcher::search` prend les statistiques de l'index, `TermSetQuery` sur des milliers de pages coûte des dizaines à des centaines de millisecondes, `RemoveLongFilter::limit(n)` écarte un mot de `n` octets, le tampon de l'écrivain a un minimum de 15 Mo ; jamais d'analyseur de requêtes du moteur sur le texte d'une personne ; un test qui affirme une absence a son témoin ; sous Windows, un fichier ouvert ne se supprime pas (vider le dossier demande d'avoir lâché l'index : `TestDir`), un lien symbolique demande un privilège que le poste n'a pas (une jonction, `link_dir`), et un nom qui ne diffère que par la casse est le même fichier ; un dossier temporaire par tâche.

### Task S4a : `mementum-sync`, les pages écrites et la lecture seule d'une page gardée

**Dépend de :** rien dans ce plan (peut partir après S1, pour ne pas croiser ses fichiers). **Livrable :** deux fonctions de `mementum-sync` ; rien ne les appelle encore.

**Files:**
- Modify: `crates/mementum-sync/src/hub.rs` (`Hub::touched`, `Hub::take_touched`, `Hub::touched_changed`), `actor.rs` (`persist`, `persist_restore` ; `stored_page`, bâtie sur `replay`), `lib.rs`
- Create: `crates/mementum-sync/tests/stored_page.rs`, `tests/touched.rs` ; `crates/mementum-sync/tests/common/clock.rs` (**`StoppedClock`, déplacé de `tests/versions.rs`**, où il est privé aujourd'hui, pour que les tests du serveur l'incluent par son chemin, comme `TestDir`)
- Modify: `crates/mementum-sync/tests/versions.rs` (prend l'aide commune)

**Interfaces (produites) :**
- `Hub::take_touched() -> Vec<Uuid>` et une notification sans contenu quand l'ensemble passe de vide à non vide.
- `mementum_sync::stored_page(store, node_id) -> Result<Option<(PageDoc, i64)>, SyncError>` : la page que le store garde et son dernier `seq`, relue comme `load` la relit (mêmes entrées écartées), **sans rien écrire** (`None` pour une page sans document), sans ligne `error` (niveau `debug` : l'acteur a déjà dit ces entrées). Synchrone dans sa partie qui calcule : l'appelant la mène sur le fil bloquant.

- [ ] **Step 1 : tests, vus rouges.** `the_actor_only_names_the_page_it_wrote` (après mille frappes, l'ensemble porte un identifiant ; `Hub::block_reads` n'a pas bougé ; aucun encodage de plus, compté) ; `a_write_of_the_server_and_a_restore_name_the_page` ; `a_refused_update_names_nothing`, `an_update_that_changes_nothing_names_nothing` (R181) ; `the_set_is_bounded_by_the_pages_not_by_the_keystrokes` ; `stored_page_reads_what_the_actor_serves` (propriété : après chaque étape d'un scénario de `tests/reload.rs`, mêmes blocs que `Hub::read_blocks`) ; `stored_page_writes_nothing` (une page sans document reste sans instantané ; aucun tour d'écriture pris, tour tenu par `hold_writes`) ; `stored_page_sets_aside_what_load_sets_aside` ; `stored_page_logs_no_error` (`Logs::capture`, avec son témoin : `load` en écrit une) ; les tests de `versions.rs` restent verts avec l'aide déplacée.
- [ ] **Step 2 : écrire**, relancer ces tests et ceux de `mementum-sync` que le changement touche (`tests/versions.rs`, dont l'aide est déplacée ; les tests unitaires de `actor.rs` et de `hub.rs`), pas toute la suite du crate (R935). Commit `sync : pages écrites et lecture seule d'une page gardée (R907)`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** nommer la page avant l'écriture au journal ; nommer à chaque message reçu, écrit ou non ; `stored_page` qui écrit l'instantané d'une page sans document ; `stored_page` qui garde une entrée que `load` écarte.

**Pièges :** yrs, `get_or_insert_*` avant d'ouvrir une transaction ; une mise à jour de client qui ne change rien n'est ni écrite ni relayée (R181) ; test qui lit le journal : `Logs::capture` ; toute lecture neuve d'une mise à jour passe par `PageDoc::read` (R784) : `stored_page` réutilise `replay`, il n'en refait rien ; un test qui lit le store après une compaction l'attend (`Hub::compacted`).

### Task S4b : l'indexeur, jusqu'à `blocks_index`

**Dépend de :** S1, S2, S4a. **Livrable :** une page écrite a sa projection à jour dans la base, sans que la frappe le paie. L'index plein texte n'est pas encore nourri.

**Files:**
- Create: `crates/mementum-server/src/indexer.rs`, `crates/mementum-server/tests/indexer.rs`
- Modify: `crates/mementum-server/src/lib.rs` (`build`, `serve`, `AppState`), `config.rs` (`index_after`, `index_at_most`, `MEMENTUM_INDEX_AFTER_MS` pour les e2e), `crates/mementum-server/Cargo.toml` (**dev-dépendance `tokio` avec `test-util`**, que seul `mementum-sync` a aujourd'hui), `tests/common/mod.rs` (inclut `clock.rs` par son chemin), `tests/journal.rs`

> **De la relecture de S2 (2026-11-20, R960)** : `mementum_search::PROJECTION_VERSION` (1 aujourd'hui) avance à tout changement de la projection ou du pliage des mots. S4b la garde avec ce qu'il a indexé (dans `search_state` ou `instance_settings`, à arrêter avec S1) et, au démarrage, quand la version gardée n'est pas celle du binaire, remet toutes les pages à `indexed_seq = -1` et tout à l'outbox : sans cela, les lignes dont l'empreinte n'a pas changé gardent l'ancienne forme. Test à écrire ici : `a_database_indexed_under_another_projection_version_is_indexed_again`. Le `kind` d'une ligne relue de `blocks_index` se refait par `mementum_search::kind_named` (`None` : une ligne d'une autre version, à reprojeter) ; `mementum-search` est à inscrire à `[workspace.dependencies]` (R946).

**Interfaces (produites) :** `Indexer::spawn(state) -> IndexerHandle`, `IndexerHandle::behind(space_id, seen: &PageSet) -> bool`, `IndexerHandle::settled()` (pour les tests : plus rien à projeter), `stop()`.

- [ ] **Step 0 : la mesure d'abord** (`#[ignore]`, release, `alone`) : `measure_the_write_turn_held_by_an_index_transaction`, sur `Store::index_page` seul, à 64 Kio, 256 Kio, 1 Mio et 4 Mio de lignes changées : le tour tenu au p95, et le nombre de points de contrôle du WAL déclenchés. Elle arrête `MAX_INDEX_TX_BYTES` (départ : 256 Kio) avant que l'indexeur soit écrit ; sa ligne `MESURE` va au registre.
- [ ] **Step 1 : tests, vus rouges.** `a_typed_word_reaches_the_index_tables` ; `erased_text_leaves_the_index` ; `a_page_written_by_put_mmd_is_indexed`, `…restored…` ; `a_created_page_and_a_duplicated_page_are_indexed_without_any_actor` (par les lignes à `-1`, R918 ; `Hub::calls` n'a pas bougé) ; `a_trashed_page_stays_indexed` ; `an_existing_instance_is_indexed_after_startup` (base du lot 3 remplie par un autre `Store`) ; `a_page_that_cannot_be_read_is_tried_once_and_leaves_the_index` (compte des lectures ; ses lignes d'avant sont retirées) ; `a_page_written_without_pause_is_indexed_at_most_every_bound` (`StoppedClock`) ; `a_keystroke_in_a_10000_block_page_writes_one_row` (déclencheur témoin) ; `pages_take_turns` (une page écrite sans relâche n'empêche pas une autre d'être indexée) ; `indexing_leaves_the_engine_free` (sur le modèle d'`engine_stays_free`) ; `the_indexer_never_asks_an_actor` (`Hub::calls`) ; `the_indexer_stops_before_the_store_closes` ; `entries_set_aside_are_logged_once` (le test de `journal.rs` qui compte les lignes `error` garde son compte avec l'indexeur lancé).
- [ ] **Step 2 : écrire**, relancer `tests/indexer.rs` et `tests/journal.rs`, pas toutes les suites de `mementum-server` (R935). Commit `serveur : indexeur des pages, jusqu'aux tables (R906, R907)`.
- [ ] **Step 3 : mesures** (`#[ignore]`, release, `alone`) : `measure_indexing_a_10000_block_page` (lecture, projection, transactions) ; `measure_first_indexing_of_10000_pages` ; `keystroke_seen_while_10000_pages_are_first_indexed` (la frappe d'un autre, p95, pendant la première indexation : c'est la mesure que X2 porte dans la CI).

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** lire les blocs par l'acteur (`Hub::read_blocks`) ; indexer sans attendre le calme ; poser `indexed_seq` au `seq` courant plutôt qu'au `seq` lu ; oublier les lignes à `-1` (la page dupliquée) ; oublier le rattrapage (l'instance existante) ; garder les lignes d'une page devenue illisible.

**Pièges (sortis de S4a et de sa relecture ; détail dans `CLAUDE.md`, « Task S4a ») :** une page prise par `Hub::take_touched` n'a plus de trace dans le hub : l'indexeur garde ses pages en attente dans son propre ensemble, et réessaie après une erreur ; `stored_page` rend `Ok(None)` pour une page inconnue comme pour une page sans document : la page sort quand même de `-1` ; `Hub::touched_changed()` n'est réveillé par rien à l'arrêt : l'indexeur a son propre signal d'arrêt dans son `select!` ; `stored_page` gèle sa tâche le temps du calcul (les autres branches d'un `select!` de la même tâche ne tournent pas) : l'appeler dans une tâche à elle.

**Pièges :** une tâche lancée par `tokio::spawn` perd la portée `writes_by` (l'indexeur écrit dans `Lane::Index`, nommée par le store, pas par la portée) ; ne jamais mener deux écritures du store ensemble dans une tâche (R518) ; un calcul long rend son fil au moteur, et `block_in_place` panique sur un moteur à un fil (R863) ; `Store::changes()` : s'abonner avant de lire, relire après `Lagged`, une annonce de trop est possible, une écriture faite par un autre `Store` n'est pas annoncée (R133) ; test qui suppose la durée d'une écriture : tenir le tour (`hold_writes`), ne pas supposer (R886) ; `StoppedClock` et ses règles (R179 : `test-util`, n'avancer l'horloge qu'une fois le serveur au repos, borner les attentes en temps réel) ; une rafale d'écritures prend le temps du disque (R441) ; le point de contrôle du WAL tombe sur l'écriture qui le déclenche (R447).

### Task S4c : l'indexeur, l'index plein texte et le démarrage

**Dépend de :** S3, S4b. **Livrable :** ce que la base a projeté devient trouvable ; l'index et la base ne se séparent jamais sans que le démarrage le voie.

**Files:**
- Modify: `crates/mementum-server/src/indexer.rs`, `lib.rs` (`build` : ouverture de l'index, comparaison des jetons), `tests/indexer.rs`
- Create: `crates/mementum-server/tests/index_recovery.rs`

- [ ] **Step 1 : tests, vus rouges.** Nourrir : `a_typed_word_becomes_findable` ; `a_keystroke_in_a_10000_block_page_rewrites_one_entry` (R919 : les appels à `put_entry` sont comptés) ; `a_rename_is_found_by_the_new_title_and_no_longer_by_the_old` ; `a_deleted_page_leaves_the_text_index` ; `a_page_that_can_no_longer_be_read_leaves_the_text_index` (trouvée avant ; son journal rendu illisible et son `seq` avancé : plus aucun résultat ni extrait d'elle, par la ligne d'outbox de page entière d'`index_failed`) ; `a_line_changed_during_a_commit_is_fed_again`. Clôture refusée (`test-hooks` : la prochaine clôture échoue `n` fois) : `a_refused_close_is_replayed_and_closed_again_without_a_restart` (aucune intention neuve entre-temps, comptées ; le mot écrit pendant le refus est trouvé après) ; `a_refused_commit_undoes_the_intent_and_is_tried_again`. Arrêts (l'indexeur arrêté à un point nommé, par une fonction `test-hooks`, jamais par un minutage) : `a_stop_before_the_commit_undoes_the_intent` ; `a_stop_between_the_commit_and_the_close_replays_the_intent` ; **`a_rename_before_a_crash_is_caught_up`** (renommage validé, serveur arrêté avant toute validation de l'index, relancé : l'ancien titre ne trouve plus la page, sans aucune écriture de plus) ; `a_keystroke_before_a_crash_is_caught_up`. Base et index séparés : **`a_copy_then_one_commit_then_the_copy_put_back_rebuilds`** (copie de la base ; un mot écrit et **une seule** validation ; la copie remise en place, `search/` gardé : au démarrage tout est reconstruit, et le mot n'est dans aucun extrait ; témoin avant la remise : il est trouvé) ; le même avec cent validations ; `a_copy_taken_between_the_intent_and_the_close_replays_from_the_database` (le texte de la base remise en place, pas celui validé) ; `a_directory_of_another_instance_is_rebuilt` ; `a_backup_put_back_before_0005_rebuilds_after_migrating_again` ; `searching_during_a_rebuild_finds_only_what_is_rebuilt`.
- [ ] **Step 2 : écrire**, relancer. Commit `serveur : index plein texte nourri par intention, validation, clôture (R909, R919)`.
- [ ] **Step 3 : mesures** (`#[ignore]`, release, `alone`) : `measure_a_feed_of_one_block_and_of_a_renamed_10000_block_page` ; `measure_database_and_index_size_for_10000_pages` (la base avant et après, versions de l'historique comprises, le dossier `search/` : décision L18, R931 ; avec la mesure du repli faite en S3, c'est ici que le choix de garder le texte dans l'index se rouvre, au registre) ; `measure_startup_with_an_index_of_10000_pages` (les jetons comparés, rien d'attendu d'autre).

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** tenir pour faites les lignes d'une intention retrouvée validée ; garder l'index quand son jeton n'est aucun des deux de la base ; valider Tantivy avant d'écrire l'intention ; clore sans comparer `gen` ; récrire toute la page à chaque frappe.

**Pièges :** on ne restaure ni ne récrit une base sous un serveur qui tourne : ces tests arrêtent le serveur, ferment le store (`Store::close`), puis copient ; une copie remise en place est reconnue au compteur de schéma près, et toute comparaison de copies passe par `same_content` (R841) ; un test qui fait tomber un événement « pendant » une écriture en suppose la durée (R886) : un point d'arrêt nommé ; démarrage à froid < 1 s : ni la reconstruction ni le rattrapage ne sont attendus par `serve`.

### Task S5 : la route de recherche

**Dépend de :** S3, S4c. **Livrable :** `POST /api/v1/spaces/{id}/search`.

**Files:**
- Create: `crates/mementum-server/src/routes/search.rs`, `src/searches.rs`, `tests/search.rs`
- Modify: `crates/mementum-server/src/lib.rs` (routeur, tableau des routes), `types.rs`, `tests/common/mod.rs`, `tests/spaces.rs`, `tests/expected_account.rs`

| Route | Il faut | Réponse |
|---|---|---|
| `POST /api/v1/spaces/{id}/search`, corps `SearchRequest { q: string, after?: number }` | un rôle | `SearchResults`, pas avant `SEARCH_FLOOR` (R932) ; `400` ou `422 invalid_json` (comme toute route JSON) ; `422 invalid_query` ; `429 search_busy` (`Retry-After: 1`) |

La route n'écrit rien : c'est un `POST` pour que la requête ne soit dans aucune adresse (R917). Elle ne passe pas par `to_completion` (rien à mener au bout), mais son calcul, lancé, va au bout sur son fil et garde sa place au débit jusque-là.

- [ ] **Step 1 : tests, vus rouges.** Droits : `a_word_only_an_unseen_page_holds_answers_like_a_word_nobody_wrote` (corps égaux, octet pour octet, `indexing` compris) ; `a_guest_finds_only_the_pages_he_was_invited_on` ; `a_private_page_is_found_by_its_owner_only` ; `withdrawn_access_hides_the_page_at_the_next_query`, `a_page_closed_to_members…`, `a_member_removed_from_the_space_gets_404` (l'indexeur arrêté : c'est la requête qui juge) ; `a_trashed_page_is_not_found_and_comes_back_restored` ; `a_subpage_of_a_trashed_page_is_not_found` ; `the_breadcrumb_names_only_seen_ancestors` (un invité sur une sous-page : le fil commence à elle) ; `the_title_is_the_current_one` (renommée, indexeur arrêté) ; `indexing_says_nothing_of_unseen_pages` (une page non vue en retard : `indexing` faux pour l'invité, vrai pour le membre) ; `two_dosed_blocks_keep_their_order_whatever_others_write` (par la route, à deux comptes : la preuve de R916 de bout en bout) ; `space_routes_by_role` gagne la route ; la matrice du compte attendu aussi. Bornes : `a_query_over_the_bounds_is_422`, `a_body_over_its_bound_is_refused_without_being_read_whole` ; `an_unreadable_body_in_a_space_without_role_is_404_not_422` (l'ordre des refus : même corps illisible, `404` sans rôle, `422` avec) ; `a_refusal_reaches_a_client_that_keeps_sending` (vraie connexion, `push_then_read`) ; `an_empty_query_answers_empty_without_computing` (compteur de calculs) ; `paging_stops_at_the_bound`, `paging_never_repeats_a_page` (par la route : deux requêtes, `after` 0 puis 20) ; `the_query_is_never_logged_and_never_in_an_address` (`all_logs`, la requête porte un mot témoin ; son témoin : un autre point de journal de la requête est vu ; un `GET` sur la route répond `405`). Débit : `a_newer_search_takes_the_place_of_the_one_waiting` ; `a_burst_of_searches_of_one_account_does_not_delay_another` (une place sur quatre laissée libre, l'ordre de service lu, sans minutage) ; `an_abandoned_search_keeps_its_place_until_its_computation_ends` (R630) ; **`the_allowed_set_is_built_under_the_guard_off_the_engine`** (I7 : les quatre places tenues, mille requêtes d'un compte arrivent ; aucun ensemble n'est bâti, compté ; et un moteur à un fil reste libre pendant qu'un ensemble de 100 000 pages se bâtit, sur le modèle d'`engine_stays_free`). Plancher (R932, horloge arrêtée, `StoppedClock`) : `an_answer_never_leaves_before_the_floor` (un mot que rien ne porte et un mot que cent entrées non permises portent : aucune réponse tant que l'horloge n'a pas passé le plancher, les deux au même instant) ; `the_account_keeps_its_place_until_the_answer_leaves` (une seconde requête du compte, envoyée pendant l'attente du plancher, ne commence son calcul qu'au départ de la première ; compteur de calculs) ; `the_floor_holds_no_place_of_the_instance` (un autre compte calcule pendant l'attente) ; `an_empty_query_and_a_refusal_do_not_wait_for_the_floor`. En-têtes : `no-store`, `nosniff`.
- [ ] **Step 2 : écrire**, régénérer les types (`packages/protocol`), relancer. Commit `serveur : recherche dans un espace (R903 à R905, R910, R914, R917)`. En-tête de `lib.rs`, registre.
- [ ] **Step 3 : mesure** (`#[ignore]`, release) : `measure_search_on_10000_pages` (membre qui voit tout, invité qui voit dix pages ; un, trois, huit termes, et un mot très courant seul, R930 ; p95 du calcul, plancher à part ; **la part de l'ensemble permis à part**, pour 10 000 et 100 000 pages : au-delà de 10 ms au p95, un cache par `(espace, compte, rev)` est à écrire, R927), ligne `MESURE`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** filtrer sans `in_trash` ; bâtir le fil d'Ariane de `parent_id` au lieu de `visible_parent` ; lire le titre dans l'index ; calculer `indexing` sur tout l'espace ; rendre la garde du débit à l'abandon de la requête ; bâtir l'ensemble permis avant de prendre sa place ; répondre avant le plancher ; rendre la place du compte à la fin du calcul ; attendre le plancher sur le fil bloquant ; appeler l'index avant `space_access` ; accepter `q` dans l'adresse.

> **De la relecture de S2 (2026-11-20, R941, R960)** : `blockId` est un texte venu d'ailleurs, cent octets quelconques choisis par un client (pas forcément un UUID : espaces, guillemets, caractères de contrôle ou de direction) ; la route le rend tel quel en JSON, ne le met dans aucun journal ni aucun chemin, et rend `null` pour une clé de remplacement (`mementum_search::is_stand_in`).

**Pièges :** les droits par `access.rs` seulement ; la session par l'extracteur `AuthUser` ; qui n'a aucun rôle reçoit sa réponse sans que l'espace soit lu (R99) ; toute garde de débit suit le travail qu'elle borne (R630) ; un test de débit abandonne la requête pendant un calcul qui tourne ; un refus rendu avant de lire un corps se perd chez un client qui envoie sans attendre (R505) : le corps est petit et borné, il est reçu avant de refuser un compte, et jugé après les droits (`Result<Json<…>, JsonRejection>`) ; compter les requêtes SQL d'une route demande un binaire à lui (R266) ; messages du serveur affichés tels quels, ils tutoient ; aucune écriture ici : ne pas introduire de `StoreError::Stale` ; un seuil « N pendant M » finit par échouer : attendre une condition.

### Task S6 : la route des rétroliens

**Dépend de :** S1, S4b. **Livrable :** `GET /api/v1/pages/{id}/backlinks`.

**Files:**
- Create: `crates/mementum-server/src/routes/backlinks.rs`, `tests/backlinks.rs`
- Modify: `lib.rs`, `types.rs`, `tests/common/mod.rs` (`page_routes`), `tests/permissions.rs`

- [ ] **Step 1 : tests, vus rouges.** `a_page_link_a_mention_and_a_link_in_a_text_are_backlinks` ; `a_backlink_from_an_unseen_page_is_not_listed` et son témoin (l'accès donné, elle paraît) ; `a_backlink_from_a_trashed_page_is_not_listed` ; `a_backlink_from_another_space_is_not_listed` ; `a_page_linking_twice_is_listed_once` (le premier bloc) ; `a_link_removed_leaves_the_backlinks` ; `a_page_does_not_backlink_itself` ; **`unseen_sources_never_hide_seen_ones`** (300 sources non vues dont les identifiants passent avant ceux de 3 sources vues : les 3 sortent ; c'est le test qui distingue « borner puis trier » de « trier puis borner ») ; `the_answer_is_the_same_whatever_unseen_sources_exist` (les 3 vues seules, puis avec les 300 : corps égaux) ; `two_hundred_seen_sources_at_most` ; `the_walk_stops_at_its_bound` (plafond réglé bas pour le test : la lecture s'arrête, compte des lots) ; `sources_of_another_space_are_not_even_read` (compte des lignes ramenées) ; `the_excerpt_is_bounded_by_the_database` ; `foreign_page_is_indistinguishable_from_missing` gagne la route ; `a_reader_gets_the_backlinks`, `…in_the_trash_too`.
- [ ] **Step 2 : écrire**, régénérer les types, relancer. Commit `serveur : rétroliens d'une page`. En-tête de `lib.rs`.
- [ ] **Step 3 : mesure** (`#[ignore]`, release) : `measure_backlinks_at_the_walk_bound` (20 000 sources non vues, puis 200 vues) : elle dit si la route demande un débit à elle.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** rendre les sources sans les trier par `space_access` ; trier par l'accès à la page visée seulement ; borner avant de trier ; dire dans la réponse que le parcours s'est arrêté ; ramener le texte entier du bloc.

**Pièges :** « ne jamais lister sans ce tri » (la leçon des favoris, R280) ; un id qu'on ne voit pas ne sort par aucune route neuve ; l'extrait vient de l'index, qui peut porter un titre copié (R251) : c'est le texte de la page source, que le lecteur lit déjà ; une requête reprend l'expression exacte de son index.

### Task S7 : `mementum reindex`

**Dépend de :** S4c. **Livrable :** la commande de la conception (§5.7).

**Files:**
- Modify: `crates/mementum-cli/src/main.rs`
- Test: `crates/mementum-cli/tests/` (un fichier neuf)

> **De la relecture de S2 (2026-11-20, R960)** : `mementum reindex` écrit `PROJECTION_VERSION` avec ce qu'il a reconstruit, comme l'indexeur (S4b) ; `a_server_started_after_reindex_rebuilds_nothing` le couvre (une version gardée qui manquerait ferait tout reconstruire au démarrage).

- [ ] **Step 1 : tests, vus rouges.** `reindex_rebuilds_everything_from_the_documents` (tables vidées à la main, dossier supprimé : après la commande, mêmes résultats qu'avant) ; `reindex_text_only_rebuilds_the_text_index_from_the_tables` ; `reindex_refuses_while_a_server_holds_the_index` (message en français, base et index intacts) ; `reindex_opens_the_database_by_the_backup_rule` (la commande passe par `Store::open`, donc par `backup.rs` : une base à migrer est copiée d'abord, une base refusée reste intacte) ; `a_server_started_after_reindex_rebuilds_nothing` (les jetons concordent).
- [ ] **Step 2 : écrire**, relancer. Commit `cli : mementum reindex (R913)`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** reconstruire depuis les documents quand `--text-only` ; ouvrir un second écrivain ; laisser les jetons de la base sans celui de l'index reconstruit.

**Pièges :** toute lecture neuve d'une base avant de l'accepter passe par une connexion sans réglage qui écrive (R840, R845) : passer par `Store::open` ; une commande dans un autre processus dispute le verrou de SQLite par `busy_timeout` ; un `mementum` lancé depuis `target/` empêche la recompilation sous Windows.

---

## Piste C

### Task C1 : la fenêtre de recherche

**Dépend de :** S5. **Livrable :** on ouvre la recherche, on tape, on choisit au clavier, on arrive sur la page.

**Files:**
- Create: `apps/web/src/search/SearchDialog.tsx`, `useSearch.ts`, `shortcut.ts`, `marks.ts`, leurs tests ; `apps/web/e2e/search.spec.ts`
- Modify: `apps/web/src/api.ts` (`searchSpace` ; `silence` : une liste des `POST` qui ne font que lire), `api.test.ts`, `App.tsx`, `sidebar/Sidebar.tsx`, `theme.css`, `e2e/helpers.ts` (`waitFindable`), `e2e/servers.ts` (`MEMENTUM_INDEX_AFTER_MS`)

Comportement :
- Une entrée « Rechercher » sous le sélecteur d'espace, avec son raccourci écrit ; le raccourci l'ouvre de partout (décision L5 ; le plan suppose Ctrl+K, laissé à l'éditeur quand sa sélection n'est pas vide).
- `searchSpace` est un `POST` (R917) qui ne fait que lire : `silence` (`api.ts`) le nomme parmi les lectures, pour qu'une réponse perdue dise « le serveur ne répond pas », jamais « ce que tu as demandé a peut-être été fait » (R757) ; il passe par `send` comme tout appel, et un `401` tardif le rejoue comme une lecture.
- La fenêtre (`<dialog>`) est chargée à la demande. Champ vide : les pages récentes (ce que la barre montre déjà). Dès la frappe : d'abord les pages de l'arbre dont le titre contient la requête (`findPages`, sans réseau), puis, 150 ms après la dernière frappe, les résultats du serveur ; une réponse d'une requête qui n'est plus la courante est jetée, une requête en route est abandonnée (`AbortSignal`).
- Une page trouvée des deux côtés n'a qu'une ligne. La ligne active est tenue par l'identifiant de la page ; l'arrivée des résultats du serveur ne la change pas, et ne déplace rien sous le pointeur (les lignes déjà montrées gardent leur rang, les autres s'ajoutent dessous).
- Chaque ligne : le titre, le fil d'Ariane, l'extrait avec ses mots surlignés (`<mark>` autour de nœuds de texte, d'après `marks`). Flèches, Entrée (ouvre), Ctrl+Entrée (nouvel onglet : chaque ligne est un vrai `<a href>`), Échap (ferme, le focus revient d'où il venait).
- États dits : « Aucun résultat », « L'indexation est en cours : des pages peuvent manquer » (`indexing`), un refus du serveur tel qu'il le dit, la session perdue (la connexion est demandée, la requête repart au retour du compte).

- [ ] **Step 1 : tests, vus rouges.** Vitest : `titles_of_the_tree_come_first_and_at_once` ; `a_late_answer_of_an_older_query_is_dropped` ; `the_active_line_is_held_by_its_id_when_results_arrive` ; `nothing_moves_under_the_pointer_when_results_arrive` ; `a_page_found_twice_has_one_line` ; `marks_wrap_text_nodes_only` (un extrait `<img onerror=…>` s'affiche en texte ; une plage hors du texte ou qui coupe une paire UTF-16 est ignorée) ; `escape_closes_and_gives_the_focus_back`, `escape_in_the_window_closes_nothing_under_it` (`event.target === event.currentTarget`) ; `the_shortcut_is_read_from_the_key_typed` (AZERTY, AltGr, sous une autre `dialog`, dans un champ) ; `search_busy_is_not_shown_for_a_replaced_query` ; `a_lost_answer_of_a_search_is_said_as_a_reading` (`api.test.ts` : statut 0 et `502` sans corps, le message est celui d'une lecture, et `answerLost` ne fait rien dire d'une écriture) ; `the_query_is_never_put_in_an_address` (l'adresse appelée ne porte que l'espace) ; `a_lost_session_asks_to_log_in_and_searches_again`. E2e : `typed_word_is_found_and_opened` ; `searching_at_the_keyboard_only` ; `the_shortcut_in_the_editor_with_a_selection_is_left_to_the_editor` (du texte sélectionné : la barre de lien de BlockNote s'ouvre, pas la fenêtre ; témoin : sans sélection, la fenêtre s'ouvre, et aucun lien n'est créé ni proposé ; la preuve lit la sélection de l'éditeur et `defaultPrevented` par un écouteur posé sur `window`, sans rien supposer de l'écouteur de BlockNote, dont l'ordre avec le nôtre est justement ce qu'on éprouve) ; `opening_the_application_asks_nothing_of_search` (compte des requêtes).
- [ ] **Step 2 : écrire**, relancer les tests Vitest de l'ajout et `api.test.ts`, les types, `e2e/search.spec.ts` seul (ports de la piste C ; pas la suite de bout en bout entière, R935). Commit `client : fenêtre de recherche`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** garder la ligne active par son rang ; afficher l'extrait par `innerHTML` ; reconnaître le raccourci par `event.code` ; ne pas jeter la réponse en retard ; charger la fenêtre au démarrage.

**Pièges :** `onCancel` et `onClose` d'un `<dialog>` remontent dans l'arbre de React ; le focus ne tombe jamais sur `body` (un élément retiré sous le focus, R339) ; une liste qui suit le réseau tient sa ligne par l'id (R572) ; rien ne bouge sous le pointeur (R296, R411 : le suivre au niveau du document) ; un raccourci se lit à `event.key`, se tait sous une `dialog` ouverte, ne prend jamais un caractère tapé avec AltGr (R176, R214) ; un raccourci qui double celui de l'éditeur ou du navigateur (R634) : BlockNote traite Ctrl+K sur l'élément de l'éditeur sans arrêter la propagation, si bien qu'un écouteur du document le reçoit **après** lui, déjà annulé ou non : décider d'après la sélection de l'éditeur, pas d'après l'ordre des écouteurs ; toute fonction neuve d'`api.ts` passe par `send` ; une liste qui trouve la session perdue rend `SESSION_LOST` et prend `afterLogin` (R212) ; clés React à soi dans la barre (R290) ; un e2e qui cherche une ligne de la barre prend un compte neuf ; les pages d'un e2e se créent avant d'ouvrir les onglets ; un e2e attend que la page soit trouvable par une condition (`waitFindable`), pas par une durée ; l'outil d'écriture change `⁨` en caractère réel (`String.fromCharCode`) ; ne pas éditer `apps/web/src` pendant une suite e2e.

### Task C2 : arriver sur le bloc trouvé

**Dépend de :** C1. **Livrable :** ouvrir un résultat mène au bloc, pour qui modifie comme pour qui lit.

**Files:**
- Modify: `apps/web/src/router.ts` (un fragment `#b=<id>` sur `/p/:id` : `navigate`, `useLinkInterception`, et un `useBlockTarget` à côté de `usePathname`), `App.tsx`, `page/PageView.tsx`, `editor/Editor.tsx` ou un module neuf `editor/goToBlock.ts` (tiré de `goToHeading`, `Toc.tsx`), `search/SearchDialog.tsx`
- Test: `router.test.ts`, `page/PageView.test.tsx`, `e2e/search.spec.ts`

Le routeur d'aujourd'hui jette le fragment à trois endroits (lu dans `router.ts`) : `navigate` ne fait rien quand le chemin et la requête sont déjà ceux de l'adresse, `useLinkInterception` ne passe que `pathname + search`, et `usePathname` ne rend que `location.pathname`. C2 les change tous les trois : `navigate` compare aussi le fragment ; l'interception le garde pour une page (elle laisse toujours au navigateur un lien d'invitation, dont le jeton est dans le fragment) ; `useBlockTarget` rend le bloc visé et suit `popstate`, `hashchange` et l'événement de `navigate`, sans faire rendre de nouveau ce qui ne lit que le chemin.

- [ ] **Step 1 : tests, vus rouges.** `a_block_fragment_is_parsed_and_kept_out_of_the_page_id` ; `an_intercepted_link_keeps_its_fragment` ; `navigating_to_another_block_of_the_open_page_is_followed` (même page, autre bloc : l'adresse change, le bloc est atteint, la page n'est pas remontée) ; `navigating_to_the_same_block_again_does_nothing` ; `an_invitation_link_is_still_left_to_the_browser` ; `back_returns_to_the_block_before` ; `a_fragment_that_is_not_a_block_is_ignored` ; `the_block_is_reached_after_the_first_sync_only` (jamais sur un document encore vide) ; `a_block_that_left_opens_the_page_at_its_top_and_says_nothing` ; `a_reader_is_taken_to_the_block_too` (éditeur en lecture seule : la sélection du document, comme `goToHeading`) ; `the_fragment_is_followed_once` (une relecture, une session recréée ne ramènent pas au bloc) ; `typing_after_arriving_is_not_lost` ; e2e `a_result_opens_on_its_block_in_a_long_page` (page de 1 000 blocs, le bloc trouvé est à l'écran) ; e2e `a_result_of_the_open_page_goes_to_its_block` (la page est déjà ouverte : sans la correction du routeur, rien ne se passe) ; budgets : `open_1000_block_page_under_300ms` reste vert.
- [ ] **Step 2 : écrire**, relancer `router.test.ts`, `PageView.test.tsx`, `e2e/search.spec.ts` et le seul budget `open_1000_block_page_under_300ms`. Commit `client : un résultat mène à son bloc`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** mener au bloc avant la première synchro ; le refaire à chaque rendu ; `editor.focus()` seul dans un éditeur en lecture seule ; `navigate` qui compare sans le fragment ; l'interception qui le jette ; un changement de bloc qui remonte `PageView` (le brouillon d'un commentaire en partirait).

> **De la relecture de S2 (2026-11-20, R941, R960)** : `blockId` est un texte venu d'ailleurs, cent octets quelconques (pas forcément un UUID) : il s'encode dans le fragment (`encodeURIComponent`) et se décode au retour, ne va jamais tel quel dans un sélecteur CSS (`CSS.escape`, ou une comparaison d'attribut faite en JavaScript), et un bloc introuvable ouvre la page en haut sans erreur. Si le client reconnaît les adresses de page, sa table de cas est `spec/search/projection.json`, « links » (R945).

**Pièges :** `setTextCursorPosition` ne tient pas dans un éditeur en lecture seule qui prend le focus ; `editor.focus()` n'y fait rien (R470) ; ne jamais relire des blocs un par un dans une boucle (R641) ; tout branchement neuf de `PageView` prend son test dans `PageView.test.tsx` ; `PageView` est rendu sous la clé de la page ; même id n'est pas même élément (R220).

### Task C3 : les rétroliens sous la page

**Dépend de :** S6, C2. **Livrable :** sous l'éditeur, « Pages qui mènent ici », pour qui lit la page.

**Files:**
- Create: `apps/web/src/page/Backlinks.tsx`, `useBacklinks.ts`, leurs tests ; `apps/web/e2e/backlinks.spec.ts`
- Modify: `apps/web/src/api.ts` (`getBacklinks`), `page/PageView.tsx`, `theme.css`

Comportement : rien n'est lu avant la première synchro de la page ; la section n'est rendue que s'il y a au moins une page ; elle est relue au retour sur l'onglet (la règle des cinq secondes) et au retour de la connexion, jamais à la frappe ; chaque ligne est un lien vers la page source, à son bloc (`#b=`), avec le titre de l'arbre (`usePageTitle`) et l'extrait ; rien n'y bouge sous le pointeur ni sous le focus.

- [ ] **Step 1 : tests, vus rouges.** Vitest : `backlinks_are_read_after_the_first_sync_only` ; `no_section_without_a_backlink` ; `the_list_is_held_under_the_pointer_and_the_focus` ; `a_refusal_is_said_and_the_page_is_read_again` (`404` : la page n'est plus vue) ; `a_lost_session_reads_again_after_login` ; `the_section_is_a_list_of_links` (`nav`, `ul`, `li`, nom accessible). E2e : `a_mention_makes_a_backlink` (à deux onglets : l'un mentionne, l'autre revient sur l'onglet de la page visée) ; `a_guest_sees_only_the_backlinks_of_pages_he_sees` ; `opening_a_page_asks_for_backlinks_after_its_first_sync` (ordre des requêtes).
- [ ] **Step 2 : écrire**, relancer ses tests Vitest et `PageView.test.tsx`, `e2e/backlinks.spec.ts`, et le budget neuf (une page visée par 200 pages s'ouvre sous son budget) ; les quatorze autres budgets du navigateur passent à la fin du lot (R935). Commit `client : rétroliens d'une page`.

**Preuves (mutations, jouées une fois, à la fin du lot : R935) :** lire au montage ; relire à chaque frappe ; montrer un titre gardé à la lecture plutôt que celui de l'arbre (la réponse n'en porte pas).

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

**Dépend de :** S4c, S5, C1. **Livrable :** les budgets du §5.10 mesurés avec l'indexeur, dans la CI.

**Files:** `crates/mementum-sync/tests/budgets.rs` ou `crates/mementum-server/tests/measure.rs`, `crates/mementum-cli/tests/` (mesures existantes), `apps/web/e2e/budgets.spec.ts`, `.github/workflows/ci.yml`

- [ ] **Step 1 : mesures** (`#[ignore]`, release, `alone`, lignes `MESURE`) : `keystroke_seen_under_150ms_while_a_space_is_being_indexed` (10 000 pages en retard, p95 au plus proche rang, la première frappe jugée à part, R488) ; `keystroke_seen_under_150ms_while_a_large_page_is_rewritten_in_a_loop` (une page de 10 000 blocs récrite par `PUT /mmd` puis renommée, sans relâche, par un compte ; la frappe d'un autre dans une autre page) ; `loading_a_10000_block_page_under_300ms_while_it_is_being_indexed` ; la mémoire au repos telle que R924 la juge : (a) la mesure existante de `mementum-cli` sur un dossier de données qui porte un index de 10 000 pages à jour ; (b) après 500 recherches, puis après un rattrapage qui indexe, la mémoire tenue en propre, lue par `RssAnon` de `/proc/self/status` sous Linux et par la mémoire privée sous Windows (jamais `virtual_memory()` de sysinfo, qui est la taille virtuelle sous Linux), sous 150 Mo ; le RSS entier, et la pointe pendant le rattrapage, pendant une reconstruction et pendant l'indexation d'une page de mots courts tous différents (RO-1), relevés à part, en lignes `MESURE` ; la même lecture (a) avec un tampon d'écrivain de 50 Mo puis de 15 (R925) ; le démarrage à froid de `mementum-cli` sur le même dossier ; `measure_search_on_10000_pages` (S5), seule, puis **pendant une fusion et sous quatre recherches de front** : c'est elle qui arrête `SEARCH_FLOOR` (au-dessus du pire p95 d'une recherche seule) et dit si les 100 ms de L10 vont au §5.10 ; côté navigateur, `results_shown_after_the_last_keystroke` (mesure, sous ×1 et ×1,5).
- [ ] **Step 2 : la CI** : si les mesures de `mementum-server` y entrent (elles n'y sont pas aujourd'hui), le dire au registre ; sinon, la mesure de la frappe pendant l'indexation va dans `mementum-sync` ou `mementum-cli`, que le job `budgets` lance déjà. Commit `budgets : la frappe, la mémoire et le démarrage avec l'index`.

**Pièges :** ne jamais relâcher un budget, sa statistique ni sa définition ; les mesures d'un binaire passent une à la fois (R341) ; le disque de la CI varie du simple au quintuple (R487) ; le point de contrôle du WAL tombe sur l'écriture qui le déclenche (R447) : les transactions de l'index en déclenchent, le lire dans la liste des frappes ; un fil qui partage un cœur attend par tranches (R585) ; mesurer le coût d'un ajout à binaire égal (interrupteur d'environnement le temps de la mesure, jamais commité).

### Task D : documentation et recette

**Dépend de :** toutes.

**Files:**
- Modify: `README.md`, `CLAUDE.md` (« Pièges rencontrés au lot 4 » ; le point 4 d'« Avant de commencer » nomme le registre du lot 4 depuis le 2026-11-20), `docs/REPRISE.md`, `docs/specs/2026-11-09-mementum-design.md` (§4.2, §5.3, §5.7, §5.10, §6.5, §7.4, §9 : les écarts E1 à E9, tels que Lenny les aura tranchés), l'en-tête de `crates/mementum-server/src/lib.rs`
- Create: `docs/recette/2026-11-19-p1-lot4-recette.md`

- [ ] **Step 1 : relire chaque document contre le code final**, pas contre ce plan : les routes ; les réglages ; `mementum reindex` ; ce qui est indexé et ce qui ne l'est pas ; les sauvegardes (`search/` ne se sauvegarde pas) ; les limites, telles que mesurées.
- [ ] **Step 2 : la checklist de recette** (ébauche plus bas).
- [ ] **Step 3 : commit** `docs : lot 4, la recherche`.

---

## Limites connues, écrites d'avance

À porter au README et au registre telles que mesurées ; chacune a son test `known_limits` quand elle se prouve.

- **L-a** : un texte effacé reste trouvable, et lisible dans un extrait, jusqu'à la réindexation de sa page (2 à 15 s, davantage si l'indexeur est en retard), par toute personne qui lit la page à ce moment, y compris quelqu'un à qui l'accès vient d'être donné. L'historique, lui, demande « Peut modifier » (R203).
- **L-b** : ni la présence ni l'ordre des résultats ne dépendent de ce que la personne ne voit pas (R916). **Le temps de réponse, si** : il croît avec le nombre d'entrées de toute l'instance (autres espaces, corbeille, pages non vues) qui portent un terme cherché. Qui mesure finement ses propres recherches peut donc estimer si un mot est courant chez les autres. S0 chiffre l'écart : 16 ns par entrée non permise qui porte le terme, 1,6 ms pour cent mille. Un temps de réponse plancher (50 ms, R932) le couvre tant que le calcul tient dessous, soit de l'ordre de trois millions d'entrées sur le poste de mesure, moins sur un serveur chargé ou lent ; au-delà, le débit de R910 borne le nombre de mesures par seconde, sans fermer le canal (décision L19).
- **L-c** : deux mots écrits dans deux blocs différents d'une page ne la trouvent pas, sauf si l'un est dans le titre (R902).
- **L-d** : un titre copié par une mention ou un lien de page (R251) est un texte de la page qui le porte : on trouve cette page en cherchant le titre de la page mentionnée, même sans voir celle-ci. Rien de neuf n'est montré (la personne lit déjà ce titre dans la page), mais la recherche le rend plus facile à trouver.
- **L-e** : les rétroliens ne viennent que du même espace ; un lien vers une page inconnue ou d'un autre espace est gardé dans `page_links` et ne se voit nulle part.
- **L-f** : pendant la première indexation d'une instance existante, ou une reconstruction, la recherche est incomplète, et le dit.
- **L-g** : rien ne borne la place de l'index sur le disque ; un compte qui peut écrire fait grossir l'index comme il fait grossir la base, et plus vite qu'elle : jusqu'à 6,7 fois les octets écrits pour des mots courts tous différents (R928). Rien ne borne non plus ce que ces textes coûtent à l'indexeur : des centaines de Mo de mémoire pendant les fusions, vingt minutes de fusions pour 655 Mo reçus (risque ouvert RO-1 ; lot 7 si S3 et X2 ne le bornent pas).
- **L-h** : au-delà de 64 Kio par bloc et de 8 Mio par page, le texte n'est pas indexé.
- **L-i** : une page écrite à l'index en plusieurs transactions (plus de `MAX_INDEX_TX_BYTES` de changements d'un coup : une première indexation, une réécriture) montre un instant un état partiel aux rétroliens (un lien déjà là, un autre pas encore) ; la recherche, elle, n'est nourrie que des blocs dont la ligne est écrite.
- **L-j** : au-delà de 20 000 pages sources d'un même espace vers une même page, des rétroliens que la personne voit peuvent manquer, sans que rien le dise.
- **L-k** : un texte effacé reste dans les fichiers de l'index plein texte jusqu'à la fusion de son segment (et dans ceux de SQLite jusqu'à leur réemploi) : il n'est plus trouvable, mais qui lit le disque du serveur le lit. La corbeille, elle, reste indexée en clair jusqu'à la suppression définitive.
- **L-l** : une page que le serveur ne sait plus relire quitte l'index (ses résultats, ses rétroliens) jusqu'à ce qu'elle se relise.
- **L-m** : le texte d'un bloc `unknown` n'est pas trouvable.
- **L-n** : cent pages au plus par recherche ; au-delà, il faut préciser la requête.
- **L-o** : un dernier mot tapé de plus de 10 caractères et pas fini n'est complété que parmi 2 000 entrées qui partagent ses 10 premiers caractères, les mieux notées, et 20 par page (R920, R929) ; fini, il est trouvé comme tout mot.
- **L-p** : le chinois, le japonais et le coréen ne sont pas découpés en mots : une suite sans espace est un seul mot, trouvé par son début seulement, et pas indexé au-delà de 40 octets (13 caractères) ; de même tout mot de plus de 40 octets (R926).
- **L-q** : un mot inachevé (le dernier de la requête, quand aucune entrée ne le porte entier) ne range ses résultats que par « titre d'abord », puis par identifiant de page : les débuts de mot n'ont pas de note (R930).
- **L-r** : la mémoire que le système compte au serveur (RSS, mémoire de travail) grandit avec les pages de l'index lues, jusqu'à la taille de l'index : 185 à 190 Mo après 500 recherches sur un index de 651 Mo, pour 20 à 24 Mo tenus en propre. Le système les reprend sous pression ; un exploitant qui lit le RSS les voit (R924).
- **L-s** : l'index plein texte et `blocks_index` pèsent ensemble de l'ordre de quatre fois le texte des pages (R931).

## Écarts et contradictions trouvés dans la conception

Signalés, pas résolus : ce que le plan suppose en attendant est dit à chaque ligne. La tâche D ne récrit la conception qu'après la réponse de Lenny.

- **E1 — « plein texte » ou « hybride » en P1.** §9 met en P1 la « recherche plein texte » et « MCP v1 (search, …) » ; §6.5 définit l'outil `search` comme « Recherche hybride » ; §5.7 et §9 placent l'hybride en P4. *Le plan suppose* : plein texte au lot 4, et l'outil `search` du lot 5 bâti dessus.
- **E2 — le fil d'Ariane dans l'index.** §5.7 : Tantivy indexe les blocs « avec le fil d'Ariane de chaque page ». Indexé comme texte cherché, le titre d'une page parente que la personne ne voit pas (un invité sur une sous-page ; en P3, une sous-page plus ouverte que sa parente) se devinerait par la recherche ; gardé pour l'affichage, il serait montré, et périmé à chaque renommage. *Le plan suppose* : le fil est calculé à la requête, d'après ce que le compte voit (R903).
- **E3 — la forme de `blocks_index`.** §5.3 : `blocks_index(block_id, node_id, type, text, path, hash)`. Un `block_id` n'est unique que dans sa page ; `path` n'est défini nulle part (chemin du bloc dans la page, ou fil d'Ariane de la page ?) ; les rétroliens demandent une table que le §5.3 ne porte pas. *Le plan suppose* : clé `(node_id, block_id)`, pas de `path`, et trois tables de plus (`page_links`, `search_state`, `search_outbox`).
- **E4 — Tantivy n'a pas été mesuré.** §9 dit de la P0 que « les choix du §5.2 [sont] confirmés ou corrigés » ; `spike/RESULTS.md` ne dit rien de la recherche. §5.2 justifie Tantivy par « même comportement sur SQLite et Postgres » et des « index reconstructibles depuis la base », sans dire le coût : deux stockages du texte, deux validations à tenir cohérentes, un dossier de plus, des fils de fusion, et deux budgets du §5.10 (mémoire au repos, démarrage) qu'il peut toucher. FTS5 est déjà compilé dans le SQLite du serveur. *Le plan suppose* : Tantivy, mesuré d'abord (S0) ; décision L1. **Mesuré le 2026-11-19 : Tantivy tient** (R933) ; la tâche D porte au §5.2 ce qu'il coûte.
- **E5 — la palette de commandes.** §4.2 : « Palette de commandes (Ctrl+K) : navigation, actions, et questions à l'IA ». Aucun lot ne porte les « actions » ; Ctrl+K est, à vérifier, le raccourci de lien de l'éditeur. *Le plan suppose* : une fenêtre de recherche, pas une palette ; décision L5.
- **E6 — `[[Page#Titre]]`.** §4.2 le range avec les rétroliens et la vue graphe ; §4.5 place les rétroliens au lot 4 et la vue graphe en P3, et ne dit rien de `[[Page#Titre]]`, que le README donne pour « pas encore ». *Le plan suppose* : hors du lot ; décision L11.
- **E7 — `mementum reindex`.** §5.7 ne dit pas s'il tourne pendant que le serveur tourne. *Le plan suppose* : serveur arrêté (R913).
- **E8 — aucun budget pour la recherche.** §5.10 n'en donne pas. *Le plan en propose trois*, à valider ; décision L10.
- **E9 — sauvegardes.** §7.4 ne dit rien d'un index dérivé. *Le plan suppose* : `search/` ne se sauvegarde pas et se refait.
- **E10 — la date de modification.** §5.3 donne `updated_at` et `updated_by` à `nodes` ; dans le code ils ne suivent que le titre et la place, pas le contenu. Le filtre « date » de l'outil `search` (§6.5) n'a donc pas de donnée aujourd'hui. *Le plan n'y touche pas* (lot 5, décision L7) ; il le signale parce que « Modifié il y a… » (ligne de la P1 sans lot) en dépend aussi.

## Décisions L1 à L19

**Depuis le 2026-11-20** : Lenny a répondu à L12 (oui) ; pour le reste il a dit « tu es autonome ». Ce qui est retenu est dans le tableau en tête du plan, et au registre (R933) : la recommandation de chaque décision, retenue par défaut, sur laquelle il peut revenir. Le texte ci-dessous est celui d'avant S0, gardé pour la question et ses raisons ; les six décisions que S0, sa relecture ou Lenny ont changées portent une ligne « Retenu ».

- **L1 — Le moteur : Tantivy, ou FTS5 de SQLite ?** La conception dit Tantivy (§5.2, §12 « Mes choix (à relire) » n° 7), sans mesure (E4). Tantivy : meilleur classement, racinisation, base de la recherche hybride de la P4, indépendant de SQLite et de Postgres ; coûte une dépendance lourde, un second stockage et sa cohérence. FTS5 : déjà là, dans la même transaction que `blocks_index`, rien à tenir cohérent ni à reconstruire à part ; mais ses écritures passent par le tour d'écriture unique (donc devant la frappe des autres), pas de racinisation française, et il faudrait autre chose le jour de Postgres. **Recommandation : garder Tantivy, comme la conception, si S0 tient les budgets ; sinon FTS5.** *Le plan suppose* Tantivy ; seules S0, S3 et S7 changeraient. **Retenu (2026-11-20, sur les mesures de S0 et leur relecture, R933) : Tantivy.** Ce qui porte le choix : il est 1,5 à 7 fois plus rapide qu'un FTS5 bien écrit, et tient R916 par son API publique, là où FTS5 demanderait une fonction de note en C. Ce qui ne le porte plus : le « meilleur classement » (la note est une BM25 à statistiques constantes, et un préfixe seul n'a pas de note, R930) et la racinisation (non retenue, L6). L'arbitrage est plus serré que le premier compte rendu ne le disait : FTS5 sans note ne manque les 100 ms que d'un facteur 1,2 à 1,7, sur le seul pire cas.
- **L2 — Chercher dans l'espace affiché, ou dans tous ses espaces ?** **Recommandation : l'espace affiché**, comme Notion ; tous les espaces demanderait un calcul d'accès par espace à chaque frappe. *Le plan suppose* un espace.
- **L3 — Ce qui est cherché.** *Le plan suppose* : les titres des pages, le texte des blocs, les légendes et noms de fichiers, l'adresse d'un signet, le LaTeX. Pas les commentaires, pas le contenu des fichiers (PDF), pas les versions de l'historique, pas les dossiers. **Recommandation : tel quel** ; les commentaires seraient le premier ajout (ils ont leurs droits à eux, R134).
- **L4 — La corbeille.** Deux questions. Les résultats : **recommandation : jamais** ; chercher dans la corbeille serait une fonction de l'écran de la corbeille, plus tard. L'index : *le plan suppose* qu'une page à la corbeille **reste indexée** (son texte reste dans les tables et les fichiers de l'index jusqu'à la suppression définitive), pour qu'une page restaurée se retrouve aussitôt ; l'autre choix la retire de l'index à la mise à la corbeille et la réindexe à la restauration. **Recommandation : la garder indexée** (le texte est de toute façon dans la base tant que la page est à la corbeille).
- **L5 — Le raccourci et la forme.** La conception annonce une palette Ctrl+K (navigation, actions, IA). **Recommandation : au lot 4, une fenêtre de recherche**, ouverte par « Rechercher » dans la barre latérale et par Ctrl+K (Cmd+K sur Mac), sauf quand du texte est sélectionné dans l'éditeur, où Ctrl+K reste « créer un lien », comme dans Notion ; les actions et l'IA plus tard, dans la même fenêtre. *Le plan suppose* cela ; changer de touche est un changement d'une ligne.
- **L6 — Tolérance de la recherche.** *Le plan suppose* : sans casse ni accents, tous les mots voulus, le dernier complété (« proj » trouve « projet »), **sans racinisation** (« pages » ne trouve pas « page ») et sans tolérance aux fautes. **Recommandation : tel quel au lot 4** : une racinisation française fausse les textes anglais d'un même espace ; S0 chiffre ce qu'elle apporterait. **Retenu par défaut (2026-11-20) : tel quel, sans racinisation.** Ce que S0 en dit n'est qu'une indication (vingt paires françaises et trente anglaises choisies à la main, pas une mesure de rappel) : elle réunit dix paires françaises sur vingt, et six paires anglaises à tort sur trente.
- **L7 — Filtres et tri.** L'outil `search` des agents annonce des filtres (type, date, auteur). **Recommandation : aucun filtre à l'écran au lot 4, tri par pertinence seul ; les filtres au lot 5**, avec l'outil, une fois décidé ce qu'est la date de modification d'une page (E10). *Le plan suppose* cela.
- **L8 — Les rétroliens : où, et lesquels.** *Le plan suppose* : une section « Pages qui mènent ici » sous le texte de la page, absente quand il n'y en a pas ; y comptent un lien de page, une mention de page, un lien du texte vers une page de l'instance ; seules les pages que le lecteur voit, du même espace. **Recommandation : tel quel.** Autre place possible : un compteur dans la barre du haut, qui ouvre un panneau.
- **L9 — Le délai d'indexation, et ce qu'il laisse voir.** Une page est trouvable 2 s après la dernière frappe (15 s pendant une frappe continue). Pendant ce délai, un texte effacé reste trouvable par qui lit la page (L-a). **Recommandation : accepter**, c'est le prix de ne rien mettre sur le chemin d'une frappe. *Le plan suppose* cela.
- **L10 — Les budgets de la recherche.** La conception n'en a pas (E8). Proposés : réponse du serveur < 100 ms au p95 sur 10 000 pages ; résultats affichés < 300 ms après la dernière frappe ; page trouvable < 5 s après la dernière frappe. **Recommandation : les adopter au §5.10 après S0**, aux chiffres mesurés s'ils sont meilleurs. *En attendant*, ce sont des mesures, pas des critères de refus. **Retenu par défaut (2026-11-20) : ils restent des mesures pendant le lot, et vont au §5.10 après X2, pas après S0.** S0 ne les porte qu'à moitié : 42,6 ms au pire p95 pour une recherche seule, index au repos, sans l'ensemble permis à bâtir ni le terme de plus de R930 ; rien sous charge. Avec le plancher de L19, « moins de 100 ms » se lit « entre 50 et 100 ms ». « Trouvable en moins de 5 s » tient pour une page ordinaire (2,1 à 2,3 s), pas pour le renommage d'une page de 10 000 blocs (5 à 6 s).
- **L11 — `[[Page#Titre]]`** (lien vers un titre d'une page). Dans aucun lot (E6). **Recommandation : pas au lot 4** ; il demande un choix de schéma (une prop ou une mention neuve, donc une version de schéma), à faire avec le lot 5 ou le lot 6 (l'import d'Obsidian en a besoin). *Le plan suppose* qu'il n'y est pas.
- **L12 — Pousser.** L'accord de pousser au fil du travail valait pour le lot 3, et `docs/REPRISE.md` dit qu'un accord donné pour un lot ne vaut pas pour le suivant. (Retirée par erreur au tour de correction 1, rétablie à la re-relecture.) **Recommandation : le redonner pour le lot 4**, aux mêmes conditions (format, clippy et tests verts, `docs/REPRISE.md` et l'avancement à jour). *En attendant*, rien du lot 4 n'est poussé. **Réponse de Lenny (2026-11-20, R934) : oui, et pour tous les lots, sans le redemander.** Avec la méthode de R935, « tests verts » se lit : les tests de l'ajout verts, la CI lue, un rouge hors de l'ajout noté au registre.
- **L13 — Indexer une instance existante.** **Recommandation : toute seule, en fond, au premier démarrage**, la recherche disant qu'elle est incomplète tant que ce n'est pas fini ; l'autre choix est une commande à lancer par l'exploitant. *Le plan suppose* l'indexation automatique.
- **L14 — Un titre mentionné devient trouvable** (L-d). Conséquence de R251, que Lenny doit encore confirmer : qui cherche le titre d'une page privée trouve les pages qui la mentionnent. **Recommandation : l'accepter avec R251** ; si R251 est défait, la projection suit sans autre travail. *Le plan suppose* R251.

- **L15 — Cent pages au plus par recherche** (20 par réponse, 5 réponses ; R911). Au-delà, la personne précise sa requête ; rien ne dit combien il en reste (R905). **Recommandation : tel quel** : c'est aussi ce qui borne le coût d'une requête et ce qu'un compte peut aspirer d'un espace en une recherche. *Le plan suppose* cela.
- **L16 — Une requête de 200 caractères et de 8 mots au plus**, sans opérateur (ni guillemets pour une expression exacte, ni « sauf ») ; au-delà, un refus qui le dit. **Recommandation : tel quel au lot 4** ; l'expression exacte entre guillemets serait le premier ajout. *Le plan suppose* cela.
- **L17 — Un bloc que cette version ne connaît pas (`unknown`) n'est pas cherché** : son texte est gardé en JSON, d'une forme que le serveur ne lit pas. **Recommandation : tel quel** (L-m). *Le plan suppose* cela.
- **L18 — Le texte gardé trois fois** : dans le document, dans `blocks_index`, dans l'index plein texte. C'est le prix de la conception (une projection dans la base, un index à côté), en place sur le disque et en sauvegarde pour la seconde copie. S0 et S4c le chiffrent sur 10 000 pages. **Recommandation : l'accepter si l'ensemble reste sous le double de la base d'aujourd'hui** ; sinon ne plus garder le texte dans l'index plein texte et bâtir l'extrait depuis `blocks_index` (une lecture de plus par résultat). *Le plan suppose* les trois copies. **Retenu par défaut (2026-11-20, R931) : les trois copies, aux tailles mesurées, sans la condition du double.** Elle ne se juge pas : la base d'aujourd'hui n'est qu'estimée (400 à 550 Mo pour ce corpus, sans les versions de l'historique), et la conclusion « ne tient pas » du premier compte rendu était fragile dans les deux sens. Mesuré : l'index 651 Mo et `blocks_index` 499 Mo pour 262 Mo de texte. Le repli (pas de texte gardé dans l'index, 256 Mo de moins) est mesuré en S3, la base réelle en S4c, où le choix se rouvre.
- **L19 — Le temps de réponse comme indice** (L-b). L'index est un pour l'instance : le temps d'une recherche croît avec le nombre d'entrées, tous espaces confondus, qui portent le mot. **Recommandation : l'accepter au lot 4 si S0 ne le distingue pas du bruit d'un appel par le réseau** ; sinon un index par espace (plus de mémoire, à mesurer) ou un temps de réponse plancher. *Le plan suppose* un index pour l'instance, et attend le chiffre de S0. **Retenu par défaut (2026-11-20, R932) : un index pour l'instance, et un temps de réponse plancher de 50 ms.** Le chiffre : 16 ns par entrée non permise qui porte le mot, 1,6 ms pour cent mille, lisible en une mesure dans le processus ; à travers un réseau, c'est un calcul, pas une mesure (quelques appels pour cent mille entrées cachées, des milliers pour mille). La condition de la recommandation (« indistinct du bruit ») ne tient donc pas pour un mot courant chez les autres, et des deux replis prévus, l'index par espace ne ferme rien pour les pages non vues du même espace : reste le plancher. Ce qu'il coûte : 50 ms sur chaque recherche du serveur.

Restent à lui, sans rapport direct avec ce plan mais à ne pas perdre : les lignes de la P1 du §4.5 sans lot, dont « Modifié il y a… » (E10).

## Ébauche de la checklist de recette

Sur ce qu'un test ne juge pas ; la tâche D l'écrit d'après le code final, et rappelle en tête les décisions L1 à L19 telles qu'elles auront été prises ou supposées.

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
- [ ] Regarder la taille du dossier des données avant et après l'indexation (décision L18).
- [ ] `mementum reindex`, serveur arrêté ; puis serveur lancé (il refuse, et le dit clairement).
- [ ] Pendant qu'un grand espace s'indexe, taper à deux dans une page : rien ne se sent.

**Pas encore** (ne pas le signaler comme un défaut) : chercher dans les commentaires, les fichiers, la corbeille, l'historique ; les filtres ; tous les espaces à la fois ; les fautes de frappe ; la palette de commandes (actions, IA) ; la vue graphe ; `[[Page#Titre]]`.

## Fin du lot

Méthode de R935 : tout ce que les tâches n'ont pas lancé se lance ici, **une fois**. Les points 1 à 3 sont la passe complète unique ; ils ne corrigent rien, ils remplissent **une liste de défauts** au registre (avec ce que « À reprendre à la fin du lot » a noté pendant le lot). Puis **une correction groupée**, puis **une passe de contrôle** : ce qui était rouge, et les suites que la correction touche. Pas de troisième tour sans un défaut neuf et grave, dit à Lenny.

1. **Relecture finale** de tout ce qui a changé, en trois passes, chaque constat prouvé par un test jetable, et **les mutations que chaque tâche a listées**, jouées une fois : **sécurité** (chaque route relue comme une attaque : ce qui sort d'une page non vue par les résultats, l'ordre, `more`, `indexing`, le temps de réponse, les rétroliens, les messages d'erreur, le journal ; puis l'indexeur comme cible : ce qu'un compte qui écrit peut lui faire coûter) ; **intégration client et serveur** ; **documentation contre le code**. Ses constats vont à la liste ; pas de vague de correction ni de re-relecture par passe.
2. **Sur un clone neuf**, étape par étape comme la CI (format, clippy, `cargo test --workspace`, fichiers générés, types, Vitest entier, licences, la suite de bout en bout entière, les budgets du navigateur, les mesures ignorées), avec les mesures de `mementum-server` en plus ; les essais au hasard larges seulement pour ce que le lot a touché (aucun n'est prévu : le lot ne touche ni la garde des mises à jour ni `vendor/yrs`).
3. **Essai de migration** sur des copies (jamais `data/instance`) : une base du lot 3 passe `0005`, avec sa copie `avant-0005.bak` ; elle s'indexe seule ; la copie remise en place, le serveur redémarre, migre de nouveau et reconstruit ; `search/` supprimé, il reconstruit ; une copie de la base prise serveur arrêté, puis **une seule** écriture indexée, puis la copie remise en place : il reconstruit, et le texte écrit entre-temps n'est dans aucun extrait.
4. **Registre** : mesures de clôture ; reports (lot 5 : l'outil `search`, les filtres, les ancres ; lot 7 : place de l'index sur le disque, débits généraux ; P4 : hybride) ; ce qui attend le propriétaire (L1 à L19 telles que retenues, R924, R925, les écarts E1 à E10, le risque ouvert RO-1). `docs/REPRISE.md` à jour ; les ateliers retirés, leur `target/` supprimé.
