# Mementum — Phase 0 (spike de validation) : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** prouver, chiffres à l'appui, que les choix techniques de la conception (§5.2) tiennent, ou les corriger, avant d'écrire Mementum pour de vrai.

Les quatre questions à trancher :
1. BlockNote + Yjs ⇄ serveur Rust yrs ⇄ SQLite, en temps réel ;
2. aller-retour MMD sans perte sur 20 types de blocs ;
3. un agent qui édite pendant qu'un humain tape, sans rien perdre ;
4. une base de 100 000 lignes en JSON + index sur une machine ARM de classe Pi 4.

Plus un nouveau benchmark Yjs/Loro.

**Architecture :** un workspace Rust jetable (`spike/`) contient :
- un serveur Axum qui parle le protocole y-websocket et persiste les mises à jour Yjs dans SQLite ;
- une crate de conversion MMD ⇄ arbre de blocs ⇄ Yjs au format BlockNote ;
- deux crates de benchmark.

Une petite app Vite + React + BlockNote sert de client. Les tests e2e Playwright pilotent deux navigateurs et l'« agent » (appels HTTP). Chaque mesure est consignée dans `spike/RESULTS.md`.

**Tech Stack :**
- Rust stable (édition 2024), axum, tokio, yrs, sqlx (SQLite), pulldown-cmark, proptest, criterion, loro ;
- Node LTS + pnpm, Vite, React, TypeScript, BlockNote, yjs, y-websocket, loro-crdt, Playwright.

**Spec :** `docs/specs/2026-11-09-mementum-design.md` : §5.2 (pile), §5.4 (synchro), §5.5 (bases), §5.10 (budgets), §6.2 à §6.4 (MMD, opérations, fusion), §9 (P0), §10 (tests).

## Global Constraints

