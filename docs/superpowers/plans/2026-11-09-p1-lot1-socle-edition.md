# Mementum — Phase 1, lot 1 « Socle et édition en direct » : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** poser le vrai code de Mementum (structure du §8) et livrer une première chose utilisable de bout en bout : créer des pages, les ranger en arbre et les écrire à plusieurs en direct dans BlockNote, sans rien perdre, avec un format MMD fidèle à tout ce que l'éditeur sait représenter.

**Architecture :**
- Un workspace Cargo à la racine contient 7 crates :
  - `mementum-schema` : le registre unique des types de blocs ;
  - `mementum-md` : MMD ⇄ arbre de blocs, en Rust pur ;
  - `mementum-crdt` : le document de page, seule crate qui voit yrs ;
  - `mementum-store` : SQLite ;
  - `mementum-sync` : un acteur par page et le protocole y-websocket ;
  - `mementum-server` : HTTP et WebSocket ;
  - `mementum-cli` : le binaire `mementum`.
- Le client `apps/web` (React, Vite, BlockNote) parle au serveur par une API REST et une WebSocket par page.
- Le spike de la phase 0 n'est **pas** réutilisé comme base. On en reprend les fixtures, les tests d'aller-retour et les leçons (`spike/RESULTS.md`, section « Ce qui change pour la P1 »).

**Tech Stack :**
- Rust stable (édition 2024) : axum, tokio, yrs 0.28 (feature `sync`), sqlx (SQLite), pulldown-cmark, uuid (v7), sha2, ts-rs, clap, proptest, sysinfo ;
- Node 24 + pnpm : Vite, React 19, TypeScript, BlockNote 0.57, yjs 13.6, y-websocket, y-prosemirror 1.x, Vitest, Playwright.

**Spec :** `docs/specs/2026-11-09-mementum-design.md`. Sections utiles : §4.5 (blocs de la P1), §5.2 à §5.4, §5.10 (budgets), §6.2 (MMD), §8 (structure), §9 (P1), §10 (tests), §11 (risques). Lire aussi `spike/RESULTS.md` (sections « Ce qui change pour la P1 » et « Relecture finale ») et les pièges de `CLAUDE.md`.

