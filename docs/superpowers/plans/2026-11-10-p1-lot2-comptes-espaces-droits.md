# Mementum — Phase 1, lot 2 « Comptes, espaces, droits et corbeille » : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** on ne lit et n'écrit plus rien sans compte. Chacun se crée des espaces à la manière de Notion (« Perso », « Pro »), y invite des gens (dans l'espace, ou sur une page), y garde des pages privées, et le serveur vérifie les droits à chaque appel, WebSocket comprise. Une page va à la corbeille et en revient.

**Architecture :**
- Une crate neuve, `mementum-acl` : niveaux, rôles, et le calcul pur « qui voit quoi » dans un espace (accès posés sur une page, hérités par ses sous-pages).
- `mementum-store` : la migration `0002` remplace organisations et coffres par des **espaces**, et ajoute comptes, sessions, membres, accès, invitations et corbeille. Chaque écriture qui touche à la structure ou aux droits d'un espace incrémente `spaces.rev` dans la même transaction : c'est la clé du cache des accès.
- `mementum-server` : comptes locaux (mot de passe haché en Argon2id, session par cookie), puis **toute** route passe par le calcul des accès. `/sync/{id}` vérifie le droit à la poignée de main, sert en lecture seule qui ne peut pas modifier, et le serveur ferme les connexions d'une page dès que ses droits changent.
- `apps/web` : connexion et inscription, sélecteur d'espace, barre latérale en trois sections (« Espace », « Partagé », « Privé »), menu Partager, réglages de l'espace, corbeille.

**Tech Stack :** celle du lot 1, plus trois crates Rust : `argon2` 0.6, `getrandom` 0.4, `cookie` 0.18 (licences MIT ou Apache-2.0). Aucun paquet npm nouveau : le fenêtrage de l'arbre et le glisser-déposer sont écrits à la main.

**Spec :** `docs/specs/2026-11-09-mementum-design.md`, **amendée le 2026-11-10** (espaces façon Notion). Sections utiles : §4.1, §4.2, §4.3 (menu Partager), §4.4, §4.5 (ligne « Espaces… » de la P1), §5.3, §5.6, §5.10, §6.7 (jetons : pour mémoire), §7.1, §7.2, §9, §10 (matrice de permissions), §12 (choix de rédaction à relire). Lire aussi les pièges de `CLAUDE.md` et, dans le registre du lot 1, R28, R31, R35, R38, R43 et la liste « À porter au plan du lot 2 ».

