# Open-source / self-hostable Notion alternatives: architectures, limitations, gaps (state as of 9 Nov 2026)

> Method note: stars, licenses, last push, archive flags, latest releases, docker-compose service lists, dependency manifests and top-reacted issues were pulled directly from the GitHub REST API on 2026-11-09 (authenticated `gh api`), so those numbers are primary-source. Qualitative claims come from official docs/release notes where possible, otherwise from secondary articles (flagged). Anything known only from background knowledge and not re-verified this session is put in Gaps or explicitly flagged as "unverified".

## Q1. Who are the main projects and how do they compare (license, stack, collaboration, self-hosting, features, AI, API/MCP, activity, funding, weaknesses)?

### Takeaway
The field consolidated hard in 2025-2026 toward **open core / source-available**: AppFlowy's server went closed-source (free self-host = 1 seat), AFFiNE's backend is under an "Enterprise Edition" license (free self-host = 10 seats/workspace), Docmost put databases ("Bases"), page-level permissions, API keys and MCP behind its EE license, Plane moved OIDC to paid, Outline is BSL 1.1, Anytype is source-available, and NocoDB moved to a "Sustainable Use License" (Jan 2026). Meanwhile several projects lost momentum (hosted Huly shut down and the repo is frozen; Colanode stalled; Focalboard unmaintained; OctoBase dormant; undb archived into Teable). The most important new entrant is France's MIT-licensed **La Suite Docs** (BlockNote + Yjs + yhub), but it has no databases.

### Cited Findings

#### A. Identity, license, activity (GitHub API snapshot 2026-11-09)

| Project | Repo | License (verified from LICENSE file) | Stars | Last push / latest release | Status notes |
|---|---|---|---|---|---|
| AppFlowy (client) | AppFlowy-IO/AppFlowy | AGPL-3.0 | 78,491 | 2026-11-02 / 0.15.0 (2026-11-02) | Server repo AppFlowy-Cloud **archived**; server now closed-source commercial |
| AFFiNE | toeverything/AFFiNE | MIT for most code; `packages/backend` + `packages/common/native` under **AFFiNE EE license** (CE parts MPL-2.0) | 74,351 | 2026-11-08 / canary v2026.11.8 | stable 0.27.x (Jul 2026) |
| BlockSuite (AFFiNE editor) | toeverything/blocksuite | MPL-2.0 | 6,087 | 2026-11-06 | — |
| y-octo (Rust Yjs-compatible CRDT) | y-crdt/y-octo | MIT | 351 | 2026-11-03 | — |
| OctoBase | toeverything/OctoBase | AGPL-3.0 | 2,083 | **2025-03-05** | effectively dormant |
| Anytype (desktop) | anyproto/anytype-ts | **Any Source Available License 1.0** (not OSI) | 9,062 | 2026-11-03 / v0.57.7 | anytype-heart (Go middleware) same license; any-sync is MIT |
| Docmost | docmost/docmost | AGPL-3.0 core + `ee` dirs under enterprise license | 22,262 | 2026-11-09 / v0.97.0 (2026-10-25) | — |
| Outline | outline/outline | **BSL 1.1** (no commercial "Document Service") | 41,204 | 2026-11-08 / v1.12.0 (2026-10-23) | — |
| SiYuan | siyuan-note/siyuan | AGPL-3.0 | 47,131 | 2026-11-08 / v3.8.10-beta.5 | some features for paid members |
| Logseq | logseq/logseq | AGPL-3.0 | 45,523 | 2026-11-09 / 2.0.2 beta (2026-10-07) | DB version still beta |
| Huly | hcengineering/platform | EPL-2.0 | 28,074 | 2026-11-06 / v0.7.426 (2026-07-05) | README: repo **frozen**, hosted Huly **shut down**; fork Platform-Collective/platform (151 stars) |
| Plane | makeplane/plane | AGPL-3.0 (Community Ed.) | 62,160 | 2026-11-07 / v1.4.3 (2026-10-28) | Commercial Ed. closed-source |
| La Suite Docs | suitenumerique/docs | MIT | 16,979 | 2026-11-08 / v6.0.4 (2026-10-26) | French-state project, created Jan 2024 |
| Colanode | colanode/colanode | Apache-2.0 | 5,193 | **2026-04-03** / v0.4.7 | community asking if abandoned |
| Trilium (TriliumNext) | TriliumNext/Trilium (zadam/trilium redirects here) | AGPL-3.0 | 38,742 | 2026-11-08 / v0.107.0 (2026-11-08) | — |
| Joplin | laurent22/joplin | AGPL-3.0-or-later (except dirs with own LICENSE, e.g. `packages/server`) | 57,099 | 2026-11-08 | — |
| Focalboard | mattermost-community/focalboard | MIT (compiled) / AGPL or commercial (source) | 26,551 | 2026-05-18 | "currently not maintained" |
| BookStack | BookStackApp/BookStack | MIT | 19,136 | 2026-11-08 | PHP |
| Wiki.js | requarks/wiki | AGPL-3.0 | 29,222 | 2026-11-08 | — |
| Twenty (CRM) | twentyhq/twenty | AGPL-3.0 + `@license Enterprise` files commercial + some MIT packages | 60,661 | 2026-11-08 / v2.56.1 | — |
| NocoDB | nocodb/nocodb | **Sustainable Use License** (updated 2026-01-29) | 65,453 | 2026-11-08 | no longer OSI open source |
| Baserow | baserow/baserow (GitHub mirror) | MIT core; `premium/` and `enterprise/` separately licensed | 6,294 | 2026-11-07 | — |
| Teable | teableio/teable | AGPL-3.0 (core apps) + MIT (packages) | 21,958 | 2026-11-06 | absorbed undb |
| Grist | gristlabs/grist-core | Apache-2.0 | 12,070 | 2026-11-08 | — |
| undb | undb-io/undb | AGPL-3.0 | 2,974 | archived | "moved to Teable" |

