# Technical architecture for an open-source, self-hostable, real-time collaborative block workspace (Notion-like), as of November 2026

Scope note: research done on 2026-11-09. Several primary sites (loro.dev docs/blog) returned HTTP 403 to the fetch tool, so some Loro facts come via a reader mirror of the official blog or from npm/docs.rs changelogs. Items marked "(secondary)" come from third-party write-ups; items marked "(unverified)" could not be confirmed against a primary source and should be treated with caution.

---

## 1. How does Notion itself work technically?

### Takeaway
Notion is a server-authoritative system built on an "everything is a block/record" graph (blocks with parent pointers and ordered child-ID arrays) stored in heavily sharded Postgres (480 logical shards), with a client-side SQLite cache (native on desktop, WASM+OPFS in the browser) for speed, a CDC-fed data lake for offline/AI workloads, and, since August 2025, a CRDT data model applied only to pages that users mark as available offline.

### Cited Findings
**Data model**
- Notion's data model is described as atomic and graph-like: any persisted data (blocks, users, workspaces) is a "record"; blocks nest inside other blocks (text inside a toggle, sub-pages inside pages). A parent block holds an ordered array of child block IDs, which act as "downward pointers" to content/display children. (Search snippet of the official post "The data model behind Notion's flexibility" by Jake Teton-Landis; full text not retrieved) — [Notion blog](https://www.notion.com/blog/data-model-behind-notion)
- Because blocks are the core data, block-related tables are sharded together so related fetches do not cross shards (search snippet) — [Notion blog](https://www.notion.com/blog/data-model-behind-notion)

**Sharded Postgres**
- Original sharding: 480 logical shards spread evenly across 32 physical Postgres databases, 15 logical shards (Postgres schemas) per physical DB, each logical shard holding a copy of every sharded table — [Notion: Herding elephants / sharding Postgres at Notion](https://notion.com/blog/sharding-postgres-at-notion)
- 480 was chosen for divisibility (lets hosts be added/removed while keeping uniform distribution) (secondary) — [sneaky-potato blog summary](https://sneaky-potato.github.io/blog/notion-sharding/)
- A follow-up post, "The Great Re-shard", describes a zero-downtime horizontal re-shard; a secondary source says it went from 32 to 96 physical DBs (unverified, body of Notion post not retrieved) — [Notion: The Great Re-shard](https://notion.com/blog/the-great-re-shard)
- Some third-party write-ups claim specific routing formulas ("space_id % 480") and data volumes; these were not confirmed against Notion's text — [Notion blog](https://notion.com/blog/sharding-postgres-at-notion)

**Client-side SQLite cache (WASM in the browser)**
- Notion's "How we sped up Notion in the browser with WASM SQLite": using SQLite improved page navigation times by 20% in all modern browsers — [Notion blog](https://www.notion.com/blog/how-we-sped-up-notion-in-the-browser-with-wasm-sqlite)
- Each tab has a dedicated Web Worker accessing SQLite through the OPFS SyncAccessHandle Pool VFS (works in all major browsers and does not require cross-origin isolation, which would have limited rollout to Chrome/Edge). A SharedWorker routes queries so only one tab's worker actually writes; earlier attempts caused database corruption, solved by this SharedWorker architecture — [Notion blog](https://www.notion.com/blog/how-we-sped-up-notion-in-the-browser-with-wasm-sqlite)
- Regression encountered: the first version slowed initial page load because the WASM SQLite library had to be downloaded and processed, blocking load — [Notion blog](https://www.notion.com/blog/how-we-sped-up-notion-in-the-browser-with-wasm-sqlite)
- Earlier (2021) native SQLite caching in Notion's desktop apps gave ~50% faster page navigation (secondary) — [NeetCode newsletter](https://blog.neetcode.io/p/notion-uses-sqlite-caching)

**Offline mode (2025) and CRDTs**
- "How we made Notion available offline" (Raymond Xu, published Dec 11, 2025): Offline Mode, announced in August 2025, was the #1 requested feature for years; the hard problems were reference tracking, background syncing, and rich-text conflict resolution — [Notion blog](https://www.notion.com/blog/how-we-made-notion-available-offline)
- Pages marked available offline are **dynamically migrated to a new CRDT data model** for conflict resolution. The post does not name the CRDT library — [Notion blog](https://www.notion.com/blog/how-we-made-notion-available-offline)
- The previous SQLite cache was "best-effort" with no guarantees on presence/freshness; Offline Mode needed a persistent layer recording which pages are offline, all data needed to render them, and why — [Notion blog](https://www.notion.com/blog/how-we-made-notion-available-offline)
- A "forest of offline page trees": tables `offline_page` (one row per offline page/database) and `offline_action` (one row per reason: `origin_page_id`, `from_page_id`, `impacted_page_id`, `type` such as toggled/favorite). Invariant: an offline page needs at least one action row, otherwise it is removed — [Notion blog](https://www.notion.com/blog/how-we-made-notion-available-offline)
- Marking a database offline auto-downloads up to 50 pages in the current view; pages are shown offline only when fully available (partial pages would make conflict resolution less predictable) — [Notion blog](https://www.notion.com/blog/how-we-made-notion-available-offline)
- Sync: push-based. When updates are applied to a page, the server emits a message on that page's channel; subscribed clients fetch changes. Polling was tested and rejected as not scaling to millions of devices. On reconnect, clients compare per-page `lastDownloadedTimestamp` with the server `lastUpdatedTime` and fetch only newer pages; this reuses Notion's existing "page version snapshot system" — [Notion blog](https://www.notion.com/blog/how-we-made-notion-available-offline)

**Data lake**
- "Building and scaling Notion's data lake": block data is update-heavy (90% of upserts are updates). Pipeline: Postgres → Debezium CDC → Kafka → Apache Hudi → S3 (+Spark). One Debezium connector per Postgres host; one Kafka topic per table (instead of 480 topics per table). Hudi COPY_ON_WRITE + UPSERT. Chosen over Iceberg/Delta (2022) for update-heavy workloads and native Debezium support — [Notion blog](https://www.notion.com/blog/building-and-scaling-notions-data-lake)
- Reported results: >$1M cost savings, freshness from days to minutes/hours (few minutes for most tables, up to 2 h for the largest block table) (secondary summaries of the Notion post) — [Onehouse](https://www.onehouse.ai/blog/notions-journey-through-different-stages-of-data-scale); [Notion blog](https://www.notion.com/blog/building-and-scaling-notions-data-lake)

### Inferences
- Notion's architecture is the canonical "hybrid": server-authoritative records + operation transactions for most data, with CRDTs added late and selectively (rich text conflict resolution for offline pages). A new product can design for CRDT page content from day one rather than retrofit it.
- The block-graph model (records with `parent_id` + ordered child arrays, sharded by workspace) maps naturally to a single SQLite file per server (small installs) or Postgres partitioned by workspace (large installs).
- The "offline set with reasons" design (`offline_page`/`offline_action`) is directly reusable for a client cache-eviction/pinning policy.
- Notion's WASM SQLite lessons (OPFS SAH-pool VFS, single-writer via SharedWorker, lazy-load the WASM so it does not block first paint) are directly applicable.

### Gaps
- The exact CRDT used by Notion offline mode (in-house vs. a library) is not disclosed in the post.
- Full text of the data-model and re-shard posts was not retrieved; exact shard key and current shard count unverified.
- Dates of the WASM SQLite post not re-verified in this session.

---

## 2. CRDT libraries compared (2025–2026): Yjs/yrs/y-octo, Loro, Automerge 3, Diamond Types/Eg-walker, json-joy

### Takeaway
Yjs remains the most mature ecosystem (editor bindings, servers, Rust port yrs used by AppFlowy, y-octo used by AFFiNE), with Yjs 14 (changesets/attributions) still in release-candidate stage mid-2026. Loro (Rust, 1.x since late 2024, now at 1.13.x) is the most feature-complete for a Notion-like product (rich text, movable list, movable tree, version DAG/time travel, shallow snapshots) and was fastest in the older crdt-benchmarks, but its editor/server ecosystem is younger. Automerge 3 (July 2025) fixed its memory problem (>10x, up to ~500x) but trails in raw speed in available benchmarks. Eg-walker/Diamond Types is research-grade (EuroSys 2025) and influences Loro.

### Cited Findings
**Benchmarks (crdt-benchmarks repo — note: OLD versions, pre-Loro 1.0 and pre-Automerge 3)**
- Versions: Yjs 13.6.11, ywasm 0.9.3, Loro 0.10.1, Automerge 2.1.10; Intel i5-8400, Node 20.5.0; no results date given — [dmonad/crdt-benchmarks](https://github.com/dmonad/crdt-benchmarks)
- B4 real-world editing trace (N=6000 config): time — Yjs 5,714 ms, ywasm 28,675 ms, Loro 3,089 ms, Automerge 14,326 ms; docSize — Yjs 159,929 B, Loro 258,228 B, Automerge 129,116 B; parseTime — Yjs 39 ms, ywasm 16 ms, Loro 13 ms, Automerge 1,805 ms — [crdt-benchmarks](https://github.com/dmonad/crdt-benchmarks)
- B4 x100: Yjs 608,908 ms vs Loro 309,689 ms (Automerge skipped); docSize Yjs ~16.0 MB vs Loro ~25.8 MB; parseTime Yjs 2,622 ms vs Loro 1,304 ms; Yjs memUsed 327.1 MB (WASM libraries report ~0 because memory is outside the JS heap, so memory is not comparable) — [crdt-benchmarks](https://github.com/dmonad/crdt-benchmarks)
- B2.4 concurrent insert/delete: Yjs 178 ms, ywasm 2,786 ms, Loro 208 ms, Automerge 1,066 ms. B3.4 (20√N clients insert in array): Yjs 72 ms, Loro 227 ms, Automerge 2,780 ms — [crdt-benchmarks](https://github.com/dmonad/crdt-benchmarks)
- Historical: Automerge 0.14.1 took 489 s and 1.1 GB memory on B4 vs Yjs 13.3.0 at 6.3 s and 3.2 MB — illustrates how far Automerge has come — [crdt-benchmarks](https://github.com/dmonad/crdt-benchmarks)

**Yjs / yrs / y-octo**
- Yjs 13.6.30 source dated 2026-03-14; a mirror lists v14.0.0-rc.7 as latest (pre-release) — [SourceForge Yjs mirror](https://sourceforge.net/projects/yjs.mirror/files/v13.6.30)
- Yjs 14 headline features: changesets and attributions (Feb 2026 talk abstract) — [Calendify session listing](https://calendify.com/session/QnkOqrQVMo0?hl=de)
- New packages `@y/y` v14 and `@y/prosemirror` v2; BlockNote had a draft PR (May 2026) adding support alongside Yjs 13 — [BlockNote PR mirror](https://github.fleetingtimer.workers.dev/TypeCellOS/BlockNote/pull/2739); [Nick the Sick blog, Apr 24 2026](https://nick-the-sick.pckt.blog/yjs-14-at-yprosemirror-wwv64pw)
- AppFlowy's collaboration layer (collab crates) is built on yrs (Rust port of Yjs); clients send CRDT updates over WebSocket to AppFlowy Cloud (secondary) — [Mintlify AppFlowy docs](https://www.mintlify.com/AppFlowy-IO/AppFlowy/developer/rust-backend)
- AFFiNE uses y-octo, a Rust Yjs-compatible implementation, through NAPI bindings for Node.js (secondary) — [openapps.pro AFFiNE](https://openapps.pro/apps/affine); [OctoBase repo](https://github.com/fourdim/OctoBase)

**Loro**
- Loro 1.0 features: rich text CRDT (text/list use Fugue to avoid interleaving), Movable List (set/insert/delete/move), Map, Movable Tree (hierarchies such as outliners/file trees), nested JSON-like types, version control (DAG of history, checkout/fork/merge), Eg-walker-inspired architecture, shallow snapshots (drop old history, like git shallow clone). Bindings: Rust, JS (WASM), Swift — [Loro 1.0 blog (reader mirror)](https://hn.jakechampion.name/reader/https://loro.dev/blog/v1.0)
- Loro 1.0 perf: loading a doc with millions of ops dropped from 16 ms to 1 ms (0.37 ms with shallow snapshot); import 10–100x faster than 0.16; uncompressed snapshot ~2x larger than the old format and other mainstream CRDTs — [Loro 1.0 blog (reader mirror)](https://hn.jakechampion.name/reader/https://loro.dev/blog/v1.0)
- Movable tree guarantees no cycles after merging concurrent moves — [Loro 1.0 blog](https://loro.dev/blog/v1.0); design described in [Loro movable tree blog](https://www.loro.dev/blog/movable-tree) (July 2024, claims production-ready performance)
- Movable-tree proof-of-concept benchmark (M1): 1M move ops in 2.07 s; 10K nodes in 18 ms — [loro-dev/movable-tree](https://github.com/loro-dev/movable-tree)
- Current version: loro-crdt 1.13.9 on npm / loro 1.13.9 on crates.io; recent changelog items include faster snapshot import, optimized shallow-doc snapshot export (1.12.3), errors instead of panics on checkout before shallow root (1.10.8) — [loro-crdt CHANGELOG](https://unpkg.com/loro-crdt@1.13.9/CHANGELOG.md); [docs.rs loro 1.13.9](https://docs.rs/crate/loro/1.13.9/source/README.md)
- Shallow snapshots and redaction are documented features — [Loro docs](https://www.loro.dev/docs/advanced/shallow_snapshot)

**Automerge 3**
- Released July 2025: runtime now uses the compressed columnar format in memory; memory down "over 10x" in many cases; pasting Moby Dick: ~700 MB (Automerge 2) → ~1.3 MB (Automerge 3); a document that had not loaded after 17 hours loads in 9 seconds. `Text` class removed, collaborative text is plain strings, `RawString` → `ImmutableString`; file format unchanged; automerge-repo 2.1.0 uses it by default — [Automerge 3 blog](https://automerge.org/blog/automerge-3/); [Ink & Switch dispatch 011](https://inkandswitch.com/newsletter/dispatch-011)
- Fixes continued through v3.3.0–v3.3.2 by July 2026 — [This Month in Automerge: July '26](https://automerge.org/blog/2026-july/)
- Lower memory makes busy sync servers holding many large docs easier to run — [Automerge 3 blog](https://automerge.org/blog/automerge-3/)

**Eg-walker / Diamond Types**
- "Collaborative Text Editing with Eg-walker: Better, Faster, Smaller" (Gentle & Kleppmann, EuroSys 2025): merging n concurrent ops costs O(n log n) vs O(n²)+ for OT; a doc that takes OT 1 hour to merge takes Eg-walker 24 ms; keeps only plain text in memory during steady-state editing → order of magnitude less memory and orders of magnitude faster loading than existing CRDTs; works peer-to-peer — [arXiv 2409.14252](https://arxiv.org/abs/2409.14252); [Kleppmann blog](https://martin.kleppmann.com/2025/03/30/eg-walker-collaborative-text.html)
- Diamond Types README calls itself a prototype to see how fast a CRDT can go; crates.io crate is out of date — [josephg/diamond-types](https://WWW.GITHUB.COM/josephg/diamond-types)
- Loro documents Eg-walker as a core concept it builds on — [Loro docs: Eg-walker](https://loro.dev/docs/advanced/event_graph_walker)

**Practitioner usage signals**
- Linear avoided CRDTs until recently and uses them only for issue descriptions (secondary talk summary) — [Recapio digest of Tuomas Artman talk](https://recapio.com/digest/tuomas-artman-building-a-synchronous-experience-with-asynchronous-data-linear-s-sync-engine-by-local)
- Zed built collaborative editing on CRDTs in the editor core (replica IDs, server over WebSocket) — [Zed blog: CRDTs](https://zed.dev/blog/crdts)
- A devlog titled "Why we don't use Yjs" (Moment) exists as a counterpoint (not read in detail) — [fooqux mirror](https://fooqux.com/article/3533)

### Inferences
- For page content (rich text + nested blocks), the practical choice in late 2026 is **Yjs (JS) + yrs (Rust server)** for ecosystem maturity, or **Loro** for a Rust-first stack wanting movable trees, built-in history/time-travel and shallow snapshots in one library. Automerge 3 is viable but has the smallest editor-binding ecosystem of the three and was slowest in the (old) benchmark.
- The crdt-benchmarks numbers predate Loro 1.x and Automerge 3, so they understate Automerge 3 and do not reflect Loro 1.x's new encoding; a project should re-run benchmarks (e.g., B4, B4x100, and a block-tree workload) with current versions before committing.
- Movable tree CRDTs (Loro) solve page-tree/block reordering under concurrency without cycles; with Yjs the common workaround is server-authoritative tree ops or fractional-index ordering with cycle checks on the server.
- Yjs 14's "attributions" (who wrote what) and "changesets" would help with Notion-like page history; until stable, Loro's version DAG is the more complete built-in option.

### Gaps
- json-joy: no current data found (version, benchmarks, adoption).
- No independent 2026 benchmark comparing Loro 1.13, Yjs 13.6/14, Automerge 3.3 and y-octo on the same machine was found; loro.dev/docs/performance returned 403.
- Licenses were not re-verified this session (from prior knowledge: Yjs MIT, yrs MIT, Loro MIT, Automerge MIT — confirm before use).
- Go server-side CRDT implementations (Yjs-compatible) were not researched.

---

## 3. Sync engines / local-first frameworks, Linear and Figma, and the hybrid design question

### Takeaway
Almost all production sync engines are relational/row-oriented with last-write-wins semantics and Postgres-centric backends (Zero, Electric, PowerSync); none handles collaborative rich text natively, and none matches a "SQLite-by-default single binary" self-host target out of the box. Linear (object graph + IndexedDB + server ordering, CRDT only for descriptions) and Figma (server-authoritative, per-property LWW, no CRDT timestamps needed) both validate the hybrid: **CRDT for document text, server-authoritative ordered operations for structured data and permissions**.

### Cited Findings
**Zero (Rocicorp)**
- Zero 1.0 released (InfoQ, June 2026) after ~2 years, 50+ releases; 1.0 is a minor update over 0.26.2, with fixes incl. corrupted Litestream restores and CDC error handling; ZQL runs queries against the local cache first while authoritative results sync in the background; SSR still on the roadmap — [InfoQ](https://infoq.com/news/2026/06/zero-version-1)
- Zero's own guidance: best for interactive apps "broadly like Linear"; partial sync defined by queries; mutators run custom authorization/validation on writes; read permissions control what syncs. Limitations: "Zero doesn't support offline writes", not local-first (authoritative server); recommended datasets < 100 GB (ceiling up to 45 TB on EC2); zero-cache keeps a SQLite replica of the (syncable subset of the) Postgres DB and wants fast local NVMe; TypeScript clients only — [Zero docs: when to use](https://zero.rocicorp.dev/docs/when-to-use)
- Production migration example: Productlane moved to Zero — [Productlane blog](https://roadmap.speakeasy.com/blog/zero-migration)

**ElectricSQL**
- Electric 1.0 GA on March 17, 2025; core sync APIs stable, no backward-incompatible changes in minor/patch releases; handles partial replication, fan-out and data delivery — [Electric 1.0 release post](https://electric-sql.com/blog/2025/03/17/electricsql-1.0-released)
- Shapes are single-table, immutable, filtered via where clauses in Postgres — [Electric 1.0 release post](https://electric-sql.com/blog/2025/03/17/electricsql-1.0-released); [Electric shapes docs](https://electric-sql.com/docs/guides/shapes)
- Releases continued into 2026 (Hex package 1.7.2 listed) — [hex.pm electric](https://hex.pm/packages/electric/1.7.2/files)
- Comparison (Oct 2026 blog, secondary): Electric is Apache-2.0, Postgres-only (requires `wal_level=logical`), writes go through your own API, row-level LWW, highest adoption (10,408 GitHub stars) — [pistack, 2026-10-21](https://www.pistack.xyz/posts/2026-10-21-local-first-sync-engines-electricsql-powersync-triplit/)
- A claimed 2026 "agents pivot" for Electric was not confirmed by any primary source (unverified) — [electric-sql.com docs](https://electric-sql.com/docs/integrations/supabase)

**PowerSync**
- (Secondary, Oct 2026) License FSL (source-available, not OSI); source DBs Postgres, MySQL, MongoDB, SQL Server; client storage SQLite; built-in upload queue to your backend; sync rules in YAML with JWT claims; row-level LWW; self-host image `journeyapps/powersync-service` (1.27.x) — [pistack](https://www.pistack.xyz/posts/2026-10-21-local-first-sync-engines-electricsql-powersync-triplit/)
- Self-hosted "Open Edition" available besides PowerSync Cloud — [PowerSync blog](https://www.powersync.com/blog/offline-first-apps-made-simple-supabase-powersync)

**Triplit**
- "Triplit joins Supabase" published Oct 8, 2025 — [Supabase blog](https://supabase.com/blog/authors/paul%5Fcopplestone)
- Repo "stalled since Jan 2026" (last push 2026-01-19); AGPL-3.0/Apache mix (secondary) — [pistack](https://www.pistack.xyz/posts/2026-10-21-local-first-sync-engines-electricsql-powersync-triplit/)

**Others**
- Convex: managed reactive DB with TypeScript functions and live queries; more server-centric, not local-first (secondary) — [doolpa](https://doolpa.com/article/triplit)
- A June 2026 RxDB comparison draft (itself generated by an AI tool) lists Realm/Atlas Device Sync EOL Sept 30 2025, Jazz v2 rewrite away from CRDT/E2EE (Apr 2026) — unverified — [RxDB PR mirror](https://github.fleetingtimer.workers.dev/pubkey/rxdb/pull/8629)

**Linear**
- Linear's engine: in-memory normalized object graph with MobX reactivity; CRDTs only for issue descriptions (secondary summary of Tuomas Artman's talk) — [Recapio digest](https://recapio.com/digest/tuomas-artman-building-a-synchronous-experience-with-asynchronous-data-linear-s-sync-engine-by-local)
- Linear treats the client's IndexedDB as a real database, not a cache (secondary) — [Architecture Maps](https://ggprompts.github.io/htmlstyleguides/architecture/linear/index.html)
- "Scaling the Linear Sync Engine" (June 29, 2023) talk covers scaling challenges and API evolution (details only in video) — [Linear blog](https://linear.app/now/scaling-the-linear-sync-engine)
- Community reverse-engineering of Linear's sync engine — [Aslam97/reverse-linear-sync-engine](https://github.com/Aslam97/reverse-linear-sync-engine)

**Figma**
- Figma (2019): clients connect over WebSockets to servers; the server is the central authority, which lets Figma drop CRDT overhead; conflict handling is per-property last-writer-wins without timestamps because the server orders events; the document is "inspired by multiple separate CRDTs" used in combination — [Figma blog](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)
- Each live document routed to a dedicated process (secondary) — [Hello Interview](https://www.hellointerview.com/learn/system-design/in-the-wild/figma-multiplayer)
- "You might not need a CRDT" argument for server-authoritative designs — [Jamsocket blog](https://jamsocket.xyz/blog/you-might-not-need-a-crdt)

### Inferences
- **Structured data (Notion database rows, page tree metadata, members, permissions)** fits a Linear/Figma/Zero-style model: server-authoritative ordered mutation log, optimistic client mutations, row/property-level LWW, server-side permission checks on every write and on read fan-out. CRDTs add little for single-cell values and complicate permission enforcement.
- **Rich text page content** benefits from CRDTs (offline edits, concurrent typing, merges) — this is exactly where Notion (offline pages) and Linear (issue descriptions) adopted them.
- Off-the-shelf engines are a poor fit for the stated self-host constraint: Zero/Electric require Postgres with logical replication (no SQLite-default), Zero has no offline writes and TS-only clients, PowerSync is FSL (not OSI open source), Triplit is effectively unmaintained. A purpose-built, simple sync protocol inside the single backend binary (per-workspace monotonic version / "lastSyncId", delta packets, mutation queue, Replicache-like rebase on the client) is the most coherent choice; Zero/Electric remain options for a "Postgres edition".
- The hybrid design is therefore recommended: CRDT doc per page (content) + server-authoritative op log (structure/data/permissions), sharing one WebSocket and one client SQLite store.

### Gaps
- LiveStore, InstantDB, Jazz (current version/status), Replicache (maintenance status), Convex self-host: no reliable 2026 primary sources gathered.
- Zero license not stated on the fetched page.
- Linear's sync protocol specifics (sync IDs, delta packets, bootstrapping) were only available via secondary sources.

---

## 4. Editor frameworks for a Notion-style block editor

### Takeaway
ProseMirror-based stacks dominate collaborative block editing: **BlockNote** (built on ProseMirror+Tiptap) gives Notion-style blocks, drag handles and slash menus out of the box with Yjs collaboration; **Tiptap 3** (MIT core, open drag-handle extension, Hocuspocus 4 server) is the flexible base; **Lexical** (MIT) requires building block UI yourself; **BlockSuite** (AFFiNE) is the most Notion/whiteboard-complete but most idiosyncratic. Licensing matters: BlockNote XL packages are GPL-3.0/commercial, Tiptap Pro extensions remain proprietary (private registry, free with account).

### Cited Findings
**BlockNote**
- Core packages MPL-2.0; XL packages (`@blocknote/xl-*`) are GPL-3.0, with a commercial license required for closed-source use — [BlockNote GitHub](https://github.com/TypeCellOS/BlockNote); [@blocknote/core README](https://unpkg.com/@blocknote/core@0.52.1/README.md)
- Still pre-1.0 (0.5x line, e.g. @blocknote/react 0.54.0) — [unpkg @blocknote/react 0.54.0](https://unpkg.com/@blocknote/react@0.54.0/README.md)
- Provides block-based editing, drag handles and slash menus that would be built by hand on Tiptap; built on ProseMirror/Tiptap; more opinionated, less flexible for highly custom UIs (secondary) — [Velt review](https://velt.dev/blog/best-rich-text-editors-react-2026); [Velt BlockNote collab guide](https://velt.dev/blog/blocknote-collaborative-editor-guide)
- XWiki evaluated BlockNote as its next rich editor, citing Tiptap's UI elements progressively becoming paid extensions — [XWiki forum](https://forum.xwiki.org/t/blocknote-as-the-next-rich-editor-for-xwiki/18087)
- OpenProject maintains a BlockNote fork — [opf/BlockNote](https://github.com/opf/BlockNote)
- Draft support for Yjs 14 / @y/prosemirror v2 (May 2026) — [BlockNote PR mirror](https://github.fleetingtimer.workers.dev/TypeCellOS/BlockNote/pull/2739)

**Tiptap + Hocuspocus**
- Tiptap Pro extensions require the private registry (e.g., `@tiptap-pro/extension-unique-id`), free with a Tiptap account (not open source) — [Tiptap UniqueID docs](https://tiptap.dev/docs/editor/api/extensions/unique-id)
- Open-source `@tiptap/extension-drag-handle` is MIT (v3.27.1) — [Tiptap Drag Handle docs](https://tiptap.dev/docs/editor/extensions/functionality/drag-handle); [jsDelivr](https://www.jsdelivr.com/package/npm/@tiptap/extension-drag-handle)
- Hocuspocus 4 (Show HN May 20, 2026; 4.7 current): Yjs WebSocket server, works with any Yjs client; v4 moved hooks to web-standard Request/Headers and runs on Node.js, Bun, Deno and Cloudflare Workers (via crossws); SQLite extension switched to better-sqlite3 (secondary) — [Hocuspocus 4 Show HN](https://hnshowcase.com/post/48208834/hocuspocus-4-yjs-collab-server); [Railway template](https://railway.com/deploy/hocuspocus); [localtonet guide](https://localtonet.com/blog/self-host-hocuspocus-sqlite-localtonet)

**Lexical**
- MIT; minimal core + plugins; block UI (drag-and-drop reordering, block manipulation) not built in; y-lexical plugin for CRDT collaboration (secondary) — [Velt comparison](https://velt.dev/blog/best-rich-text-editors-react-comparison); [Weaver editor comparison](https://weaver-docs.pages.dev/docs/comparison)

**BlockSuite**
- Toolkit from Toeverything (powers AFFiNE) for collaborative block-based editors; ships a block-based PageEditor plus an Edgeless (canvas) mode; custom blocks and inline embeds — [BlockSuite README](https://cdn.jsdelivr.net/gh/toeverything/blocksuite@main/README.md); [blocksuite.io](https://blocksuite.io/guide/overview)

### Inferences
- For a Notion clone, **BlockNote on Yjs** is the fastest path (block model, nested blocks, drag handles, slash menu, collaboration cursors), as long as the project avoids the GPL XL packages or is itself GPL/AGPL-compatible (an AGPL-3.0 project can combine with GPL-3.0 code — confirm with counsel).
- If the team wants full control or Loro instead of Yjs, build on **ProseMirror/Tiptap 3 core** (MIT) with your own block UI; ProseMirror's node schema handles nested blocks and inline database "embed" nodes rendered as React node views.
- Inline databases should be embedded as a block referencing a separate database entity (not CRDT content), so a 100k-row table is never part of the page's CRDT document.

### Gaps
- Plate/Slate, Milkdown, Novel: no current data gathered.
- Loro editor bindings (loro-prosemirror etc.) maturity not verified this session (loro.dev 403).
- Lexical's Yjs binding quality and Loro support not verified.

---

## 5. Backend language/framework for performance and single-binary self-hosting

### Takeaway
Real-world open-source workspaces split between Rust (AppFlowy Cloud, Zed collab server, y-octo core in AFFiNE), Go (Anytype any-sync), and Node with Rust cores (AFFiNE, Hocuspocus). Rust offers native CRDT engines (yrs, Loro, y-octo) and a single static binary with embedded SQLite; Go offers simple single binaries (Anytype bundle, PocketBase precedent) but lacks first-class Yjs/Loro implementations; TypeScript (Node/Bun) is fastest to build with the richest Yjs tooling.

### Cited Findings
- AppFlowy Cloud is a Rust application serving AppFlowy desktop/web clients; collab crates on yrs; self-hosting requires Postgres, Redis, MinIO and the gotrue auth service (secondary) — [Mintlify AppFlowy docs](https://www.mintlify.com/AppFlowy-IO/AppFlowy/developer/rust-backend); [selfhostyourself](https://selfhostyourself.com/alternative-to/appflowy-cloud)
- AFFiNE: Node.js backend with y-octo (Rust Yjs implementation) via NAPI; OctoBase is a separate Rust data engine (affine-cloud and keck collaboration backends) (secondary) — [openapps.pro AFFiNE](https://openapps.pro/apps/affine); [OctoBase](https://github.com/fourdim/OctoBase)
- Anytype any-sync (Go): any-sync-node (stores spaces/objects), any-sync-filenode (files), any-sync-coordinator (network config); official docker-compose is for personal/home use, not heavy use — [any-sync-dockercompose](https://www.github.com/anyproto/any-sync-dockercompose); [pkg.go.dev any-sync](https://pkg.go.dev/github.com/anyproto/any-sync@v0.4.9)
- Community `any-sync-bundle` merges all Anytype sync modules into a single binary using two ports (TCP 33010 DRPC, UDP 33020 QUIC) — [any-sync-bundle](https://pkg.go.dev/github.com/grishy/any-sync-bundle)
- Zed: nearly 1M lines of Rust incl. distributed collaboration systems built on CRDTs; editor GPL, server components AGPL — [Zed blog: CRDTs](https://zed.dev/blog/crdts); [InfoQ: Zed open-sourced](https://www.infoq.com/news/2024/02/zed-code-editor-open-sourced)
- Electric's sync service lineage is Elixir (published on hex.pm) — [hex.pm electric](https://hex.pm/packages/electric/1.0.2)
- Hocuspocus 4 runs on Node, Bun, Deno, Cloudflare Workers — [Hocuspocus 4 Show HN](https://hnshowcase.com/post/48208834/hocuspocus-4-yjs-collab-server)

### Inferences
- **Rust (Axum + tokio + yrs or Loro + rusqlite/sqlx)** best fits "single binary, Raspberry Pi to large org": native CRDT merge/compaction on the server, low memory, static binary embedding the web assets, SQLite embedded with optional Postgres. Cost: slower iteration and a smaller web-dev talent pool.
- **Go** is a strong second for ops simplicity, but server-side CRDT work would require cgo/WASM bindings to yrs/Loro or treating CRDT updates as opaque blobs (server can't compact/inspect) — acceptable if the server only stores/forwards updates and clients do compaction.
- **TypeScript (Bun/Node)** maximizes ecosystem reuse (Yjs, Hocuspocus, Zero) and can share code with the client; memory/CPU per connection is higher and "single binary" relies on runtime bundling.
- Elixir/Phoenix offers excellent WebSocket fan-out (Electric precedent) but complicates single-binary distribution and CRDT libraries are via NIFs.

### Gaps
- PocketBase (Go + SQLite "single file backend") was not re-verified this session.
- No benchmark comparing per-connection memory of Hocuspocus vs. a yrs-based Rust server was found (a ">25k connections" Hocuspocus figure is an unverified commenter claim).
- Bun `--compile` single-binary maturity not researched.

---

## 6. Database: SQLite vs PostgreSQL, CRDT storage, Notion-style databases, permissions

### Takeaway
SQLite (WAL) is a credible default for single-server installs, and its operational gap (backup/replication) narrowed with Litestream's 2025 revamp (LTX format, v0.5.0, VFS read replicas); Postgres remains the choice for large multi-tenant installs and is required by Zero/Electric. CRDT documents should be stored as snapshot + append-only update log with periodic compaction (Loro/Automerge provide compact snapshots; Loro adds shallow snapshots). For permissions, OpenFGA can run on SQLite, while SpiceDB's embedded mode is check-only — an embedded Zanzibar-style tuple table in the main DB is the simplest default.

### Cited Findings
**SQLite operations**
- Litestream revamped (Fly.io, May 2025): LTX files (sorted page changesets, mergeable) replace "generations", which had blocked failover and read replicas — [Fly.io: Litestream Revamped](https://fly.io/blog/litestream-revamped/)
- Litestream v0.5.0 shipped Oct 2, 2025 (LTX on the wire, monotonic TXIDs, compaction ladder) — [Fly.io: Litestream v0.5.0](https://fly.io/blog/litestream-v050-is-here/); summary in [sysdesign-wiki](https://sysdesign-wiki.pages.dev/systems/litestream/)
- Litestream VFS (SQLite extension; read pages from S3 while hydrating) reported Dec 11, 2025 (secondary, unverified) — [sysdesign-wiki: Litestream VFS](https://sysdesign-wiki.pages.dev/sources/2025-12-11-flyio-litestream-vfs/)
- Zero's server (zero-cache) itself keeps a SQLite replica and uses Litestream (1.0 fixed "corrupted litestream restores") — [InfoQ](https://infoq.com/news/2026/06/zero-version-1); [Zero docs](https://zero.rocicorp.dev/docs/when-to-use)

**Postgres at scale**
- Notion: 480 logical shards over 32 physical Postgres DBs, block tables co-sharded — [Notion blog](https://notion.com/blog/sharding-postgres-at-notion)
- Electric requires Postgres `wal_level=logical` and a replication slot — [pistack](https://www.pistack.xyz/posts/2026-10-21-local-first-sync-engines-electricsql-powersync-triplit/)

**CRDT document storage**
- Loro shallow snapshots remove old history (like git shallow clone); a shallow-snapshot load of a multi-million-op doc took 0.37 ms — [Loro 1.0 blog (mirror)](https://hn.jakechampion.name/reader/https://loro.dev/blog/v1.0); [Loro docs: shallow snapshots](https://www.loro.dev/docs/advanced/shallow_snapshot)
- Automerge 3 stores and now runs on compressed columnar format; format unchanged from v2 — [Automerge 3 blog](https://automerge.org/blog/automerge-3/)
- Notion applies push updates via a per-page "page version snapshot system" and per-page last-updated timestamps — [Notion offline blog](https://www.notion.com/blog/how-we-made-notion-available-offline)

**Permissions (ReBAC)**
- OpenFGA supports `--datastore-engine sqlite` (file URI) as a storage engine; a roadmap issue discusses embedding OpenFGA as a library + SQLite — [OpenFGA configure docs](https://openfga.dev/docs/getting-started/setup-openfga/configure-openfga); [OpenFGA roadmap #56](https://worker-github.zxx930427.workers.dev/openfga/roadmap/issues/56)
- SpiceDB embedded package provides only the Check API (no schema/relationship writes, watch, lookup); wrappers list memory/postgres/cockroachdb/spanner/mysql datastores, no SQLite — [spicedb-embedded](https://docs.rs/crate/spicedb-embedded/0.7.0/source/src/spicedb.rs); [SpiceDbEmbedded](https://www.nuget.org/packages/SpiceDbEmbedded); [SpiceDB datastores](https://docs.authzed.com/spicedb/selecting-a-datastore)

### Inferences
- **Default: SQLite WAL, one file per server** (optionally one file per workspace for large installs, enabling per-tenant backup/move), Litestream to S3/local for continuous backup. **Optional: Postgres** via the same repository layer (sqlx supports both), partitioned by `workspace_id` like Notion.
- **CRDT storage schema**: `doc_snapshot(doc_id, version, bytes, created_at)` + `doc_update(doc_id, seq, bytes, client_id, created_at)`; compact when the update log exceeds N updates or M bytes (merge updates into a new snapshot server-side with yrs/Loro); keep periodic snapshots for page history; use shallow snapshots (Loro) for clients that don't need full history.
- **Notion-style databases at 100k+ rows** (no primary source found; reasoning): store rows as records (`row_id, database_id, properties JSON`) with property values keyed by stable property IDs (Notion's model), plus a separate `relation(edge)` table for relations; create expression/generated-column indexes on frequently filtered/sorted properties (JSON1/JSONB); evaluate filters/sorts in SQL server-side and in client SQLite for cached views; precompute rollups and formulas incrementally (dependency graph per database, recompute on change, cache results in a materialized column) rather than at query time. EAV scales poorly for multi-property filters; per-database physical tables give the best query performance but make schema changes and multi-tenant migrations heavy — JSON properties + targeted indexes is the usual middle ground.
- **Permissions default**: embedded tuple table (`object, relation, subject`) evaluated in-process with inheritance along the page tree (workspace → teamspace → page → child pages), cached per request; offer OpenFGA (works on SQLite/Postgres) as an optional external engine for large orgs. Permission checks must also gate sync fan-out (which rows/docs a client may subscribe to).

### Gaps
- No primary benchmark found on SQLite vs Postgres for this workload or on JSONB vs EAV vs per-table for 100k+ row Notion-like databases.
- How Notion computes rollups/formulas at scale was not found in the engineering blog.
- LiteFS status in 2026 not researched.
- OpenFGA SQLite production-readiness not stated in docs.

---

## 7. Search: full-text and vector

### Takeaway
For the SQLite default, FTS5 + sqlite-vec (pre-1.0, 0.1.9) with Reciprocal Rank Fusion is an established hybrid-search pattern; for Postgres, native tsvector/GIN is adequate for small installs while Tantivy-based ParadeDB pg_search claims large speedups (vendor-run) plus pgvector for semantic search. An embedded Tantivy index in a Rust binary would give identical search behavior across both database backends.

### Cited Findings
- sqlite-vec latest npm release 0.1.9 (~Mar 31, 2026); 0.1.10 alpha builds by May 2026 (still pre-1.0) — [Socket: sqlite-vec](https://socket.dev/npm/package/sqlite-vec); [RubyGems versions](https://index.rubygems.org/gems/sqlite-vec/versions)
- Default `vec0` table does exact (brute-force) KNN; described as fast enough up to a few hundred thousand vectors (single-author claim) — [DEV: hybrid RAG with FTS5 + sqlite-vec](https://dev.to/soytuber/building-a-hybrid-rag-in-200-lines-sqlite-fts5-sqlite-vec-rrf-38h1)
- FTS5 BM25 + sqlite-vec KNN merged with RRF (k≈60) is a common hybrid pattern — [DEV guide](https://dev.to/soytuber/building-a-hybrid-rag-in-200-lines-sqlite-fts5-sqlite-vec-rrf-38h1); [gnosis-mcp SQLite backend](https://glama.ai/mcp/servers/@nicholasglazer/gnosis-mcp/blob/c98bf9ef8f3067a6a63b78e3c3ff9c126d52042c/src/gnosis_mcp/sqlite_backend.py)
- Reported: a `vec1` vector extension added to the SQLite source tree on 2026-09-30 (single secondary source, unverified) — [st-hakky news](https://book.st-hakky.com/news/sqlite-vec1-vector-search-extension)
- ParadeDB pg_search (Tantivy inside Postgres): up to 1,000x faster than tuned GIN/tsvector on a 10M-row dataset (vendor benchmark with Neon) — [Neon blog](https://neon.com/blog/pgsearch-on-neon); [ParadeDB blog](https://www.paradedb.com/blog/introducing-search)
- Contested: Timescale's pg_textsearch (custom BM25 index) reportedly beat ParadeDB on MS MARCO latency in March 2026 (secondary); ParadeDB itself shows results vary by query shape — [aicoolies comparison](https://aicoolies.com/comparisons/paradedb-vs-pg-textsearch); [ParadeDB blog](https://www.paradedb.com/blog)

### Inferences
- Recommended default: SQLite FTS5 (+ trigram tokenizer for CJK/substring) and sqlite-vec for semantic search; Postgres edition: tsvector or pg_search + pgvector. Alternative: embed Tantivy in the Rust server for both editions (consistent ranking, typo tolerance via fuzzy queries), storing the index on local disk.
- Index page text extracted from CRDT snapshots asynchronously (after compaction), not on every keystroke; enforce permissions by filtering results against the permission model (post-filter or per-workspace indices).
- Meilisearch as an optional external service for large orgs wanting typo-tolerant instant search (not researched this session).

### Gaps
- Tantivy, Meilisearch, LanceDB current versions/benchmarks/licenses not researched.
- pgvector version and performance at scale not researched.
- No benchmark of FTS5 vs Tantivy on workspace-like corpora found.

---

## 8. File storage

### Takeaway
Use local disk by default behind an S3-compatible abstraction; avoid MinIO Community Edition (no binaries since Oct 2025, maintenance mode Dec 2025, archived early 2026). For S3-compatible self-hosting, SeaweedFS (Go, Apache-2.0) and Garage (Rust, AGPLv3, geo-distributed, no versioning) are the main alternatives; RustFS (Apache-2.0) is young.

### Cited Findings
- MinIO stopped publishing community Docker images and binaries in Oct 2025; repo moved to maintenance mode in Dec 2025 (no new features, no PR reviews, no guaranteed security patches); archived read-only in early 2026 (sources disagree on Feb vs. Apr 2026) — [InfoQ](https://infoq.com/news/2025/12/minio-s3-api-alternatives/); [Elest.io](https://blog.elest.io/minio-is-in-maintenance-mode-your-guide-to-s3-compatible-storage-alternatives/); [Pinggy](https://pinggy.io/blog/minio_archived_self_hosted_s3_alternatives/)
- SeaweedFS: Go, Apache-2.0, strong with billions of small files, ships an admin UI; Garage: Rust, AGPLv3, designed for self-hosting and geo-distribution, no object versioning yet; RustFS: Rust, Apache-2.0, recommended only for monitored pilots (secondary) — [Elest.io comparison](https://blog.elest.io/rustfs-vs-seaweedfs-vs-garage-which-minio-alternative-should-you-pick/)
- Open edX (tutor-minio) chose Garage to replace MinIO — [Open edX discuss](https://discuss.openedx.org/t/replacing-minio-in-tutor-minio-why-we-chose-garage-and-what-comes-next/18868)
- AppFlowy Cloud's self-host stack still lists MinIO for files (secondary) — [selfhostyourself](https://selfhostyourself.com/alternative-to/appflowy-cloud)
- Anytype uses a dedicated file node (any-sync-filenode) — [pkg.go.dev any-sync](https://pkg.go.dev/github.com/anyproto/any-sync@v0.4.9)

### Inferences
- Default: content-addressed files on local disk (hash-named, deduplicated, served with range requests and signed URLs), with an `ObjectStore` trait implementing local FS and generic S3 (AWS S3, Cloudflare R2, Backblaze B2, Garage, SeaweedFS). This keeps the Raspberry Pi install to one container and one volume.
- Garage's AGPL is not an issue when run as a separate service; it matters only if embedded/distributed in a proprietary product.

### Gaps
- Ceph RGW only mentioned in passing; no benchmarks between SeaweedFS/Garage/RustFS were found.

---

## 9. Federation / "anyone can run a server"

### Takeaway
Federation for private, permissioned workspace data is still immature: AT Protocol's permissioned-data "Spaces" only reached alpha in 2026 and has no relay for private data; Anytype lets users choose or self-host a sync network but does not federate networks. For a workspace product, independent servers with optional sharing (public links, guest accounts, export/import, server-to-server page sharing later) is the realistic design.

### Cited Findings
- atproto Spaces (extension for non-public data) is in alpha; protocol design, SDKs and schema not final; spaces range from single-member to communities with potentially millions of participants — [atproto blog: Spaces alpha](https://atproto.com/blog/atproto-spaces-alpha)
- AT Protocol Spring 2026 roadmap (Mar 24, 2026): permissioned data a major focus through summer; other teams (Blacksky, Northsky, Habitat) building extensions — [atproto roadmap](https://atproto.com/blog/2026-spring-roadmap)
- No relay for permissioned data; reading a space means syncing from members' PDS hosts directly (secondary) — [atproto blog](https://atproto.com/blog/atproto-spaces-alpha); [minifeed: Proposal 0016](https://minifeed.net/items/YcPr2quT8HZU)
- Critique: atproto's public-first design forces developers to build two separate applications for private data (Luke Kanies, secondary) — [zeli.app](https://zeli.app/story/49025984)
- Anytype: users choose which provider(s) to use or self-host the infrastructure — [any-sync networking docs](https://sync.any.org/networking/); no evidence of interoperation between separate any-sync networks found

### Inferences
- Federation adds identity, permission-propagation and moderation problems across trust boundaries that Matrix/ActivityPub/atproto are still working through for private data; it is not a v1 requirement for a Notion-like product.
- Practical "anyone can run a server" approach: each server is an independent authority (own users/SSO, own data); cross-server collaboration via (a) guest accounts / OIDC federation of identity, (b) public/read-only share links, (c) later, a server-to-server "shared page" protocol that syncs one CRDT document between two servers (CRDTs make this technically straightforward for page content; permissions remain owned by the origin server).
- Keep data portable (Markdown/JSON export, CRDT snapshot export) so users can move between servers.

### Gaps
- Matrix-based collaborative document efforts and ActivityPub for documents were not researched this session.

---

## 10. Client performance techniques

### Takeaway
Instant page opens come from a local, persistent client database (SQLite WASM on OPFS in a worker, as Notion does; IndexedDB as a real DB, as Linear does), rendering from local state first and reconciling with the server in the background, plus aggressive virtualization and lazy-loading of block subtrees and database rows.

### Cited Findings
- Notion: SQLite WASM in Web Workers with OPFS SyncAccessHandle Pool VFS, SharedWorker single-writer; 20% faster navigation; WASM download initially regressed first load — [Notion WASM SQLite blog](https://www.notion.com/blog/how-we-sped-up-notion-in-the-browser-with-wasm-sqlite)
- Zero runs queries against the local cache first; authoritative results stream in; apps commonly load in < 1 s with query-defined partial sync — [InfoQ](https://infoq.com/news/2026/06/zero-version-1); [Zero docs](https://zero.rocicorp.dev/docs/when-to-use)
- Linear: in-memory object graph + MobX, IndexedDB as database (secondary) — [Recapio](https://recapio.com/digest/tuomas-artman-building-a-synchronous-experience-with-asynchronous-data-linear-s-sync-engine-by-local); [Architecture Maps](https://ggprompts.github.io/htmlstyleguides/architecture/linear/index.html)
- Loro shallow snapshot loads a multi-million-op document in 0.37 ms — [Loro 1.0 blog (mirror)](https://hn.jakechampion.name/reader/https://loro.dev/blog/v1.0)
- Eg-walker keeps only plain text in memory during editing (order of magnitude less memory) — [arXiv 2409.14252](https://arxiv.org/abs/2409.14252)
- Notion offline: auto-download up to 50 rows per database view, full-page-or-nothing availability — [Notion offline blog](https://www.notion.com/blog/how-we-made-notion-available-offline)

### Inferences
- Client architecture: SQLite WASM (official sqlite-wasm or wa-sqlite) in a dedicated worker with OPFS SAH-pool VFS; a SharedWorker/Web Locks leader election for single-writer; CRDT docs (Yjs/Loro) loaded in the worker and applied incrementally; UI thread receives view models only.
- Database views over 100k rows: server-side (or client-SQLite) filter/sort/paginate with windowed fetching; virtualized grid (row + column virtualization); board/calendar views fetch grouped aggregates first then lazy-load cards.
- Long pages: render the first viewport of blocks from the local snapshot, virtualize or lazy-mount below-the-fold blocks, load sub-page/inline-database blocks on demand.
- Load the WASM SQLite asynchronously (do not block first paint), as Notion's regression showed.

### Gaps
- No primary benchmarks gathered for list/grid virtualization libraries, or for OPFS vs IndexedDB throughput in 2026.

---

## 11. Recommended reference architecture and main alternatives considered

### Takeaway
A **hybrid, server-authoritative + CRDT** design in a **single Rust binary** (Axum + yrs or Loro + SQLite by default, Postgres optional) with a **local-first web client** (SQLite WASM/OPFS cache, CRDT page docs, optimistic mutation queue) best satisfies: live collaboration, permissions, Notion-style databases at 100k+ rows, offline, instant UI and one-container self-hosting. Each component choice below is anchored in the cited evidence above; items labeled "inference" are design reasoning rather than measured results.

### Cited Findings
- Hybrid precedent: Notion uses CRDT only for (offline) page content on top of a server-authoritative record model — [Notion offline blog](https://www.notion.com/blog/how-we-made-notion-available-offline); Linear uses CRDT only for issue descriptions — [Recapio](https://recapio.com/digest/tuomas-artman-building-a-synchronous-experience-with-asynchronous-data-linear-s-sync-engine-by-local); Figma uses server ordering + per-property LWW — [Figma blog](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)
- Rust CRDT servers in production open-source workspaces: AppFlowy Cloud (yrs), AFFiNE (y-octo), Zed — [Mintlify AppFlowy](https://www.mintlify.com/AppFlowy-IO/AppFlowy/developer/rust-backend); [openapps.pro AFFiNE](https://openapps.pro/apps/affine); [Zed blog](https://zed.dev/blog/crdts)
- Off-the-shelf sync engine constraints (Postgres-only, no offline writes, FSL, stalled) — [Zero docs](https://zero.rocicorp.dev/docs/when-to-use); [pistack](https://www.pistack.xyz/posts/2026-10-21-local-first-sync-engines-electricsql-powersync-triplit/)
- SQLite operational maturity (Litestream LTX/v0.5.0) — [Fly.io](https://fly.io/blog/litestream-v050-is-here/)
- MinIO CE end of life — [InfoQ](https://infoq.com/news/2025/12/minio-s3-api-alternatives/)

### Inferences
**Reference architecture (recommended)**

| Layer | Recommendation | Why / evidence | Maturity & license notes |
|---|---|---|---|
| Data model | Notion-style block graph: `block(id, workspace_id, parent_id, type, props JSON, child order)`; **one CRDT document per page** holds the page's block tree + rich text; pages, page-tree, databases, rows, members, permissions are server-authoritative records | Notion data model; Notion/Linear hybrid | — |
| Page content sync | **Yjs (client) + yrs (Rust server)** today; adopt Yjs 14 when stable for attributions/changesets. Alternative: **Loro 1.13** if Rust-first, movable tree and built-in time travel are priorities | Ecosystem maturity (BlockNote, Hocuspocus, AppFlowy, AFFiNE); Loro fastest in old benchmarks, richest feature set | Yjs 13.6.x stable, 14 RC; Loro 1.x stable since 2024 |
| Structured data sync | Custom server-authoritative op log (per-workspace monotonic version, delta packets over WebSocket, client mutation queue with optimistic apply + rebase), row/property LWW, server permission checks on write and fan-out | Linear/Figma/Zero pattern; no off-the-shelf engine fits SQLite-default single binary | Zero 1.0 / Electric 1.x optional for a Postgres edition |
| Page tree / block moves | Server-authoritative move ops with cycle check + fractional indexing; or Loro Movable Tree if fully CRDT | Figma server-ordering; Loro movable tree | — |
| Editor | **BlockNote** (MPL core) on Yjs for fastest Notion UX; or Tiptap 3 core (MIT) + custom block UI; avoid GPL XL / Tiptap Pro unless licensing allows | BlockNote features; licensing findings | BlockNote 0.5x (pre-1.0); Tiptap 3 stable |
| Backend | **Rust**: Axum + tokio, yrs/Loro, sqlx (SQLite+Postgres), embedded static web assets → one binary/one container | Rust precedents; single-binary goal | — |
| Database | **SQLite WAL default** (+ Litestream continuous backup), **Postgres optional** partitioned by workspace; CRDT stored as snapshot + update log with server-side compaction; Notion databases as row records with JSON properties + indexed hot properties + relation edge table; rollups/formulas incrementally maintained | Litestream revamp; Notion sharding; Loro/Automerge snapshot formats | Litestream v0.5.x |
| Permissions | Embedded Zanzibar-style tuple table with page-tree inheritance; optional OpenFGA for large orgs | OpenFGA supports SQLite; SpiceDB embedded is check-only | OpenFGA (CNCF) |
| Search | SQLite FTS5 + sqlite-vec (RRF hybrid); Postgres: tsvector or pg_search + pgvector; or embedded Tantivy for both | Hybrid FTS5/vec pattern; ParadeDB benchmarks (vendor) | sqlite-vec pre-1.0 |
| Files | Local disk (content-addressed) default; S3-compatible optional (Garage/SeaweedFS/R2/S3); not MinIO CE | MinIO EOL; alternatives | Garage AGPL, SeaweedFS Apache-2.0 |
| Client | SQLite WASM on OPFS (SAH-pool VFS) in a worker, SharedWorker single-writer, render-from-local-first, virtualization, lazy block loading, offline set with reasons | Notion WASM SQLite + offline posts | — |
| Federation | Independent servers; optional sharing via links/guests/OIDC; future server-to-server shared-page CRDT sync | atproto Spaces still alpha | — |
| Deployment | Single Docker image (or static binary) with one data volume; env-var switch to Postgres/S3; horizontal scale later by routing each page/workspace to one node (Figma "process per document") | Figma per-document routing (secondary) | — |

**Main alternatives considered**
1. **TypeScript stack (Bun/Node + Hocuspocus 4 + Yjs + BlockNote + Zero/Electric on Postgres)** — fastest to build, richest Yjs tooling, Zero gives Linear-class UI speed; but Postgres becomes mandatory (no SQLite default), Zero has no offline writes and TS-only clients, heavier runtime footprint on a Pi.
2. **Fully local-first CRDT everything (Loro or Automerge for pages *and* databases, Anytype-style)** — best offline/P2P story; but permissions, server-side queries over 100k-row databases, rollups and formulas become hard (every client must hold and query large CRDT docs), and revocation/ACLs are weak in pure CRDT systems.
3. **Go single binary (PocketBase-like) with CRDT updates stored as opaque blobs** — very simple ops; but no first-class Go Yjs/Loro implementation, so server cannot compact/inspect docs (search indexing must happen client-side or via WASM).
4. **Elixir/Phoenix (Electric-style)** — excellent real-time fan-out; harder single-binary distribution, CRDT via NIFs.
5. **Off-the-shelf commercial-ish engines (PowerSync, Convex, Liveblocks)** — rejected for licensing (FSL) or managed/server-centric orientation conflicting with "fully open source, easy self-host".

**Key risks to validate with prototypes**
- Re-benchmark Yjs 13/14 vs Loro 1.13 vs Automerge 3.3 on a block-tree workload (10k-block page, concurrent moves) — existing public numbers are outdated.
- Measure 100k-row database views on SQLite (JSON properties + expression indexes) on a Raspberry Pi 4/5 class device.
- Validate BlockNote's Yjs 14 migration path and licensing of needed features.

### Gaps
- No single published end-to-end reference architecture matching all constraints was found; the table above is a synthesis.
- License status of several components (Zero, OpenFGA, Loro, Yjs) not re-verified in this session.