**Hors de ce lot :**
- lot 3 : commentaires (le niveau « Peut commenter » existe déjà mais vaut « Peut lire »), présence affichée et vérifiée par le serveur, historique, duplication, favoris, récents, arbre mis à jour en direct ;
- lot 5 : jetons d'API (personnels, limités à un espace ou à des pages), REST complet, MCP ;
- lots 4, 6 et 7 : inchangés (recherche ; imports et export ; livraison) ;
- P3 : groupes, espaces d'équipe, membres restreints, sous-page plus fermée que son parent, liens publics, publication web, SSO, passkeys (Ruling R46), journal d'audit ;
- lot 3 : tableaux mal formés écrits directement par un client Yjs (notes du lot 1) ;
- lot 7 : fichier `mementum.toml`, inscription par domaine e-mail (§7.2), limites de débit générales (API, journaux, rechargements et instantanés qu'un compte autorisé peut forcer : Ruling R54 ; ici, seul l'essai de mot de passe est limité) ;
- sans date, à décider : SMTP (e-mails d'invitation, mot de passe oublié), suppression d'un espace ou d'un compte, création d'espaces réservée par l'admin d'instance, purge automatique de la corbeille, l'option `MEMENTUM_ALLOWED_HOSTS`.

## Global Constraints

- **Langue** : documentation, commentaires et messages de commit en français, accents compris. Identifiants de code en anglais.
- **Commits** : petits, un sujet chacun, **sans aucune ligne d'attribution d'outil** (ni `Co-Authored-By`, ni `Claude-Session`, ni « Generated with »). Rien n'est poussé : le propriétaire décide des pushes.
- **Licences, versions, Yjs, identifiants, SQLite, durabilité, design A « Papier », dépôt** : les contraintes du plan du lot 1 (`docs/superpowers/plans/2026-11-09-p1-lot1-socle-edition.md`, « Global Constraints ») restent valables telles quelles.
- **Dépendances** : seules `argon2`, `getrandom` et `cookie` s'ajoutent. Toute autre dépendance passe par un Ruling. `cargo deny check licenses bans` et `node tools/check-npm-licenses.mjs` restent au vert.
- **Budgets** (§5.10, tels quels) : « Ouvrir une page jamais vue (réseau local) < 300 ms » jusqu'à 1 000 blocs, moins de 3 s pour 10 000 blocs, « Frappe → visible chez un autre utilisateur < 150 ms », « Mémoire du serveur au repos < 150 Mo », « Démarrage à froid du binaire < 1 s ». **Un budget ne se relâche jamais pour faire passer un test** : on mesure, on consigne, on corrige, ou on le dit au propriétaire.
- **Sécurité** :
  - le serveur vérifie les permissions **à chaque appel** (§5.6, §6.8) : aucune route de page ou d'espace ne lit le store sans passer par `access.rs` ;
  - une page qu'on ne peut pas lire répond `404 not_found`, exactement comme une page qui n'existe pas (même statut, même corps). `403 forbidden` ne sert que pour une action au-dessus de son niveau sur une page qu'on voit ;
  - mots de passe : Argon2id (`Argon2::default()`), 10 à 200 caractères, hachés dans `spawn_blocking` ; jamais journalisés, jamais renvoyés ;
  - jetons de session et d'invitation : 32 octets de `getrandom`, écrits en base64url, **stockés hachés en SHA-256**, jamais journalisés ; un jeton d'invitation n'est renvoyé qu'une fois, à sa création ;
  - cookie de session : `mementum_session`, `HttpOnly`, `SameSite=Lax`, `Path=/`, 30 jours, `Secure` quand `MEMENTUM_PUBLIC_URL` est en `https` ;
  - sans SMTP, une adresse e-mail n'est jamais vérifiée : elle ne donne aucun droit. C'est le jeton d'une invitation qui fait foi (Ruling R47) ;
  - aucun secret dans le dépôt. Les mots de passe des tests sont des constantes manifestement fausses (`mot-de-passe-de-test`).
- **Erreurs** : JSON `{ "error": <code>, "message": <texte en français> }`, comme au lot 1. Sans session : `401 unauthenticated`.
- **Types partagés** : tout type de l'API est déclaré dans `crates/mementum-server/src/types.rs`, ajouté à `bindings()`, et régénéré (`UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed`). `packages/protocol` ne se modifie pas à la main.
- **Postes et ports** : les e2e prennent 5173 et 8080, ou `MEMENTUM_E2E_WEB_PORT=5174` et `MEMENTUM_E2E_API_PORT=8081`. **Jamais 5175 ni 8090** (instance que le propriétaire regarde), jamais le dossier `scratchpad/live-data`, et on n'arrête jamais un processus qu'on n'a pas lancé. Aucun binaire du lot 2 ne doit ouvrir les données de cette instance avant la clôture du lot : la migration `0002` peut encore changer.
- **Registre des écarts** : `docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md`, sous la forme `Ruling RNN: <décision> — <pourquoi> — <coût si faux>`. La numérotation continue celle du lot 1 (R45 et suivants).
- **Relectures** (décision du propriétaire) : une relecture par groupe de tâches (1-2, 3-5, 6-7, 8-9, 10-11, 12-14), constats mineurs reportés d'office ; la relecture finale de tout le lot et les suites sur clone neuf sont gardées. Les tâches 1-2 (client seul) et 3-5 (crates seules) n'ont aucun fichier commun : elles peuvent avancer en parallèle, avec des commits par chemins.

## Review Focus

1. **Droit retiré pendant que la page est ouverte** : la personne ne reçoit plus rien et plus rien de ce qu'elle tape n'arrive au serveur, sans attendre qu'elle recharge. Tests : Task 10, `revoked_access_closes_live_connection` ; Task 13, `revoked_user_loses_the_page`.
2. **Lien d'invitation rejoué, expiré ou révoqué** : refusé ; et une invitation sur une page ne donne jamais accès au reste de l'espace. Tests : Task 5, `invitation_is_single_use_and_expires` ; Task 11, `page_invitation_makes_a_guest_not_a_member`.
3. **Page d'un espace où l'on n'est pas** : même réponse qu'une page inexistante sur toutes les routes, WebSocket comprise. Tests : Task 9, `foreign_page_is_indistinguishable_from_missing` ; Task 10, `sync_on_unreadable_page_is_404`.
4. **Base du lot 1 mise à jour** : les pages déjà écrites sont toujours là après la migration, et reviennent au premier compte créé. Tests : Task 4, `migration_keeps_lot1_pages` ; Task 6, `first_account_adopts_lot1_space`.
5. **Corbeille d'une page qui a des sous-pages, ouvertes chez quelqu'un d'autre** : tout le sous-arbre part et revient ensemble, et personne n'écrit dans une page à la corbeille. Tests : Task 9, `trash_takes_subtree_and_restore_brings_it_back` ; Task 10, `trashed_page_is_read_only_for_everyone`.

---

## Modèle des droits (décisions de ce plan)

Ces règles traduisent les §4.1, §4.4 et §5.6 amendés. Elles valent pour toutes les tâches.

**Rôles dans un espace** (`members.role`) : `owner`, `admin`, `member`, `guest`. Quelqu'un qui n'a pas de ligne dans `members` n'a aucun droit dans l'espace, admin d'instance compris.

| | owner | admin | member | guest |
|---|---|---|---|---|
| Voir la section « Espace », créer une page à la racine | oui | oui | oui | non |
| Voir la liste des membres | oui | oui | oui | non |
| Inviter dans l'espace, retirer quelqu'un, renommer l'espace | oui | oui | non | non |
| Changer un rôle | tous | `admin` ⇄ `member` | non | non |

Un espace garde toujours au moins un `owner` (`409 last_owner`).

**Niveaux sur une page**, du plus faible au plus fort : `read` < `comment` < `edit` < `full`. Au lot 2, `comment` donne les mêmes droits que `read`.

| Action | Niveau minimal |
|---|---|
| Lire (`GET` page, `GET` mmd, synchro en lecture) | `read` |
| Commenter (lot 3 : aucune route ne s'en sert encore) | `comment` |
| Écrire le contenu (`PUT` mmd, synchro en écriture), renommer, créer une sous-page, changer de place parmi les mêmes frères, mettre à la corbeille, restaurer | `edit` |
| Changer de parent (ou passer à la racine), voir et changer les accès, inviter sur la page, supprimer définitivement | `full` |

**Accès posés sur une page** (`acl`) : chaque ligne donne un niveau à un sujet, `user:<id>` ou `members` (« tous les membres de l'espace » : rôles `owner`, `admin`, `member`).

**Héritage** : le niveau de quelqu'un sur une page est le plus fort de ceux que lui donnent cette page et ses ancêtres. Rien ne retire un droit hérité (une sous-page plus fermée viendra en P3). Un `owner` ou un `admin` a `full` sur toute page que les membres voient, c'est-à-dire là où une ligne `members` s'applique. Il ne voit **pas** les pages privées des autres.

**Sections** (calculées, jamais stockées), pour le plus haut nœud que la personne voit :
- « Espace » : la personne est membre, et une ligne `members` existe sur ce nœud ou un de ses ancêtres ;
- « Privé » : aucune ligne, sur ce nœud ni sur ses ancêtres, ne nomme un autre sujet que `user:<elle>` ;
- « Partagé » : tout le reste. Un invité ne voit donc que « Partagé ».

Les descendants suivent la section de ce nœud.

**Création** : une page créée à la racine de la section « Espace » reçoit `(members, full)`, comme dans Notion ; à la racine de « Privé », `(user:<auteur>, full)`. Une sous-page ne reçoit aucune ligne : elle hérite.

**Corbeille** : `trashed_at` se pose sur la page ; ses descendants sont « à la corbeille » par héritage. Une page à la corbeille se lit encore (en lecture seule) et ne s'écrit plus (`409 trashed`). Restaurer une page dont un ancêtre est à la corbeille est refusé (`409 parent_trashed`) : on restaure l'ancêtre.

---

### Task 1 : budgets du navigateur mesurés en production (Ruling R38)

Les deux budgets du navigateur tournent aujourd'hui sur Vite en développement et un serveur debug : un pic sur cinq ouvertures fait échouer la suite environ une fois sur sept. La parade prévue par R38 est de les mesurer sur ce que les gens utiliseront.

**Files :**
- Create : `apps/web/playwright.budgets.config.ts`, `apps/web/e2e/servers.ts` (ce que les deux configurations partagent : ports, contrôle des ports pris, dossier de données).
- Modify : `apps/web/playwright.config.ts` (`testIgnore` sur `budgets.spec.ts`), `apps/web/vite.config.ts` (`preview.proxy` identique à `server.proxy`, sans `changeOrigin`), `apps/web/package.json` et `package.json` (script `e2e:budgets`), `apps/web/src/editor/Editor.tsx`, `apps/web/e2e/helpers.ts`, `apps/web/e2e/budgets.spec.ts`, `.github/workflows/ci.yml`, `CLAUDE.md` (section « Tester »), conception §5.10 (la mesure se fait sur un build de production et un serveur `--release`).

**Interfaces :**
- Produces : `corepack pnpm -C apps/web e2e:budgets` lance `budgets.spec.ts` contre `cargo run --release -p mementum-cli -- serve …` et `vite build` puis `vite preview --strictPort`. Le conteneur de l'éditeur porte `data-synced="true|false"` (première synchro faite) et `data-editable="true|false"`, en développement comme en production : `waitSynced` et la mesure s'y fient, plus à `window.__mementum`, qui n'existe qu'en développement.

- [ ] **Step 1 : écrire la configuration et la lancer.** Attendu : les deux tests de budget échouent, faute de `window.__mementum` dans le build de production.
- [ ] **Step 2 : poser les deux attributs et y brancher `waitSynced` et la mesure.** La définition de la mesure (§5.10) ne change pas.
- [ ] **Step 3 : lancer dix fois de suite** `corepack pnpm -C apps/web e2e:budgets`. Attendu : dix réussites. Consigner au registre les dix lignes `MESURE`. S'il reste un pic au-dessus de 300 ms, ne toucher ni au seuil ni à la statistique : chercher la cause, la consigner, et le dire au propriétaire.
- [ ] **Step 4 : CI.** Le job `web` lance `playwright test`, puis `playwright test -c playwright.budgets.config.ts`.
- [ ] **Step 5 : commit** `e2e : budgets du navigateur mesurés sur un build de production et un serveur release (R38)`.

### Task 2 : un arbre des pages qui ne ralentit pas l'ouverture (Ruling R43)

Chaque page affichée dans l'arbre ajoute environ 0,26 ms à l'ouverture d'une page : l'arbre entier est rendu à chaque navigation, et les recalculs de styles suivent le nombre de nœuds du document. Le remède : ne mettre dans le document que les lignes visibles.

**Files :**
- Create : `apps/web/src/sidebar/rows.ts`, `apps/web/src/sidebar/rows.test.ts`, `apps/web/src/sidebar/TreeRow.tsx`.
- Modify : `apps/web/src/sidebar/PageTree.tsx`, `apps/web/src/App.tsx` (`onCreate` stable), `apps/web/src/theme.css`, `apps/web/e2e/budgets.spec.ts`, `apps/web/e2e/pages.spec.ts` (sélecteurs de l'arbre).

**Interfaces :**
- Produces (reprises par la Task 12) :
  ```ts
  // rows.ts
  export type Row = { node: PageNode; depth: number; hasChildren: boolean; open: boolean };
  export function flattenRows(nodes: PageNode[], collapsed: ReadonlySet<string>): Row[];
  export const ROW_HEIGHT = 30; // px, §4.3
  export const OVERSCAN = 10;   // lignes rendues au-delà de la fenêtre, de chaque côté
  ```
  `TreeRow` est un composant `memo` ; ses props ne changent pas d'identité d'une navigation à l'autre, sauf `active`. L'arbre garde `role="tree"`, chaque ligne `role="treeitem"`, `aria-level`, `aria-expanded` et `data-page-id`.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - Vitest, `rows.test.ts` : `flatten_orders_by_position_and_skips_collapsed` (les enfants d'une page repliée sont absents, la profondeur suit l'imbrication) ; `orphan_parent_goes_to_root` (un parent absent de la liste : la page est à la racine).
  - e2e, `budgets.spec.ts`, après les mesures existantes (qui gardent leur petit arbre) : 1 000 pages vides créées à la racine par l'API, 8 à la fois, puis
    - `open_page_with_1000_pages_in_tree_under_300ms` : la mesure existante, sur des pages de 1 000 blocs : p95 sur 5 ouvertures < 300 ms ;
    - `tree_renders_only_visible_rows` : le document compte moins de 100 lignes d'arbre ; après un défilement jusqu'en bas, la dernière page est affichée et cliquable.
- [ ] **Step 2 : lancer.** Attendu : les deux e2e échouent (R43 donne une médiane vers 330 ms dès 500 pages).
- [ ] **Step 3 : implémenter** `flattenRows`, puis le fenêtrage dans `PageTree` : hauteur totale `rows.length × ROW_HEIGHT`, seules les lignes de la fenêtre (plus `OVERSCAN`) sont rendues, décalées par leur rang. Rendre `onCreate` stable dans `App.tsx` (`useCallback`).
- [ ] **Step 4 : lancer** Vitest, `playwright test` (les sélecteurs de `pages.spec.ts` suivent le nouvel arbre), puis `e2e:budgets`. Attendu : tout passe ; consigner les mesures.
- [ ] **Step 5 : commit** `web : arbre des pages fenêtré, lignes mémoïsées (R43)`.

### Task 3 : `mementum-acl`, niveaux, rôles et calcul des accès

**Files :**
- Create : `crates/mementum-acl/Cargo.toml`, `crates/mementum-acl/src/lib.rs`, `crates/mementum-acl/src/access.rs`, `crates/mementum-acl/src/cache.rs`, `crates/mementum-acl/tests/access.rs`.
- Modify : `Cargo.toml` (dépendance de workspace `mementum-acl`).

**Interfaces :**
- Produces (dépendances : `uuid` seulement ; `proptest` en dev) :
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub enum Level { Read, Comment, Edit, Full }          // "read" | "comment" | "edit" | "full"
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub enum Role { Owner, Admin, Member, Guest }         // "owner" | "admin" | "member" | "guest"
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub enum Subject { User(Uuid), Members }              // "user:<uuid>" | "members"
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Grant { pub subject: Subject, pub level: Level }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub enum Section { Space, Shared, Private }           // "space" | "shared" | "private"
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub enum Action { Read, Comment, Edit, Rename, CreateChild, Reorder, Trash, Restore, Move, Share, Delete }

  // Chaque type texte a `as_str(self) -> &'static str` et `parse(&str) -> Option<Self>`.
  impl Level { pub fn allows(self, action: Action) -> bool }
  impl Role { pub fn is_member(self) -> bool }          // faux pour Guest

  pub struct TreeNode { pub id: Uuid, pub parent_id: Option<Uuid>, pub trashed: bool }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct NodeAccess {
      pub level: Level,
      pub section: Section,
      pub in_trash: bool,                 // la page ou un de ses ancêtres est à la corbeille
      pub visible_parent: Option<Uuid>,   // None : racine, ou parent que la personne ne voit pas
  }
  pub struct SpaceAccess { /* privé */ }
  impl SpaceAccess {
      pub fn compute(nodes: &[TreeNode], grants: &[(Uuid, Grant)], user: Uuid, role: Option<Role>) -> SpaceAccess;
      pub fn role(&self) -> Option<Role>;
      pub fn node(&self, id: Uuid) -> Option<&NodeAccess>;   // None : la personne ne voit pas ce nœud
      pub fn can(&self, action: Action, id: Uuid) -> bool;   // niveau seulement, sans regarder la corbeille
      pub fn visible(&self) -> impl Iterator<Item = (Uuid, &NodeAccess)> + '_;
  }
  pub struct AccessCache { /* privé */ }
  impl AccessCache {
      pub fn new(capacity: usize) -> AccessCache;            // plein : tout est vidé
      pub fn get(&self, user: Uuid, space: Uuid, rev: i64) -> Option<Arc<SpaceAccess>>;
      pub fn put(&self, user: Uuid, space: Uuid, rev: i64, access: Arc<SpaceAccess>);
  }
  ```
- Règles : celles de la section « Modèle des droits ». `compute` se termine sur toute entrée : un parent absent de `nodes` vaut la racine, et un cycle (que le store interdit) ne boucle pas.

- [ ] **Step 1 : écrire les tests qui échouent** (`tests/access.rs`).
  - `level_allows_matrix` : le tableau « Action / Niveau minimal » ci-dessus, écrit en entier (4 niveaux × 11 actions).
  - `sections_follow_the_grants` : une page `(members, full)` est en « Espace » pour un membre et invisible pour un invité ; la même page avec en plus `(user:G, read)` est en « Partagé » pour l'invité G ; une page `(user:A, full)` est en « Privé » pour A, invisible pour un `owner` B ; avec en plus `(user:B, edit)`, elle est en « Partagé » pour A et pour B.
  - `admin_has_full_wherever_members_see` et `private_page_is_invisible_to_admins`.
  - `shared_subpage_shows_as_root_for_the_guest` : `visible_parent` vaut `None` quand le parent n'est pas visible.
  - `trash_is_inherited` : `in_trash` sur tous les descendants d'une page à la corbeille.
  - proptest (256 cas, arbres et accès aléatoires) : `child_level_is_at_least_parent_level` ; `outsider_sees_nothing` (`role == None`) ; `guest_sees_only_user_granted_subtrees` ; `adding_a_grant_never_lowers_anyone` ; `compute_terminates_on_cycles`.
  - `cache_misses_on_another_rev`.
- [ ] **Step 2 : lancer** `cargo test -p mementum-acl`. Attendu : échec de compilation.
- [ ] **Step 3 : implémenter.** Un parcours de haut en bas, sans récursion.
- [ ] **Step 4 : lancer** `cargo test -p mementum-acl`, `cargo clippy -p mementum-acl --all-targets -- -D warnings`. Attendu : tout passe.
- [ ] **Step 5 : commit** `acl : niveaux, rôles et calcul des accès d'un espace`.

### Task 4 : store, migration 0002 : espaces, nœuds, accès, corbeille

**Files :**
- Create : `crates/mementum-store/migrations/0002_spaces_accounts.sql`, `crates/mementum-store/src/spaces.rs`, `crates/mementum-store/src/acl.rs`, `crates/mementum-store/tests/migration.rs`, `crates/mementum-store/tests/spaces.rs`, `crates/mementum-store/tests/acl.rs`.
- Modify : `crates/mementum-store/src/lib.rs`, `src/nodes.rs`, `src/docs.rs`, `Cargo.toml` (`mementum-acl`), `tests/nodes.rs`, `tests/docs.rs` ; et, pour que tout compile, les appelants de `vault_id` dans `mementum-sync` (tests) et `mementum-server`.

**Interfaces :**
- Consumes : `Level`, `Role`, `Subject`, `Grant` (Task 3).
- Produces, schéma après la migration (une seule migration pour tout le lot : les tables de la Task 5 y sont aussi) :
  ```text
  users(id PK, email UNIQUE NOT NULL, name NOT NULL, password_hash NOT NULL, instance_admin INTEGER NOT NULL DEFAULT 0, created_at)
  sessions(token_hash BLOB PK, user_id → users ON DELETE CASCADE, created_at, expires_at)
  spaces(id PK, name NOT NULL, rev INTEGER NOT NULL DEFAULT 0, created_by → users NULL, created_at)
  members(space_id → spaces, user_id → users, role CHECK (owner|admin|member|guest), created_at, PK (space_id, user_id))
  nodes(id PK, space_id → spaces NOT NULL, parent_id → nodes, kind, title, position,
        created_by → users NULL, created_at, updated_by → users NULL, updated_at, trashed_at, trashed_by → users NULL)
        index nodes_by_parent (space_id, parent_id, position)
  acl(node_id → nodes ON DELETE CASCADE, subject TEXT NOT NULL, level CHECK (read|comment|edit|full), PK (node_id, subject))
  invitations(id PK, space_id → spaces, node_id → nodes NULL ON DELETE CASCADE, email NOT NULL,
              grant_role NULL, grant_level NULL, token_hash BLOB UNIQUE NOT NULL, created_by → users,
              created_at, expires_at, accepted_at NULL, accepted_by NULL,
              CHECK ((node_id IS NULL) = (grant_role IS NOT NULL)), CHECK ((node_id IS NULL) = (grant_level IS NULL)))
  doc_updates(…, actor_id → users NULL)        doc_snapshots : inchangé
  ```
  Les tables `orgs` et `vaults` disparaissent. Pas de colonne `slug` : aucune URL ne s'en sert encore.
- Produces, données migrées : chaque coffre du lot 1 devient un espace **de même id**, nommé « Mon espace », sans membre ; ses nœuds gardent leur id, leur place et leur document ; chaque nœud de racine reçoit `(members, full)`.
- Produces, API :
  ```rust
  pub struct Space { pub id: Uuid, pub name: String, pub rev: i64, pub created_at: i64 }
  pub struct Node { pub id: Uuid, pub space_id: Uuid, pub parent_id: Option<Uuid>, pub kind: NodeKind, pub title: String,
                    pub position: String, pub created_by: Option<Uuid>, pub created_at: i64,
                    pub updated_by: Option<Uuid>, pub updated_at: i64, pub trashed_at: Option<i64> }
  pub struct NewNode { pub space_id: Uuid, pub parent_id: Option<Uuid>, pub after: Option<Uuid>, pub kind: NodeKind,
                       pub title: String, pub created_by: Option<Uuid>,
                       pub grants: Vec<Grant>,          // écrits avec le nœud
                       pub doc: Option<Vec<u8>> }       // instantané de seq 0, écrit avec le nœud
  // StoreError gagne : EmailTaken, LastOwner, ParentTrashed, NotTrashed.
  impl Store {
      pub async fn create_space(&self, name: &str, owner: Option<Uuid>) -> Result<Space, StoreError>;   // `owner` devient membre `owner`
      pub async fn get_space(&self, id: Uuid) -> Result<Option<Space>, StoreError>;
      pub async fn rename_space(&self, id: Uuid, name: &str) -> Result<Space, StoreError>;
      pub async fn space_rev(&self, id: Uuid) -> Result<Option<i64>, StoreError>;
      pub async fn default_space(&self) -> Result<Space, StoreError>;   // transitoire : le plus ancien espace, créé s'il n'y en a aucun ; retiré à la Task 9
      pub async fn create_node(&self, n: NewNode) -> Result<Node, StoreError>;
      pub async fn get_node(&self, id: Uuid) -> Result<Option<Node>, StoreError>;
      pub async fn list_nodes(&self, space_id: Uuid) -> Result<Vec<Node>, StoreError>;                 // corbeille comprise
      pub async fn rename_node(&self, id: Uuid, title: &str, by: Option<Uuid>) -> Result<Node, StoreError>;
      pub async fn move_node(&self, id: Uuid, parent_id: Option<Uuid>, after: Option<Uuid>, by: Option<Uuid>) -> Result<Node, StoreError>;
      /// Titre et place en une transaction (report du lot 1 : `PATCH` faisait deux écritures). `Place { parent_id, after }`.
      pub async fn update_node(&self, id: Uuid, title: Option<&str>, place: Option<Place>, by: Option<Uuid>) -> Result<Node, StoreError>;
      pub async fn trash_node(&self, id: Uuid, by: Uuid) -> Result<Node, StoreError>;
      pub async fn restore_node(&self, id: Uuid) -> Result<Node, StoreError>;                          // ParentTrashed
      pub async fn delete_node(&self, id: Uuid) -> Result<Vec<Uuid>, StoreError>;                      // NotTrashed ; rend les ids supprimés
      pub async fn subtree_ids(&self, id: Uuid) -> Result<Vec<Uuid>, StoreError>;                      // le nœud compris
      pub async fn space_grants(&self, space_id: Uuid) -> Result<Vec<(Uuid, Grant)>, StoreError>;
      pub async fn node_grants(&self, node_id: Uuid) -> Result<Vec<Grant>, StoreError>;
      pub async fn set_grant(&self, node_id: Uuid, grant: Grant) -> Result<(), StoreError>;            // remplace le niveau du sujet
      pub async fn remove_grant(&self, node_id: Uuid, subject: Subject) -> Result<(), StoreError>;
      pub async fn append_updates(&self, node_id: Uuid, updates: &[&[u8]], actor: Option<Uuid>) -> Result<i64, StoreError>;  // une transaction (M3)
  }
  ```
  `append_update(node_id, update)` reste jusqu'à la Task 10, comme simple appel à `append_updates`.
- `restore_node` : `ParentTrashed` dès qu'un ancêtre est à la corbeille, que le nœud y soit lui-même ou non ; sans effet sur un nœud qui n'y est pas.
- Règle de `rev` : toute méthode qui crée, renomme, déplace, met à la corbeille, restaure ou supprime un nœud, ou qui écrit dans `acl` ou `members`, incrémente `spaces.rev` **dans la même transaction**.
- Ordre de la migration (SQLite refuse de retirer une colonne qui porte une clé étrangère ; sqlx lance chaque migration dans une transaction, où `PRAGMA foreign_keys` ne se change pas) : créer les tables neuves et `nodes_new`, `doc_updates_new`, `doc_snapshots_new` ; copier ; supprimer d'abord les tables filles (`doc_updates`, `doc_snapshots`), puis `nodes`, `vaults`, `orgs` ; renommer les `_new` ; recréer les index. Les parents copiés avant leurs enfants, ou `PRAGMA defer_foreign_keys = ON`.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `migration.rs`, `migration_keeps_lot1_pages` : construire une base du lot 1 (un `sqlx::migrate::Migrator` sur un dossier temporaire qui ne contient que `0001_init.sql`), y écrire en SQL une organisation, un coffre, trois pages dont une sous-page, deux `doc_updates` et un `doc_snapshots` ; ouvrir avec `Store::open`. Attendu : un espace « Mon espace » de l'id du coffre, les trois nœuds avec leurs id, parent, position et titre, `load_doc` identique octet pour octet, `(members, full)` sur les deux racines et rien sur la sous-page, `PRAGMA foreign_key_check` vide, plus de table `orgs` ni `vaults`.
  - `migration.rs`, `fresh_database_has_no_space`.
  - `spaces.rs` : `every_structural_write_bumps_rev` (une assertion par méthode de la règle de `rev`) ; `nodes_do_not_cross_spaces` (un parent ou un `after` d'un autre espace : `NotFound`).
  - `nodes.rs` : `update_node_is_atomic` (nouveau titre et déplacement en cycle : `Cycle`, et le titre n'a pas changé) ; `trash_then_restore_roundtrip` ; `restore_under_trashed_parent_is_refused` (`ParentTrashed`) ; `delete_requires_trash` (`NotTrashed`) ; `delete_removes_subtree_docs_and_grants` (plus aucune ligne dans `nodes`, `doc_updates`, `doc_snapshots`, `acl` pour les ids rendus) ; `create_node_writes_grants_and_doc_atomically` (une panne simulée par déclencheur SQLite sur `acl` : ni nœud ni instantané).
  - `acl.rs` : `set_grant_replaces_level`, `subject_text_roundtrip` (`user:<uuid>`, `members`).
  - `docs.rs` : `append_updates_is_atomic` (panne au deuxième : aucun des deux n'est écrit) ; `append_updates_records_actor`.
- [ ] **Step 2 : lancer** `cargo test -p mementum-store`. Attendu : échec.
- [ ] **Step 3 : écrire la migration, puis l'API.** Adapter les appelants pour que `cargo test --workspace` compile ; le serveur garde ses routes du lot 1, sur `default_space()`, et `Tree.vaultId` porte l'id de l'espace jusqu'à la Task 9.
- [ ] **Step 4 : lancer** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Attendu : tout passe.
- [ ] **Step 5 : commit** `store : migration 0002, espaces à la place des organisations et des coffres, accès, corbeille`.

### Task 5 : store, comptes, sessions, membres et invitations

**Files :**
- Create : `crates/mementum-store/src/users.rs`, `src/members.rs`, `src/invitations.rs`, `tests/users.rs`, `tests/members.rs`, `tests/invitations.rs`.
- Modify : `crates/mementum-store/src/lib.rs`.

**Interfaces :**
- Consumes : le schéma et la règle de `rev` de la Task 4.
- Produces :
  ```rust
  pub struct User { pub id: Uuid, pub email: String, pub name: String, pub password_hash: String,
                    pub instance_admin: bool, pub created_at: i64 }
  pub struct NewUser { pub email: String, pub name: String, pub password_hash: String }
  pub struct Member { pub user_id: Uuid, pub name: String, pub email: String, pub role: Role }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum InvitationGrant { Space(Role), Page { node_id: Uuid, level: Level } }
  pub struct NewInvitation { pub space_id: Uuid, pub grant: InvitationGrant, pub email: String,
                             pub token_hash: [u8; 32], pub created_by: Uuid, pub expires_at: i64 }
  pub struct Invitation { pub id: Uuid, pub space_id: Uuid, pub grant: InvitationGrant, pub email: String,
                          pub created_by: Uuid, pub created_at: i64, pub expires_at: i64 }
  impl Store {
      pub async fn create_user(&self, u: NewUser) -> Result<User, StoreError>;        // EmailTaken ; le premier compte est admin d'instance
      pub async fn get_user(&self, id: Uuid) -> Result<Option<User>, StoreError>;
      pub async fn user_by_email(&self, email: &str) -> Result<Option<User>, StoreError>;
      pub async fn count_users(&self) -> Result<i64, StoreError>;
      pub async fn set_password(&self, user_id: Uuid, password_hash: &str) -> Result<(), StoreError>;   // supprime toutes ses sessions
      pub async fn create_session(&self, user_id: Uuid, token_hash: [u8; 32], expires_at: i64) -> Result<(), StoreError>;
      pub async fn session_user(&self, token_hash: [u8; 32], now: i64, ttl_ms: i64) -> Result<Option<User>, StoreError>;
      pub async fn delete_session(&self, token_hash: [u8; 32]) -> Result<(), StoreError>;
      pub async fn purge_sessions(&self, now: i64) -> Result<u64, StoreError>;
      pub async fn spaces_of(&self, user_id: Uuid) -> Result<Vec<(Space, Role)>, StoreError>;          // par date de création
      pub async fn role_in(&self, space_id: Uuid, user_id: Uuid) -> Result<Option<Role>, StoreError>;
      pub async fn members(&self, space_id: Uuid) -> Result<Vec<Member>, StoreError>;
      pub async fn add_member(&self, space_id: Uuid, user_id: Uuid, role: Role) -> Result<(), StoreError>;
      pub async fn set_role(&self, space_id: Uuid, user_id: Uuid, role: Role) -> Result<(), StoreError>;   // LastOwner
      pub async fn remove_member(&self, space_id: Uuid, user_id: Uuid) -> Result<(), StoreError>;          // LastOwner ; voir les règles
      pub async fn adopt_orphan_spaces(&self, user_id: Uuid) -> Result<Vec<Space>, StoreError>;
      pub async fn create_invitation(&self, i: NewInvitation) -> Result<Invitation, StoreError>;
      pub async fn invitation_by_token(&self, token_hash: [u8; 32], now: i64) -> Result<Option<Invitation>, StoreError>;
      pub async fn accept_invitation(&self, token_hash: [u8; 32], user_id: Uuid, now: i64) -> Result<Invitation, StoreError>;
      pub async fn revoke_invitation(&self, id: Uuid) -> Result<(), StoreError>;
      pub async fn pending_invitations(&self, space_id: Uuid) -> Result<Vec<Invitation>, StoreError>;      // d'espace et de pages
      pub async fn revoke_user_grant(&self, node_id: Uuid, user_id: Uuid) -> Result<(), StoreError>;
  }
  ```
- Règles :
  - l'e-mail est gardé rogné et en minuscules ; `user_by_email` cherche de même ;
  - `create_user` décide « premier compte » dans sa transaction (`BEGIN IMMEDIATE`) ;
  - `session_user` rend `None` pour un jeton inconnu ou échu ; s'il reste moins de `ttl_ms` moins un jour, l'échéance repart à `now + ttl_ms` ;
  - `add_member` n'abaisse jamais un rôle existant (rang : `guest` < `member` < `admin` < `owner`) ;
  - `remove_member` retire aussi les lignes `user:<id>` de l'`acl` de l'espace, **sauf** sur les pages dont la personne est le seul sujet (ses pages privées) : sans rôle, elle ne les voit plus, et elle les retrouve si elle revient dans l'espace. Rien n'est supprimé ;
  - `adopt_orphan_spaces` rend `owner` de tout espace sans aucun membre ;
  - `accept_invitation`, en une transaction : `NotFound` si le jeton est inconnu, échu, révoqué ou déjà accepté ; une invitation d'espace ajoute le membre au rôle donné ; une invitation de page pose `(user:<id>, level)` sans abaisser un niveau déjà posé, et ajoute la personne comme `guest` si elle n'est pas dans l'espace ; puis marque l'invitation acceptée ;
  - `revoke_user_grant` retire la ligne ; si la personne est `guest` et n'a plus aucune ligne dans l'espace, elle en sort.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `users.rs` : `first_user_is_instance_admin_only` (deux créations concurrentes : un seul admin) ; `email_is_unique_ignoring_case_and_spaces` (`EmailTaken`) ; `set_password_deletes_sessions` ; `session_expires_and_slides`.
  - `members.rs` : `last_owner_cannot_leave_or_be_demoted` ; `add_member_never_downgrades` ; `remove_member_drops_shared_grants_and_keeps_private_pages` ; `adopt_orphan_spaces_takes_only_memberless_spaces` ; `membership_writes_bump_rev`.
  - `invitations.rs` : `invitation_is_single_use_and_expires` (acceptée deux fois : `NotFound` la seconde ; échue : `NotFound` ; révoquée : `NotFound`) ; `page_invitation_adds_a_guest_with_one_grant` ; `page_invitation_keeps_a_member_a_member` ; `accept_never_lowers_level_or_role` ; `guest_leaves_with_last_grant`.
- [ ] **Step 2 : lancer** `cargo test -p mementum-store`. Attendu : échec.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** `cargo fmt --all`, clippy, `cargo test -p mementum-store`. Attendu : tout passe.
- [ ] **Step 5 : commit** `store : comptes, sessions, membres et invitations`.

### Task 6 : serveur, authentification

**Files :**
- Create : `crates/mementum-server/src/auth.rs`, `src/routes/auth.rs`, `crates/mementum-server/tests/auth.rs`, `crates/mementum-server/tests/common/mod.rs` (les aides de `tests/api.rs`, partagées).
- Modify : `Cargo.toml` (workspace : `argon2`, `getrandom`, `cookie`), `crates/mementum-server/Cargo.toml`, `src/lib.rs`, `src/config.rs`, `src/types.rs`, `src/error.rs`, `src/guard.rs`, `src/routes/info.rs`, `crates/mementum-cli/src/main.rs`, `crates/mementum-cli/tests/cli.rs`, `packages/protocol/src/generated/*` (régénérés).

**Interfaces :**
- Consumes : l'API de la Task 5.
- Produces, configuration : `Config.signup: SignupMode` (`MEMENTUM_SIGNUP`, `--signup`, `open` ou `invite`, **défaut `invite`**) ; `Config.public_url: Option<String>` (`MEMENTUM_PUBLIC_URL`, `--public-url`).
- Produces, routes :

  | Route | Corps | Réponse |
  |---|---|---|
  | `POST /api/v1/auth/signup` | `SignUp { email, name, password, invitation? }` | `201 Me` + cookie ; `403 signup_closed` ; `409 email_taken` ; `422 invalid_email`, `invalid_name`, `invalid_password` ; `404 invalid_invitation` |
  | `POST /api/v1/auth/login` | `LogIn { email, password }` | `200 Me` + cookie ; `401 invalid_credentials` ; `429 too_many_attempts` (+ `Retry-After`) |
  | `POST /api/v1/auth/logout` | — | `204`, cookie effacé |
  | `POST /api/v1/auth/password` | `ChangePassword { current, new }` | `204` ; les autres sessions du compte sont supprimées |
  | `GET /api/v1/me` | — | `Me { user: UserInfo { id, email, name, instanceAdmin }, spaces: SpaceInfo[] { id, name, role } }` ; `401 unauthenticated` |
  | `GET /api/v1/info` | — | `Info` gagne `signup: "open" \| "invite"` et `setup: boolean` (aucun compte n'existe encore) |

- Produces, code : l'extracteur `auth::AuthUser { id: Uuid, email: String, name: String, instance_admin: bool }` (cookie → session → compte, sinon `401`) ; `auth::hash_password`, `auth::verify_password` (dans `spawn_blocking`) ; `auth::new_token() -> (String, [u8; 32])` (le jeton en base64url et son SHA-256) ; `auth::token_hash(&str) -> Option<[u8; 32]>`. Constantes : `SESSION_COOKIE = "mementum_session"`, `SESSION_TTL_MS = 30 jours`, `PASSWORD_MIN_CHARS = 10`, `PASSWORD_MAX_CHARS = 200`, `MAX_NAME_CHARS = 100`, `LOGIN_MAX_FAILURES = 10`, `LOGIN_WINDOW_MS = 15 minutes`.
- Produces, CLI : `mementum user set-password --email <adresse> [--data-dir <dossier>]`, le nouveau mot de passe lu sur l'entrée standard (première ligne). C'est l'issue d'un mot de passe oublié, faute de SMTP.
- Règles :
  - l'inscription est permise si aucun compte n'existe, si `signup = open`, ou si `invitation` est un jeton valide ; l'invitation est acceptée dans la foulée ;
  - à l'inscription, le premier compte adopte les espaces sans membre (`adopt_orphan_spaces`) ; un compte qui n'a alors aucun espace et n'arrive pas par une invitation reçoit « Espace de `<nom>` », dont il est `owner` ;
  - adresse inconnue et mauvais mot de passe donnent la même réponse, et coûtent une vérification Argon2 dans les deux cas ;
  - 10 échecs pour une même adresse en 15 minutes : `429` jusqu'à la fin de la fenêtre, même avec le bon mot de passe ; un succès remet le compteur à zéro. Compteur en mémoire ;
  - toute requête autre que `GET`, `HEAD` ou `OPTIONS` qui porte une en-tête `Origin` refusée par `guard::origin_allowed` (à qui l'on ajoute l'origine de `public_url`) reçoit `403 bad_origin` ;
  - le contrôle de `Host` reste celui du lot 1 (R31), et l'en-tête de `guard.rs` est récrite : hors de la boucle locale, ce sont les comptes qui protègent, pas le contrôle de l'`Origin` (note du lot 1) ; l'avertissement d'une écoute hors de la boucle locale dit maintenant que tout passe par les comptes ; s'il n'existe aucun compte au démarrage, un `warn!` dit que le premier compte créé sera admin d'instance.
- Les routes de pages du lot 1 restent ouvertes jusqu'à la Task 9.

- [ ] **Step 1 : écrire les tests qui échouent** (`tests/auth.rs`).
  - `first_signup_is_instance_admin_and_gets_a_space` ; `first_account_adopts_lot1_space` (un espace sans membre avec deux pages existe : après l'inscription, `me.spaces` ne contient que lui, rôle `owner`).
  - `signup_is_closed_without_invitation` (mode `invite` : `403` ; avec un jeton d'invitation créé par le store : `201`, membre de l'espace, pas d'espace à lui, jeton ensuite inutilisable) ; `signup_open_allows_anyone`.
  - `login_sets_http_only_lax_cookie` (`HttpOnly`, `SameSite=Lax`, `Path=/`, `Max-Age=2592000` ; `Secure` seulement avec `public_url` en `https`).
  - `wrong_password_and_unknown_email_are_indistinguishable` (même statut, même corps).
  - `login_is_throttled_after_10_failures` (la 11ᵉ : `429` et `Retry-After` ; le bon mot de passe aussi, tant que dure la fenêtre).
  - `logout_kills_the_session` ; `expired_session_is_401` ; `change_password_kills_other_sessions`.
  - `password_rules` (9 caractères : `422` ; 10 : `201` ; 201 : `422`) ; `email_is_normalized` (` A@B.fr ` puis `a@b.fr` : `409`).
  - `secrets_never_leave` : ni les corps de réponse ni les journaux ne contiennent `$argon2`, le mot de passe, ou la valeur d'un cookie de session.
  - `mutating_request_with_foreign_origin_is_403`.
  - `cli.rs`, `cli_set_password_recovers_account` : après la commande, l'ancien mot de passe échoue, le nouveau passe, les sessions d'avant sont mortes.
- [ ] **Step 2 : lancer** `cargo test -p mementum-server --test auth`. Attendu : échec.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** `cargo fmt --all`, clippy, `cargo test --workspace`, `cargo deny check licenses bans`. Attendu : tout passe ; types régénérés et commités.
- [ ] **Step 5 : commit** `serveur : comptes locaux, sessions par cookie, inscription ouverte ou sur invitation`.

### Task 7 : client, session

**Files :**
- Create : `apps/web/src/session/SessionGate.tsx`, `session/LoginPage.tsx`, `session/SignupPage.tsx`, `session/AccountMenu.tsx`, `session/session.ts`, `session/session.test.ts`, `apps/web/e2e/global-setup.ts`, `apps/web/e2e/auth.spec.ts`.
- Modify : `apps/web/src/main.tsx`, `src/api.ts`, `src/router.ts` (`/login`, `/signup`), `src/App.tsx`, `src/editor/collab.ts` (`guest()` disparaît), `src/sidebar/PageTree.tsx` (menu du compte en bas), `src/theme.css`, `apps/web/e2e/helpers.ts`, `e2e/servers.ts` (`MEMENTUM_SIGNUP=open` pour le serveur des e2e), les deux configurations Playwright (`globalSetup`, `storageState`).

**Interfaces :**
- Consumes : les routes de la Task 6.
- Produces :
  ```ts
  // api.ts
  export function getMe(): Promise<Me>;                         // ApiError 401 : pas de session
  export function signUp(body: SignUp): Promise<Me>;
  export function logIn(body: LogIn): Promise<Me>;
  export function logOut(): Promise<void>;
  export function changePassword(body: ChangePassword): Promise<void>;
  // session.ts
  export function useSession(): { me: Me; refresh(): Promise<void> };   // sous SessionGate seulement
  export function colorOf(userId: string): string;              // couleur de présence, stable pour un compte
  // e2e/helpers.ts
  export type Account = { email: string; name: string; password: string; storageState: string };
  export async function signUpAccount(name?: string): Promise<Account>;   // compte neuf, par l'API
  export async function newUser(browser: Browser, account?: Account): Promise<Page>;   // défaut : le compte du globalSetup
  export async function apiAs(account?: Account): Promise<APIRequestContext>;
  ```
- Comportement : `SessionGate` appelle `getMe()` ; sur `401`, il montre la connexion, et l'inscription si `info.setup` (« Créer le premier compte : il administrera cette instance »), si `info.signup === "open"`, ou si l'adresse porte un jeton d'invitation (Task 13). Toute réponse `401` d'un appel ramène à la connexion **sans démonter la page ouverte** : ce qui a été tapé reste dans l'onglet et repart après la reconnexion. La présence porte le nom du compte et `colorOf(id)`.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - Vitest, `session.test.ts` : `color_is_stable_per_user`.
  - e2e, `auth.spec.ts` : `signup_then_lands_in_the_app` ; `login_logout_roundtrip` ; `wrong_password_shows_the_server_message` ; `change_password_then_old_one_fails` ; `presence_carries_the_account_name` (deux comptes sur une page : l'awareness de l'un donne le nom de l'autre).
- [ ] **Step 2 : lancer.** Attendu : échec.
- [ ] **Step 3 : implémenter.** `globalSetup` crée le compte par défaut et enregistre son `storageState` ; toutes les spécifications existantes tournent avec lui.
- [ ] **Step 4 : lancer** typecheck, Vitest, `playwright test`, `e2e:budgets`. Attendu : tout passe.
- [ ] **Step 5 : commit** `web : connexion, inscription, menu du compte ; les e2e portent une session`.

### Task 8 : serveur, espaces, membres et invitations d'espace

**Files :**
- Create : `crates/mementum-server/src/access.rs`, `src/routes/spaces.rs`, `src/routes/invitations.rs`, `crates/mementum-server/tests/spaces.rs`.
- Modify : `crates/mementum-server/Cargo.toml` (`mementum-acl`), `src/lib.rs`, `src/types.rs`, `src/error.rs`, types régénérés.

**Interfaces :**
- Consumes : `AuthUser` (Task 6), `SpaceAccess` et `AccessCache` (Task 3), le store (Tasks 4 et 5).
- Produces, `access.rs` (le seul chemin vers les droits, pour cette tâche et les suivantes) :
  ```rust
  pub(crate) const ACCESS_CACHE_ENTRIES: usize = 10_000;
  impl AppState {
      /// Les accès de `user` dans l'espace : lus dans le cache si `spaces.rev` n'a pas bougé. `404` s'il n'y a aucun rôle.
      pub(crate) async fn space_access(&self, user: &AuthUser, space_id: Uuid) -> Result<Arc<SpaceAccess>, ApiError>;
      /// Le rôle de `user`, au moins `min` (rang guest < member < admin < owner) : `404` sans rôle, `403 forbidden` en dessous.
      pub(crate) async fn role_at_least(&self, user: &AuthUser, space_id: Uuid, min: Role) -> Result<Role, ApiError>;
  }
  ```
- Produces, routes :

  | Route | Qui (rôle minimal) | Réponse |
  |---|---|---|
  | `POST /api/v1/spaces` `CreateSpace { name }` | tout compte | `201 SpaceInfo` (rôle `owner`) |
  | `PATCH /api/v1/spaces/{id}` `UpdateSpace { name }` | `admin` | `SpaceInfo` |
  | `GET /api/v1/spaces/{id}/members` | `member` | `Member[] { userId, name, email, role }` |
  | `PATCH /api/v1/spaces/{id}/members/{userId}` `UpdateMember { role }` | voir « Modèle des droits » | `Member` ; `409 last_owner` |
  | `DELETE /api/v1/spaces/{id}/members/{userId}` | `admin`, ou soi-même | `204` ; `409 last_owner` |
  | `POST /api/v1/spaces/{id}/invitations` `InviteToSpace { email, role: "admin" \| "member" }` | `admin` | `201 InvitationCreated { id, token }` |
  | `GET /api/v1/spaces/{id}/invitations` | `admin` | `PendingInvitation[] { id, email, role, level, pageId, expiresAt }` (celles de l'espace) |
  | `DELETE /api/v1/invitations/{id}` | `admin` de l'espace | `204` |
  | `POST /api/v1/invitations/lookup` `InvitationToken { token }` | sans session | `InvitationInfo { spaceName, pageTitle, invitedBy, email }` ; `404 invalid_invitation` |
  | `POST /api/v1/invitations/accept` `InvitationToken { token }` | tout compte | `Accepted { spaceId, pageId }` ; `404 invalid_invitation` |

- Règles : nom d'espace rogné, 1 à 100 caractères (`422 invalid_name`) ; une invitation dure 7 jours (`INVITATION_TTL_MS`) ; le jeton n'apparaît que dans `InvitationCreated`, jamais dans une liste ni dans un journal ; `invalid_invitation` est la même réponse pour un jeton inconnu, échu, révoqué ou déjà accepté ; un `admin` ne change ni ne retire un `owner` (`403`).

- [ ] **Step 1 : écrire les tests qui échouent** (`tests/spaces.rs`).
  - `two_spaces_of_one_account_are_separate` ; `space_routes_by_role` (tableau : chaque route × `owner`, `admin`, `member`, `guest`, étranger, sans session → statut attendu ; étranger : `404`).
  - `admin_cannot_touch_owners` ; `last_owner_cannot_leave_or_be_demoted` ; `member_can_leave`.
  - `space_invitation_roundtrip` (inviter, `lookup`, accepter : la personne est membre ; second `accept` : `404`) ; `invitation_expires_after_7_days` (échéance avancée par SQL) ; `revoked_invitation_is_404` ; `accept_never_downgrades_a_role`.
  - `invitation_token_is_shown_once` (absent de `GET …/invitations` et des journaux).
  - `access_cache_follows_rev` (un membre retiré perd l'espace à l'appel suivant).
- [ ] **Step 2 : lancer.** Attendu : échec.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** `cargo fmt --all`, clippy, `cargo test --workspace`. Attendu : tout passe.
- [ ] **Step 5 : commit** `serveur : espaces, membres et invitations d'espace`.

### Task 9 : serveur, pages sous droits et corbeille

**Files :**
- Create : `crates/mementum-server/tests/permissions.rs`, `crates/mementum-server/tests/trash.rs`.
- Modify : `crates/mementum-server/src/access.rs`, `src/routes/pages.rs`, `src/lib.rs` (routes, plus de `vault_id` dans `AppState`), `src/types.rs`, `src/error.rs`, `tests/api.rs` (chaque test se donne un compte et un espace), `crates/mementum-store/src/spaces.rs` (`default_space` retiré), types régénérés ; côté client, le minimum pour que tout reste vert : `apps/web/src/api.ts`, `src/App.tsx` (l'arbre du premier espace de `me`), `apps/web/e2e/helpers.ts` (`createPage` vise par défaut l'espace du compte, section `space`).

**Interfaces :**
- Consumes : `access.rs` (Task 8).
- Produces, `access.rs` :
  ```rust
  impl AppState {
      /// Le nœud et les accès de `user` dessus. `404` s'il ne le voit pas ; `403 forbidden` si son niveau ne
      /// permet pas `action` ; `409 trashed` si `action` écrit (Edit, Rename, CreateChild, Reorder, Move, Share, Trash)
      /// dans une page à la corbeille.
      pub(crate) async fn node_access(&self, user: &AuthUser, id: Uuid, action: Action) -> Result<(Node, NodeAccess), ApiError>;
  }
  ```
- Produces, types : `PageNode` gagne `spaceId`, `level: Level`, `section: Section`, `trashedAt: number | null` ; son `parentId` est le parent **que la personne voit**, sinon `null`. `Tree { spaceId, nodes }` (plus de `vaultId`). `CreatePage` gagne `spaceId?` (obligatoire sans `parentId`) et `section?: "space" | "private"` (racine seulement ; défaut `private` ; refusé avec `parentId`).
- Produces, routes (toutes avec session) :

  | Route | Action demandée | Réponse |
  |---|---|---|
  | `GET /api/v1/spaces/{id}/tree` | un rôle dans l'espace | `Tree` : les nœuds visibles hors corbeille |
  | `GET /api/v1/spaces/{id}/trash` | un rôle dans l'espace | `PageNode[]` : les pages mises à la corbeille (pas leurs descendants) où l'on a `edit`, la plus récente d'abord |
  | `POST /api/v1/pages` | racine : rôle `member` ; sous-page : `CreateChild` sur le parent | `201 PageNode` ; nœud, accès et document écrits en une transaction |
  | `GET /api/v1/pages/{id}` | `Read` | `PageNode` |
  | `PATCH /api/v1/pages/{id}` | titre : `Rename` ; place sous le même parent : `Reorder` ; autre parent : `Move` sur la page et `CreateChild` sur le nouveau parent (racine : rôle `member`) | `PageNode` |
  | `POST /api/v1/pages/{id}/trash` | `Trash` | `PageNode` |
  | `POST /api/v1/pages/{id}/restore` | `Restore` | `PageNode` ; `409 parent_trashed` |
  | `DELETE /api/v1/pages/{id}` | `Delete` | `204` ; `409 not_trashed` |
  | `GET /api/v1/pages/{id}/mmd` | `Read` | comme au lot 1 |
  | `PUT /api/v1/pages/{id}/mmd` | `Edit` | comme au lot 1 |

  `GET /api/v1/tree` disparaît. Le document d'une page neuve est celui que l'acteur écrirait (`PageDoc::new_page`, instantané de `seq` 0) : la route le passe à `NewNode.doc`. Une page passée à la racine sans accès propre reçoit `(user:<qui la déplace>, full)`, pour ne pas devenir invisible de tous.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `permissions.rs`, `permission_matrix` : test généré. Sujets : `owner`, `admin`, `member`, invité, compte étranger à l'espace, sans session. Pages : privée d'un autre ; `(members, read)`, `(members, edit)`, `(members, full)` ; `(user:<sujet>, read | comment | edit | full)` ; les mêmes accès posés sur le parent (hérités). Actions : les dix routes ci-dessus, `PATCH` compté trois fois (titre, place, parent). Le statut attendu vient d'une fonction écrite dans le test d'après les tableaux de « Modèle des droits », sans appeler `mementum-acl` : `401` sans session, `404` sans accès, `403` sous le niveau, sinon le succès de la route.
  - `permissions.rs`, `foreign_page_is_indistinguishable_from_missing` : pour chaque route de page, la réponse pour une page d'un autre espace et pour un id inconnu a le même statut et le même corps.
  - `permissions.rs` : `tree_shows_only_readable_nodes_with_sections` ; `hidden_parent_is_not_leaked` (`parentId` `null`) ; `private_page_is_invisible_to_admin_and_instance_admin` ; `guest_cannot_create_root_page` ; `move_to_another_parent_requires_full` ; `page_moved_to_root_stays_visible_to_its_mover`.
  - `permissions.rs`, `patch_title_and_cycle_changes_nothing` (`PATCH` passe par `update_node` : `409 cycle`, titre inchangé).
  - `permissions.rs`, `new_page_doc_is_written_with_the_node` : après `POST /api/v1/pages`, l'instantané de `seq` 0 est en base avant toute connexion, et la première synchro rend un paragraphe vide sans qu'un second instantané soit écrit.
  - `trash.rs` : `trash_takes_subtree_and_restore_brings_it_back` (page avec deux sous-pages : après `trash`, aucune des trois dans l'arbre, la page seule dans la corbeille ; `PUT mmd` sur une sous-page : `409 trashed` ; `GET mmd` : `200` ; après `restore`, les trois reviennent à leur place) ; `restore_under_trashed_parent_is_409` ; `delete_requires_trash_and_full` ; `deleted_page_is_gone_for_good` (`404` partout, plus rien en base).
- [ ] **Step 2 : lancer.** Attendu : échec.
- [ ] **Step 3 : implémenter**, puis adapter `tests/api.rs` et le client au minimum.
- [ ] **Step 4 : mesurer la création d'une page** (report du lot 1 : 110 ms par page, serveur debug) : 200 créations par l'API sur un serveur `--release`, p50 et p95, consignés au registre. Au-dessus de 30 ms de médiane, chercher la cause avant de clore la tâche.
- [ ] **Step 5 : lancer** `cargo fmt --all`, clippy, `cargo test --workspace`, typecheck, Vitest, `playwright test`. Attendu : tout passe.
- [ ] **Step 6 : commit** `serveur : pages sous droits, arbre par espace, corbeille`.

### Task 10 : synchro sous droits

**Files :**
- Create : `crates/mementum-server/tests/sync_rights.rs`.
- Modify : `crates/mementum-sync/src/hub.rs`, `src/actor.rs`, `src/lib.rs`, `src/testkit.rs`, `tests/sync.rs`, `tests/schema.rs`, `tests/durability.rs` ; `crates/mementum-crdt/src/guard.rs` et son test ; `crates/mementum-store/src/docs.rs` (`append_update` retiré) ; `crates/mementum-server/src/routes/sync.rs`, `src/routes/pages.rs`, `src/routes/spaces.rs`, `src/access.rs` ; `apps/web/e2e/helpers.ts` (une WebSocket ouverte hors navigateur envoie le cookie de session).

**Interfaces :**
- Consumes : `node_access` (Task 9), `append_updates` (Task 4).
- Produces :
  ```rust
  // mementum-sync
  pub struct ConnInfo { pub client_schema: u32, pub can_write: bool, pub actor: Option<Uuid> }
  impl Hub {
      pub async fn serve_connection<S>(self: Arc<Self>, node_id: Uuid, conn: ConnInfo, socket: S) -> Result<(), SyncError>;
      /// Ferme toutes les connexions de ces pages (celles qui ont un acteur vivant). Les clients se reconnectent,
      /// et la poignée de main relit leurs droits.
      pub async fn kick(&self, node_ids: &[Uuid]);
      /// Arrête sans rien écrire les acteurs de pages supprimées.
      pub async fn forget(&self, node_ids: &[Uuid]);
      pub async fn live_pages(&self) -> Vec<Uuid>;
  }
  // testkit
  impl Client { pub async fn connect_with_headers(url: &str, headers: &[(&str, &str)]) -> Result<Client, String>; }
  // mementum-server, access.rs
  impl AppState {
      /// À appeler après toute écriture qui change des droits : `nodes` = les pages touchées, sous-arbres compris ;
      /// `None` = toutes les pages vivantes de l'espace.
      pub(crate) async fn rights_changed(&self, space_id: Uuid, nodes: Option<&[Uuid]>);
  }
  ```
- Règles :
  - une connexion écrit si `client_schema == SCHEMA_VERSION` **et** `can_write` ; sinon elle reçoit tout et ses mises à jour sont ignorées (le chemin du lot 1) ;
  - `/sync/{id}` : `401` sans session ; `404` si la personne ne lit pas la page ; `can_write` = le niveau permet `Edit` et la page n'est pas à la corbeille ; `actor` = le compte ;
  - `rights_changed` est appelé par : corbeille et restauration (le sous-arbre), changement de parent (le sous-arbre), retrait d'un membre et changement de rôle (tout l'espace) ; la suppression définitive appelle `forget` ;
  - la mise à jour d'un client et le correctif de dédoublonnage qui la suit sont écrits par un seul `append_updates` (M3), avec `actor` ; les écritures du serveur (`PUT mmd`) portent le compte qui les demande ;
  - M7 : `mementum-crdt` refuse la mise à jour d'un client qui touche à la `Y.Map` des métadonnées (`schemaVersion`) ; le client fautif est fermé, comme pour toute mise à jour refusée. Ce refus ne vaut que pour ce qui arrive d'un client : les mises à jour déjà en base, dont celle du serveur qui pose `schemaVersion`, se rechargent comme avant.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `mementum-crdt` : `client_cannot_overwrite_schema_version` ; `stored_pages_still_load_after_the_guard_tightens` (les fixtures `spec/mmd/fixtures/yjs/*.bin` et une page neuve du serveur passent toujours le chargement).
  - `mementum-sync`, `sync.rs` : `kick_closes_every_connection_of_the_page` ; `cannot_write_connection_is_ignored` (`can_write: false` : la mise à jour n'est ni écrite ni diffusée, la connexion reste ouverte et reçoit les autres) ; `client_update_and_dedupe_fix_are_one_write` (panne simulée sur la seconde ligne : aucune des deux en base, rien de diffusé) ; `updates_carry_the_actor`.
  - `mementum-server`, `sync_rights.rs` : `sync_requires_session` (`401`) ; `sync_on_unreadable_page_is_404` (page d'un autre espace, page privée d'un autre, id inconnu : même réponse) ; `reader_connection_is_read_only` ; `revoked_access_closes_live_connection` (B écrit sur une page partagée ; A lui retire son accès par le store puis `rights_changed` ; la connexion de B se ferme en moins de 2 s, sa reconnexion reçoit `404`, et ce qu'il envoie ensuite n'est pas dans le MMD) ; `trashed_page_is_read_only_for_everyone` (deux connexions ouvertes sur une sous-page ; le parent part à la corbeille : elles sont fermées, se reconnectent, et plus aucune mise à jour n'est écrite ; après restauration et reconnexion, l'écriture repasse) ; `removed_member_is_kicked` ; `deleted_page_actor_is_forgotten`.
- [ ] **Step 2 : lancer.** Attendu : échec.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** `cargo fmt --all`, clippy, `cargo test --workspace`, `cargo test --release -p mementum-sync -- --ignored`, `playwright test`. Attendu : tout passe.
- [ ] **Step 5 : commit** `synchro : droits vérifiés à la poignée de main, connexions fermées quand ils changent, écritures attribuées`.

### Task 11 : serveur, partage d'une page

**Files :**
- Create : `crates/mementum-server/src/routes/sharing.rs`, `crates/mementum-server/tests/sharing.rs`.
- Modify : `crates/mementum-server/src/lib.rs`, `src/types.rs`, `src/error.rs`, `src/routes/invitations.rs`, types régénérés.

**Interfaces :**
- Consumes : `node_access` (Task 9), `rights_changed` (Task 10), les invitations (Tasks 5 et 8).
- Produces, routes (toutes demandent `Share`, donc `full`) :

  | Route | Réponse |
  |---|---|
  | `GET /api/v1/pages/{id}/access` | `PageAccess { general: Level \| null, entries: AccessEntry[] { userId, name, email, level, inherited }, invitations: PendingInvitation[] }` |
  | `PUT /api/v1/pages/{id}/access/general` `SetGeneral { level: Level \| null }` | `PageAccess` ; `null` = « seulement les invités » (la ligne `members` de la page est retirée) |
  | `PUT /api/v1/pages/{id}/access/users/{userId}` `SetLevel { level }` | `PageAccess` ; `404` si le compte n'a aucun rôle dans l'espace |
  | `DELETE /api/v1/pages/{id}/access/users/{userId}` | `PageAccess` ; `404` si la page n'a pas de ligne propre pour ce compte |
  | `POST /api/v1/pages/{id}/invitations` `InviteToPage { email, level }` | `201 InvitationCreated { id, token }` |

  `DELETE /api/v1/invitations/{id}` accepte aussi qui a `Share` sur la page d'une invitation de page.
- Règles :
  - `entries` liste les comptes qui ont une ligne sur la page (`inherited: false`) ou sur un ancêtre (`inherited: true`, au niveau hérité) ; `general` est le niveau de la ligne `members` de la page ou d'un ancêtre ;
  - après tout changement, la page garde au moins un sujet au niveau `full`, à elle ou hérité (une ligne `members` compte : les admins y ont `full`) ; sinon `409 last_full_access`, rien n'est écrit ;
  - retirer le dernier accès d'un invité le sort de l'espace (`revoke_user_grant`) ;
  - chaque changement appelle `rights_changed` sur le sous-arbre de la page ;
  - une invitation de page ne révèle jamais si l'adresse a déjà un compte : la réponse est la même dans les deux cas (Ruling R47).

- [ ] **Step 1 : écrire les tests qui échouent** (`tests/sharing.rs`).
  - `sharing_requires_full` (`edit` : `403` sur les cinq routes ; sans accès : `404`).
  - `general_access_switch_moves_the_page_between_sections` (page privée → `members: read` : en « Espace » pour un autre membre, en lecture ; → `null` : elle redevient invisible pour lui).
  - `page_invitation_makes_a_guest_not_a_member` (B, invité sur une page, accepte : il voit la page et ses sous-pages ; l'arbre ne lui montre rien d'autre ; `GET …/members` : `403` ; créer une page à la racine : `403`).
  - `invited_member_gets_only_the_extra_level` ; `guest_leaves_the_space_with_his_last_grant`.
  - `last_full_access_cannot_be_removed` ; `inherited_entries_cannot_be_deleted_from_the_child` (`404`).
  - `page_invitation_answer_does_not_reveal_accounts` (même statut et même forme de corps pour une adresse connue et une inconnue).
  - `sharing_change_kicks_live_connections`.
- [ ] **Step 2 : lancer.** Attendu : échec.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** `cargo fmt --all`, clippy, `cargo test --workspace`. Attendu : tout passe.
- [ ] **Step 5 : commit** `serveur : partage d'une page (accès général, personnes, invitations)`.

### Task 12 : client, espaces, sections, corbeille et déplacement

**Files :**
- Create : `apps/web/src/sidebar/SpaceSwitcher.tsx`, `sidebar/Sections.tsx`, `sidebar/sections.ts`, `sidebar/sections.test.ts`, `sidebar/drag.ts`, `sidebar/drag.test.ts`, `apps/web/src/trash/TrashView.tsx`, `src/ui/Confirm.tsx`, `apps/web/e2e/spaces.spec.ts`.
- Modify : `apps/web/src/App.tsx`, `src/api.ts`, `src/router.ts`, `src/sidebar/PageTree.tsx`, `src/sidebar/TreeRow.tsx`, `src/page/PageView.tsx`, `src/icons.tsx`, `src/theme.css`, `apps/web/e2e/helpers.ts`, `e2e/pages.spec.ts`.

**Interfaces :**
- Consumes : `flattenRows`, `TreeRow` (Task 2) ; `useSession` (Task 7) ; les routes des Tasks 8 et 9.
- Produces :
  ```ts
  // router.ts : "/" (le dernier espace ouvert, sinon le premier), "/s/:spaceId", "/s/:spaceId/trash",
  //             "/s/:spaceId/settings" (Task 13), "/p/:id" (inchangé : l'espace est celui de la page)
  // api.ts
  export function createSpace(body: CreateSpace): Promise<SpaceInfo>;
  export function getTree(spaceId: string): Promise<Tree>;
  export function getTrash(spaceId: string): Promise<PageNode[]>;
  export function getPage(id: string): Promise<PageNode>;
  export function trashPage(id: string): Promise<PageNode>;
  export function restorePage(id: string): Promise<PageNode>;
  export function deletePage(id: string): Promise<void>;
  // sections.ts
  export function bySection(nodes: PageNode[]): Record<Section, PageNode[]>;
  // drag.ts
  export type Drop = { parentId: string | null; afterId: string | null };
  export function dropTarget(rows: Row[], dragged: string, overIndex: number, zone: "before" | "inside" | "after"): Drop | null; // null : dépôt refusé
  ```
- Comportement :
  - sélecteur d'espace en tête de barre latérale : les espaces de `me`, « Créer un espace » ; l'espace ouvert est gardé dans `localStorage` (`mementum.space`) ;
  - trois sections dans l'ordre « Espace », « Partagé », « Privé », chacune avec son arbre ; « + » sur « Espace » et « Privé » seulement ; un invité ne voit que « Partagé » ; une section vide de « Partagé » ne s'affiche pas ;
  - menu « … » d'une ligne : « Mettre à la corbeille », montré si le niveau le permet ;
  - « Corbeille » en bas de barre latérale : liste, « Restaurer », « Supprimer définitivement » après confirmation dans la page (`Confirm`, jamais `window.confirm`) ; une page ouverte qui est à la corbeille porte un bandeau « Cette page est à la corbeille » et son bouton « Restaurer » ;
  - glisser-déposer d'une ligne, à l'intérieur d'une même section : changer de place ou de parent. `dropTarget` refuse de déposer une page sur elle-même ou sous une de ses descendantes, et un dépôt que le niveau interdit (changer de parent sans `full`). Le serveur fait foi : sur refus, la ligne revient et le message s'affiche. Au clavier : « Déplacer vers le haut / le bas » dans le menu « … ».

- [ ] **Step 1 : écrire les tests qui échouent.**
  - Vitest : `sections.test.ts`, `groups_top_nodes_by_section` ; `drag.test.ts`, `drop_inside_makes_last_child`, `drop_before_and_after_pick_the_right_sibling`, `drop_on_own_descendant_is_refused`, `reparent_without_full_is_refused`.
  - e2e, `spaces.spec.ts` : `two_spaces_are_isolated` (une page créée dans « Pro » n'apparaît pas dans « Perso ») ; `private_page_is_invisible_to_another_member` ; `space_page_is_visible_to_another_member` ; `guest_sees_only_the_shared_section` (accès posé par l'API) ; `trash_and_restore_from_the_sidebar` ; `delete_forever_asks_first` ; `drag_a_page_under_another` (l'arbre du serveur porte le nouveau parent) ; `last_space_is_remembered`.
- [ ] **Step 2 : lancer.** Attendu : échec.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** typecheck, Vitest, `playwright test`, `e2e:budgets` (le budget de l'arbre peuplé tient toujours). Attendu : tout passe.
- [ ] **Step 5 : commit** `web : sélecteur d'espace, sections, corbeille, déplacement des pages`.

### Task 13 : client, partage, membres et lecture seule

**Files :**
- Create : `apps/web/src/share/ShareMenu.tsx`, `share/levels.ts`, `apps/web/src/settings/SpaceSettings.tsx`, `apps/web/src/session/InvitePage.tsx`, `apps/web/e2e/sharing.spec.ts`.
- Modify : `apps/web/src/api.ts`, `src/router.ts` (`/invite`), `src/session/SessionGate.tsx`, `src/session/SignupPage.tsx`, `src/page/PageView.tsx`, `src/editor/schemaGate.ts`, `src/editor/collab.ts`, `src/editor/ReadOnlyBanner.tsx`, `src/App.tsx`, `src/theme.css`.

**Interfaces :**
- Consumes : les routes des Tasks 8 et 11 ; `getPage` (Task 12) ; le passage en lecture seule du lot 1 (R35 : session recréée sur un `Y.Doc` neuf).
- Produces :
  ```ts
  // api.ts
  export function getAccess(pageId: string): Promise<PageAccess>;
  export function setGeneralAccess(pageId: string, level: Level | null): Promise<PageAccess>;
  export function setUserLevel(pageId: string, userId: string, level: Level): Promise<PageAccess>;
  export function removeUserAccess(pageId: string, userId: string): Promise<PageAccess>;
  export function inviteToPage(pageId: string, body: InviteToPage): Promise<InvitationCreated>;
  export function getMembers(spaceId: string): Promise<Member[]>;
  export function updateMember(spaceId: string, userId: string, body: UpdateMember): Promise<Member>;
  export function removeMember(spaceId: string, userId: string): Promise<void>;
  export function inviteToSpace(spaceId: string, body: InviteToSpace): Promise<InvitationCreated>;
  export function getSpaceInvitations(spaceId: string): Promise<PendingInvitation[]>;
  export function revokeInvitation(id: string): Promise<void>;
  export function lookupInvitation(token: string): Promise<InvitationInfo>;
  export function acceptInvitation(token: string): Promise<Accepted>;
  // levels.ts
  export const LEVEL_LABELS: Record<Level, string>; // « Peut lire », « Peut commenter », « Peut modifier », « Accès complet »
  export function invitationLink(token: string): string; // `${location.origin}/invite#${token}` : le jeton reste dans le fragment, jamais envoyé dans une URL au serveur
  ```
- Comportement :
  - bouton **Partager** en haut à droite de la page, montré à qui a `full`. Le menu suit le §4.3 amendé, de haut en bas : champ d'invitation (adresse et niveau), qui affiche le lien créé et « Copier le lien d'invitation » ; liste des accès, niveau modifiable, « Retirer », les accès hérités marqués « hérité » et non modifiables ici ; **Accès général** : « Seulement les invités » ou « Tous les membres de l'espace » et son niveau ; « Copier le lien » de la page. Pas d'onglet « Publier » : la publication est en P3 ;
  - réglages de l'espace (`/s/:spaceId/settings`, lien montré aux `admin` et `owner`) : nom, membres (rôle, « Retirer »), invitations en attente (« Révoquer »), invitation par adresse et rôle, qui affiche le lien ; « Quitter l'espace » pour soi ;
  - `/invite#<jeton>` : `lookup`, puis « `<Nom>` t'invite dans l'espace … » ou « … sur la page … » ; avec une session, « Accepter » mène à l'espace ou à la page ; sans session, inscription ou connexion, puis acceptation ;
  - lecture seule par droit : `PageView` ouvre en lecture seule si le niveau ne permet pas de modifier, avec le bandeau « Tu peux lire cette page, pas la modifier » (celui du schéma périmé garde son texte) ;
  - avant chaque connexion de synchro, avec la relecture de `/api/v1/info`, le client relit `getPage(id)` : `404` → la session s'arrête et la page affiche « Tu n'as plus accès à cette page » ; niveau tombé sous `edit` → lecture seule pour de bon, sur un `Y.Doc` neuf. Un niveau qui monte prend effet à la prochaine ouverture de la page. Sur `401`, la connexion est demandée par-dessus la page, sans la démonter (Task 7).

- [ ] **Step 1 : écrire les tests qui échouent** (e2e, `sharing.spec.ts`).
  - `invite_by_link_then_edit_together` : A invite une adresse sur une page et copie le lien ; B ouvre le lien, crée son compte, arrive sur la page, tape ; A voit le texte ; la barre latérale de B n'a que « Partagé ».
  - `reader_cannot_type` (`contenteditable="false"`, aucune trame d'écriture, bandeau).
  - `revoked_user_loses_the_page` : B a la page ouverte ; A retire son accès ; en moins de 5 s B voit « Tu n'as plus accès à cette page » ; ce que B tape entre-temps n'est pas dans le MMD du serveur.
  - `downgrade_to_read_makes_the_open_editor_read_only`.
  - `member_invited_to_the_space_sees_space_pages` ; `owner_changes_a_role_and_removes_a_member` ; `revoked_invitation_link_says_so`.
  - `general_access_makes_a_private_page_a_space_page`.
  - `expired_session_keeps_what_was_typed` : cookie effacé, reconnexion forcée, B tape ; la connexion est demandée ; après s'être reconnecté, ce qu'il a tapé est dans le MMD du serveur.
- [ ] **Step 2 : lancer.** Attendu : échec.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** typecheck, Vitest, `playwright test`, `e2e:budgets`. Attendu : tout passe.
- [ ] **Step 5 : commit** `web : menu Partager, réglages de l'espace, invitations par lien, lecture seule par droit`.

### Task 14 : documentation du lot

**Files :**
- Modify : `README.md` (état du projet, démarrage : créer le premier compte, variables `MEMENTUM_SIGNUP` et `MEMENTUM_PUBLIC_URL`, récupération d'un mot de passe par la CLI, prochaines étapes), `CLAUDE.md` (pièges rencontrés au lot 2, section « Tester » : comptes des e2e), `docs/specs/2026-11-09-mementum-design.md` (§9 « Avancement de la P1 » : ce que le lot 2 livre et ses limites connues ; §7.2 : ce qui est déjà configurable ; §5.6 : une phrase sur `spaces.rev` et le cache), `crates/mementum-server/src/lib.rs` (tableau des routes en tête de fichier), `docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md` (bilan).

- [ ] **Step 1 : relire le registre du lot** et reporter chaque Ruling qui change un comportement visible là où un lecteur le cherchera.
- [ ] **Step 2 : vérifier** que chaque commande écrite dans le README et le `CLAUDE.md` tourne telle quelle.
- [ ] **Step 3 : commit** `docs : lot 2, comptes, espaces, droits et corbeille`.

---

## Fin du lot

- [ ] **Relecture de toute la branche** par un relecteur indépendant, avec une passe dédiée à la sécurité : authentification, sessions, invitations, fuites d'existence, chemins qui liraient le store sans passer par `access.rs`.
- [ ] **Toutes les suites relancées sur un clone neuf**, étape par étape comme la CI, `e2e:budgets` compris, et `cargo deny check advisories` à la main (Ruling R44).
- [ ] **Base du lot 1 réelle** : copier le dossier de données de l'instance que le propriétaire regarde, ouvrir la **copie** avec le binaire du lot 2, créer le premier compte, vérifier que ses pages sont là. Ne jamais ouvrir l'original sans son accord.
- [ ] **Bilan du lot** au registre : Rulings, mesures, ce qui est porté au lot 3 ; `README.md` et §9 de la conception à jour.