**Hors de ce lot** (lots suivants, dans cet ordre) :
2. comptes, organisations, coffres, droits et corbeille ;
3. collaboration et blocs riches (présence affichée, commentaires, historique, liens `[[…]]`, mentions insérables, fichiers téléversés, icônes et couvertures de page, onglets, fil d'Ariane, Mermaid, intégrations oEmbed) ;
4. recherche ;
5. agents (REST complet, MCP v1, jetons) ;
6. imports et export ;
7. livraison (Docker, binaires, CI ARM, application embarquée dans le binaire).

## Global Constraints

- **Langue** : documentation, commentaires et messages de commit en français, accents compris. Identifiants de code en anglais.
- **Commits** : petits, un sujet chacun, **sans aucune ligne d'attribution d'outil** (ni `Co-Authored-By`, ni `Claude-Session`, ni « Generated with »).
- **Licence du projet** : AGPL-3.0.
  - Dépendances permises : MIT, Apache-2.0, MPL-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, BSL-1.0, 0BSD, CC0-1.0.
  - GPL-3.0 tolérée **seulement** pour `@blocknote/xl-multi-column`.
  - **Interdit** : Tiptap Pro et tout paquet propriétaire.
- **Versions** :
  - Rust 1.96 ou plus, édition 2024 ;
  - Node 24 ou plus, pnpm 11 ou plus (`corepack pnpm` si pnpm n'est pas dans le PATH) ;
  - **yjs fixé en `~13.6.33`** (jamais 13.7 ni 14) ;
  - BlockNote `~0.57`, par le chemin Yjs 13 (`withCollaboration` de `@blocknote/core/yjs`, y-prosemirror 1.x) ;
  - yrs 0.28 avec la feature `sync`.
- **Yjs** :
  - tout `yrs::Doc` est créé avec `OffsetKind::Utf16` ;
  - nom du fragment partagé : `FRAGMENT = "document-store"` ;
  - les `get_or_insert_*` s'appellent **avant** d'ouvrir une transaction ;
  - aucune position n'est jamais calculée sur `XmlTextRef::get_string()`.
- **Identifiants** : UUIDv7 pour les entités du serveur (`uuid::Uuid::now_v7()`).
- **SQLite** : WAL, `synchronous=FULL`, `foreign_keys=ON`, `busy_timeout=5000`. Requêtes sqlx à l'exécution (`sqlx::query`, pas les macros `query!`). Valeurs toujours liées en paramètres.
- **Réseau** : le serveur écoute sur `127.0.0.1:8080` par défaut. Le lot 1 n'a **pas d'authentification** : ne jamais écouter sur une autre interface par défaut. En développement, Vite tourne sur le port 5173.
- **Durabilité** : une mise à jour est écrite dans SQLite **avant** d'être diffusée. Ce qui a été diffusé n'est jamais perdu.
- **Budgets** (§5.10, tels quels) :
  - « Ouvrir une page jamais vue (réseau local) < 300 ms » ;
  - « Frappe → visible chez un autre utilisateur (même région) < 150 ms » ;
  - « Mémoire du serveur au repos < 150 Mo » ;
  - « Démarrage à froid du binaire < 1 s ».
- **Design A « Papier »** (§4.3) :
  - Inter partout ;
  - barre latérale : fond `#F6F5F1`, bordure `#E8E6E1`, éléments de 30–32 px ;
  - page centrée, ≈ 760 px au plus ;
  - titre en 38 px gras ;
  - texte `#2F2E2B`, texte secondaire `#66655F` ;
  - accent `#23704F` ;
  - pas d'emoji dans l'interface (icônes vectorielles).
- **Dépôt** :
  - fins de ligne LF (`.gitattributes` existe) ;
  - le workspace racine exclut `spike/` (`exclude = ["spike"]`) ;
  - les fixtures vivent dans `spec/mmd/fixtures/` (§8 : `spec/mmd/`).
- **Registre des écarts** : chaque écart au plan va dans `docs/superpowers/logs/2026-11-09-p1-lot1-ledger.md`, sous la forme `Ruling: <décision> — <pourquoi> — <coût si faux>`.

## Review Focus

1. **Serveur tué en pleine frappe** (`kill -9`, coupure de courant du processus) : tout ce qu'un autre client a reçu est encore là après le redémarrage. Test : Task 6, `killed_server_loses_nothing_broadcast`.
2. **Onglet resté ouvert après une mise à jour du serveur** (schéma plus ancien que celui du serveur) : l'onglet passe en lecture seule et n'efface jamais les blocs qu'il ne connaît pas. Tests : Task 5, `stale_schema_client_is_read_only`, et Task 7, `stale_schema_is_read_only`.
3. **Caractères spéciaux tapés dans l'éditeur** (`* _ \ < > & { } $ | ^ [ ] ~`, emoji, accents, texte de droite à gauche) : l'aller-retour MMD est exact. Tests : Task 2, générateur proptest, et Task 8, `typed_special_characters_roundtrip`.
4. **Coupure réseau pendant que deux onglets écrivent** : les deux convergent sans perte au retour du réseau. Test : Task 7, `offline_edits_merge_on_reconnect`.
5. **Arbre de pages maltraité** : déplacer une page sous sa propre descendante est refusé. Un titre vide devient « Sans titre », et un titre de plus de 500 caractères est refusé. Tests : Task 4, `move_refuses_cycle`, et Task 6, `title_rules`.

---

## Format MMD de la P1 (décisions de ce plan)

Ces règles **complètent** le §6.2 de la conception (règles de la P0 et « Limites connues »), qui reste valable. La Task 2 les reporte dans le §6.2. Le principe : **tout ce que BlockNote sait représenter a une forme MMD, et la relecture est exacte**. Les props égales à leur valeur par défaut (déclarée dans `mementum-schema`) ne s'écrivent pas.

| Type BlockNote | Forme MMD | Props portées par la forme | Autres props (nom MMD) |
|---|---|---|---|
| `paragraph` | une ligne | — | `bg`, `color`, `align` |
| `heading` | `#` × niveau, puis une espace (niveaux 1 à 4) | `level` | `bg`, `color`, `align`, `toggle` (`isToggleable`) |
| `bulletListItem` | `- ` | — | `bg`, `color`, `align` |
| `numberedListItem` | `N. ` (N = `start` sur le premier élément de la liste, s'il vaut autre chose que 1) | `start` | `bg`, `color`, `align` |
| `checkListItem` | `- [ ] ` ou `- [x] ` | `checked` | `bg`, `color`, `align` |
| `toggleListItem` | `<toggle title="…">` … `</toggle>` | contenu en ligne = `title` | `bg`, `color`, `align` (attributs) |
| `quote` | `> ` | — | `bg`, `color` |
| `codeBlock` | clôture de backticks + langue | `language` | — |
| `divider` | `---` | — | — |
| `image` | `![légende](url)` | `url`, `caption` | `name`, `preview` (`showPreview`), `width` (`previewWidth`), `align`, `bg` |
| `video` | `<video src="…" caption="…"/>` | `url`, `caption` | `name`, `preview`, `width`, `align`, `bg` |
| `audio` | `<audio src="…" caption="…"/>` | `url`, `caption` | `name`, `preview`, `bg` |
| `file` | `<file src="…" name="…" caption="…"/>` | `url`, `name`, `caption` | `bg` |
| `table` | tableau GFM ; la ligne `{…}` qui suit porte `header-rows` (omis s'il vaut 1), `widths` (`"120,,80"`, vide = auto) et `color` | lignes | `header-rows`, `widths`, `color` |
| `callout` | `<callout icon="…" color="…">` : 1ᵉʳ paragraphe = contenu, puis enfants ; contenu vide et 1ᵉʳ enfant paragraphe : `<p/>` en tête | `icon`, `color` | — |
| `equation` | `$$`, le LaTeX, puis `$$` | `latex` | — |
| `bookmark` | `<bookmark url="…"/>` | `url` | — |
| `columnList` / `column` | `<columns>` / `<column width="…">` (`width` omis s'il vaut 1) | `width` | — |
| `pageLink` | `<page id="…">titre</page>` | `pageId`, `title` | — |
| `databaseRef` | `<database id="…" view="…"/>` | `databaseId`, `view` | — |
| `syncedBlock` | `<synced ref="…">` … `</synced>` | `ref` | — |
| `toc` | `<toc/>` | — | — |
| `unknown` et **tout type inconnu du serveur** | `<block type="…" id="…" data="…"/>`, `data` = base64 du JSON `{"attrs":{…},"content":[…],"children":[…]}` du nœud Yjs d'origine | — | — |

**Props hors forme.**
- Sur une forme en ligne, elles s'écrivent dans une liste `{…}` en fin de première ligne, avant l'ancre : `Texte {align="center" color="red"} ^b3`. Sur une forme à balise, elles s'écrivent en attributs.
- Clés triées par ordre alphabétique.
- Valeurs : chaîne toujours entre guillemets (échappée comme un attribut) ; nombre et booléen sans guillemets (`width=320`, `preview=false`).
- Une prop inconnue du registre garde son nom BlockNote tel quel et son type d'après cette règle. C'est le « sac de props » : rien ne se perd.

**Enfants de tout bloc.**
- Les enfants d'un bloc à forme en ligne ou à clôture suivent sa dernière ligne, **sans ligne vide**.
- Ils sont indentés de 2 espaces. Pour un élément numéroté, l'indentation vaut la largeur du marqueur (« 1. » = 3, « 10. » = 4).
- Entre deux enfants, la séparation est la même qu'au premier niveau.
- Les formes à balise (`toggle`, `callout`, `columns`, `column`, `synced`) contiennent leurs enfants entre leurs balises.

**Contenu en ligne.**
- Règles de la P0 pour `**`, `*`, `~~`, `` ` ``, `<u>`, `<span color>` et `$…$`.
- Surlignage : `<span bg="…">`. Couleur et surlignage ensemble : `<span bg="…" color="…">`.
- Saut de ligne (`hardBreak`) : `<br/>`.
- Lien : `[texte](href)` si le `href` ne contient ni blanc, ni `(`, `)`, `<`, `>`, `\`, `&` ; sinon `<a href="…">texte</a>`.
- Deux liens voisins de même `href` restent deux liens.

**Échappements.** Le jeu de la P0 s'applique, plus `{` (écrit `\{`), puisqu'une liste `{…}` en fin de ligne porte des props.

**Lecture stricte.**
- `parse` n'accepte que la forme canonique.
- Tout le reste est une erreur `MmdError { line, message }`, avec un message en français qui dit quoi écrire. Exemples :
  - marqueurs `*` ou `+` ;
  - titres soulignés (setext) ;
  - `#####` ;
  - code indenté ;
  - balise ou clôture non fermée ;
  - indentation incohérente ;
  - liste `{…}` mal formée.
- L'import de Markdown quelconque (Obsidian, Notion) est un autre convertisseur, prévu au lot 6.

**Identifiants.**
- `^xyz` en fin de ligne et `id="…"` sur une balise donnent l'id du bloc, tel quel. La traduction des alias `bN` est faite par l'API des agents, au lot 5.
- Un bloc sans id est lu avec `id = ""`. `mementum-crdt` lui donne un UUIDv7 à l'écriture.

---

### Task 1 : socle du dépôt et registre de schéma (`mementum-schema`)

**Files :**
- Create :
  - `Cargo.toml` : workspace, `members = ["crates/*"]`, `exclude = ["spike"]`, `resolver = "3"`, `[workspace.package] edition = "2024"`, `rust-version = "1.96"`, `license = "AGPL-3.0-only"`, `[workspace.dependencies]` ;
  - `deny.toml` (licences de la Global Constraints, `bans` sur les doublons majeurs en avertissement) ;
  - `.github/workflows/ci.yml` ;
  - `crates/mementum-schema/Cargo.toml`, `src/lib.rs`, `src/blocks.rs`, `src/inline.rs` ;
  - `spec/schema/blocks.json` (généré).
- Test : `crates/mementum-schema/tests/registry.rs`.

**Interfaces :**
- Produces (lib `mementum_schema`) :
```rust
pub const SCHEMA_VERSION: u32 = 1;
pub enum PropType { String, Int, Float, Bool }
pub enum PropDefault { Str(&'static str), Int(i64), Float(f64), Bool(bool) }
pub struct PropSpec { pub name: &'static str, pub mmd: &'static str, pub ty: PropType, pub default: Option<PropDefault>, pub in_form: bool }
pub enum ContentKind { Inline, Table, None }
pub enum MmdForm { Line, Heading, Bullet, Numbered, Check, Quote, Code, Divider, Image, Table, Equation, Tag(&'static str), SelfClosing(&'static str), Unknown }
pub struct BlockSpec { pub name: &'static str, pub content: ContentKind, pub form: MmdForm, pub props: &'static [PropSpec], pub wrapped: bool }
pub fn blocks() -> &'static [BlockSpec];
pub fn block(name: &str) -> Option<&'static BlockSpec>;
pub const STYLES: &[&str]; // bold, italic, underline, strike, code, textColor, backgroundColor
pub const INLINE_TYPES: &[&str]; // link, mention, math, hardBreak
pub fn schema_json() -> serde_json::Value;
```
  - `wrapped` vaut `false` pour `columnList` et `column`, qui ne sont pas dans un `blockContainer`, et `true` partout ailleurs.
  - `schema_json()` renvoie `{"version":1,"blocks":[{"name","content","props":[{"name","type","default"}]}],"styles":[…],"inline":[…]}`.

- [ ] **Step 1 : écrire les tests qui échouent** (`tests/registry.rs`) :
  - `registry_covers_p1_types` : les noms de `blocks()` sont exactement : `paragraph`, `heading`, `bulletListItem`, `numberedListItem`, `checkListItem`, `toggleListItem`, `quote`, `codeBlock`, `divider`, `image`, `video`, `audio`, `file`, `table`, `callout`, `equation`, `bookmark`, `columnList`, `column`, `pageLink`, `databaseRef`, `syncedBlock`, `toc`, `unknown` (24) ;
  - `defaults_match_blocknote` : les défauts de `paragraph` valent `backgroundColor="default"`, `textColor="default"` et `textAlignment="left"` ; `heading` a `level` (Int, 1) et `isToggleable` (Bool, false) ; `image` a `showPreview` (Bool, true) ; `column` a `width` (Float, 1.0) ;
  - `mmd_names_unique_per_block` : aucun nom MMD en double dans un même type ;
  - `schema_json_is_committed` : `spec/schema/blocks.json` est égal à `schema_json()` sérialisé avec une indentation de 2 espaces et un saut de ligne final ; avec `UPDATE_SCHEMA=1`, le test réécrit le fichier.
- [ ] **Step 2 : lancer** `cargo test -p mementum-schema`. Attendu : échec de compilation, les éléments n'existent pas encore.
- [ ] **Step 3 : écrire le registre.**
  - Les props et défauts des types déjà présents se relèvent dans `spec/mmd/fixtures/blocknote/p0-all-blocks.json` (copie de `spike/fixtures/blocknote/all-blocks.json`, faite ici).
  - Ceux de `video`, `audio` et `numberedListItem.start` se relèvent dans `@blocknote/core` 0.57 (`apps/web/node_modules` après la Task 7, ou `spike/web/node_modules` dès maintenant).
  - La Task 7 vérifie la correspondance avec BlockNote.
  - Générer `spec/schema/blocks.json` avec `UPDATE_SCHEMA=1`.
- [ ] **Step 4 : écrire la CI** (`ci.yml`).
  - Job `rust`, sur `ubuntu-latest` et `windows-latest` : `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
  - Job `deny`, sur ubuntu : `cargo deny check licenses bans`.
  - Le job `web` arrive à la Task 7.
- [ ] **Step 5 : lancer** `cargo test -p mementum-schema && cargo clippy --workspace --all-targets -- -D warnings && cargo deny check licenses`. Attendu : tout passe.
- [ ] **Step 6 : commit** `schema: registre des 24 types de blocs de la P1 et socle du workspace`.

### Task 2 : `mementum-md`, arbre de blocs ⇄ MMD

**Files :**
- Create :
  - `crates/mementum-md/Cargo.toml` (dépend de `mementum-schema`, `pulldown-cmark`, `base64`, `serde_json` ; feature `arbitrary` qui expose le générateur proptest) ;
  - `src/lib.rs`, `src/model.rs`, `src/inline.rs`, `src/attrs.rs` (listes `{…}` et attributs), `src/parse.rs`, `src/render.rs`, `src/arb.rs` (générateur, derrière la feature) ;
  - `spec/mmd/fixtures/p0-all-blocks.mmd` (copie de `spike/fixtures/mmd/all-blocks.mmd`) ;
  - `spec/mmd/fixtures/p1-edge-cases.mmd`.
- Modify : `docs/specs/2026-11-09-mementum-design.md`, §6.2. Y ajouter la section « Format MMD de la P1 » de ce plan, et retirer des « Limites connues » ce que ce lot lève.
- Test : `crates/mementum-md/tests/{canonical.rs, roundtrip.rs, strict.rs}`.

**Interfaces :**
- Consumes : `mementum_schema::{block, blocks, BlockSpec, PropSpec, PropType, MmdForm}`.
- Produces :
```rust
pub struct Block { pub id: String, pub kind: String, pub props: Props, pub content: Content, pub children: Vec<Block> }
pub type Props = std::collections::BTreeMap<String, PropValue>;
pub enum PropValue { Str(String), Int(i64), Float(f64), Bool(bool) }
pub enum Content { None, Inline(Vec<Inline>), Table(Table) }
pub struct Table { pub header_rows: u32, pub column_widths: Vec<Option<f64>>, pub rows: Vec<Vec<Cell>> }
pub struct Cell { pub content: Vec<Inline>, pub props: Props }
pub enum Inline { Text { text: String, styles: Styles }, Link { href: String, children: Vec<Inline> }, Mention { kind: String, id: String, label: String }, Math { latex: String }, HardBreak }
pub struct Styles { pub bold: bool, pub italic: bool, pub underline: bool, pub strike: bool, pub code: bool, pub text_color: Option<String>, pub background_color: Option<String> }
pub struct MmdError { pub line: usize, pub message: String }
pub struct RenderOpts { pub anchors: bool }
pub struct Rendered { pub markdown: String, pub aliases: std::collections::BTreeMap<String, String> }
pub fn parse(mmd: &str) -> Result<Vec<Block>, MmdError>;
pub fn render(blocks: &[Block], opts: RenderOpts) -> Rendered;
pub fn normalize(blocks: Vec<Block>) -> Vec<Block>;
pub fn block_text(block: &Block) -> String;
#[cfg(feature = "arbitrary")] pub mod arb { pub fn tree() -> impl proptest::strategy::Strategy<Value = Vec<mementum_md::Block>>; }
```
  - Un bloc de type inconnu est lu en `kind = "unknown"`, avec `props["originalType"]` et `props["data"]`.
  - `normalize` retire les props égales au défaut du registre et les textes vides, et fusionne les textes voisins de mêmes styles.
  - `block_text` donne le texte brut : `U+FFFC` pour une mention ou une formule, `\n` pour un `HardBreak`.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `canonical.rs`, `fixtures_are_canonical` : pour chaque `spec/mmd/fixtures/*.mmd`, `render(&parse(f)?, RenderOpts { anchors: false }).markdown == f`, octet pour octet.
  - `canonical.rs`, `edge_cases_cover_registry` : chaque type du registre apparaît dans `p0-all-blocks.mmd` ou dans `p1-edge-cases.mmd`.
  - `canonical.rs`, `p1_forms` : une assertion par règle nouvelle, avec ces chaînes exactes :
    - `"Parent\n  Enfant\n"` : paragraphe avec un paragraphe enfant ;
    - `"a<br/>b\n"` : saut de ligne ;
    - `"Texte {align=\"center\" color=\"red\"}\n"` : props hors forme ;
    - `"## Titre {toggle=true}\n"` : titre repliable ;
    - `"3. Trois\n4. Quatre\n"` : `start = 3` ;
    - `"Prix \\{x\\}\n"` : accolades échappées ;
    - `"<a href=\"https://ex.com/a b\">lien</a>\n"` : lien dont le `href` contient une espace ;
    - `"<callout icon=\"💡\" color=\"blue\">\n<p/>\nSuite.\n</callout>\n"` : callout vide suivi d'un paragraphe enfant ;
    - `"| A |\n| --- |\n| 1 |\n{header-rows=0 widths=\"120\"}\n"` : tableau sans en-tête, colonne de 120 ;
    - `"Fond <span bg=\"yellow\">jaune</span>\n"` : surlignage ;
    - `"Texte {foo=1 bar=\"x\"}\n"` : prop inconnue, relue avec son type.
  - `roundtrip.rs`, `prop_roundtrip` (proptest, 512 cas, et `PROPTEST_CASES` respecté) : `parse(&render(&t, plain).markdown) == Ok(t)` pour tout arbre `t` de `arb::tree()`. Le générateur produit :
    - tous les types du registre, des enfants sous **tout** type, des props hors défaut et des props inconnues ;
    - des `HardBreak`, des liens dont le `href` contient une espace, `)`, `&` ou `\` ;
    - du code qui contient des suites de backticks ;
    - des mentions et des formules collées à du texte stylé ;
    - un callout vide suivi d'un paragraphe enfant ;
    - les caractères `* _ \ < > & { } $ | ^ [ ] ~`, des emoji, des accents et du texte de droite à gauche.
  - `strict.rs`, `non_canonical_input_is_rejected` : chaque entrée donne une `Err` avec ce numéro de ligne, et un message qui contient l'indice entre guillemets :
    - `"* item\n"` → ligne 1, message contenant « - » ;
    - `"+ item\n"` → ligne 1 ;
    - `"Titre\n=====\n"` → ligne 2 ;
    - `"##### Titre\n"` → ligne 1, message contenant « niveaux 1 à 4 » ;
    - `"<toggle title=\"x\">\nsans fin\n"` → ligne 1, message contenant « </toggle> » ;
    - `"Texte {align=center\n"` → ligne 1.
- [ ] **Step 2 : lancer** `cargo test -p mementum-md`. Attendu : échec, rien n'est implémenté.
- [ ] **Step 3 : écrire le module `inline.rs`** : échappements, choix entre délimiteurs Markdown et balises (règle de la P0), `<br/>`, `<span bg color>`, `<a href>`. pulldown-cmark ne sert qu'au contenu en ligne, comme dans la P0.
- [ ] **Step 4 : écrire `attrs.rs`, puis `parse.rs` et `render.rs`.**
  - Parseur ligne à ligne, piloté par `MmdForm`.
  - Le rendu suit la table « Format MMD de la P1 ».
  - Le spike (`spike/mmd/src/`) se relit comme référence. On ne le recopie pas : le modèle a changé, avec des props génériques et des enfants pour tout bloc.
- [ ] **Step 5 : écrire `p1-edge-cases.mmd`.** Une occurrence de chaque forme de la table qui n'est pas dans `p0-all-blocks.mmd`, dont `video` et `audio`.
- [ ] **Step 6 : lancer** `cargo test -p mementum-md && PROPTEST_CASES=4096 cargo test -p mementum-md --test roundtrip`. Attendu : tout passe.
- [ ] **Step 7 : reporter le format dans la conception** (§6.2, voir **Files**).
- [ ] **Step 8 : commit** `md: format MMD de la P1 (props, enfants de tout bloc, saut de ligne, lecture stricte)`.

### Task 3 : `mementum-crdt`, le document de page

**Files :**
- Create :
  - `crates/mementum-crdt/Cargo.toml` (yrs 0.28 avec `sync`, `mementum-md`, `mementum-schema`, `sha2`, `uuid`, `base64`) ;
  - `src/lib.rs`, `src/page.rs`, `src/blocknote.rs` (lecture et écriture du format Yjs de BlockNote), `src/edit.rs`, `src/dedupe.rs`, `src/revision.rs` ;
  - `spec/mmd/fixtures/yjs/p0-all-blocks.bin` (copie de `spike/fixtures/yjs/all-blocks.bin`).
- Test : `crates/mementum-crdt/tests/{fixture.rs, roundtrip.rs, edit.rs, dedupe.rs, revision.rs, unknown.rs}`.

**Interfaces :**
- Consumes : `mementum_md::{Block, Content, Inline, normalize, block_text, render, RenderOpts}`, `mementum_md::arb::tree` (en dev, feature `arbitrary`), `mementum_schema::{SCHEMA_VERSION, block}`.
- Produces :
```rust
pub const FRAGMENT: &str = "document-store";
pub const META: &str = "mementum"; // Y.Map ; clé "schemaVersion" (entier)
pub struct PageDoc { /* yrs::Doc, OffsetKind::Utf16 */ }
pub enum CrdtError { Decode(String), BlockNotFound(String), OutOfRange { block_id: String } }
impl PageDoc {
    pub fn new_page() -> (PageDoc, Vec<u8>);                 // racine blockGroup > blockContainer > paragraph, et META.schemaVersion = SCHEMA_VERSION ; renvoie aussi la mise à jour
    pub fn from_updates<'a>(updates: impl IntoIterator<Item = &'a [u8]>) -> Result<PageDoc, CrdtError>;
    pub fn apply_update(&self, update: &[u8]) -> Result<(), CrdtError>;
    pub fn encode_state(&self) -> Vec<u8>;
    pub fn state_vector(&self) -> Vec<u8>;
    pub fn encode_diff(&self, remote_state_vector: &[u8]) -> Result<Vec<u8>, CrdtError>;
    pub fn schema_version(&self) -> Option<u32>;
    pub fn read_blocks(&self) -> Vec<mementum_md::Block>;
    pub fn write_blocks(&self, blocks: &[mementum_md::Block]) -> Vec<u8>;
    pub fn replace_text(&self, block_id: &str, start: usize, len: usize, new: &str) -> Result<Vec<u8>, CrdtError>;
    pub fn insert_blocks_after(&self, after_id: &str, blocks: &[mementum_md::Block]) -> Result<Vec<u8>, CrdtError>;
    pub fn move_block(&self, block_id: &str, after_id: Option<&str>) -> Result<Vec<u8>, CrdtError>;
    pub fn dedupe_blocks(&self) -> Option<Vec<u8>>;
    pub fn revision(&self) -> String;
    pub fn yrs_doc(&self) -> &yrs::Doc; // pour mementum-sync seulement (Awareness) ; jamais pour lire le contenu
}
```
  - `write_blocks` remplace tout le contenu, en gardant la racine. Il donne un UUIDv7 aux blocs sans id et renvoie la mise à jour.
  - Les positions de `replace_text` se comptent en caractères de `block_text`. La conversion en UTF-16 se fait sur le texte brut issu de `diff()`, jamais sur `get_string()`.
  - `move_block` et `insert_blocks_after` vont en tête de page quand `after_id` vaut `None`.
  - `dedupe_blocks` garde la première occurrence de chaque id de `blockContainer`, dans l'ordre du document. Il renvoie `None` s'il n'y a rien à faire.
  - `revision` renvoie `"r_"` suivi de 16 caractères hexadécimaux du SHA-256 d'un encodage canonique : vecteur d'état trié par identifiant de client, puis ensemble des suppressions trié. Ne jamais hacher `Snapshot::encode` tel quel (son ordre vient d'une HashMap).

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `fixture.rs`, `reads_p0_blocknote_fixture` : `from_updates([p0-all-blocks.bin])`, puis `read_blocks`, puis `render` donne exactement `p0-all-blocks.mmd`.
  - `roundtrip.rs`, `prop_roundtrip_through_yjs` (proptest, 256 cas) : pour tout `t` de `arb::tree()`, une page neuve sur laquelle on fait `write_blocks(&t)` relit `normalize(t)`, ids générés mis à part. C'est le proptest qui traverse la couche Yjs, absent de la P0.
  - `roundtrip.rs`, `hard_break_is_a_yjs_element` : un paragraphe `a`, `HardBreak`, `b` donne dans le fragment l'élément `hardBreak` entre deux `XmlText`.
  - `edit.rs`, `replace_after_marks_and_emoji` : sur `Du **gras** puis 🚀 déjà`, remplacer `déjà` par `prêt` donne `Du **gras** puis 🚀 prêt`.
  - `edit.rs`, `replace_after_mention` : sur `Voir **ceci** et <mention type="user" id="u1">Lenny</mention> puis la cible`, remplacer `cible` par `fin` réussit.
  - `edit.rs`, `replace_out_of_range` : une plage qui dépasse le texte donne `Err(OutOfRange)`, sans panique.
  - `dedupe.rs`, `concurrent_moves_then_dedupe` : deux `PageDoc` issus du même état déplacent le même bloc à deux endroits différents. Après échange des mises à jour et `dedupe_blocks` sur l'un, puis application de cette mise à jour sur l'autre, les deux relisent un seul exemplaire du bloc et le même MMD.
  - `revision.rs`, `pure_delete_changes_revision` : une suppression seule change la révision.
  - `revision.rs`, `revision_is_canonical` : les mêmes mises à jour appliquées dans deux ordres différents donnent la même révision.
  - `unknown.rs`, `unknown_node_passes_through` : un élément `futureWidget` (attributs `a="1"` et `n=2`, un `XmlText` « x ») construit à la main dans un `blockContainer` se relit en `kind = "unknown"`. Réécrit par `write_blocks` dans une page neuve, il redonne un élément `futureWidget` avec les mêmes attributs (comparer les maps d'attributs, jamais `get_string`) et le même texte.
  - `unknown.rs`, `new_page_has_root_and_schema` : `new_page()` relit un paragraphe vide, et `schema_version() == Some(SCHEMA_VERSION)`.
- [ ] **Step 2 : lancer** `cargo test -p mementum-crdt`. Attendu : échec.
- [ ] **Step 3 : écrire `blocknote.rs`.**
  - Format Yjs de BlockNote : attributs typés, marques simples à `{}`, couleur à `{"stringValue":…}`, lien à `{"href":…}` ; `columnList` et `column` hors `blockContainer` ; `tableContent` avec `columnWidths` et `headerRows`.
  - Une prop inconnue du registre est copiée telle quelle.
  - `spike/mmd/src/ydoc.rs` se relit comme référence.
- [ ] **Step 4 : écrire `page.rs`, `edit.rs`, `dedupe.rs` et `revision.rs`.**
- [ ] **Step 5 : lancer** `cargo test -p mementum-crdt`. Attendu : tout passe.
- [ ] **Step 6 : commit** `crdt: document de page (format BlockNote fidèle, éditions, dédoublonnage, révision canonique)`.

### Task 4 : `mementum-store`, SQLite

**Files :**
- Create : `crates/mementum-store/Cargo.toml`, `src/lib.rs`, `src/nodes.rs`, `src/docs.rs`, `src/position.rs`, `migrations/0001_init.sql`.
- Test : `crates/mementum-store/tests/{nodes.rs, docs.rs, position.rs}`.

**Interfaces :**
- Produces :
```rust
pub struct Store { /* SqlitePool */ }
pub struct Space { pub org_id: Uuid, pub vault_id: Uuid }
pub enum NodeKind { Page, Folder }
pub struct Node { pub id: Uuid, pub vault_id: Uuid, pub parent_id: Option<Uuid>, pub kind: NodeKind, pub title: String, pub position: String, pub created_at: i64, pub updated_at: i64 }
pub struct NewNode { pub vault_id: Uuid, pub parent_id: Option<Uuid>, pub after: Option<Uuid>, pub kind: NodeKind, pub title: String }
pub struct DocState { pub snapshot: Option<(i64, Vec<u8>)>, pub updates: Vec<(i64, Vec<u8>)> }
pub enum StoreError { NotFound, Cycle, Sqlx(sqlx::Error) }
impl Store {
    pub async fn open(path: &std::path::Path) -> Result<Store, StoreError>;
    pub async fn default_space(&self) -> Result<Space, StoreError>;
    pub async fn create_node(&self, n: NewNode) -> Result<Node, StoreError>;
    pub async fn get_node(&self, id: Uuid) -> Result<Option<Node>, StoreError>;
    pub async fn list_nodes(&self, vault_id: Uuid) -> Result<Vec<Node>, StoreError>;
    pub async fn rename_node(&self, id: Uuid, title: &str) -> Result<Node, StoreError>;
    pub async fn move_node(&self, id: Uuid, parent_id: Option<Uuid>, after: Option<Uuid>) -> Result<Node, StoreError>;
    pub async fn append_update(&self, node_id: Uuid, update: &[u8]) -> Result<i64, StoreError>;
    pub async fn load_doc(&self, node_id: Uuid) -> Result<DocState, StoreError>;
    pub async fn put_snapshot(&self, node_id: Uuid, seq: i64, state: &[u8]) -> Result<(), StoreError>;
}
pub fn position_between(before: Option<&str>, after: Option<&str>) -> String;
```
  - `open` crée la base et le dossier, lance les migrations et applique les pragmas de la Global Constraints.
  - `default_space` crée, à la première demande, l'organisation `default` et le coffre « Mon coffre » (`kind = 'personal'`). Le lot 2 remplacera ce raccourci.
  - `list_nodes` trie par `(parent_id, position)`.
  - `move_node` renvoie `Cycle` si le nouveau parent est le nœud lui-même ou un de ses descendants.
  - `append_update` renvoie le `seq` attribué : `MAX(seq) + 1` par page, dans un seul `INSERT … SELECT`.
  - `load_doc` renvoie les mises à jour de `seq` > celui de l'instantané.
  - `put_snapshot` tient en une transaction : `INSERT … ON CONFLICT(node_id) DO UPDATE … WHERE excluded.seq > doc_snapshots.seq`, puis la suppression des mises à jour de `seq` inférieur ou égal à celui de l'instantané **effectivement gardé**.
  - `position_between` renvoie une chaîne strictement comprise entre ses bornes, en ordre d'octets (index fractionnaire ; `None` = sans borne). Pas de rééquilibrage dans ce lot.
- Tables de `0001_init.sql` :
  - `instance_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)` ;
  - `orgs(id TEXT PRIMARY KEY, slug TEXT UNIQUE NOT NULL, name TEXT NOT NULL, created_at INTEGER NOT NULL)` ;
  - `vaults(id TEXT PRIMARY KEY, org_id TEXT NOT NULL REFERENCES orgs(id), name TEXT NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('personal','shared')), created_at INTEGER NOT NULL)` ;
  - `nodes(id TEXT PRIMARY KEY, org_id TEXT NOT NULL REFERENCES orgs(id), vault_id TEXT NOT NULL REFERENCES vaults(id), parent_id TEXT REFERENCES nodes(id), kind TEXT NOT NULL CHECK (kind IN ('folder','page')), title TEXT NOT NULL, position TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, trashed_at INTEGER)`, avec un index sur `(vault_id, parent_id, position)` ;
  - `doc_updates(node_id TEXT NOT NULL REFERENCES nodes(id), seq INTEGER NOT NULL, update_blob BLOB NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY (node_id, seq))` ;
  - `doc_snapshots(node_id TEXT PRIMARY KEY REFERENCES nodes(id), seq INTEGER NOT NULL, state_blob BLOB NOT NULL, created_at INTEGER NOT NULL)`.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `create_and_list_in_order` : trois pages créées avec `after` relisent dans l'ordre voulu.
  - `move_refuses_cycle` : A > B > C ; déplacer A sous C donne `Err(Cycle)` et l'arbre ne change pas ; déplacer A sous A aussi.
  - `position_between_is_ordered` (proptest) : pour toutes bornes `a < b`, `a < position_between(Some(a), Some(b)) < b`, y compris pour des bornes très proches générées en série (200 insertions au même endroit).
  - `append_and_load`.
  - `snapshot_never_goes_backwards` : 12 mises à jour ; `put_snapshot(seq = 10)` puis `put_snapshot(seq = 5)`. L'instantané reste celui de 10, les mises à jour 1 à 10 sont supprimées, 11 et 12 restent.
  - `reopen_keeps_everything` : fermer, rouvrir, relire.
- [ ] **Step 2 : lancer** `cargo test -p mementum-store`. Attendu : échec.
- [ ] **Step 3 : écrire `position.rs`, la migration, `nodes.rs` et `docs.rs`.**
- [ ] **Step 4 : lancer** `cargo test -p mementum-store`. Attendu : tout passe.
- [ ] **Step 5 : commit** `store: SQLite (arbre de pages, journal Yjs, instantanés qui ne reculent jamais)`.

### Task 5 : `mementum-sync`, un acteur par page et le protocole y-websocket

**Files :**
- Create : `crates/mementum-sync/Cargo.toml`, `src/lib.rs`, `src/hub.rs`, `src/actor.rs`, `src/protocol.rs`.
- Test : `crates/mementum-sync/tests/{common/mod.rs, sync.rs, durability.rs, schema.rs}`. `common/mod.rs` contient un client Yjs de test en yrs et un petit serveur axum qui monte `Hub::serve_connection`.

**Interfaces :**
- Consumes : `PageDoc` (Task 3), `Store` (Task 4).
- Produces :
```rust
pub struct SyncConfig { pub broadcast_capacity: usize, pub snapshot_every: u64, pub idle_close: std::time::Duration }
impl Default for SyncConfig { /* 1024, 500, 30 s */ }
pub struct Hub { /* registre node_id -> poignée d'acteur, avec un numéro de génération */ }
pub enum SyncError { NotFound, Store(StoreError), Crdt(CrdtError) }
impl Hub {
    pub fn new(store: Store, cfg: SyncConfig) -> std::sync::Arc<Hub>;
    pub async fn create_page_doc(&self, node_id: Uuid) -> Result<(), SyncError>;
    pub async fn read_blocks(&self, node_id: Uuid) -> Result<Vec<mementum_md::Block>, SyncError>;
    pub async fn write_blocks(&self, node_id: Uuid, blocks: Vec<mementum_md::Block>) -> Result<(), SyncError>;
    pub async fn serve_connection<S>(self: std::sync::Arc<Self>, node_id: Uuid, client_schema: u32, socket: S) -> Result<(), SyncError>
    where S: futures::Stream<Item = Vec<u8>> + futures::Sink<Vec<u8>> + Unpin + Send + 'static;
}
```
  - `create_page_doc` crée le document d'une page neuve (`PageDoc::new_page`) et persiste son état initial en instantané.
  - `read_blocks` et `write_blocks` passent par l'acteur de la page. Une écriture serveur est persistée puis diffusée.
  - `serve_connection` renvoie `Err(NotFound)` **avant toute écriture** si la page n'existe pas.
- **L'acteur** est une tâche tokio par page ouverte. Il possède le `PageDoc` et traite ses commandes une par une :
  - **mise à jour d'un client** : application ; puis `dedupe_blocks` ; puis `append_update` de la mise à jour (et de celle du dédoublonnage), **attendu** ; puis diffusion aux autres connexions (et de la mise à jour du dédoublonnage à toutes) ;
  - **compaction** toutes les `snapshot_every` mises à jour : `put_snapshot(seq, encode_state)`, dans l'acteur ;
  - **fermeture** après `idle_close` sans connexion. Le registre ne retire l'acteur que si la génération correspond, pour ne jamais avoir deux acteurs pour une page.
- **Protocole** : celui de y-websocket.
  - Messages `0` (synchro : étape 1, étape 2, mise à jour) et `1` (awareness, relayée et gardée pour les nouveaux venus) ;
  - mises à jour vides ignorées ;
  - `spike/server/src/ws.rs` et `rooms.rs` se relisent comme référence.
  - Sur `RecvError::Lagged`, la connexion est fermée et le client se reconnecte et se resynchronise.
- **Schéma** : si `client_schema != SCHEMA_VERSION`, le client reçoit tout mais ses mises à jour sont ignorées, ni appliquées, ni persistées. Il est en lecture seule côté serveur.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `sync.rs`, `two_clients_converge` : un caractère tapé chez A est visible chez B en moins de 150 ms.
  - `sync.rs`, `unknown_page_is_rejected` : une connexion à un `node_id` absent donne `NotFound`, et le store n'a aucune nouvelle ligne.
  - `sync.rs`, `concurrent_moves_are_deduped` : A et B déplacent le même bloc en même temps. Après convergence, le serveur, A et B relisent un seul exemplaire.
  - `sync.rs`, `lagged_client_is_disconnected` : avec `broadcast_capacity = 4`, un client qui ne lit plus est déconnecté, et un client qui se reconnecte reçoit l'état complet.
  - `durability.rs`, `update_is_persisted_before_broadcast` : dès que B reçoit la mise à jour de A, `store.load_doc` la contient, contrôlé immédiatement après la réception.
  - `durability.rs`, `compaction_keeps_concurrent_updates` : deux clients écrivent chacun 600 mises à jour en même temps. Après les compactions, un `PageDoc` reconstruit depuis le store relit le même texte que le document vivant.
  - `durability.rs`, `restart_keeps_everything` : arrêter le hub et le serveur, en rouvrir de nouveaux sur la même base, relire.
  - `schema.rs`, `stale_schema_client_is_read_only` : un client avec `client_schema = SCHEMA_VERSION - 1` envoie une mise à jour. Le serveur et un autre client ne la voient jamais, et le store ne la contient pas. Ce client reçoit pourtant les mises à jour des autres.
- [ ] **Step 2 : lancer** `cargo test -p mementum-sync`. Attendu : échec.
- [ ] **Step 3 : écrire `protocol.rs`, `actor.rs` et `hub.rs`.**
- [ ] **Step 4 : lancer** `cargo test -p mementum-sync`. Attendu : tout passe.
- [ ] **Step 5 : commit** `sync: un acteur par page (persister puis diffuser, compaction sûre, lecture seule des schémas anciens)`.

### Task 6 : `mementum-server` et `mementum-cli`, HTTP, API des pages, binaire

**Files :**
- Create :
  - `crates/mementum-server/Cargo.toml`, `src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/types.rs` (types d'API avec `ts-rs`), `src/routes/{pages.rs, sync.rs, info.rs}` ;
  - `crates/mementum-cli/Cargo.toml` (binaire `mementum`), `src/main.rs` ;
  - `packages/protocol/package.json`, `packages/protocol/src/generated/` (fichiers générés).
- Test : `crates/mementum-server/tests/api.rs`, `crates/mementum-cli/tests/{killed.rs, budgets.rs}`.

**Interfaces :**
- Consumes : `Hub`, `Store`, `mementum_md::{parse, render, RenderOpts}`.
- Produces :
```rust
pub struct Config { pub bind: std::net::SocketAddr, pub data_dir: std::path::PathBuf }
// défaut : bind 127.0.0.1:8080, data_dir ./data ; variables MEMENTUM_BIND et MEMENTUM_DATA_DIR
pub async fn app(cfg: &Config) -> anyhow::Result<axum::Router>;
pub async fn serve(cfg: Config) -> anyhow::Result<()>;
#[derive(serde::Serialize, ts_rs::TS)] #[serde(rename_all = "camelCase")]
pub struct PageNode { pub id: String, pub parent_id: Option<String>, pub kind: String, pub title: String, pub position: String, pub updated_at: i64 }
#[derive(serde::Serialize, ts_rs::TS)] #[serde(rename_all = "camelCase")]
pub struct Info { pub version: String, pub schema_version: u32 }
```
- **Routes** :

| Méthode et chemin | Réponse |
|---|---|
| `GET /health` | `ok` |
| `GET /api/v1/info` | `Info` |
| `GET /api/v1/tree` | `{ "vaultId", "nodes": [PageNode] }` |
| `POST /api/v1/pages` `{ "parentId"?, "afterId"?, "title"? }` | `201`, `PageNode`. Crée le nœud **et** son document (`Hub::create_page_doc`) |
| `PATCH /api/v1/pages/{id}` `{ "title"?, "parentId"?, "afterId"? }` | `PageNode` ; `409 {"error":"cycle"}` ; `404` |
| `GET /api/v1/pages/{id}/mmd` | `text/markdown; charset=utf-8` ; `404` sans rien créer |
| `PUT /api/v1/pages/{id}/mmd` (corps MMD) | `204` ; `422 {"error":"invalid_mmd","line":…,"message":…}` |
| `GET /sync/{id}?schema=N` | WebSocket ; `404` si la page n'existe pas |

  - Un `parentId` absent de `PATCH` laisse le parent ; `"parentId": null` place la page à la racine.
  - Les erreurs sont en JSON : `{ "error": <code>, "message": <texte en français> }`.
  - **Règles du titre** : espaces de bord retirées ; vide → « Sans titre » ; plus de 500 caractères → `422 {"error":"title_too_long"}`.
- **CLI** : `mementum serve [--bind <adresse>] [--data-dir <dossier>]`, construite avec clap.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `api.rs`, `pages_crud` : créer, lister, renommer, déplacer.
  - `api.rs`, `move_cycle_is_409`.
  - `api.rs`, `mmd_get_put` : un `PUT` de `p0-all-blocks.mmd` relit exactement le même texte en `GET`.
  - `api.rs`, `invalid_mmd_is_422` : `"* x\n"` donne `line` 1.
  - `api.rs`, `unknown_page_is_404_and_not_created` (`GET mmd` et `GET /sync`).
  - `api.rs`, `title_rules` (Review Focus 5).
  - `api.rs`, `bindings_are_committed` : les fichiers générés par ts-rs sont égaux à ceux de `packages/protocol/src/generated/`. Avec `UPDATE_BINDINGS=1`, le test les réécrit.
  - `killed.rs`, `killed_server_loses_nothing_broadcast` (Review Focus 1) :
    1. lancer `env!("CARGO_BIN_EXE_mementum")` en sous-processus, sur un dossier temporaire ;
    2. créer une page ; le client A envoie 300 mises à jour ; le client B note le texte reçu ;
    3. `Child::kill()` pendant l'envoi ;
    4. relancer sur le même dossier : le texte de la page contient tout ce que B a reçu.
  - `budgets.rs`, `cold_start_under_1s` : du lancement à `/health` = `ok`.
  - `budgets.rs`, `idle_rss_under_150mb` : RSS lue avec sysinfo 2 s après le démarrage.
  - Écarter un premier lancement plus lent : l'antivirus analyse un exécutable neuf. Prendre le meilleur de 3 lancements.
- [ ] **Step 2 : lancer** `cargo test -p mementum-server -p mementum-cli`. Attendu : échec.
- [ ] **Step 3 : écrire `config.rs`, `error.rs`, `types.rs`, les routes, `lib.rs` et `main.rs`.**
- [ ] **Step 4 : générer les types** avec `UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed`.
- [ ] **Step 5 : lancer** `cargo test --workspace`. Attendu : tout passe.
- [ ] **Step 6 : CI.** Ajouter au job `rust` : `git diff --exit-code packages/protocol spec/schema`.
- [ ] **Step 7 : commit** `server: API des pages, WebSocket de synchro et binaire mementum`.

### Task 7 : `apps/web`, l'application (arbre de pages et éditeur en direct)

**Files :**
- Create :
  - `pnpm-workspace.yaml` (`apps/*`, `packages/*`) et `package.json` à la racine, privé, avec les scripts `dev`, `test` et `e2e` ;
  - `apps/web/package.json`, `vite.config.ts` (port 5173, proxy `/api` et `/sync` vers `127.0.0.1:8080`, WebSocket compris), `index.html`, `playwright.config.ts`, `vitest.config.ts` ;
  - `apps/web/src/main.tsx`, `App.tsx`, `theme.css` (variables du design A) ;
  - `apps/web/src/api.ts` (client REST typé par `packages/protocol`) ;
  - `apps/web/src/sidebar/PageTree.tsx`, `src/page/PageView.tsx`, `src/page/TitleInput.tsx` ;
  - `apps/web/src/editor/{schema.tsx, collab.ts, Editor.tsx, ReadOnlyBanner.tsx}` ;
  - `tools/check-npm-licenses.mjs`.
- Modify : `.github/workflows/ci.yml` (job `web`), `README.md` (section « Lancer Mementum »), `CLAUDE.md` (section « Tester »).
- Test : `apps/web/src/editor/schema.test.ts`, `apps/web/e2e/{helpers.ts, pages.spec.ts, collab.spec.ts}`.

**Interfaces :**
- Consumes : REST et WebSocket de la Task 6, `spec/schema/blocks.json` (Task 1), types de `packages/protocol`.
- Produces : la route `/p/:id` ; `window.__mementum = { editor, provider, doc, synced: boolean, readOnly: boolean }`, pour les tests e2e et en développement seulement ; l'export `CLIENT_SCHEMA_VERSION` de `editor/schema.tsx`, égal à `blocks.json.version`.
- **Comportements fixés** :
  - **Schéma BlockNote** : types natifs, plus les blocs custom de la table « Format MMD » (`callout`, `equation`, `bookmark`, `pageLink`, `databaseRef`, `syncedBlock`, `toc`, `unknown`), plus les contenus en ligne `mention` et `math`, plus le multi-colonnes de `@blocknote/xl-multi-column`.
  - **Rendu du bloc `unknown`** : un cadre gris « Bloc non pris en charge », non modifiable.
  - **Code et maths** : coloration du code avec `@blocknote/code-block`, à condition que le script de licences l'accepte (sinon Ruling et bloc de code sans coloration). Équations et maths en ligne rendues avec KaTeX (MIT).
  - **Présence sans comptes** : le lot 1 n'a pas d'utilisateurs. Le nom affiché près du curseur vient de `localStorage` (« Invité » suivi de 4 chiffres, tirés au premier lancement) et la couleur d'une palette fixe. Les curseurs nommés sont ceux de BlockNote ; les comptes du lot 2 donneront les vrais noms.
  - **Éditeur verrouillé** (`editable = false`) tant que `provider.synced` est faux. La racine d'une page est donc toujours celle du serveur.
  - **Version de schéma** : au chargement, `GET /api/v1/info`. Si `schemaVersion !== CLIENT_SCHEMA_VERSION`, l'éditeur passe en lecture seule et `ReadOnlyBanner` affiche « Une nouvelle version de Mementum est disponible. Recharge la page pour continuer à écrire. ». La WebSocket est ouverte avec `params: { schema: String(CLIENT_SCHEMA_VERSION) }`.
  - **Barre latérale** : arbre imbriqué, bouton « Nouvelle page » (racine) et « + » au survol d'une page (sous-page). Un clic ouvre `/p/:id`. Déplacer les pages à la souris viendra au lot 2.
  - **Titre** : champ au-dessus de l'éditeur. `PATCH` à la perte du focus ou sur Entrée ; l'arbre se met à jour.
  - **Playwright** :
    - `webServer` lance `cargo run -p mementum-cli -- serve --data-dir <dossier temporaire>`, puis `vite` ;
    - **`reuseExistingServer: false`**. La configuration vérifie avant tout que 8080 et 5173 sont libres, et s'arrête sinon avec un message qui nomme le port pris. Piège rencontré en P0 : un autre projet occupait 5173.
  - **Licences npm** : `tools/check-npm-licenses.mjs` lit `pnpm licenses list --prod --json`. Il échoue sur toute licence hors de la Global Constraints, sauf `@blocknote/xl-multi-column` (GPL-3.0).

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `schema.test.ts`, `registry_matches_blocknote` (Vitest) :
    - pour chaque type de `blocks.json`, `editor.schema.blockSchema[name]` existe et a les mêmes props, avec les mêmes valeurs par défaut ;
    - aucun type BlockNote n'est absent de `blocks.json` ;
    - les styles et contenus en ligne correspondent à `styles` et `inline`.
  - `pages.spec.ts`, `create_page_and_type_persists` : créer une page dans l'interface, la titrer « Essai », taper « Bonjour Café 🚀 », puis recharger. Le texte et le titre sont toujours là, et `GET /api/v1/pages/{id}/mmd` contient « Bonjour Café 🚀 ».
  - `pages.spec.ts`, `subpage_appears_in_tree`.
  - `collab.spec.ts`, `two_browsers_converge` : 49 frappes chez A, une latence mesurée par frappe jusqu'à l'affichage chez B (événement `update` du `Y.Doc` de B). Le p95 doit rester sous 150 ms.
  - `collab.spec.ts`, `editing_locked_until_synced` : avec `page.routeWebSocket`, retenir les messages du serveur. L'éditeur a `contenteditable="false"`. Après relâchement, il passe à `true`.
  - `collab.spec.ts`, `stale_schema_is_read_only` (Review Focus 2) : `page.route` renvoie `schemaVersion + 1` sur `/api/v1/info`. La bannière est visible, l'éditeur n'est pas modifiable, et une frappe ne change pas le MMD du serveur.
  - `collab.spec.ts`, `offline_edits_merge_on_reconnect` (Review Focus 4) : B passe hors ligne (`context.setOffline(true)` **et** `provider.disconnect()`), A et B tapent, puis B revient. Les deux convergent, avec le texte des deux.
- [ ] **Step 2 : lancer** `corepack pnpm -C apps/web test && corepack pnpm -C apps/web exec playwright test`. Attendu : échec.
- [ ] **Step 3 : écrire l'application.**
  - Le schéma s'inspire de `spike/web/src/schema.tsx`, en le complétant.
  - La collaboration passe par `withCollaboration` de `@blocknote/core/yjs`, avec le fragment `document-store`, et par `WebsocketProvider(`${location.origin.replace(/^http/, "ws")}/sync`, id, doc, { params })`.
- [ ] **Step 4 : lancer** les mêmes commandes, plus `node tools/check-npm-licenses.mjs`. Attendu : tout passe.
- [ ] **Step 5 : CI et docs.**
  - Job `web` dans la CI : `corepack pnpm install --frozen-lockfile`, tests Vitest, licences npm, `playwright install chromium`, tests e2e.
  - README : section « Lancer Mementum », avec `cargo run -p mementum-cli -- serve` et `corepack pnpm -C apps/web dev`, puis `http://localhost:5173`.
  - `CLAUDE.md` : structure du dépôt et commandes de test.
- [ ] **Step 6 : commit** `web: arbre de pages et éditeur BlockNote en direct`.

### Task 8 : fidélité de bout en bout, BlockNote ⇄ serveur

**Files :**
- Create :
  - `apps/web/src/editor/fixtures/{all-blocks.input.json, non-default.input.json}` ;
  - `apps/web/e2e/fidelity.spec.ts` ;
  - `spec/mmd/fixtures/{all-blocks.mmd, non-default.mmd}` ;
  - `spec/mmd/fixtures/yjs/{all-blocks.bin, non-default.bin}` ;
  - `spec/mmd/fixtures/blocknote/{all-blocks.json, non-default.json}`.
- Modify : `apps/web/src/page/PageView.tsx` (paramètre `?fixture=<nom>` actif seulement si `import.meta.env.DEV` : charge le JSON d'entrée dans une page neuve).
- Test : `crates/mementum-crdt/tests/blocknote_fixtures.rs`, `apps/web/e2e/fidelity.spec.ts`.

**Interfaces :**
- Consumes : `PageDoc::from_updates`, `read_blocks`, `mementum_md::render`, l'API de la Task 6.
- **Contenu de `non-default.input.json`** (props hors défaut, une par cas) :
  - un paragraphe coloré et centré ;
  - un titre repliable de niveau 2 ;
  - un tableau à `headerRows = 0` avec une colonne de 120 px ;
  - une image avec `name`, une légende, `showPreview = false` et `previewWidth = 320` ;
  - un paragraphe avec un saut de ligne ;
  - un paragraphe avec un paragraphe enfant ;
  - une citation avec un enfant ;
  - un callout vide suivi d'un paragraphe enfant ;
  - une liste numérotée commençant à 3 ;
  - un texte surligné ;
  - un lien dont le `href` contient `%20` ;
  - du souligné coloré.
- **Les fixtures se régénèrent sur demande seulement** : `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec playwright test e2e/fidelity.spec.ts` écrit les `.bin` et les `.json`. Sans cette variable, le test **compare** au lieu d'écrire.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `blocknote_fixtures.rs`, `reads_blocknote_fixture_all_blocks` et `reads_blocknote_fixture_non_default` : le `.bin` relu puis rendu donne exactement le `.mmd`.
  - `fidelity.spec.ts`, `blocknote_output_matches_committed` : le document produit à partir de chaque `.input.json` est égal au `.json` commité, ids retirés.
  - `fidelity.spec.ts`, `server_written_doc_opens_identical` : `PUT` du `.mmd`, puis ouverture de la page. `editor.document`, après un aller-retour JSON et ids retirés, est égal au `.json`.
  - `fidelity.spec.ts`, `browser_edit_reads_back_as_mmd` : ouvrir la page, ajouter « ! » à la fin du premier paragraphe. Le `GET mmd` est égal au `.mmd`, avec ce « ! » inséré.
  - `fidelity.spec.ts`, `typed_special_characters_roundtrip` (Review Focus 3) : taper `a*b_c \ <d> & {e} $f$ | ^g [h] ~i~ 🚀 שלום`. Le `GET mmd` relu par `parse`, puis réécrit par `PUT`, ne change pas le texte affiché.
- [ ] **Step 2 : écrire les deux `.input.json`, générer les fixtures** (`EXPORT_FIXTURES=1`) **et écrire les `.mmd` attendus.**
- [ ] **Step 3 : lancer** `cargo test -p mementum-crdt --test blocknote_fixtures`, puis `corepack pnpm -C apps/web exec playwright test e2e/fidelity.spec.ts`. Attendu : tout passe. Un écart révèle un trou du format ou du convertisseur : corriger en Task 2 ou 3, et ajouter le cas au générateur proptest.
- [ ] **Step 4 : commit** `fidélité: fixtures BlockNote (blocs hors défaut) relues en MMD exact dans les deux sens`.

### Task 9 : budgets et pages géantes

**Files :**
- Create : `apps/web/e2e/budgets.spec.ts`, `crates/mementum-sync/tests/budgets.rs`.
- Modify : `docs/specs/2026-11-09-mementum-design.md`, §5.10 (note sur la taille des pages).

**Interfaces :**
- Consumes : l'API de la Task 6 (création d'une page et `PUT mmd` de N paragraphes générés dans le test).
- **Décision de ce plan, à valider par Alex** : le budget « page jamais vue < 300 ms » vaut pour les pages **jusqu'à 1 000 blocs**. Au-delà, on demande seulement que la page s'ouvre sans geler l'onglet (< 3 s pour 10 000 blocs). La phase 0 a montré que le coût vient de la liaison y-prosemirror, sans virtualisation disponible. La note va dans le §5.10, et un Ruling dans le registre.

- [ ] **Step 1 : écrire les tests qui échouent.**
  - `budgets.spec.ts`, `open_1000_block_page_under_300ms` : de la navigation au premier bloc affiché, p95 sur 5 ouvertures, application déjà chargée.
  - `budgets.spec.ts`, `open_10000_block_page_under_3s`.
  - `budgets.rs`, `load_10000_block_page_from_store_under_300ms` (`#[ignore]`, à lancer en `--release`) : p95 sur 20 chargements d'un `PageDoc` depuis SQLite.
- [ ] **Step 2 : lancer.** Si un budget n'est pas tenu, profiler et corriger, sans relâcher le budget. Consigner chaque mesure dans le registre.
- [ ] **Step 3 : CI.** Ajouter un job `budgets` : `cargo test --release -p mementum-sync -- --ignored`.
- [ ] **Step 4 : reporter la note dans le §5.10.**
- [ ] **Step 5 : commit** `budgets: ouverture des pages (1 000 et 10 000 blocs) et chargement serveur`.

### Task 10 : exploration, éditions concurrentes d'un bloc déplacé

Un déplacement concurrent avec Yjs perd les éditions faites en même temps sur l'original. La phase 0 l'a montré, et `dedupe_blocks` ne les récupère pas. Cette tâche tente **une seule approche**, avec un critère de sortie net.

**Files :**
- Create : `crates/mementum-crdt/tests/move_edit.rs` ; `crates/mementum-crdt/src/recover.rs` si l'approche aboutit.

**Interfaces :**
- Produces, si l'approche aboutit : `PageDoc::recover_moved_edits(&self) -> Option<Vec<u8>>`, appelé par l'acteur juste avant `dedupe_blocks`.

- [ ] **Step 1 : écrire le test** `edit_during_concurrent_move_survives`.
  - Partir d'un état commun.
  - A déplace le bloc X après Z ; B change le texte de X (`replace_text`).
  - Appliquer les deux mises à jour sur un troisième document, dans les deux ordres.
  - Attendu : un seul X, à la place choisie par A, avec l'édition de B.
- [ ] **Step 2 : tenter l'approche.**
  - Document serveur avec `skip_gc`.
  - Après chaque mise à jour, repérer un `blockContainer` supprimé dont l'id existe vivant ailleurs et dont le contenu a changé dans la mise à jour.
  - Reporter ce changement sur la copie vivante, sous forme de diff de texte.
  - L'approche doit aussi garder vert le proptest de convergence `prop_roundtrip_through_yjs`.
- [ ] **Step 3 : décider.**
  - **Si le test passe** : brancher l'appel dans l'acteur, ajouter le test e2e « édition pendant un glisser-déposer » à `collab.spec.ts`, commit `crdt: récupération des éditions faites sur un bloc déplacé`.
  - **Sinon** : marquer le test `#[ignore = "perte connue : édition concurrente d'un bloc déplacé (RESULTS P0)"]` et consigner le Ruling. Laisser le risque ouvert au §11, avec la piste suivante : déplacements interceptés côté client et faits par le serveur, au lot 3. Commit `crdt: test de la perte connue lors d'un déplacement concurrent`.

---

## Fin du lot

- [ ] **Relecture de toute la branche** par un relecteur indépendant, suivie de **toutes les suites relancées sur un clone neuf**. C'est la leçon de la phase 0 : la relecture avait été oubliée, et le clone neuf a révélé des échecs cachés.
- [ ] **Bilan du lot** : `docs/superpowers/logs/2026-11-09-p1-lot1-ledger.md` complété, `README.md` (état du projet) et le §9 de la conception à jour.