Sources for the table: GitHub API on each repo — [AppFlowy](https://github.com/AppFlowy-IO/AppFlowy), [AppFlowy-Cloud](https://github.com/AppFlowy-IO/AppFlowy-Cloud), [AFFiNE LICENSE](https://github.com/toeverything/AFFiNE/blob/HEAD/LICENSE), [AFFiNE EE license](https://github.com/toeverything/AFFiNE/blob/HEAD/packages/backend/server/LICENSE), [BlockSuite](https://github.com/toeverything/blocksuite), [y-octo](https://github.com/y-crdt/y-octo), [OctoBase](https://github.com/toeverything/OctoBase), [anytype-ts](https://github.com/anyproto/anytype-ts), [any-sync](https://github.com/anyproto/any-sync), [Docmost](https://github.com/docmost/docmost), [Outline LICENSE](https://github.com/outline/outline/blob/HEAD/LICENSE), [SiYuan](https://github.com/siyuan-note/siyuan), [Logseq](https://github.com/logseq/logseq), [Huly platform](https://github.com/hcengineering/platform), [Platform-Collective](https://github.com/Platform-Collective/platform), [Plane](https://github.com/makeplane/plane), [La Suite Docs](https://github.com/suitenumerique/docs), [Colanode](https://github.com/colanode/colanode), [Trilium](https://github.com/TriliumNext/Trilium), [Joplin LICENSE](https://github.com/laurent22/joplin/blob/HEAD/LICENSE), [Focalboard](https://github.com/mattermost-community/focalboard), [BookStack](https://github.com/BookStackApp/BookStack), [Wiki.js](https://github.com/requarks/wiki), [Twenty LICENSE](https://github.com/twentyhq/twenty/blob/HEAD/LICENSE), [NocoDB LICENSE](https://github.com/nocodb/nocodb/blob/HEAD/LICENSE), [Baserow LICENSE](https://github.com/baserow/baserow/blob/HEAD/LICENSE), [Teable LICENSE](https://github.com/teableio/teable/blob/HEAD/LICENSE), [Grist](https://github.com/gristlabs/grist-core), [undb](https://github.com/undb-io/undb).

#### B. Architecture, collaboration and self-hosting footprint (from repos' manifests and compose files)

| Project | Frontend | Backend / DB | Editor / CRDT (verified in manifests) | Real-time collab | Official self-host compose (service count) |
|---|---|---|---|---|---|
| AppFlowy | Flutter (desktop/mobile) + AppFlowy-Web (TS, AGPL) | Rust "AppFlowy Cloud" (now closed), Postgres+pgvector, Redis, GoTrue auth, MinIO | AppFlowy-Collab on **yrs 0.25** | yes (CRDT), but free self-host = 1 seat + 3 guests | **12**: nginx, minio, postgres, redis, gotrue, appflowy_cloud, admin_frontend, ai, appflowy_worker, appflowy_search, appflowy_backup, appflowy_web |
| AFFiNE | React app + **BlockSuite** (Lit web components) | Node server (EE-licensed), Postgres (pgvector), Redis | **Yjs** (patched 13.6.21) + **y-octo 0.1.1** (Rust) | yes | **4**: affine, affine_migration, redis, postgres |
| Anytype | Electron/TS + native mobile | Go middleware (anytype-heart) + any-sync nodes; MongoDB, Redis, S3 (Garage/MinIO) | own **any-sync** protocol (v0.13.8 in go.mod) | E2EE sync of shared spaces; live co-editing maturity disputed | **15**: s3-guard, any-sync-init, mongo-1, redis, garage, minio-legacy, s3-migrate, coordinator_bootstrap, coordinator, filenode, node-1/2/3, consensusnode, netcheck |
| Docmost | React + Mantine | Node (NestJS – unverified), Postgres 18, Redis 8 | **Tiptap + Hocuspocus + Yjs + y-prosemirror** | yes | **3**: docmost, db, redis |
| Outline | React | Node **Koa + Sequelize**, Postgres, Redis | **ProseMirror + y-prosemirror + Yjs + Hocuspocus** (server + redis extension) | yes | 2 infra services in repo compose (redis, postgres) + app; needs external auth provider |
| SiYuan | TypeScript/Electron | **Go kernel**, **Lute** markdown engine, SQLite | custom block editor | no multi-user real-time (single-user, sync) | single container `b3log/siyuan` |
| Logseq (DB) | ClojureScript, Electron | **Datascript** + **@sqlite.org/sqlite-wasm** | custom outliner | RTC sync in beta for supporters | no official self-host server |
| Huly | Svelte/TS | CockroachDB, Redpanda (Kafka), Elasticsearch, MinIO + many TS services | **Yjs + Hocuspocus** (collaborator service) | yes | **14**: nginx, cockroach, redpanda, minio, elastic, rekoni, transactor, collaborator, account, workspace, front, fulltext, stats, kvs |
| Plane | React (web, space, admin) | **Django** API + workers, Postgres 15, Valkey, RabbitMQ, S3 | Pages: **Tiptap + Hocuspocus + Yjs** ("live" service) | yes (Pages) | **13**: web, space, admin, live, api, worker, beat-worker, migrator, plane-db, plane-redis, plane-mq, plane-minio, proxy |
| La Suite Docs | **Next.js** | **Django** ("impress"), Postgres 16, Redis, Valkey, S3, **Keycloak/OIDC** | **BlockNote** (incl. xl-ai, xl-docx/odt/pdf exporters, xl-multi-column) + **Yjs**; collab server **yhub** since v6.0 | yes + offline editing | **7**: postgresql, redis, yhub-valkey, backend, yhub, y-provider, frontend (+ separate Keycloak, MinIO examples) |
| Colanode | React (desktop/web) | Fastify + Kysely + Postgres(pgvector), Valkey, MinIO; clients use local SQLite (Kysely) | **Tiptap + Yjs** (`packages/crdt`) | yes, local-first | **5**: postgres, valkey, minio, server, web |
| Teable | Next.js | **NestJS + knex + ShareDB** (OT), Postgres, Redis | ShareDB (OT, not CRDT) | yes (grid) | **3**: teable, teable-db, teable-cache |
| Trilium | Preact + jQuery | Node, SQLite (unverified) | **CKEditor 5** | no (single-user; multi-user is a milestone issue) | single container (unverified) |

Sources: [AppFlowy SelfHost compose](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial/blob/HEAD/docker-compose.yml); [AppFlowy-Collab Cargo.toml](https://github.com/AppFlowy-IO/AppFlowy-Collab/blob/HEAD/Cargo.toml); [AFFiNE selfhost compose](https://github.com/toeverything/AFFiNE/blob/HEAD/.docker/selfhost/compose.yml); [AFFiNE Cargo.toml](https://github.com/toeverything/AFFiNE/blob/HEAD/Cargo.toml); [AFFiNE package.json](https://github.com/toeverything/AFFiNE/blob/HEAD/package.json); [BlockSuite packages](https://github.com/toeverything/blocksuite/tree/HEAD/packages/affine); [any-sync-dockercompose](https://github.com/anyproto/any-sync-dockercompose/blob/HEAD/docker-compose.yml); [anytype-heart go.mod](https://github.com/anyproto/anytype-heart/blob/HEAD/go.mod); [Docmost compose](https://github.com/docmost/docmost/blob/HEAD/docker-compose.yml); [Docmost package.json](https://github.com/docmost/docmost/blob/HEAD/package.json); [Outline package.json](https://github.com/outline/outline/blob/HEAD/package.json); [SiYuan kernel go.mod](https://github.com/siyuan-note/siyuan/blob/HEAD/kernel/go.mod); [Logseq deps.edn](https://github.com/logseq/logseq/blob/HEAD/deps.edn); [huly-selfhost compose](https://github.com/hcengineering/huly-selfhost/blob/HEAD/compose.yml); [Huly collaborator package.json](https://github.com/hcengineering/platform/blob/HEAD/server/collaborator/package.json); [Plane community compose](https://github.com/makeplane/plane/blob/HEAD/deployments/cli/community/docker-compose.yml); [Plane live package.json](https://github.com/makeplane/plane/blob/HEAD/apps/live/package.json); [La Suite Docs compose example](https://github.com/suitenumerique/docs/blob/HEAD/documentation/examples/compose/compose.yaml); [La Suite Docs frontend package.json](https://github.com/suitenumerique/docs/blob/HEAD/src/frontend/apps/impress/package.json); [Colanode compose](https://github.com/colanode/colanode/blob/HEAD/hosting/docker/docker-compose.yaml); [Teable compose](https://github.com/teableio/teable/blob/HEAD/dockers/examples/standalone/docker-compose.yaml); [Teable backend package.json](https://github.com/teableio/teable/blob/HEAD/apps/nestjs-backend/package.json); [Trilium client package.json](https://github.com/TriliumNext/Trilium/blob/HEAD/apps/client/package.json).

#### C. Feature coverage vs Notion (summary; details per project below)

| Project | Databases / views | Relations / rollups / formulas | Permissions / orgs | Publishing | AI | API / MCP |
|---|---|---|---|---|---|---|
| AppFlowy | Database fields incl. Relation, Rollup, Person, AI Summary/Translate | Relation ✔, Rollup ✔, **no Formula field type** | workspaces, guests (self-host free: 1 seat + 3 guest editors) | Publish pages ✔ | AI service container, AI fields | OpenAPI/Swagger on commercial server; **no MCP** (FR open) |
| AFFiNE | database block + data-view (BlockSuite) | relation/rollup/formula **missing** (open FRs) | workspace members; 10 free seats/workspace self-host | public share (edit-share requested) | AFFiNE AI (needs config on self-host) | **built-in MCP** (read stable; write only canary), Markdown writes |
| Anytype | Sets/Collections over typed objects | object relations ✔; **formulas missing** (open FR) | shared spaces, Owner/Editor/Viewer (secondary source) | web publishing (unverified) | — | official **anytype-mcp** via desktop local API |
| Docmost | **Bases (Table + Kanban), EE only** since v0.95 | not documented | spaces, groups; **page-level permissions EE** | public page share (whole-space share requested) | AI settings (EE) | **MCP EE-only**, API keys EE |
| Outline | none | none | collections, groups; permissions improvements requested | public share | AI answers (unverified) | REST API + **built-in MCP** (OAuth/API key) |
| SiYuan | Database ("attribute view") with table + calendar etc. | relation ✔, rollup ✔, template field (formula-like) ✔ | single user | publish (unverified) | AI writing + chat, agent | kernel API + **built-in MCP (OAuth 2.1)** |
| Trilium | Collections: table, board, calendar, geo map | attributes/relations (unverified) | single-user | share notes to web | LLM chat/notes | ETAPI + optional **MCP** (auth required) |
| Plane | work items DB (issues), not generic | n/a for Pages | workspaces/projects; OIDC paywalled | Pages publish (space app) | Plane AI (paid, unverified) | REST + official **plane-mcp-server**, but **no Pages create/update API** |
| La Suite Docs | none | none | per-doc sharing; team access requested | public link | optional AI writing helpers (BlockNote xl-ai) | external API ("resource server"); no MCP found |
| Teable / Baserow / NocoDB / Grist | full spreadsheet-DB, many views | ✔ link/lookup/rollup/formula | ✔ | forms/shares | ✔ | REST + MCP (Teable, Baserow) |

#### D. Per-project notes

**AppFlowy (Flutter + Rust)**
- AppFlowy-Cloud repo is archived: "no longer used to build or distribute the latest AppFlowy self-hosted server"; the server is now "a closed-source fork of this open-source core combined with proprietary features", shipped only as prebuilt Docker images; "AppFlowy Web and AppFlowy Flutter remain entirely open source", but active Flutter code is being consolidated "within private repositories" and will be merged back "at a later stage" — [AppFlowy-Cloud README](https://github.com/AppFlowy-IO/AppFlowy-Cloud)
- Self-hosted Free Tier: one user seat per instance, web app, up to 3 guest editors with view/comment/edit, version history, publish pages, unlimited workspaces — [AppFlowy-SelfHost-Commercial README](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial)
- The commercial self-host README advertises audit logging, Google Drive/Calendar connections, backups and Swagger/OpenAPI "business APIs" — [AppFlowy-SelfHost-Commercial README](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial)
- Database field types in `collab-database`: RichText, Number, DateTime, SingleSelect, MultiSelect, Checkbox, URL, Checklist, LastEditedTime, CreatedTime, Relation, Summary, Translate, Time, Media, Person, Rollup — no Formula — [AppFlowy-Collab entity.rs](https://github.com/AppFlowy-IO/AppFlowy-Collab/blob/HEAD/collab/src/database/entity.rs). Note AppFlowy-Collab public repo last pushed 2026-02-18 — [GitHub API](https://github.com/AppFlowy-IO/AppFlowy-Collab)
- Most-reacted open issues: bulk export of all MD files (#3334, 49 reactions), CalDAV (#487), n8n (#731), **MCP server** (#8043, 33), Gantt (#3392), iPad app (#4250), custom OpenAI URL (#5379) — [AppFlowy issues](https://github.com/AppFlowy-IO/AppFlowy/issues/8043)
- An unofficial "built from AGPL source: no 1-user seat limit" Docker image exists — [Docker Hub mreza0100/appflowy-cloud-agpl](https://hub.docker.com/r/mreza0100/appflowy-cloud-agpl) (unofficial; legal/compat status unclear).
- Secondary reviews describe the self-hosted free tier as a single-user web tool, with desktop/mobile apps reportedly needing a paid tier — reported via search summaries of [sumguy.com](https://sumguy.com/appflowy-vs-notion/) / [ssdnodes](https://www.ssdnodes.com/learn/self-host-appflowy-on-a-vps); **not verified against AppFlowy's pricing page**.
- Funding: $6.4M seed led by OSS Capital (Nov 2023) — [FinSMEs](https://www.finsmes.com/2023/11/appflowy-raises-6-4m-in-seed-funding.html)

**AFFiNE (BlockSuite, y-octo, OctoBase)**
- Licensing: all content outside `packages/backend` and `packages/common/native` is MIT; the backend is under the "AFFiNE Enterprise Edition (EE) license" (production use requires a subscription for the correct number of seats; parts distributed as AFFiNE CE or client-side are MPL-2.0) — [LICENSE](https://github.com/toeverything/AFFiNE/blob/HEAD/LICENSE), [EE LICENSE](https://github.com/toeverything/AFFiNE/blob/HEAD/packages/backend/server/LICENSE)
- "Free self-hosting covers up to 10 seats per workspace"; beyond that a Team subscription or installable license — [affine.pro/enterprise](https://affine.pro/enterprise)
- Self-hosted blob storage: docs say unlimited is planned but "a limitation" currently exists — [AFFiNE docs: basic user quota](https://docs.affine.pro/self-host-affine/features/basic-user-quota) (date unclear, may be stale)
- Cloud free: 10 GB, real-time collab up to 3 members/workspace, 7-day history — [AFFiNE pricing](https://affine.pro/pt-br/pricing)
- Stack: BlockSuite packages are Lit web components with Yjs and include `affine-block-database` and `data-view` blocks, edgeless (whiteboard) gfx packages, etc. — [BlockSuite repo](https://github.com/toeverything/blocksuite/tree/HEAD/packages/affine); AFFiNE uses y-octo 0.1.1 natively and a patched yjs 13.6.21 — [Cargo.toml](https://github.com/toeverything/AFFiNE/blob/HEAD/Cargo.toml), [package.json](https://github.com/toeverything/AFFiNE/blob/HEAD/package.json)
- OctoBase (the earlier Rust local-first DB) last pushed 2025-03-05 — [OctoBase](https://github.com/toeverything/OctoBase)
- Database gaps: open FRs "Implement Relation and Rollup column" (#15157), "Add Formula Support" (#15273), "Relation field" (#14977) — [AFFiNE #15157](https://github.com/toeverything/AFFiNE/issues/15157), [#15273](https://github.com/toeverything/AFFiNE/issues/15273)
- Most-reacted open issues: Notion-like columns layout (#7479, 56), local storage encryption (#5491), RTL (#8966), math/mermaid (#7450), public doc share with edit permissions (#13517), iPad pencil (#13782) — [AFFiNE issues](https://github.com/toeverything/AFFiNE/issues/7479)
- Funding: $8M seed (Feb 2023) reported by a single secondary source in search results — unverified.

**Anytype (any-sync, local-first, P2P)**
- License: Any Source Available License 1.0 — use/modify/redistribute only for non-commercial use or commercial use "in Allowed Networks" — [anytype-ts LICENSE](https://github.com/anyproto/anytype-ts/blob/HEAD/LICENSE); any-sync protocol is MIT — [any-sync](https://github.com/anyproto/any-sync)
- Shared spaces are E2EE; changes appear instantly when members are online or on the same LAN; can self-host — [Anytype docs: collaboration](https://doc.anytype.io/anytype/collaborate/collaboration)
- Third-party reviews disagree whether Anytype supports Notion-grade concurrent co-editing — [opensourcealternatives.to](https://www.opensourcealternatives.to/item/anytype); [toolguide.io](https://toolguide.io/en/tool/anytype/) (secondary, low reliability)
- Self-hosting = 15-container any-sync network (MongoDB, Redis Stack, Garage S3 + legacy MinIO, coordinator, filenode, 3 sync nodes, consensus node) — [any-sync-dockercompose](https://github.com/anyproto/any-sync-dockercompose/blob/HEAD/docker-compose.yml)
- Top issues: Vim mode (#247, 117), Notion-like formulas (#642, 22), Arabic/RTL (#1373, #757), multi-tab/window (#1490) — [anytype-ts #642](https://github.com/anyproto/anytype-ts/issues/642)
- Official MCP server (MIT, 538 stars) — [anyproto/anytype-mcp](https://github.com/anyproto/anytype-mcp); MCP servers talk to the desktop app's local API at `localhost:31009` with an app/API key, so the desktop app must be running — [mcpservers.org listing](https://mcpservers.org/servers/Qwinty/anytype-mcp), [glama listing](https://glama.ai/mcp/servers/ohneben/Anytype-MCP)

**Docmost (AGPL wiki, Confluence/Notion-like)**
- Stack: Tiptap + Hocuspocus (server/provider/transformer) + Yjs + y-prosemirror; React + Mantine client; Postgres 18 + Redis 8; 3-service compose — [package.json](https://github.com/docmost/docmost/blob/HEAD/package.json), [docker-compose.yml](https://github.com/docmost/docmost/blob/HEAD/docker-compose.yml)
- EE (paid) features by release: API keys management (v0.24.0, Dec 2025); MCP and page-level permissions (v0.70.0, Mar 2026); templates (v0.90.0, May 2026); **Bases (Table + Kanban)**, personal spaces, DOCX export (v0.95.0, Jul 2026 — "New in commercial editions"); MCP OAuth (v0.96.0, Sep 2026) — [Docmost releases](https://github.com/docmost/docmost/releases/tag/v0.95.0), [v0.70.0](https://github.com/docmost/docmost/releases/tag/v0.70.0)
- MCP "requires a valid Business or Enterprise license"; tools cover pages (search/read/create/update/list/duplicate/copy/move), spaces, comments, attachments search, members; OAuth or API key; respects app permissions — [Docmost MCP docs](https://docmost.com/docs/user-guide/mcp)
- Top issues: custom icons (#240, 32), cover photo (#853), publicly share entire space (#1090), custom uid/gid (#1264), OAuth for MCP (#2134), password-protected sharing (#1098) — [Docmost issues](https://github.com/docmost/docmost/issues/1090)
- Vendor (AppFlowy) comparison says Docmost lists no native desktop/mobile apps — [appflowy.com/compare](https://appflowy.com/compare/appflowy-vs-docmost) (competitor source)

**Outline (team wiki)**
- License BSL 1.1 by General Outline, Inc.; Additional Use Grant forbids offering a commercial "Document Service" to third parties — [LICENSE](https://github.com/outline/outline/blob/HEAD/LICENSE). A listicle claims conversion to Apache in 2030 — [toolindex.net](https://toolindex.net/blog/open-source-notion-alternatives-2026) (change date not verified).
- Stack: Koa + Sequelize, ProseMirror, Yjs + y-prosemirror, Hocuspocus server with Redis extension — [package.json](https://github.com/outline/outline/blob/HEAD/package.json)
- Built-in MCP server added in v1.6.0 (2026-03-15), disabled by default for existing workspaces; OAuth provider with dynamic client registration — [v1.6.0 release](https://github.com/outline/outline/releases/tag/v1.6.0); "Skills are now exposed over MCP" (v1.11.0); MCP SDK v2 / 2026-07-28 protocol, OAuth Client ID Metadata Documents (v1.12.0) — [releases](https://github.com/outline/outline/releases/tag/v1.12.0)
- MCP: search/read/create/edit documents; OAuth (default) or API key Bearer; Streamable HTTP at `https://your-domain/mcp`; admin toggle in Settings → Workspace → AI — [Outline MCP docs](https://docs.getoutline.com/s/guide/doc/mcp-6j9jtENNKL)
- Most-reacted open issues: **Local Authentication (#1881, 341 reactions)**, document tags (#765, 133), permission improvements (#11760), approvals (#1084), collection owners (#835), LDAP group mapping (#8168) — [Outline #1881](https://github.com/outline/outline/issues/1881)

**SiYuan (local-first PKM, Go kernel)**
- AGPL-3.0; "Some features are only available to paid members"; AI writing and Q/A chat via OpenAI API; Docker image `b3log/siyuan` — [README](https://github.com/siyuan-note/siyuan)
- Go kernel with Lute markdown engine and SQLite — [kernel/go.mod](https://github.com/siyuan-note/siyuan/blob/HEAD/kernel/go.mod)
- Database template field can use relation/rollup fields (v3.0.10) and rollup supports template fields (v3.3.0) — [newreleases v3.0.10-dev4](https://newreleases.io/project/github/siyuan-note/siyuan/release/v3.0.10-dev4), [v3.3.0-dev6](https://newreleases.io/project/github/siyuan-note/siyuan/release/v3.3.0-dev6)
- v3.8.6 (2026-09-29): expands mind maps and database calendars; built-in MCP server supports OAuth 2.1; workspace AGENTS.md for the AI agent — [v3.8.6 release](https://github.com/siyuan-note/siyuan/releases/tag/v3.8.6)
- Top issue: "Card Whiteboard" (#2024, 200 reactions) — [SiYuan #2024](https://github.com/siyuan-note/siyuan/issues/2024)

**Logseq (DB version)**
- Stack: ClojureScript, Datascript, `@sqlite.org/sqlite-wasm`, Electron — [deps.edn](https://github.com/logseq/logseq/blob/HEAD/deps.edn), [package.json](https://github.com/logseq/logseq/blob/HEAD/package.json)
- Releases: 2.0.1 "Beta Testing" (2026-07-13), 2.0.2 "Beta Testing" (2026-10-07) — [Logseq releases](https://github.com/logseq/logseq/releases)
- Split in April 2026 into DB app (Logseq) and Markdown app (Logseq OG); RTC sync opened to monthly Open Collective supporters from 2026-07-13 — [issoh.co.jp guide](https://www.issoh.co.jp/md/tech/4954) (secondary); DB changelogs focus on sync reliability (reconnects, encrypted graphs, large payloads) — [Logseq DB changelog](https://discuss.logseq.com/t/logseq-db-changelog/30013/37)

**Huly (all-in-one: Linear+Notion+Slack)**
- README: "This repository is frozen and is no longer actively maintained. Development continues in Platform-Collective/platform"; "Hosted Huly has shut down ... because its hosting is no longer funded" — [hcengineering/platform README](https://github.com/hcengineering/platform). Contradiction: the repo still received fix commits through 2026-11-06 — [commits](https://github.com/hcengineering/platform/commits)
- Fork Platform-Collective/platform: EPL-2.0, created 2026-06-26, 151 stars, "developed independently" — [Platform-Collective](https://github.com/Platform-Collective/platform)
- Self-host: 14 services incl. CockroachDB, Redpanda, Elasticsearch 7.14, MinIO — [compose.yml](https://github.com/hcengineering/huly-selfhost/blob/HEAD/compose.yml); README calls it resource-heavy, min 2 vCPU / 8 GB RAM — [huly-selfhost README mirror](https://cdn.jsdelivr.net/gh/hcengineering/huly-selfhost@main/README.md)
- Top issues are integrations: GitLab (#6653, 98), Gitea (#6950), generic Git (#9743) — [Huly #6653](https://github.com/hcengineering/platform/issues/6653)

**Plane (PM tool with Pages)**
- Pages run on a dedicated "live" Node service using Hocuspocus (database, redis extensions) + Tiptap + Yjs — [apps/live/package.json](https://github.com/makeplane/plane/blob/HEAD/apps/live/package.json)
- Community Edition: AGPL, free, no seat limits, parity with cloud Free tier (projects, work items, cycles, modules, views, pages, intake) — [Plane editions docs](https://developers.plane.so/self-hosting/editions-and-versions); Commercial Edition is closed-source; 10-seat minimum on new self-hosted Commercial purchases — [Plane forum](https://forum.plane.so/t/new-seat-minimums-on-self-hosted-commercial-and-air-gapped-starting-april-24/96)
- OIDC was in free self-host in v1 but moved behind paywall in v2 ($8/seat) — [Plane #8047](https://github.com/makeplane/plane/issues/8047)
- No API to create/update Pages (#7319) and no Page MCP tools (#8598) — [Plane #8598](https://github.com/makeplane/plane/issues/8598); official MCP server exists (MIT, 357 stars) — [plane-mcp-server](https://github.com/makeplane/plane-mcp-server)
- Funding: $4M seed led by OSS Capital — [Plane blog](https://plane.so/blog/plane-raises-4m-seed)

**La Suite Docs (new entrant, French state "suitenumerique")**
- MIT; features: real-time collaboration, offline editing, optional AI helpers (rewrite/summarize/translate/fix typos), subpages, PDF export, docx/odt/pdf export — [README](https://github.com/suitenumerique/docs)
- v6.0.0 (2026-10-09): collaboration server moved to **yhub** by Kevin Jahns (Yjs creator); soft-migrates legacy S3 docs and replays version history into yhub — [v6.0.0 release](https://github.com/suitenumerique/docs/releases/tag/v6.0.0)
- yhub: y-websocket-compatible backend; only needs Redis/Valkey + S3 or Postgres; doesn't keep Y.Doc in memory; horizontally scalable; "beta"; dual-licensed AGPL/proprietary — [yjs/yhub](https://github.com/yjs/yhub)
- v6.0.2 replaced "outdated AGPL mentions of BlockNote XL packages" — [releases](https://github.com/suitenumerique/docs/releases/tag/v6.0.2); BlockNote is MPL-2.0 except XL packages under GPL-3.0 or commercial — [BlockNote LICENSE](https://github.com/TypeCellOS/BlockNote/blob/HEAD/LICENSE)
- Top issues: dark mode (#1247), full-text search across sub-docs (#1493), WOPI for Nextcloud (#815), rich diff with attribution (#119), team access (#139, #138), ARM image (#1227), change-request/review workflow (#2156) — [La Suite Docs issues](https://github.com/suitenumerique/docs/issues/139)

**Colanode (2025 entrant: local-first Slack+Notion)**
- Show HN launch as "open-source and local-first Slack and Notion alternative" — [HN item 43780176](https://news.ycombinator.com/item?id=43780176)
- Last commit April 2026; issue #381 (2026-09-17) asks if still developed; commenter notes 300+ forks (e.g. "worknest", "WorkspaceOS") — [Colanode #381](https://github.com/colanode/colanode/issues/381)

**Trilium (TriliumNext)**
- v0.107.0 (2026-11-08): collections (table, board, calendar, geo map) reworked; LLM can change note type — [v0.107.0](https://github.com/TriliumNext/Trilium/releases/tag/v0.107.0); MCP now requires authentication and can be network-accessible (v0.105.0) — [v0.105.0](https://github.com/TriliumNext/Trilium/releases/tag/v0.105.0)
- Top issue "Milestone: Multi-user support" (#4956, 40), "Note editors other than CKEditor" (#4969) — [Trilium #4956](https://github.com/TriliumNext/Trilium/issues/4956)

**Joplin, Focalboard, BookStack, Wiki.js (brief)**
- Joplin: AGPL-3.0-or-later, `packages/server` has its own license; "Joplin" trademark of JOPLIN SAS — [LICENSE](https://github.com/laurent22/joplin/blob/HEAD/LICENSE)
- Focalboard: "This repository is currently not maintained"; Mattermost plugin lives in mattermost/mattermost-plugin-boards — [README](https://github.com/mattermost-community/focalboard)
- BookStack MIT PHP, Wiki.js AGPL; both actively pushed Nov 2026 — [BookStack](https://github.com/BookStackApp/BookStack), [Wiki.js](https://github.com/requarks/wiki)

**Twenty (data-model approach only)**
- Stack: TypeScript, Nx, NestJS + BullMQ, PostgreSQL, Redis, React (Jotai, Linaria, Lingui) — [README](https://github.com/twentyhq/twenty)
- Custom objects are defined by metadata tables; the `/metadata` API updates metadata and computes a GraphQL schema cached for later use — [Twenty custom objects docs](https://docs.twenty.com/developers/backend-development/custom-objects); custom objects get the same API endpoints, views, permissions and workflow triggers as built-ins — [Twenty data model](https://docs.twenty.com/user-guide/data-model/overview); each workspace gets an isolated Postgres schema with migrations applied by a metadata engine (third-party code tour) — [pyshine](https://pyshine.com/Twenty-Metadata-Engine-Source-Tour/)
- MCP tool listing in v2.56 — [Twenty releases](https://github.com/twentyhq/twenty/releases); $5M seed led by Runa Capital (Nov 2024) — [tech.eu](https://tech.eu/2024/11/19/twenty-secures-5m-to-challenge-crm-giants-with-open-source/)

**NocoDB / Baserow / Teable / Grist (database layer)**
- NocoDB: master and develop branches under "Sustainable Use License" (updated 2026-01-29) — [LICENSE](https://github.com/nocodb/nocodb/blob/HEAD/LICENSE)
- Baserow: MIT core, `premium/` and `enterprise/` separately licensed — [LICENSE](https://github.com/baserow/baserow/blob/HEAD/LICENSE); official MCP config + agent skill repo — [baserow-mcp](https://github.com/baserow/baserow-mcp)
- Teable: AGPL core apps + MIT packages — [LICENSE](https://github.com/teableio/teable/blob/HEAD/LICENSE); NestJS + knex + ShareDB, Postgres — [package.json](https://github.com/teableio/teable/blob/HEAD/apps/nestjs-backend/package.json); MCP: read-only query-data tool (filter/aggregate/group without SQL), confirmation before modifying data, Claude connector fixes (Oct-Nov 2026) — [Teable releases](https://github.com/teableio/teable/releases); top FR "Mount any existing database" (#556) — [Teable #556](https://github.com/teableio/teable/issues/556)
- undb: archived, "moved to Teable" — [undb README](https://github.com/undb-io/undb)

### Inferences
- The "fully open, team-ready, self-hostable Notion" slot is effectively **vacant**: the projects with the strongest Notion parity (AppFlowy, AFFiNE) monetize precisely the multi-user server, and the fully-open ones (La Suite Docs MIT, Outline-ish, Docmost CE) have no databases.
- Project-health risk is a real differentiator: 4 notable projects stalled/frozen/archived in 2025-2026 (Huly, Colanode, Focalboard, OctoBase/undb).
- Plane and Docmost show a recurring pattern: features first shipped open are later moved/added only to paid tiers (OIDC, page permissions, DB views, MCP), which drives community distrust.

### Gaps
- Exact Docmost "Bases" capabilities (relations? formulas?) not documented in release notes; no public docs found this session.
- AppFlowy views (grid/board/calendar/gallery?) and formula roadmap not verified this session; AppFlowy's current pricing for desktop/mobile sync with self-host not verified on official page.
- Funding for Docmost, Outline, Huly (Hardcore Engineering), Anytype (Any Association), Teable, Colanode not found; AFFiNE $8M seed and Twenty's 2025 Series A only in unreliable sources.
- SiYuan's multi-user/real-time status and paid sync details (S3/WebDAV) not re-verified; Trilium storage (SQLite) and Outline auth providers are from background knowledge.

## Q2. Which projects truly offer Notion-style databases (relations, rollups, formulas, multiple views) AND real-time multiplayer AND orgs/teams/permissions AND easy self-hosting?

### Takeaway
**None does all four in a genuinely open-source, free-for-teams form.** AppFlowy is closest on databases (relation + rollup, no formula) but its multi-user server is now closed-source with a 1-seat free tier; AFFiNE has easy self-host (4 containers) and real-time but no relations/rollups/formulas and a 10-seat cap; Docmost has easy self-host, real-time and permissions but its basic Bases (table/kanban) and page-level permissions are paid; SiYuan has the richest open database (relation/rollup/template) but is single-user.

### Cited Findings
- AppFlowy DB has Relation and Rollup but no Formula field — [entity.rs](https://github.com/AppFlowy-IO/AppFlowy-Collab/blob/HEAD/collab/src/database/entity.rs); free self-host = 1 seat + 3 guest editors and 12 containers — [SelfHost-Commercial](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial)
- AFFiNE lacks relation/rollup/formula (open FRs) — [#15157](https://github.com/toeverything/AFFiNE/issues/15157), [#15273](https://github.com/toeverything/AFFiNE/issues/15273); free self-host up to 10 seats/workspace — [affine.pro/enterprise](https://affine.pro/enterprise); 4-container compose — [compose](https://github.com/toeverything/AFFiNE/blob/HEAD/.docker/selfhost/compose.yml)
- Docmost: Bases (Table + Kanban), page-level permissions, templates, API keys and MCP are commercial-edition features — [v0.95.0](https://github.com/docmost/docmost/releases/tag/v0.95.0), [v0.70.0](https://github.com/docmost/docmost/releases/tag/v0.70.0); 3-container compose — [compose](https://github.com/docmost/docmost/blob/HEAD/docker-compose.yml)
- Anytype: relations exist but formulas are an open FR (#642); collaboration is E2EE sync; self-host 15 containers and non-commercial license — [#642](https://github.com/anyproto/anytype-ts/issues/642), [compose](https://github.com/anyproto/any-sync-dockercompose/blob/HEAD/docker-compose.yml), [LICENSE](https://github.com/anyproto/anytype-ts/blob/HEAD/LICENSE)
- SiYuan: relation/rollup/template DB fields — [v3.0.10-dev4](https://newreleases.io/project/github/siyuan-note/siyuan/release/v3.0.10-dev4); database calendars (v3.8.6) — [release](https://github.com/siyuan-note/siyuan/releases/tag/v3.8.6)
- Trilium: table/board/calendar/geo-map collections but multi-user is still a milestone issue — [v0.107.0](https://github.com/TriliumNext/Trilium/releases/tag/v0.107.0), [#4956](https://github.com/TriliumNext/Trilium/issues/4956)
- Outline and La Suite Docs: real-time + permissions + easy-ish self-host, but no databases (README feature lists contain no database/table views) — [Outline](https://github.com/outline/outline), [La Suite Docs README](https://github.com/suitenumerique/docs)
- Teable/Baserow/NocoDB/Grist deliver relational DB features + collaboration but are spreadsheet-first, not page/doc-first — [Teable](https://github.com/teableio/teable), [Baserow](https://github.com/baserow/baserow); a comparison article notes none of the doc-first tools fully match Notion's relational databases — [openalternative.co](https://openalternative.co/alternatives/notion) (secondary)

| Criterion | AppFlowy | AFFiNE | Docmost | Outline | Anytype | SiYuan | La Suite Docs | Plane | Huly |
|---|---|---|---|---|---|---|---|---|---|
| Relations | ✔ | ✘ | ? (EE Bases) | ✘ | ✔ | ✔ | ✘ | n/a | partial (tracker) |
| Rollups | ✔ | ✘ | ? | ✘ | ✘ | ✔ | ✘ | ✘ | ✘ |
| Formulas | ✘ | ✘ | ? | ✘ | ✘ | template field | ✘ | ✘ | ✘ |
| Multiple views | ✔ | ✔ (data-view) | Table+Kanban (EE) | ✘ | ✔ | ✔ | ✘ | issues only | issues only |
| Real-time multiplayer | ✔ (paid beyond 1 seat) | ✔ | ✔ | ✔ | sync (disputed) | ✘ | ✔ | ✔ (Pages) | ✔ |
| Orgs/permissions | ✔ (paid) | ✔ (≤10 free) | spaces/groups; page-level EE | collections/groups | spaces/roles | ✘ | per-doc | ✔ (OIDC paid) | ✔ |
| Easy self-host | ✘ (12 svc) | ✔ (4) | ✔ (3) | ~ (needs SSO) | ✘ (15) | ✔ (1) | ~ (7 + Keycloak + S3) | ✘ (13) | ✘ (14, 8 GB) |

### Inferences
- The combination "Notion-grade relational DB inside a real-time doc editor, permissioned per page, in ≤3 containers, under an OSI license" is the clearest unclaimed position.
- AFFiNE is the most likely incumbent to close the DB gap (BlockSuite already has database/data-view blocks), but its 10-seat EE gate remains.

### Gaps
- No hands-on verification of Docmost Bases field types or AppFlowy view list; "?" cells reflect missing docs, not confirmed absence.

## Q3. What do self-hosters complain about?

### Takeaway
Five recurring complaint families: (1) **open-core paywalls / license shifts** on exactly the team features (seats, SSO, page permissions, MCP); (2) **heavy multi-service stacks** (12-15 containers, CockroachDB/Elastic/Kafka/MongoDB); (3) **auth friction** (Outline's #1 issue is local auth); (4) **project abandonment** risk; (5) gaps in mobile/iPad, RTL, export, and granular sharing.

### Cited Findings
- Paywalls: AppFlowy free self-host = 1 seat — [SelfHost-Commercial](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial); AFFiNE 10 seats/workspace — [affine.pro/enterprise](https://affine.pro/enterprise); Plane OIDC moved behind paywall ("steep $8.00 per seat subscription just to implement a core authentication feature") — [Plane #8047](https://github.com/makeplane/plane/issues/8047); Docmost page permissions/Bases/MCP in EE — [releases](https://github.com/docmost/docmost/releases/tag/v0.95.0); NocoDB moved to Sustainable Use License — [LICENSE](https://github.com/nocodb/nocodb/blob/HEAD/LICENSE)
- Community workaround for AppFlowy seat limit (unofficial AGPL build) — [Docker Hub](https://hub.docker.com/r/mreza0100/appflowy-cloud-agpl)
- Heavy stacks (service counts from official compose): Anytype 15, Huly 14, Plane 13, AppFlowy 12, La Suite Docs 7 (+Keycloak, S3) vs Docmost 3, Teable 3, AFFiNE 4 — see compose links in Q1 table B; Huly README: resource-heavy, min 8 GB RAM — [huly-selfhost README](https://cdn.jsdelivr.net/gh/hcengineering/huly-selfhost@main/README.md); a hosting blog says the most common Huly complaint is undersized boxes breaking search — [Elestio blog](https://blog.elest.io/self-host-huly-one-open-source-app-to-replace-jira-linear-and-notion/) (vendor blog)
- Guides say AppFlowy self-host needs ~5+ Docker services incl. S3 and 4-8 GB RAM, Docmost runs on a 2 GB VPS — [Cloudzy comparison](https://cloudzy.com/fr/blog/affine-vs-appflowy-vs-docmost/) (secondary)
- Auth: Outline "Local Authentication" is the #1 issue with 341 reactions — [Outline #1881](https://github.com/outline/outline/issues/1881)
- Abandonment: Hosted Huly shut down "because its hosting is no longer funded" — [Huly README](https://github.com/hcengineering/platform); Colanode "Is the project still in active development?" — [#381](https://github.com/colanode/colanode/issues/381); Focalboard "currently not maintained" — [README](https://github.com/mattermost-community/focalboard)
- Stability/sync: Logseq DB still beta and warns of possible data loss — [search summary of Logseq forum changelogs](https://discuss.logseq.com/t/logseq-db-changelog/30013/37)
- Export/lock-in: AppFlowy's top issue is bulk Markdown export (#3334) — [AppFlowy #3334](https://github.com/AppFlowy-IO/AppFlowy/issues/3334)
- Mobile/tablet: AppFlowy iPad version (#4250), AFFiNE iPad pencil (#13782) — [AppFlowy #4250](https://github.com/AppFlowy-IO/AppFlowy/issues/4250), [AFFiNE #13782](https://github.com/toeverything/AFFiNE/issues/13782)
- RTL: AFFiNE #8966, Anytype #1373/#757 — [AFFiNE #8966](https://github.com/toeverything/AFFiNE/issues/8966), [Anytype #1373](https://github.com/anyproto/anytype-ts/issues/1373)
- Granular sharing/permissions: Outline permission improvements (#11760), La Suite Docs team access (#139/#138), AFFiNE edit-share (#13517), Docmost whole-space public share (#1090) and password share (#1098) — [Outline #11760](https://github.com/outline/outline/issues/11760), [Docs #139](https://github.com/suitenumerique/docs/issues/139), [AFFiNE #13517](https://github.com/toeverything/AFFiNE/issues/13517), [Docmost #1090](https://github.com/docmost/docmost/issues/1090)
- Integrations: Git forge integrations dominate Huly and Plane FRs (Gitea/GitLab) — [Huly #6653](https://github.com/hcengineering/platform/issues/6653), [Plane #1495](https://github.com/makeplane/plane/issues/1495); local-AI support (Plane #5941), custom OpenAI URL (AppFlowy #5379) — [Plane #5941](https://github.com/makeplane/plane/issues/5941)

### Inferences
- Self-hosters weight **trust in licensing trajectory** as heavily as features; a credible, irrevocable license commitment (e.g., OSI license + no seat caps on core collaboration) is itself a differentiator.
- "Single container + SQLite/Postgres" deployments win attention (Docmost, SiYuan, Teable) despite fewer features.

### Gaps
- Direct r/selfhosted and HN threads from 2025-2026 were not retrievable via search this session (results were dominated by SEO listicles); complaint evidence relies mostly on GitHub issue reactions.

## Q4. Which ones expose a good API or MCP server for AI agents, and how do agents read/write pages?

### Takeaway
By late 2026, built-in MCP became table stakes: **Outline** (free in all editions, OAuth + API key, Streamable HTTP, "skills"), **SiYuan** (OAuth 2.1 + agent with AGENTS.md), **AFFiNE** (built-in, but writes only on canary), **Trilium**, **Teable**, **Twenty** ship it in-product; **Docmost** ships it only in paid editions; **Anytype** and **Plane** have official external MCP servers (Plane can't write Pages); **AppFlowy has none**. Markdown is the dominant agent interchange format for page bodies.

### Cited Findings
- Outline: built-in MCP since v1.6.0, search/read/create/edit docs; OAuth or API key; `/mcp` Streamable HTTP; skills over MCP — [v1.6.0](https://github.com/outline/outline/releases/tag/v1.6.0), [v1.11.0](https://github.com/outline/outline/releases/tag/v1.11.0), [Outline MCP docs](https://docs.getoutline.com/s/guide/doc/mcp-6j9jtENNKL); community servers wrap Outline's API with "full Markdown formatting" — [HelicopterHelicopter/outline-mcp-server](https://mcpservers.org/servers/HelicopterHelicopter/outline-mcp-server)
- Docmost: MCP requires Business/Enterprise license; page/space/comment/attachment tools; OAuth enforced optionally — [Docmost MCP docs](https://docmost.com/docs/user-guide/mcp); MCP migrated to `@modelcontextprotocol/server` v2 in v0.97.0 — [v0.97.0](https://github.com/docmost/docmost/releases/tag/v0.97.0)
- AFFiNE: built-in MCP with `doc_search`, `read_document`; `create_document`/`update_document` (Markdown body) only in dev/canary; DB-block and image insertion unsupported; per-workspace Bearer token; self-host needs AI enabled — [affine.pro/mcp](https://affine.pro/mcp); community server supports only server-backed workspaces — [DAWNCR0W/affine-mcp-server](https://github.com/DAWNCR0W/affine-mcp-server)
- SiYuan: built-in MCP server with OAuth 2.1; agent/MCP can manage Bazaar packages; AGENTS.md per workspace — [v3.8.6](https://github.com/siyuan-note/siyuan/releases/tag/v3.8.6); fixed "MCP 2026-07-28 servers without tool list notifications fail to connect" — [v3.8.7](https://github.com/siyuan-note/siyuan/releases/tag/v3.8.7)
- Trilium: optional MCP server, auth required since v0.105.0, hardened against DNS rebinding (v0.104.0) — [v0.105.0](https://github.com/TriliumNext/Trilium/releases/tag/v0.105.0)
- Teable: MCP with read-only `query-data` tool (filter/aggregate/group without SQL), confirmation before data modification, pagination of bases, Claude connector compatibility — [Teable releases](https://github.com/teableio/teable/releases)
- Twenty: MCP tool lists synced via MCP SDK (v2.56) — [Twenty releases](https://github.com/twentyhq/twenty/releases)
- Anytype: official anytype-mcp (MIT) against desktop local API `localhost:31009` — [anytype-mcp](https://github.com/anyproto/anytype-mcp), [listing](https://mcpservers.org/servers/Qwinty/anytype-mcp)
- Plane: official plane-mcp-server — [repo](https://github.com/makeplane/plane-mcp-server); but no Pages create/update endpoints or MCP tools — [#7319](https://github.com/makeplane/plane/issues/7319), [#8598](https://github.com/makeplane/plane/issues/8598)
- AppFlowy: MCP server is an open feature request (#8043) — [AppFlowy #8043](https://github.com/AppFlowy-IO/AppFlowy/issues/8043); commercial self-host exposes OpenAPI/Swagger business APIs — [SelfHost-Commercial README](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial)
- Baserow: MCP config + skill for Claude/ChatGPT/Codex/Cursor etc. — [baserow-mcp](https://github.com/baserow/baserow-mcp)
- Huly: typed API client for programmatic access — [Huly README](https://github.com/hcengineering/platform)
- Notion (reference): official remote MCP with OAuth and Markdown editing; third-party servers warn some only support a few block types and drop callouts/toggles/tables — [Algolia DocSearch listing of makenotion/notion-mcp-server](https://docsearch.algolia.com/mcp/docs/repo/makenotion/notion-mcp-server), [Better Notion MCP](https://mcpservers.org/ru/servers/n24q02m/better-notion-mcp) (secondary)

### Inferences
- The common agent pattern is **Markdown in/out over a block/CRDT store**, which is lossy for databases, embeds and comments; no project exposes a typed block-JSON + database-schema API to agents as first-class MCP tools with round-trip fidelity.
- Writing through MCP into a live Yjs/CRDT doc while users edit (with attribution and undo) is not documented anywhere; AFFiNE gating writes to canary suggests it is hard.
- Database-aware MCP (query/aggregate rows, respect permissions, confirm destructive writes) currently exists in table tools (Teable) rather than doc tools.

### Gaps
- Whether Outline's `documents.info` and Docmost MCP return Markdown vs ProseMirror JSON was not confirmed from official docs this session.
- La Suite Docs external API surface (resource-server endpoints) and any MCP plans not documented in what was retrieved.

## Q5. Which reusable open-source building blocks do they rely on?

### Takeaway
**Yjs is the de-facto standard**: Outline, Docmost, Plane, Huly, La Suite Docs, Colanode and AFFiNE all use Yjs (or Yjs-compatible yrs/y-octo); **Hocuspocus** is the dominant collab server (Outline, Docmost, Plane, Huly), with **yhub** emerging (La Suite Docs v6). Editors split between **Tiptap/ProseMirror** (Docmost, Plane, Colanode, Outline), **BlockNote** (La Suite Docs), **BlockSuite** (AFFiNE), **CKEditor 5** (Trilium). No major Notion alternative found using **Loro** or **Automerge**.

### Cited Findings
- Outline: ProseMirror + y-prosemirror + Yjs + Hocuspocus server/provider/redis — [package.json](https://github.com/outline/outline/blob/HEAD/package.json)
- Docmost: Tiptap core/react + Hocuspocus server/provider/transformer + Yjs + y-prosemirror — [package.json](https://github.com/docmost/docmost/blob/HEAD/package.json)
- Plane Pages: Tiptap + Hocuspocus (database/redis/logger extensions) + Yjs — [apps/live/package.json](https://github.com/makeplane/plane/blob/HEAD/apps/live/package.json)
- Huly: Yjs + Hocuspocus server/transformer — [server/collaborator/package.json](https://github.com/hcengineering/platform/blob/HEAD/server/collaborator/package.json)
- La Suite Docs: BlockNote (core, react, mantine, code/diagram/math blocks, xl-ai, xl-docx/odt/pdf exporters, xl-multi-column) + Yjs; yhub collab server — [frontend package.json](https://github.com/suitenumerique/docs/blob/HEAD/src/frontend/apps/impress/package.json), [v6.0.0](https://github.com/suitenumerique/docs/releases/tag/v6.0.0)
- Colanode: Tiptap (client/ui) + Yjs (`packages/crdt`) + Kysely (SQLite client / Postgres server) — [Colanode repo](https://github.com/colanode/colanode/tree/HEAD/packages)
- AFFiNE: BlockSuite (Lit + Yjs) + y-octo — [BlockSuite](https://github.com/toeverything/blocksuite), [AFFiNE Cargo.toml](https://github.com/toeverything/AFFiNE/blob/HEAD/Cargo.toml)
- AppFlowy: yrs 0.25 (Rust port of Yjs) — [AppFlowy-Collab Cargo.toml](https://github.com/AppFlowy-IO/AppFlowy-Collab/blob/HEAD/Cargo.toml)
- Non-Yjs: Anytype any-sync (own protocol) — [go.mod](https://github.com/anyproto/anytype-heart/blob/HEAD/go.mod); Teable ShareDB (OT) — [package.json](https://github.com/teableio/teable/blob/HEAD/apps/nestjs-backend/package.json); Logseq Datascript + SQLite-wasm — [deps.edn](https://github.com/logseq/logseq/blob/HEAD/deps.edn); SiYuan Lute + SQLite — [go.mod](https://github.com/siyuan-note/siyuan/blob/HEAD/kernel/go.mod); Trilium CKEditor 5 — [package.json](https://github.com/TriliumNext/Trilium/blob/HEAD/apps/client/package.json)
- Library snapshot (GitHub API 2026-11-09): Tiptap 38,991★ MIT; Hocuspocus 2,647★ MIT; BlockNote 10,367★ MPL-2.0 (XL: GPL-3.0/commercial); Yjs 23,036★ MIT; y-crdt/yrs 2,198★ MIT; yhub 309★ AGPL/proprietary (beta); Loro 6,281★ MIT; Automerge 6,724★ MIT; Lexical 24,003★ MIT (repo now under `meta/lexical`); Plate 16,695★ MIT; Novel 16,463★ Apache-2.0 (last push 2025-01-18); BlockSuite 6,087★ MPL-2.0; y-octo 351★ MIT; ProseMirror umbrella repo 8,693★ MIT shows `archived: true` (last push 2026-04-01) — [Tiptap](https://github.com/ueberdosis/tiptap), [Hocuspocus](https://github.com/ueberdosis/hocuspocus), [BlockNote](https://github.com/TypeCellOS/BlockNote), [Yjs](https://github.com/yjs/yjs), [y-crdt](https://github.com/y-crdt/y-crdt), [yhub](https://github.com/yjs/yhub), [Loro](https://github.com/loro-dev/loro), [Automerge](https://github.com/automerge/automerge), [Lexical](https://github.com/facebook/lexical), [Plate](https://github.com/udecode/plate), [Novel](https://github.com/steven-tey/novel), [ProseMirror](https://github.com/ProseMirror/prosemirror)

### Inferences
- Safest reusable stack for a new project (license-wise, permissive): **Yjs (MIT) + Tiptap/ProseMirror (MIT) + Hocuspocus (MIT)**, or BlockNote core (MPL-2.0) if avoiding its GPL/commercial XL packages (AI, exporters, multi-column). yhub is technically attractive (stateless, Redis-streamed) but AGPL/proprietary and beta.
- BlockSuite (MPL-2.0) is the only open library offering doc + whiteboard + database blocks together, but it is tightly coupled to AFFiNE.
- Loro/Automerge adoption remains a differentiator opportunity (e.g., native rich-text + movable tree + version DAG) but carries ecosystem risk (fewer editor bindings than Yjs).

### Gaps
- Did not verify whether the ProseMirror umbrella repo archive reflects a move of development elsewhere (per-package repos or a self-hosted forge).
- Did not inspect AppFlowy Web's editor library or Docmost's server framework directly.

## Q6. What is the "gap in the market" — what nobody does well yet?

### Takeaway
Nobody offers an **OSI-licensed, uncapped, lightweight (≤3 containers) team workspace that combines a Notion-grade relational database (relations + rollups + formulas + views) inside real-time collaborative pages, with page-level permissions and a first-class, database-aware MCP/API for agents**. Each incumbent misses at least two of these.

### Cited Findings
- DB-in-docs is weak everywhere doc-first: AFFiNE lacks relations/rollups/formulas — [#15157](https://github.com/toeverything/AFFiNE/issues/15157); AppFlowy lacks formulas — [entity.rs](https://github.com/AppFlowy-IO/AppFlowy-Collab/blob/HEAD/collab/src/database/entity.rs); Anytype lacks formulas — [#642](https://github.com/anyproto/anytype-ts/issues/642); Docmost Bases are paid table/kanban — [v0.95.0](https://github.com/docmost/docmost/releases/tag/v0.95.0); Outline/La Suite Docs have none — [Outline](https://github.com/outline/outline), [La Suite Docs](https://github.com/suitenumerique/docs)
- Team features are where open-core gates sit: seats (AppFlowy 1, AFFiNE 10), SSO (Plane OIDC), page permissions (Docmost) — [AppFlowy](https://github.com/AppFlowy-IO/AppFlowy-SelfHost-Commercial), [AFFiNE](https://affine.pro/enterprise), [Plane #8047](https://github.com/makeplane/plane/issues/8047), [Docmost v0.70.0](https://github.com/docmost/docmost/releases/tag/v0.70.0)
- Lightweight deploys exist only for less complete products (Docmost 3, Teable 3, AFFiNE 4) — compose links in Q1
- Agent access is mostly Markdown-only and doc-centric; AFFiNE writes are canary-only and DB blocks unsupported — [affine.pro/mcp](https://affine.pro/mcp); Plane Pages not writable via API — [#8598](https://github.com/makeplane/plane/issues/8598); AppFlowy no MCP — [#8043](https://github.com/AppFlowy-IO/AppFlowy/issues/8043)
- Granular sharing (public space, password links, edit-share, team access) is a top request across Docmost, AFFiNE, La Suite Docs, Outline — [Docmost #1090](https://github.com/docmost/docmost/issues/1090), [AFFiNE #13517](https://github.com/toeverything/AFFiNE/issues/13517), [Docs #139](https://github.com/suitenumerique/docs/issues/139), [Outline #11760](https://github.com/outline/outline/issues/11760)
- Simple auth for self-host (local accounts without external IdP) is the most-demanded Outline feature — [#1881](https://github.com/outline/outline/issues/1881); La Suite Docs requires OIDC/Keycloak in its compose examples — [compose examples](https://github.com/suitenumerique/docs/tree/HEAD/documentation/examples/compose)
- Twenty demonstrates a reusable pattern for user-defined schemas: metadata tables → generated GraphQL/REST, per-workspace Postgres schemas, permissions and workflows on custom objects — [Twenty docs](https://docs.twenty.com/developers/backend-development/custom-objects), [pyshine](https://pyshine.com/Twenty-Metadata-Engine-Source-Tour/)
- Teable shows Postgres-native tables with MCP query/aggregate tooling and is asked to "mount any existing database" — [Teable releases](https://github.com/teableio/teable/releases), [#556](https://github.com/teableio/teable/issues/556)

### Inferences
Concrete differentiation opportunities for a new project:
1. **Real relational DB inside pages**: database blocks backed by real Postgres tables (Teable/Twenty-style metadata engine) rather than CRDT-blob rows, giving relations/rollups/formulas, SQL-grade queries and scalable views — while page bodies stay in Yjs.
2. **No seat caps on core collaboration**, with an OSI license (MIT/Apache/AGPL) and monetization on hosting/support/enterprise compliance only — directly countering the AppFlowy/AFFiNE/Plane/Docmost backlash.
3. **One-container default** (app + Postgres; Redis/S3 optional), local accounts out of the box, OIDC free.
4. **Agent-native API**: MCP tools that are database-aware (schema introspection, typed row CRUD, aggregate queries, permission-respecting), block-level JSON *and* Markdown views of pages, safe concurrent agent edits into live CRDT docs with attribution/undo, and confirmation for destructive writes (Teable's pattern).
5. **Granular sharing as a core feature** (page-level perms, public spaces, password/expiring links, guest editors) — currently paywalled or missing in most.
6. **Data portability guarantees** (lossless bulk Markdown/JSON export incl. databases) — AppFlowy's top issue shows demand.
7. Optional local-first/offline (Yjs + SQLite clients, as Colanode/La Suite attempted) without Anytype-style 15-node infrastructure.

### Gaps
- No quantitative market data (user counts, revenue) for these projects was found; the "gap" is inferred from feature/license/issue evidence, not from surveys.
- Did not find 2026 HN/Reddit threads summarizing user sentiment directly; a targeted manual read of r/selfhosted would strengthen the complaint analysis.