- **Code jetable** : tout vit sous `spike/` dans le dépôt `Mementum/`. On garde les leçons (RESULTS.md, fixtures, tests d'aller-retour), pas le code.
- Licences autorisées : MIT, Apache-2.0, MPL-2.0, et GPL-3.0 seulement pour `@blocknote/xl-multi-column` (compatible avec l'AGPL-3.0 du projet). **Interdit** : Tiptap Pro et tout paquet propriétaire.
- **Yjs 13.6.x** (pas la 14 en RC), **Loro 1.x**, BlockNote : dernière 0.x stable.
- Le `yrs::Doc` côté serveur est **toujours** créé avec `OffsetKind::Utf16`, pour compter les positions comme Yjs dans le navigateur.
- Nom du fragment Yjs partagé avec BlockNote : `FRAGMENT = "document-store"`.
- Ports : serveur `8080`, Vite `5173`. Base SQLite du spike : `spike/data/spike.db`.
- **Budgets** (spec §5.10, à vérifier tels quels) :
  - page jamais vue (réseau local) < 300 ms. Le budget « page déjà en cache < 100 ms » dépend du cache client SQLite WASM, absent de la P0 : il sera mesuré en P1 ;
  - frappe → visible chez un autre utilisateur < 150 ms ;
  - base de 100 000 lignes : première fenêtre < 500 ms, changer un filtre ou un tri < 300 ms ;
  - mémoire du serveur au repos < 150 Mo ; démarrage à froid < 1 s.
- **Machine ARM cible** : Raspberry Pi 4 (4 Go), sinon un VPS ARM64 2 vCPU / 4 Go (Hetzner CAX11). Les mesures du PC Windows sont notées à titre de référence, jamais comme verdict.
- Toutes les commandes s'exécutent en bash (Git Bash sur le PC), depuis `Mementum/`.

## Review Focus

1. **Accents et emojis** : une édition d'agent sur « Café 🚀 déjà » ne doit jamais couper un caractère. C'est le piège des positions UTF-16 / octets entre Yjs et yrs. Testé en Task 5.
2. **Coupure réseau** : un client qui édite hors ligne puis se reconnecte doit converger sans perte. Testé en Task 2.
3. **Agent et humain dans le même bloc** : l'édition de l'agent doit être refusée proprement (409 avec un extrait à jour), jamais appliquée de travers. Testé en Task 5.
4. **Redémarrage du serveur pendant l'édition** : rien de ce qui a été accusé reçu ne doit être perdu, et les clients se resynchronisent. Testé en Task 1.
5. **Bloc inconnu** (venu d'une version plus récente) : il doit traverser MMD → Yjs → BlockNote → Yjs → MMD intact. Testé en Tasks 3 et 4.

---

## Correspondance MMD ⇄ BlockNote (utilisée par les Tasks 2, 3 et 4)

| MMD (forme canonique) | Type BlockNote | Props / contenu |
|---|---|---|
| paragraphe | `paragraph` | contenu en ligne |
| `#` à `####` | `heading` | `level` 1–4 |
| `- ` | `bulletListItem` | enfants indentés de 2 espaces |
| `1. ` (numéros consécutifs) | `numberedListItem` | idem |
| `- [ ] ` / `- [x] ` | `checkListItem` | `checked` |
| `<toggle title="…">enfants</toggle>` | `toggleListItem` | titre = contenu en ligne, enfants = children |
| `> ` | `quote` | contenu en ligne |
| `<callout icon="…" color="…">texte</callout>` | `callout` *(custom)* | `icon`, `color` ; contenu en ligne |
| ```` ```lang ```` | `codeBlock` | `language` |
| `$$` … `$$` | `equation` *(custom)* | `latex` |
| `---` | `divider` *(custom si absent de BlockNote)* | — |
| `![alt](url)` | `image` | `url`, `caption` = alt |
| `<file src="…" name="…"/>` | `file` | `url`, `name` |
| `<bookmark url="…"/>` | `bookmark` *(custom)* | `url` |
| tableau GFM | `table` | `tableContent` |
| `<columns><column>…</column></columns>` | `columnList` / `column` (xl-multi-column) | `width` |
| `<page id="…">Titre</page>` | `pageLink` *(custom)* | `pageId`, `title` |
| `<database id="…" view="…"/>` | `databaseRef` *(custom)* | `databaseId`, `view` |
| `<synced ref="…">enfants</synced>` | `syncedBlock` *(custom)* | `ref` ; enfants |
| `<toc/>` | `toc` *(custom)* | — |
| `<block type="x" id="…" data="…"/>` | `unknown` *(custom)* | `originalType`, `data` (base64 JSON), id du bloc conservé |

**En ligne** :
- `**gras**` → `bold` ; `*italique*` → `italic` ; `~~barré~~` → `strike` ; `` `code` `` → `code` ;
- `<u>…</u>` → `underline` ; `<span color="red">…</span>` → `textColor` ; `[texte](url)` → lien ;
- `<mention type="page|user|date" id="…">Libellé</mention>` → contenu en ligne custom `mention` (`kind`, `id`, `label`) ;
- `$…$` → contenu en ligne custom `math` (`latex`).

Pour le texte littéral : `<` s'écrit `&lt;`, et `*`, `_`, `[`, `` ` `` sont échappés par `\`.

---

### Task 1 : serveur de synchro Yjs (axum + yrs + SQLite)

**Files :**
- Create : `spike/Cargo.toml` (workspace), `spike/.gitignore` (`target/`, `data/`, `node_modules/`), `spike/RESULTS.md`
- Create : `spike/server/Cargo.toml`, `spike/server/src/main.rs`, `spike/server/src/store.rs`, `spike/server/src/rooms.rs`, `spike/server/src/ws.rs`
- Test : `spike/server/tests/sync.rs`

**Interfaces :**
- Produit :
  - `store::Store::open(path: &Path) -> Result<Store>`
  - `Store::load_doc(&self, page: &str) -> Result<yrs::Doc>` (instantané + mises à jour, Doc en `OffsetKind::Utf16`)
  - `Store::append_update(&self, page: &str, update: &[u8], origin: &str) -> Result<i64>`
  - `rooms::Rooms::get_or_load(&self, page: &str) -> Result<Arc<Room>>`
  - `Room::transact(&self, origin: &str, f: impl FnOnce(&mut yrs::TransactionMut, &yrs::XmlFragmentRef))`, qui persiste puis diffuse la mise à jour à tous les clients WebSocket de la page
  - routes `GET /health` → `ok`, et WebSocket `GET /sync/{page}` au protocole y-websocket (message 0 = sync, 1 = awareness)
- Tables SQLite : `doc_updates(page TEXT, seq INTEGER, update_blob BLOB, origin TEXT, created_at INTEGER, PRIMARY KEY(page, seq))` et `doc_snapshots(page TEXT PRIMARY KEY, seq INTEGER, state_blob BLOB)`. Compaction en instantané toutes les **500** mises à jour.

- [ ] **Step 1 : initialiser le dépôt.** `git init -b main` dans `Mementum/` s'il n'existe pas, puis brancher `spike/phase-0`. Créer le workspace avec les fichiers ci-dessus et un `RESULTS.md` qui contient un titre et un tableau vide « Mesure | Budget | PC | ARM | Verdict ».

- [ ] **Step 2 : écrire les tests qui échouent** dans `spike/server/tests/sync.rs`. Ils lancent le serveur sur un port libre avec une base temporaire, et les clients de test sont des `yrs::Doc` connectés par `tokio-tungstenite` qui parlent `yrs::sync`.
  - `two_clients_converge` : A insère « Bonjour » dans `FRAGMENT` et B le reçoit en moins de **150 ms** ;
  - `persists_across_restart` : A écrit, le serveur est arrêté puis relancé sur la même base, et un nouveau client C lit « Bonjour » *(Review Focus 4)* ;
  - `reconnect_after_restart_keeps_unsent_edits` : A écrit pendant que le serveur est arrêté, puis se reconnecte, et C voit le texte *(Review Focus 4)* ;
  - `compacts_after_500_updates` : après 501 mises à jour, `doc_snapshots` a une ligne et `doc_updates` ≤ 1 ligne pour la page.

- [ ] **Step 3 : vérifier l'échec.** Lancer `cargo test -p spike-server --test sync`. Attendu : les 4 tests en FAIL (serveur absent).

- [ ] **Step 4 : implémenter** `store.rs`, `rooms.rs` (une `Room` en mémoire par page, chargée paresseusement, libérée 60 s après le départ du dernier client), `ws.rs` (échange sync step 1/2, relais des updates et de l'awareness via `yrs::sync`) et `main.rs` (axum sur `0.0.0.0:8080`, chemin de la base passé par `SPIKE_DB`).

- [ ] **Step 5 : vérifier le succès.** Lancer `cargo test -p spike-server --test sync`. Attendu : 4 PASS.

- [ ] **Step 6 : mesurer.** Ajouter un binaire de mesure `spike/server/src/bin/measure.rs`. Il génère une page de **10 000 paragraphes** de 80 caractères, puis mesure :
  - la mémoire au repos (RSS) ;
  - le démarrage à froid jusqu'à `/health` OK ;
  - le chargement de la page depuis SQLite (p50/p95 sur 20 essais).

  La page est écrite sous l'id `bench-10k` dans `spike/data/spike.db`, que la Task 2 réutilise. Le lancer avec `cargo run --release -p spike-server --bin measure` sur le PC, puis sur la machine ARM, et reporter les 3 lignes dans `RESULTS.md` contre les budgets « mémoire < 150 Mo », « démarrage < 1 s » et « page jamais vue < 300 ms ».

- [ ] **Step 7 : commit**
```bash
git add spike/ && git commit -m "spike: serveur de synchro Yjs (axum + yrs + SQLite)"
```

---

### Task 2 : éditeur BlockNote branché sur le serveur, et fixtures

**Files :**
- Create : `spike/web/` (Vite React TS) : `package.json`, `vite.config.ts`, `src/main.tsx`, `src/App.tsx`, `src/schema.tsx` (blocs et contenus en ligne custom), `src/collab.ts`
- Create : `spike/web/playwright.config.ts`, `spike/web/e2e/collab.spec.ts`, `spike/web/e2e/export-fixture.spec.ts`
- Create : `spike/fixtures/blocknote/all-blocks.input.json`, qui reproduit à l'identique le contenu de `spike/fixtures/mmd/all-blocks.mmd` (Task 3)
- Produit : `spike/fixtures/yjs/all-blocks.bin` et `spike/fixtures/blocknote/all-blocks.json`

**Interfaces :**
- Consomme : WebSocket `ws://localhost:8080/sync/{page}` (Task 1).
- Produit :
  - URL `http://localhost:5173/?page=<id>&user=<nom>` ;
  - `src/schema.tsx` exporte `schema`, avec tous les types du tableau de correspondance (les blocs custom ont un rendu minimal : libellé + props) ;
  - handle de test `window.__mementum = { doc: Y.Doc, editor, provider }` ;
  - la page `?fixture=all-blocks` remplace le document par `all-blocks.input.json`.

- [ ] **Step 1 : écrire les tests e2e qui échouent** dans `e2e/collab.spec.ts`. `playwright.config.ts` lance `cargo run --release -p spike-server` et `pnpm dev` via `webServer`, avec deux contextes de navigateur sur la même page.
  - `typing_shows_up_remotely` : 50 frappes dans A, avec horodatage `Date.now()` à l'envoi côté A et à la réception côté B (événement `update` du `Y.Doc` de B). Le **p95 doit rester < 150 ms** et le texte final être identique dans A et B ;
  - `remote_cursor_visible` : le curseur nommé de A apparaît chez B ;
  - `offline_edits_merge_on_reconnect` : A passe hors ligne (`context.setOffline(true)`), tape « hors ligne », B tape « en ligne » ; A revient en ligne et les deux documents sont identiques et contiennent les deux textes *(Review Focus 2)* ;
  - `open_10k_block_page` : la page `?page=bench-10k` (10 000 blocs, générée en Task 1) s'affiche (premier bloc visible) en moins de **300 ms** après la navigation, avec un serveur déjà démarré.

- [ ] **Step 2 : vérifier l'échec.** Lancer `pnpm --dir spike/web exec playwright test e2e/collab.spec.ts`. Attendu : FAIL (app absente).

- [ ] **Step 3 : implémenter l'app.**
  - `collab.ts` crée `Y.Doc`, `WebsocketProvider("ws://localhost:8080/sync", page, doc)` et un utilisateur `{ name, color }` ;
  - `App.tsx` appelle `useCreateBlockNote({ schema, collaboration: { provider, fragment: doc.getXmlFragment("document-store"), user } })` ;
  - `schema.tsx` déclare les blocs custom (`createReactBlockSpec`) et les contenus en ligne `mention` et `math` du tableau de correspondance, plus `@blocknote/xl-multi-column`.

  Si `heading` niveau 4 ou `divider` manquent dans BlockNote, les ajouter en custom et le noter dans RESULTS.md.

- [ ] **Step 4 : vérifier le succès.** Relancer la commande du Step 2. Attendu : 4 PASS. Reporter p95 frappe → distant et l'ouverture de 10 000 blocs dans RESULTS.md.

- [ ] **Step 5 : exporter les fixtures.** `e2e/export-fixture.spec.ts` ouvre `?fixture=all-blocks` puis écrit :
  - `Y.encodeStateAsUpdate(doc)` dans `spike/fixtures/yjs/all-blocks.bin` ;
  - `editor.document` dans `spike/fixtures/blocknote/all-blocks.json`.

  Lancer `pnpm --dir spike/web exec playwright test e2e/export-fixture.spec.ts`. Attendu : les deux fichiers existent et le JSON contient 28 blocs de premier niveau, soit le même nombre que dans `all-blocks.mmd`.

- [ ] **Step 6 : commit**
```bash
git add spike/web spike/fixtures && git commit -m "spike: éditeur BlockNote collaboratif + fixtures"
```

---

### Task 3 : convertisseur MMD ⇄ arbre de blocs (Rust pur)

**Files :**
- Create : `spike/mmd/Cargo.toml`, `spike/mmd/src/lib.rs`, `spike/mmd/src/model.rs`, `spike/mmd/src/parse.rs`, `spike/mmd/src/render.rs`
- Create : `spike/fixtures/mmd/all-blocks.mmd` (contenu exact ci-dessous)
- Test : `spike/mmd/tests/roundtrip.rs`

**Interfaces :**
- Produit (`model.rs`) :
```rust
pub struct Block { pub id: String, pub kind: BlockKind, pub content: Vec<Inline>, pub children: Vec<Block> }
pub enum BlockKind {
    Paragraph, Heading { level: u8 }, BulletItem, NumberedItem, Todo { checked: bool },
    Toggle, Quote, Callout { icon: String, color: String }, Code { lang: String }, Equation { latex: String },
    Divider, Image { url: String, alt: String }, File { url: String, name: String }, Bookmark { url: String },
    Table { rows: Vec<Vec<Vec<Inline>>> }, Columns, Column { width: Option<f32> },
    PageLink { page_id: String, title: String }, DatabaseRef { database_id: String, view: String },
    Synced { reference: String }, Toc, Unknown { type_name: String, data: String },
}
pub enum Inline { Text { text: String, marks: Marks }, Link { href: String, children: Vec<Inline> },
                  Mention { kind: String, id: String, label: String }, Math { latex: String } }
pub struct Marks { pub bold: bool, pub italic: bool, pub strike: bool, pub code: bool, pub underline: bool, pub color: Option<String> }
```
- `parse(src: &str) -> Result<Vec<Block>, MmdError>` (ids lus depuis `^bN` / `id="…"` sinon générés en UUIDv7)
- `render(blocks: &[Block], opts: RenderOpts) -> Rendered`, avec `RenderOpts { anchors: bool }` et `Rendered { markdown: String, anchors: Vec<(String, String)> }` (alias `bN` → id de bloc, numérotés en profondeur d'abord, enfants compris : `b1` = premier bloc, puis ses enfants, puis le bloc suivant)
- `MmdError { line: usize, message: String }`

- [ ] **Step 1 : écrire la fixture** `spike/fixtures/mmd/all-blocks.mmd`, exactement ainsi (forme canonique, 28 blocs de premier niveau) :

````markdown
# Spécification v1

Livrer un espace de travail **rapide** où l’équipe et ses *agents* écrivent ensemble, en ~~différé~~ direct, avec du `code`, du <u>souligné</u>, de la <span color="red">couleur</span>, un [lien](https://mementum.dev) et $E = mc^2$.

## Mentions

Voir <mention type="page" id="pg_spec01">Spécification</mention>, demander à <mention type="user" id="us_lenny">Lenny</mention> avant le <mention type="date" id="2026-12-01">1 déc. 2026</mention>.

### Listes

- Premier point
  - Point imbriqué
- Second point

1. Étape un
2. Étape deux

- [x] Éditeur en blocs
- [ ] Partage des coffres

#### Blocs riches

<toggle title="Détails techniques">
Le serveur tourne sur un Raspberry Pi 4.
</toggle>

> Une citation courte.

<callout icon="⚠️" color="yellow">
Penser à la migration des données.
</callout>

```rust
fn main() {
    println!("Café 🚀 déjà");
}
```

$$
\int_0^1 x^2 \, dx = \frac{1}{3}
$$

---

![Logo Mementum](https://mementum.dev/logo.png)

<file src="https://mementum.dev/spec.pdf" name="spec.pdf"/>

<bookmark url="https://mementum.dev"/>

| Nom | Statut |
| --- | --- |
| Prototype | En cours |
| Benchmark | Terminé |

<columns>
<column>
Colonne de gauche.
</column>
<column>
Colonne de droite.
</column>
</columns>

<page id="pg_child01">Sous-page</page>

<database id="db_tasks" view="Tableau"/>

<synced ref="b_origin01">
Contenu synchronisé.
</synced>

<toc/>

<block type="future_widget" id="b_unknown01" data="eyJ4IjoxfQ=="/>
````

- [ ] **Step 2 : écrire les tests qui échouent** dans `tests/roundtrip.rs` :
  - `fixture_is_canonical` : `render(parse(fixture), RenderOpts{anchors:false}).markdown == fixture` (octet pour octet) ;
  - `fixture_has_28_top_level_blocks` ;
  - `fixture_covers_all_kinds` : chaque variante de `BlockKind` apparaît au moins une fois ;
  - `unknown_block_survives` : le bloc `future_widget` garde `type_name`, `data` et l'id `b_unknown01` après aller-retour *(Review Focus 5)* ;
  - `anchors_render_and_parse` : avec `anchors:true`, le paragraphe « Livrer… » se termine par ` ^b2`, le callout porte `id="b17"`, et re-parser redonne les mêmes ids ;
  - `prop_roundtrip` (proptest, 512 cas) : arbre aléatoire (profondeur ≤ 3, toutes variantes, texte Unicode incluant `é`, `🚀`, `<`, `*`) → `render` → `parse` → arbre égal, ids compris quand `anchors:true`.

- [ ] **Step 3 : vérifier l'échec.** Lancer `cargo test -p spike-mmd`. Attendu : FAIL (fonctions absentes).

- [ ] **Step 4 : implémenter `parse` et `render`.**
  - **Parseur** en deux niveaux.
    1. Un pré-découpage ligne à ligne repère les balises de bloc (`<toggle`, `<callout`, `<columns>`, `<column>`, `<synced`, ainsi que les balises auto-fermantes `<file`, `<bookmark`, `<page`, `<database`, `<toc`, `<block`) placées seules en début de ligne, et les traite récursivement.
    2. Les segments Markdown restants passent à `pulldown-cmark` (tables, cases à cocher, barré, maths), et les balises en ligne (`<u>`, `<span>`, `<mention>`) sont reconnues dans le flux HTML en ligne.
  - **Rendu** : il produit exactement la forme canonique de la fixture : une ligne vide entre blocs, enfants de liste indentés de 2 espaces, numéros consécutifs, et un attribut `id` seulement si `anchors` est actif ou si le bloc est `Unknown`.

- [ ] **Step 5 : vérifier le succès.** Lancer `cargo test -p spike-mmd`. Attendu : tout PASS, y compris les 512 cas proptest.

- [ ] **Step 6 : commit**
```bash
git add spike/mmd spike/fixtures/mmd && git commit -m "spike: convertisseur MMD <-> arbre de blocs"
```

---

### Task 4 : MMD ⇄ Yjs au format BlockNote (côté serveur)

**Files :**
- Create : `spike/mmd/src/ydoc.rs` (dépendance `yrs`)
- Test : `spike/mmd/tests/ydoc.rs`, `spike/web/e2e/server-written.spec.ts`

**Interfaces :**
- Consomme : `Block`, `parse` et `render` (Task 3) ; les fixtures de la Task 2 ; `Rooms` et `Room::transact` (Task 1).
- Produit :
  - `ydoc::FRAGMENT: &str = "document-store"` ;
  - `ydoc::read_blocks(txn: &impl yrs::ReadTxn, frag: &yrs::XmlFragmentRef) -> Vec<Block>` ;
  - `ydoc::write_blocks(txn: &mut yrs::TransactionMut, frag: &yrs::XmlFragmentRef, blocks: &[Block])`, qui remplace tout le contenu ;
  - `ydoc::block_text(block: &Block) -> String` (texte brut du contenu en ligne, utilisé par la Task 5).

  La structure suit BlockNote : `blockGroup > blockContainer[id] > <type props…>` (contenu en ligne dans un `XmlText` avec les attributs de marque), suivi d'un `blockGroup` optionnel pour les enfants. Les noms suivent le tableau de correspondance.
- Route de test du serveur : `PUT /pages/{id}/mmd`, dont le corps MMD est écrit via `Room::transact("import", …)`.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `reads_blocknote_fixture` : décoder `fixtures/yjs/all-blocks.bin` dans un `Doc` en `OffsetKind::Utf16`, puis `render(read_blocks(...), anchors:false).markdown == all-blocks.mmd` ;
  - `write_then_read_is_identity` : `write_blocks(parse(all-blocks.mmd))` dans un Doc neuf, puis `read_blocks` redonne le même arbre ;
  - `unknown_block_survives_yjs` : le bloc `future_widget` traverse `write_blocks` → `read_blocks` intact *(Review Focus 5)* ;
  - e2e `server_written_doc_opens_in_blocknote` (`e2e/server-written.spec.ts`) : `PUT /pages/t4/mmd` avec la fixture, puis ouvrir `?page=t4`. `editor.document` doit être égal à `fixtures/blocknote/all-blocks.json` (ids ignorés sauf `b_unknown01`) ;
  - e2e `browser_edit_reads_back_as_mmd` (même fichier, écrit maintenant mais lancé en Task 5 car il a besoin de `GET /pages/{id}`) : taper « ok » dans le premier paragraphe, puis vérifier que le MMD du serveur ne diffère de la fixture que par « ok ».

- [ ] **Step 2 : vérifier l'échec.** Lancer `cargo test -p spike-mmd --test ydoc`. Attendu : FAIL.

- [ ] **Step 3 : implémenter `read_blocks`, `write_blocks` et `block_text`**, ajouter la route `PUT /pages/{id}/mmd` au serveur, puis passer les tests Rust.

- [ ] **Step 4 : vérifier le succès.** Lancer `cargo test -p spike-mmd --test ydoc`, puis `pnpm --dir spike/web exec playwright test e2e/server-written.spec.ts -g server_written_doc_opens_in_blocknote`. Attendu : PASS. Noter dans RESULTS.md chaque type qui a demandé une adaptation.

- [ ] **Step 5 : commit**
```bash
git add spike/mmd spike/server spike/web/e2e && git commit -m "spike: MMD <-> Yjs au format BlockNote"
```

---

### Task 5 : l'agent, pair CRDT côté serveur (`update_page`)

**Files :**
- Create : `spike/server/src/agent.rs`, `spike/server/src/revisions.rs`
- Modify : `spike/server/src/main.rs` (routes)
- Test : `spike/server/tests/agent.rs`, `spike/web/e2e/agent-vs-human.spec.ts`

**Interfaces :**
- Consomme : `Room::transact` (Task 1) ; `read_blocks`, `write_blocks`, `block_text`, `parse`, `render` (Tasks 3 et 4).
- Produit (HTTP JSON) :
  - `GET /pages/{id}?anchors=block` → `200 {"markdown": String, "revision": String, "anchors": {"b1": "<id>", …}}`
  - `POST /pages/{id}/update`, corps `{"base_revision": String, "agent": {"id": String, "run": String}, "operations": [Op]}`, avec `Op = {"op":"str_replace","old":String,"new":String} | {"op":"insert","after":"bN","markdown":String}` ;
    - `200 {"revision": String, "touched_markdown": [String], "warnings": []}` ;
    - `409 {"error":"conflict","blocks":[{"anchor":"bN","markdown":String}]}` ;
    - `422 {"error":"no_match"|"ambiguous_match","candidates":[String]}`.
  - `revision = hex(sha256(state_vector_v1))[..16]`. `revisions.rs` garde en mémoire, pour les **100** dernières révisions par page : alias → id de bloc et hash du texte de chaque bloc.
- Règles de traitement :
  - toutes les opérations d'un appel tiennent dans **une** transaction yrs d'origine `agent:{id}:{run}` ;
  - `str_replace` cherche `old` dans `block_text` de chaque bloc. Le texte doit être unique sur la page, sinon 422, et une correspondance à cheval sur deux blocs donne 422 `no_match`. L'édition se traduit en suppression puis insertion **aux positions UTF-16** dans le `XmlText` du bloc, en gardant les marques du premier caractère remplacé ;
  - avant d'appliquer, si le hash du bloc touché diffère de celui de `base_revision`, la réponse est 409 (rebase seulement si le bloc est inchangé, spec §6.3).

- [ ] **Step 1 : écrire les tests qui échouent.**
  - Rust (`tests/agent.rs`) :
    - `str_replace_applies` ;
    - `insert_after_anchor` ;
    - `no_match_returns_candidates` (422 avec les 3 blocs les plus proches) ;
    - `ambiguous_match_rejected` ;
    - `unicode_safe` : bloc « Café 🚀 déjà », `str_replace` de « déjà » par « prêt ». Le résultat doit être « Café 🚀 prêt », et un client Yjs connecté doit lire la même chaîne *(Review Focus 1)* ;
    - `conflict_when_block_changed` : lecture à r1, un client modifie le bloc ciblé, puis l'agent envoie `base_revision = r1`. Attendu : 409, document inchangé *(Review Focus 3)* ;
    - `rebase_when_other_block_changed` : même scénario mais sur un autre bloc. Attendu : 200.
  - e2e (`agent-vs-human.spec.ts`) : l'humain tape 200 caractères dans le bloc 1 (30 ms entre frappes). Pendant ce temps, le test enchaîne 20 appels agent (`str_replace` dans le bloc 5 et `insert` après le bloc 3), chacun relisant d'abord la révision. À la fin :
    - le texte de l'humain est complet et dans l'ordre ;
    - les 20 éditions de l'agent sont présentes ;
    - le MMD du serveur est égal au rendu du document vu par le navigateur ;
    - le p95 entre la réponse 200 de l'agent et la visibilité dans le navigateur reste **< 150 ms**.

- [ ] **Step 2 : vérifier l'échec.** Lancer `cargo test -p spike-server --test agent`. Attendu : FAIL.

- [ ] **Step 3 : implémenter `agent.rs` et `revisions.rs`, et brancher les routes.**

- [ ] **Step 4 : vérifier le succès.** Lancer `cargo test -p spike-server --test agent`, puis `pnpm --dir spike/web exec playwright test e2e/agent-vs-human.spec.ts e2e/server-written.spec.ts`. Attendu : PASS. Reporter le p95 agent → navigateur dans RESULTS.md.

- [ ] **Step 5 : commit**
```bash
git add spike/server spike/web/e2e && git commit -m "spike: agent pair CRDT (str_replace, insert, conflits)"
```

---

### Task 6 : base de 100 000 lignes (SQLite JSON + index d'expression)

**Files :**
- Create : `spike/dbbench/Cargo.toml`, `spike/dbbench/src/lib.rs` (schéma, jeu de données, moteur de filtre), `spike/dbbench/src/main.rs` (bench CLI)
- Test : `spike/dbbench/tests/query.rs`

**Interfaces :**
- Produit :
  - schéma : `rows(id TEXT PRIMARY KEY, source TEXT, props TEXT /* JSON */, created_at INTEGER)` et `relations(from_row TEXT, prop TEXT, to_row TEXT)`, avec des index d'expression sur `json_extract(props,'$.status')`, `'$.due'`, `'$.assignee'` et `'$.priority'` ;
  - `build_dataset(conn: &SqlitePool, rows: usize, seed: u64) -> Result<()>` : `rows` tâches avec 12 propriétés (title, status ∈ {À faire, En cours, Terminé}, assignee parmi 50 personnes, due, priority 1–5, tags multi parmi 20, done bool, estimate f64, url, notes, created_at, relation `project` vers 1 000 projets) ;
  - `enum Filter { And(Vec<Filter>), Or(Vec<Filter>), Cond { prop: String, op: Op, value: serde_json::Value } }`, avec `enum Op { Eq, Neq, Contains, Lt, Gt, Before, After, IsEmpty }` et une imbrication ≤ **3** niveaux ;
  - `struct Query { source: String, filter: Option<Filter>, sort: Vec<(String, Dir)>, limit: usize, after: Option<Cursor> }` ;
  - `query(pool, &Query) -> Result<PageOfRows>` (pagination par curseur sur (valeurs de tri, id)) ;
  - `group_counts(pool, source, prop) -> Result<Vec<(String, i64)>>` ;
  - `rollup_sum(pool, prop_relation, prop_value) -> Result<Vec<(String, f64)>>`.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `filter_matches_reference` (différentiel) : sur 2 000 lignes, 200 filtres aléatoires à 3 niveaux. Les résultats SQL doivent être égaux à une évaluation Rust en mémoire des mêmes lignes ;
  - `cursor_pagination_is_stable` : parcourir 2 000 lignes par pages de 200 redonne chaque id exactement une fois ;
  - `budgets_100k` (`#[ignore]`, à lancer sur la machine cible) : 100 000 lignes, p95 sur 20 essais. Première fenêtre de 200 lignes triée par `due` **< 500 ms** ; filtre ET/OU à 2 niveaux + tri **< 300 ms** ; `group_counts(status)` < 300 ms.

- [ ] **Step 2 : vérifier l'échec.** Lancer `cargo test -p spike-db`. Attendu : FAIL.

- [ ] **Step 3 : implémenter le schéma, `build_dataset`, la compilation `Filter` → SQL paramétré (`json_extract`), `query`, `group_counts` et `rollup_sum`.** Le binaire `spike-db bench --rows 100000` affiche un tableau p50/p95 pour : première fenêtre, filtre, filtre + tri, groupes, rollup, insertion d'une ligne, modification d'une cellule.

- [ ] **Step 4 : vérifier le succès.** Lancer `cargo test -p spike-db`, puis sur la machine ARM `cargo test --release -p spike-db -- --ignored budgets_100k`. Attendu : PASS. Copier la sortie de `cargo run --release -p spike-db -- bench --rows 100000` (PC et ARM) dans RESULTS.md. **Si un budget échoue sur ARM** : refaire le bench avec une vraie table SQL par source (une colonne par propriété) et noter les deux résultats, car c'est le repli prévu par la spec (§5.5).

- [ ] **Step 5 : commit**
```bash
git add spike/dbbench spike/RESULTS.md && git commit -m "spike: bench base 100k lignes SQLite JSON"
```

---

### Task 7 : benchmark CRDT, Yjs/yrs contre Loro (page de 10 000 blocs)

**Files :**
- Create : `spike/crdtbench/Cargo.toml`, `spike/crdtbench/benches/page.rs` (criterion : yrs contre loro)
- Create : `spike/crdtbench-js/package.json`, `spike/crdtbench-js/bench.mjs` (yjs contre loro-crdt, Node)
- Test : `spike/crdtbench/tests/converge.rs`

**Interfaces :**
- Modèle de la page :
  - yrs/yjs : structure BlockNote (`blockGroup > blockContainer > paragraph > XmlText`) ;
  - Loro : `LoroTree` (arbre déplaçable), avec par nœud une `LoroMap { type }` et une `LoroText`.
- Charges de travail identiques des deux côtés :

  | Code | Charge |
  |---|---|
  | W1 | construire 10 000 blocs de 80 caractères |
  | W2 | taille de l'instantané encodé (octets) |
  | W3 | chargement de l'instantané |
  | W4 | mémoire après chargement (RSS en Rust, `heapUsed` en Node) |
  | W5 | 2 pairs × 1 000 éditions concurrentes aléatoires (insertions/suppressions de caractères + **50 déplacements de blocs**), puis fusion |
  | W6 | taille du journal de mises à jour après 10 000 éditions |

  Avec yrs/yjs, un déplacement est fait par suppression puis réinsertion du contenu ; Loro utilise `mov`.

- [ ] **Step 1 : écrire le test qui échoue** `converge.rs`. Après W5, les deux pairs ont le même texte pour chaque bloc, avec yrs comme avec Loro, et aucun bloc n'est dupliqué ni perdu après les 50 déplacements (vérifier aussi qu'aucun bloc déplacé à la fois par les deux pairs n'apparaît en double avec yrs ; le noter s'il le fait).

- [ ] **Step 2 : vérifier l'échec.** Lancer `cargo test -p spike-crdt-bench`. Attendu : FAIL.

- [ ] **Step 3 : implémenter les charges en Rust et en JS.** Lancer `cargo bench -p spike-crdt-bench` et `node spike/crdtbench-js/bench.mjs`, chacun écrivant un CSV dans `spike/crdtbench/results/`.

- [ ] **Step 4 : vérifier le succès et décider.** Le test de convergence passe. Reporter W1 à W6 dans RESULTS.md.

  **Règle de décision** (spec §5.2 et §11) : on garde Yjs, **sauf** si Loro est au moins **2× meilleur** sur W3, W4 ou W5, **et** si une liaison Loro ⇄ ProseMirror utilisable existe (vérifier `loro-prosemirror` : dernière version, éditeur de démo qui fonctionne). Écrire le verdict en une ligne dans RESULTS.md.

- [ ] **Step 5 : commit**
```bash
git add spike/crdtbench spike/crdtbench-js spike/RESULTS.md && git commit -m "spike: benchmark yjs/yrs vs loro"
```

---

### Task 8 : verdict de fin de phase 0

**Files :**
- Modify : `spike/RESULTS.md`, `docs/specs/2026-11-09-mementum-design.md` (§5.2, §5.5, §11, et §9 si besoin)

- [ ] **Step 1 : compléter RESULTS.md.** Une ligne par budget (§5.10) et par question P0, avec la valeur PC, la valeur ARM et un verdict « confirmé » ou « corrigé ». Ajouter la liste des adaptations BlockNote (types custom, limites), le verdict CRDT et le verdict de stockage des bases.

- [ ] **Step 2 : répercuter dans la conception.** Mettre à jour chaque choix corrigé de §5.2 et §5.5 avec la mesure qui l'a fait changer, et retirer de §11 les risques levés.

  Critère de sortie de la P0 (spec §9) : les 4 mesures sont chiffrées et chaque choix de §5.2 est confirmé ou corrigé.

- [ ] **Step 3 : commit**
```bash
git add spike/RESULTS.md docs/specs && git commit -m "spike: verdict de la phase 0"
```
