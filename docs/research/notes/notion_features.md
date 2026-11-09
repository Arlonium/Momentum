# Notion feature inventory, as of November 2026, for building an open-source self-hosted alternative with feature parity

> Research date: 2026-11-09. Primary sources: notion.com/releases (changelog pages 1-8, Nov 2023 to Oct 19 2026), notion.com/help, notion.com/pricing, developers.notion.com, and Notion engineering blog posts. Third-party sources are flagged when used.
> Changelog page URLs used throughout:
> R1 = https://www.notion.com/releases (Jul 30 to Oct 19 2026) · R2 = https://www.notion.com/releases/page/2 (May 5 to Jul 24 2026) · R3 = https://www.notion.com/releases/page/3 (Apr 9 to May 4 2026) · R4 = https://www.notion.com/releases/page/4 (Mar 12 to Apr 8 2026) · R5 = https://www.notion.com/releases/page/5 (Feb 9 to Mar 11 2026) · R6 = https://www.notion.com/releases/page/6 (Jul 22 2025 to Feb 4 2026) · R7 = https://www.notion.com/releases/page/7 (Jul 2024 to Jul 2025, v2.43 to v2.52) · R8 = https://www.notion.com/releases/page/8 (Nov 2023 to Jul 2024, v2.35 to v2.42)
> Labels: **[NEW 2024]**, **[NEW 2025]**, **[NEW 2026]** mark the launch year. **[PAID: Plus+ / Business+ / Enterprise]** marks the minimum plan. **[UNVERIFIED]** means I could not confirm it from a source in this session.

## Editor & blocks: full block inventory, inline elements, editing interactions, page-level features

### Takeaway
Notion's editor is a tree of typed blocks. The public API exposes about 38 block types. Recent additions are the Tabs block (Mar 2026), Heading 4 (Mar 2026), merged cells in simple tables (May 2026), the AI Meeting Notes block (May 2025, renamed `meeting_notes` in API 2026-03-11), interactive HTML blocks built by agents (Jul 2026), and Presentation mode (Mar 2026). Parity therefore needs a block-tree CRDT/OT model, slash and markdown shortcuts, synced (transcluded) blocks, rich-text mentions, and version history with attribution.

### Cited Findings

**Block types exposed by the API (canonical machine list)**
- API `type` enum: `bookmark`, `breadcrumb`, `bulleted_list_item`, `callout`, `child_database`, `child_page`, `column`, `column_list`, `divider`, `embed`, `equation`, `file`, `heading_1`, `heading_2`, `heading_3`, `heading_4`, `image`, `link_preview`, `numbered_list_item`, `paragraph`, `pdf`, `quote`, `synced_block`, `table`, `table_of_contents`, `table_row`, `template`, `to_do`, `toggle`, `transcription`, `unsupported`, `video`. The doc also documents `audio`, `code`, `meeting_notes` and `tab` block sections. `mention` is a rich-text element, not a block — [Notion API: Block object](https://developers.notion.com/reference/block)
- `link_preview` is read-only in the API. `meeting_notes` can't be created through generic block endpoints and needs a dedicated "Create a meeting note" endpoint. `transcription` was deprecated in API version `2026-03-11` and renamed `meeting_notes`. Creating `template` blocks was deprecated on March 27, 2023. Synced-block content can't be updated through the API. `column_list` needs ≥2 columns. `table_width` is set only at creation. Uploading an `.html` file as an embed creates an HTML block — [Notion API: Block object](https://developers.notion.com/reference/block)
- Unsupported blocks come back as `type: "unsupported"` with a free-string `block_type`, so Notion has more internal block types than the API exposes — [Notion API: Block object](https://developers.notion.com/reference/block)
- Code-block languages, about 75: abap, arduino, bash, basic, c, clojure, coffeescript, c++, c#, css, dart, diff, docker, elixir, elm, erlang, flow, fortran, f#, gherkin, glsl, go, graphql, groovy, haskell, html, java, javascript, json, julia, kotlin, latex, less, lisp, livescript, lua, makefile, markdown, markup, matlab, **mermaid** (rendered diagrams), nix, objective-c, ocaml, pascal, perl, php, plain text, powershell, prolog, protobuf, python, r, reason, ruby, rust, sass, scala, scheme, scss, shell, sql, swift, typescript, vb.net, verilog, vhdl, visual basic, webassembly, xml, yaml, java/c/c++/c# — [Notion API: Block object](https://developers.notion.com/reference/block)
- Colors. Text: default, gray, brown, orange, yellow, green, blue, purple, pink, red. Backgrounds: the same nine colors with a `_background` suffix. Rich-text annotations: bold, italic, strikethrough, underline, code, color — [Notion API: Block object](https://developers.notion.com/reference/block)

**Block categories in the editor (Help Center)**
- Basic: Text, Page, To-do, Headings, Bulleted list, Numbered list, Toggle list, Quote, Divider, Callout. Database blocks: Table, Board, Calendar, Gallery, List. Media: Image, Web bookmark, Video, Audio, Code, File. Embeds: "more than 500 apps". Advanced: Math equation, Template button, Breadcrumbs, Table of contents — [Notion Help: Types of content blocks](https://www.notion.com/help/guides/types-of-content-blocks)

**Recent block and editor additions (dated)**
- **[NEW 2026]** **Tabs block** (`/tabs`) splits a page into clickable sections without subpages. Shipped with Notion 3.4 part 1 (Mar 26 2026) and announced separately on Mar 30 2026 — [R4](https://www.notion.com/releases/page/4)
- **[NEW 2026]** **H4 headings**, smaller than H3, shipped Mar 26 2026 in 3.4 — [R4](https://www.notion.com/releases/page/4)
- **[NEW 2026]** **Merge cells in simple tables** (headers spanning columns, grouped rows), May 26 2026 — [R2](https://www.notion.com/releases/page/2)
- **[NEW 2026]** **Interactive HTML blocks**: agents build ROI calculators, quizzes and org charts inside docs (Notion 3.6, Jul 1 2026) — [R2](https://www.notion.com/releases/page/2)
- **[NEW 2026] [PAID: Plus+]** **Presentation mode**: any page becomes fullscreen slides, with dividers as slide breaks (⌘⌥P / Ctrl+Alt+P). Launched Mar 2 2026, beta in 3.4 — [R5](https://www.notion.com/releases/page/5); [R4](https://www.notion.com/releases/page/4)
- **[NEW 2026]** AI generation of images, charts, slides and diagrams inside a page (`/ai image`, Mar 9 2026) — [R5](https://www.notion.com/releases/page/5); [R4](https://www.notion.com/releases/page/4)
- **[NEW 2025]** **AI Meeting Notes block** (`/meet`) captures and summarizes meetings (2.51, May 13 2025). It gained linked summaries tied to transcript timestamps in Nov 2025, speaker labels in Jul 2026, audio-file upload in Jul 2026, and custom summary instructions in Mar 2026 — [R7](https://www.notion.com/releases/page/7); [R6](https://www.notion.com/releases/page/6); [R2](https://www.notion.com/releases/page/2); [R4](https://www.notion.com/releases/page/4)
- **[NEW 2024]** **Chart block** (`/chart`) on any page, updating live from its database (2.44, Aug 13 2024) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2024]** Custom emojis (upload your own; 2.46, Oct 2024) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2024]** Simple image cropping in Notion (2.39, Apr 2024). A floating table of contents appears on hover at the right of pages with headings (2.40, Jun 2024). "Paste as mention" link previews with favicon, author and title (2.40) — [R8](https://www.notion.com/releases/page/8)
- **[NEW 2026]** New page-cover art collections (Met Museum, USDA watercolors, etc.), Apr 9 2026, so page covers and icons remain a core feature — [R3](https://www.notion.com/releases/page/3)
- **[NEW 2026]** Hovering a breadcrumb shows sibling pages, Apr 24 2026 — [R3](https://www.notion.com/releases/page/3)
- **[NEW 2026]** High-contrast display mode, Jul 30 2026 — [R1](https://www.notion.com/releases)
- Mail blocks embedded in pages keep working after the Notion Mail shutdown, so a "mail" block type exists — [Notion Help: Notion Mail inbox is going away](https://www.notion.com/fr/help/notion-mail-inbox-is-going-away-what-to-do-next)

**Synced blocks**
- The same content can appear in several places across pages and workspaces. The original is marked "ORIGINAL", and the header lists every location. Editing copies needs edit access to the original. With more than 10 copies, deleting the original or using "Unsync all" removes every copy without undo. Copies can be unsynced individually — [Notion Help: Synced blocks](https://www.notion.com/help/synced-blocks)

**Markdown shortcuts, slash commands, @-mentions, keyboard editing**
- Inline markdown: `**bold**`, `*italic*`, `` `code` ``, `~strike~`. At line start: `*`/`-`/`+` (bullet), `[]` (to-do), `1.`/`a.`/`i.` (numbered), `#`/`##`/`###` (H1 to H3), `>` (toggle), `"` (quote), `---` (divider) — [Notion Help: Keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts)
- Slash commands. Basic: `/text` `/page` `/bullet` `/num` `/todo` `/toggle` `/div` `/quote` `/h1` to `/h3` `/link`. Inline: `/mention` `/date` `/reminder` `/equation` `/emoji`. Media: `/image` `/pdf` `/book` `/video` `/audio` `/code` `/file` `/embed`. Advanced: `/duplicate` `/moveto` `/delete` `/toc` `/button` `/template` `/bread` `/math` `/latex`. Colors: `/blue`, `/blue background`, `/default`. `/turn` turns a block into another type — [Notion Help: Keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts)
- @-commands: `@person` (sends a notification), `@page` (inline link), `@date` (natural language: "today", "next Wednesday"), `@remind` (reminder). `[[` and `+` link to or create pages and subpages inline — [Notion Help: Keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts)
- Block shortcuts: Cmd/Ctrl+D duplicate, Cmd/Ctrl+Shift+arrows move, Tab/Shift+Tab nest, Cmd/Ctrl+/ opens the block menu (turn into, color, duplicate, move; works on multi-selected DB rows), Cmd/Ctrl+Opt/Alt+T expands or collapses all toggles, Cmd/Ctrl+Shift+H reapplies the last color, Cmd/Ctrl+Opt+0-9 turns a block into a type, Opt/Alt+drag duplicates, Cmd/Ctrl+R/D fills table cells right or down, Cmd/Ctrl+Shift+M comments, Cmd/Ctrl+Shift+U goes up one hierarchy level — [Notion Help: Keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts)
- Enter in a code block keeps indentation (2.39, Apr 2024) — [R8](https://www.notion.com/releases/page/8)

**Page-level features**
- **Backlinks** are created automatically whenever a page is @-mentioned and shown under the page title. You only see backlinks you have access to — [Notion Help: Links & backlinks](https://www.notion.com/help/create-links-and-backlinks?slug=create-links-and-backlinks)
- **Page lock** (••• menu) blocks all edits, including by the owner, until it is unlocked. Databases can also be locked — [Notion Help guide: keep pages up to date](https://www.notion.com/help/guides/tips-to-keep-your-teams-notion-pages-up-to-date)
- **Page analytics**: owners and editors see total and unique views over time under ••• → Updates & analytics. Users can opt out of recording their views — [Notion Help: Page analytics](https://www.notion.com/help/page-analytics). Pricing lists Free/Plus as "basic page analytics", Business as "advanced page analytics", Enterprise as "workspace analytics" — [Notion Pricing](https://www.notion.com/pricing)
- **Version history**: a snapshot every 10 minutes during active editing, plus one 2 minutes after the last edit. Viewing needs Can edit. Retention is 7 days (Free), 30 (Plus), 90 (Business), custom (Enterprise). You can restore a whole version or copy blocks. A database restore brings back pages and properties but not page bodies — [Notion Help: Duplicate, delete & restore](https://www.notion.com/help/duplicate-delete-and-restore-content)
- **[NEW 2026]** **Version history redesign** (Notion 3.8, Oct 13 2026): highlighted edits, detailed database change records, attribution, one-click restore, covering pages and databases — [R1](https://www.notion.com/releases)
- **[NEW 2026]** **Archive pages** (Mar 27 2026): marks a page outdated with a banner showing who archived it and when. It is hidden from search, sidebar and DB views by default, cascades to subpages, and links keep working — [R4](https://www.notion.com/releases/page/4). The Help Center calls Archive **Beta, Business and Enterprise only**, while the release note gives no plan restriction — [Notion Help: Duplicate, delete & restore](https://www.notion.com/help/duplicate-delete-and-restore-content)
- **[NEW 2025] [PAID: Business+]** **Verified pages** (2.49, Mar 2025) get priority in search, citations and mentions, with re-verification reminders — [R7](https://www.notion.com/releases/page/7); "Verify any page" is listed for Business and Enterprise — [Notion Pricing](https://www.notion.com/pricing)
- **[NEW 2024]** Database-page "Customize layout" (2.46, Oct 2024) and **tabbed page layouts** (`Customize layout` → `Tabbed`, 2.49, Mar 2025) — [R7](https://www.notion.com/releases/page/7)
- Page and view descriptions on properties and views (2.36, Dec 2023) — [R8](https://www.notion.com/releases/page/8)

### Inferences
- The internal set of block types is larger than the API enum (mail block, chart block, form, dashboard, tabs, HTML block, AI blocks). A parity clone should design an extensible block registry with an "unsupported or unknown" fallback, as Notion's own API does.
- The release cadence shows Notion is still adding basic editor primitives in 2026 (H4, tabs, merged cells). A clone can reach v1 parity without them but should plan for them.
- Mermaid as a code language means diagrams are rendered from code blocks, not a separate diagram block.

### Gaps
- **Toggle headings** (toggleable H1 to H3), **columns UI details**, **inline equations** (the `$$` markdown trigger), and the ```` ``` ```` code trigger were not confirmed. The shortcuts page explicitly doesn't document ``` or `$$`. Toggle headings exist in the API as `is_toggleable` on headings according to general knowledge **[UNVERIFIED]**.
- No full list of the 500+ embed providers was found.
- Real-time collaborative editing internals (OT vs CRDT, presence cursors) are not documented in sources found. Real-time presence or avatars are assumed but **[UNVERIFIED]** this session.
- The help page on templates (template button vs database templates) was not fetched. `/template` is still listed as a slash command, and creating `template` blocks through the API was deprecated in 2023.
- Nesting depth limits for blocks were not documented.

## Databases: property types, view types, filters/sorts/groups, data sources, templates, automations, permissions

### Takeaway
A Notion database is now a container of one or more **data sources** (since Sept 2025 / API 2025-09-03). Each data source has a schema of 24 property types and pages as rows. Databases render through 11 view types: table, board, timeline, calendar, list, gallery, chart, feed, map, form and dashboard. Since 2025-2026 they also support row-level permissions, a "Can create pages" level, column (property-level) permissions (Oct 2026), and AI Autofill properties driven by Custom Agents.

### Cited Findings

**Property types (24, from API data-source schema)**
- `title` (exactly one), `rich_text`, `number`, `select`, `multi_select`, `status`, `date`, `people`, `files`, `checkbox`, `url`, `email`, `phone_number`, `formula`, `relation`, `rollup`, `created_time`, `created_by`, `last_edited_time`, `last_edited_by`, `button`, `unique_id`, `place`, `verification` — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Number formats** (45): number, number_with_commas, percent, dollar, australian_dollar, canadian_dollar, singapore_dollar, euro, pound, yen, ruble, rupee, won, yuan, real, lira, rupiah, franc, hong_kong_dollar, new_zealand_dollar, krona, norwegian_krone, mexican_peso, rand, new_taiwan_dollar, danish_krone, zloty, baht, forint, koruna, shekel, chilean_peso, philippine_peso, dirham, colombian_peso, riyal, ringgit, leu, argentine_peso, uruguayan_peso, peruvian_sol, vietnamese_dong, pakistani_rupee, nigerian_naira, bitcoin — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Select/status options** have 10 colors, a stable `id`, a unique case-insensitive `name` with no commas, and an optional `description` — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Status**: three fixed groups, To-do, In progress and Complete. The defaults are Not started, In progress and Done, and a configurable default option is supported. Group definitions are managed only in the UI — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Relation**: one-way (`single_property`) or two-way (`dual_property`, which creates a synced reverse property). It targets a `data_source_id`, with a read-only `limit` field — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Rollup functions** (24): count, count_values, empty, not_empty, unique, show_unique, percent_empty, percent_not_empty, sum, average, median, min, max, range, earliest_date, latest_date, date_range, checked, unchecked, percent_checked, percent_unchecked, count_per_group, percent_per_group, show_original — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **[NEW 2026]** Rollups support number formats (currency, percent) and decimal places, Apr 27 2026 — [R3](https://www.notion.com/releases/page/3)
- **Unique ID**: optional prefix (e.g. `TASK-`), auto-numbered and not writable — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **[NEW 2025]** **Place** property (lat, lon, name, address, AWS/Google place IDs) alongside the **Map view** (Notion 3.1, Nov 17 2025) — [R6](https://www.notion.com/releases/page/6); [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Verification** property: created automatically for wiki databases, storing state, date and verified_by — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Button** property: configured only in the UI. The API can't press buttons — [Notion API: Property object](https://developers.notion.com/reference/property-object)
- **Formula** property: an `expression` string referencing `prop("Name")`, with read-only results — [Notion API: Property object](https://developers.notion.com/reference/property-object)

**Formula language (Formulas 2.0 function inventory)**
- Operators: `+ - * / % ^`, `== != > >= < <=`, `and/&&`, `or/||`, `not/!`, ternary `? :` — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- Logic: if, ifs, and, or, not, empty, equal, unequal, **let, lets** (variables), true, false — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- Text: length, substring, contains, test, match, replace, replaceAll (regex), lower, upper, repeat, trim, split, join, format, toNumber — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- Number/Math: add, subtract, multiply, divide, mod, pow, min, max, sum, median, mean, abs, round, ceil, floor, sqrt, cbrt, exp, ln, log10, log2, sign, pi, e, formatNumber, toNumber — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- Date: now, today, timestamp, fromTimestamp, parseDate, formatDate, minute, hour, day, date, week, month, year, dateAdd, dateSubtract, dateBetween, dateRange, dateStart, dateEnd — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- List: at, first, last, slice, concat, sort, reverse, join, split, unique, includes, flat, length. Higher-order: find, findIndex, filter, some, every, map (with `current` and `index`) — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- People: name, email. Other: id, prop, style, unstyle (rich colored output), link (hyperlinks) — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- Data types: text, number, boolean, date (and ranges), list, person, page (relation and rollup lists). Formulas can traverse relations, e.g. lists of pages — [Notion Help: Formula syntax](https://www.notion.com/help/formula-syntax)
- New functions Mean, Median and Today arrived in 2.39 (Apr 2024) — [R8](https://www.notion.com/releases/page/8)
- **[NEW 2025]** **AI formulas**: describe a formula in plain language and Notion AI writes it (3.0, Sep 2025) — [R6](https://www.notion.com/releases/page/6)

**View types (11)**
- Table, Board, Timeline, Calendar, List, Gallery, Chart (bar, line, donut) — [Notion Help: Views, filters & sorts](https://www.notion.com/help/views-filters-and-sorts)
- **[NEW 2024]** **Chart view** (2.44, Aug 13 2024): bar, line and pie/donut, with colors, axis names and labels. Click-through drilldown to underlying data came in 2.49 (Mar 2025) — [R7](https://www.notion.com/releases/page/7). **[PAID]** Free = 1 chart, paid = unlimited — [Notion Pricing](https://www.notion.com/pricing)
- **[NEW 2024]** **Form view** (2.46, Oct 24 2024). Unlimited forms are free — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2025]** **Feed view** (2.52, Jul 10 2025): scroll entries like a blog feed with comments and reactions — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2025]** **Map view** (3.1, Nov 17 2025) — [R6](https://www.notion.com/releases/page/6)
- **[NEW 2026] [PAID: Business+]** **Dashboard view** (Mar 10 2026, featured in 3.4): combines multiple views, KPIs and properties. Agents can build and update dashboards — [R5](https://www.notion.com/releases/page/5); [R4](https://www.notion.com/releases/page/4)

**View settings, filters, sorts, groups**
- Pages open in Side peek (default for table, board, list, timeline), Center peek (gallery, calendar), or Full page — [Notion Help: Views, filters & sorts](https://www.notion.com/help/views-filters-and-sorts)
- Simple filters can be personal or "Save for everyone". **Advanced filters** use AND/OR **filter groups nested up to 3 levels** — [Notion Help: Views, filters & sorts](https://www.notion.com/help/views-filters-and-sorts)
- Multiple sorts apply in a draggable order. Select and multi-select sort by a custom option order — [Notion Help: Views, filters & sorts](https://www.notion.com/help/views-filters-and-sorts)
- Group plus **sub-group** (two levels). Groups can be hidden, manually ordered, and empty groups hidden — [Notion Help: Views, filters & sorts](https://www.notion.com/help/views-filters-and-sorts)
- Per-view property visibility, frozen columns, in-database search (on databases with ≥3 pages), "Copy link to view", views nested under full-page databases in the sidebar, and personal tab display (icon/text/both, Apr 10 2026) — [Notion Help: Views, filters & sorts](https://www.notion.com/help/views-filters-and-sorts); [R3](https://www.notion.com/releases/page/3)
- **[NEW 2025]** Conditional coloring works with formulas, relations and rollups. Filters can include sub-items (3.1, Nov 2025) — [R6](https://www.notion.com/releases/page/6)
- Viewers of locked databases can use advanced filters (2.40, Jun 2024). Calendar views can hide weekends (2.38) — [R8](https://www.notion.com/releases/page/8)
- **[NEW 2025]** Database redesign (2.52, Jul 2025): new `+ property` flow, **property groups**, insert or rename from the property menu, plus better mobile database editing and filtering — [R7](https://www.notion.com/releases/page/7)

**Data sources (2025 multi-source databases)**
- A database is a container with one or more data sources. A data source is a set of pages sharing a schema. Examples: a CRM combining contacts, companies, deals and activities in one database. Managed through "Manage data sources", which offers add new, link existing, move to another database or page, and remove link. Views left behind become linked views — [Notion Help: Data sources & linked databases](https://www.notion.com/help/data-sources-and-linked-databases)
- Access can't differ per source inside one database. A linked source inherits its original's permissions. View changes on a linked source don't affect the original, while edits to schema and pages do. Forms can be added only on the original database — [Notion Help: Data sources & linked databases](https://www.notion.com/help/data-sources-and-linked-databases)
- **[NEW 2025]** API 2025-09-03 split `/v1/databases` (container) from `/v1/data_sources` (schema plus rows). This is a breaking change. Permissions are managed at database level. **Wikis were excluded from multi-source at launch** — [Notion API upgrade guide 2025-09-03](https://developers.notion.com/docs/upgrade-guide-2025-09-03); Whalesync dates the launch to Sept 3 2025 — [Whalesync docs](https://docs.whalesync.com/connectors/notion/multiple-data-sources)
- Duplicating a database keeps its structure but drops content (2.36, Dec 2023) — [R8](https://www.notion.com/releases/page/8)

**Sub-items, dependencies, templates, recurrence**
- "Subtasks & dependencies" and "custom properties & filtering" are on all plans — [Notion Pricing](https://www.notion.com/pricing)
- **[NEW 2025]** Recurring automations schedule database page updates or creation (2.48, Feb 2025) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2025]** The API can create pages from database templates (3.1, Nov 2025) — [R6](https://www.notion.com/releases/page/6)
- **[NEW 2025]** "Build with AI" database setup builder (`/database` → Build with AI) and prebuilt one-click workflows (2.48, Feb 2025) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2025]** CSV import picks a type per column and can create a new database or merge into an existing one (2.52) — [R7](https://www.notion.com/releases/page/7)

**Database permissions**
- Database-only levels are **Can edit content** (edit pages and values, but not structure, views or filters) and **Can create** (Business+), where users add pages without seeing others — [Notion Help: Sharing & permissions](https://www.notion.com/help/sharing-and-permissions)
- **[NEW 2026] [PAID: Business+]** "Can create pages" permission, Mar 5 2026 — [R5](https://www.notion.com/releases/page/5)
- **[NEW 2025] [PAID: Business+]** **Row-level (page-level) permissions** (3.0, Sep 18 2025). Rules are based on a Person or Created-by property in the source data source, apply across all views and linked views, and can differ per data source — [R6](https://www.notion.com/releases/page/6); [Notion Help: Sharing & permissions](https://www.notion.com/help/sharing-and-permissions)
- **[NEW 2026]** **Column permissions**: database owners set which properties each person or agent can view or edit (3.8, Oct 13 2026) — [R1](https://www.notion.com/releases)
- "Granular database permissions" is listed for Business and Enterprise — [Notion Pricing](https://www.notion.com/pricing)

**Synced databases and external data**
- **[NEW 2024]** Jira Sync upgrade (2.43, Jul 2024) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2026]** Jira integration syncs into Notion properties for dashboards. **Enterprise** can edit key Jira fields with write-back (3.2, Jan 20 2026) — [R6](https://www.notion.com/releases/page/6)
- **[NEW 2026]** **Synced databases rebuilt** (3.8, Oct 13 2026): GitHub, Jira and Salesforce first, with live sync health and support for custom properties, views and automations on synced data — [R1](https://www.notion.com/releases)
- **[NEW 2026]** "Database sync from any API source" (Beta) via Workers, e.g. Zendesk or Salesforce (3.5, May 13 2026) — [R2](https://www.notion.com/releases/page/2)
- "1-way database syncs" appears as a pricing row, and "Connected properties" appears under API and integrations — [Notion Pricing](https://www.notion.com/pricing)

**AI in databases**
- **[NEW 2026]** **AI Autofill**, powered by Custom Agents, continuously enriches, extracts and categorizes across rows (3.4 part 2, Apr 14 2026). Basic AI Autofill remains included in Business and Enterprise — [R3](https://www.notion.com/releases/page/3)
- **[NEW 2025]** Research-mode "database chat": @-mention a database to ask questions across properties and page bodies (2.52) — [R7](https://www.notion.com/releases/page/7)

### Inferences
- Parity requires a two-level model: database (container, with views and permissions) above data sources (schemas plus rows). Views can target one source each.
- Permissions now operate at four granularities: workspace/teamspace, page/database, row (person-property rule), and column (property). That is a significant engineering burden for a clone, since permission computation already needs "expensive tree traversal" at Notion scale (see Developer section).
- The status property's fixed three groups and 10-color palette are simple to replicate. The formula engine (typed, with list higher-order functions, `let`, regex and styled output) is the largest single database component.

### Gaps
- Per-property filter operators (e.g. "is relative to today", "contains", "is within") were not fetched.
- Calculation/aggregation footer options, load limits per view, wrap-cells and card-preview options were not documented on the fetched page.
- Timeline-view dependency arrows and the "Sub-items" toggle UI details were not fetched. Dependencies exist per pricing, but mechanics are **[UNVERIFIED]**.
- Recurring **templates** (a template set to repeat daily or weekly) are a well-known Notion feature, but I did not find the specific help page this session **[UNVERIFIED]**.
- The meaning of the pricing row "Connected properties" is unclear and wasn't verified.
- Max relations per property, max rows per database, and performance ceilings are not published.

## Workspace, collaboration, sharing, permissions, enterprise admin

### Takeaway
Notion has a hierarchy: organization → workspaces → teamspaces (open, closed, or private on Business+) → pages and databases. It has six page permission levels, and roles include owner, membership admin, member, restricted member, temporary member and guest. Collaboration covers comments/discussions, suggested edits (2024), inbox, mentions, publishing (Sites), and enterprise controls such as SAML SSO (Business+), SCIM, audit log, SIEM/DLP, legal hold, IP restrictions and organization-level admin (Enterprise).

### Cited Findings

**Permission levels**
- **Full access** (edit and share), **Can edit** (edit, no sharing), **Can edit content** (DB only), **Can create** (DB only, Business+), **Can comment**, **Can view** — [Notion Help: Sharing & permissions](https://www.notion.com/help/sharing-and-permissions)
- General access: Only invited, Everyone at workspace (with a "Hide in search" toggle), or Anyone on the web with link (login needed to comment or edit). **Link expiration** is available for public links. Users can request access, and view or comment users can request edit (introduced 2.39, Apr 2024) — [Notion Help: Sharing & permissions](https://www.notion.com/help/sharing-and-permissions); [R8](https://www.notion.com/releases/page/8)
- Inheritance: subpages inherit from the parent and can be overridden. The broadest grant wins. "Anyone with link" pages can be reached through mentions, two-way relations or nesting under broadly shared pages — [Notion Help: Sharing & permissions](https://www.notion.com/help/sharing-and-permissions)
- **[NEW 2025]** The Share menu shows who has access and at which level (2.48, Feb 2025) — [R7](https://www.notion.com/releases/page/7)

**Roles**
- Workspace owner, **membership admin** (Enterprise), member, **restricted member** (must be added to specific teamspaces and pages, can't create teamspaces, limited mentions and sharing, no agents), **guest** (page-by-page, needs a Notion account, no groups), **temporary member** (approved Marketplace consultants, up to 1 year, no paid seat), and **organization owner** (manages multiple workspaces, delegates via predefined or custom admin roles) — [Notion Help: Who's who in a workspace](https://www.notion.com/help/whos-who-in-a-workspace)
- **Groups** contain members and restricted members, not guests. Adding a group to a teamspace grants access to all its members. Group membership is restored if a user rejoins within 30 days — [Notion Help: Who's who in a workspace](https://www.notion.com/help/whos-who-in-a-workspace)

**Teamspaces**
- Types: **Open** (anyone joins), **Closed** (visible, invite-only), **Private** (hidden; Business and Enterprise). The "Default" toggle auto-adds all members, and Notion recommends at most three default teamspaces. Roles are teamspace owner and member. Settings control who invites, who edits the sidebar, and the invite link. Enterprise adds per-member or per-group custom permissions plus disabling public publishing, guests and export per teamspace — [Notion Help: Intro to teamspaces](https://www.notion.com/help/intro-to-teamspaces)

**Sidebar, navigation, home**
- **[NEW 2024]** Home (2.38, Feb 2024) shows recents, tasks and trending pages, with a "My tasks" view. Home gained a calendar and an upcoming-projects database (2.40, Jun 2024). Reorderable sidebar sections (Favorites, Private, Teamspaces) arrived in 2.39 — [R8](https://www.notion.com/releases/page/8)
- **[NEW 2025]** AI Home tab (2.51, May 2025) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2026]** **Library** (Feb 18 2026): a full-page view with tabs for Teamspaces, Recents, Favorites, Private and Shared — [R5](https://www.notion.com/releases/page/5)
- **[NEW 2026]** **New sidebar** (opt-in, 3.4, Mar 2026) with four tabs (pages, agent chats, meetings, notifications) — [R4](https://www.notion.com/releases/page/4)
- **[NEW 2025]** Meetings tab in the sidebar (3.1, Nov 2025) — [R6](https://www.notion.com/releases/page/6)
- **[NEW 2026]** **People Directory** of workspace members, importable from HR systems (3.2, Jan 2026) — [R6](https://www.notion.com/releases/page/6)

**Comments, discussions, suggested edits, inbox**
- **[NEW 2024]** **Suggested edits**, accepted or rejected by owners or teammates (2.43, Jul 29 2024). The inbox groups suggestions and comments into threads — [R7](https://www.notion.com/releases/page/7)
- Comment threads can be filtered by person or status (2.39). Users can comment on database rows via right-click without opening them (2.42). Exports can include comments (2.36) — [R8](https://www.notion.com/releases/page/8)
- **[NEW 2025]** **Page discussions view**, a simplified catch-up view of comments (Nov 14 2025). **Grouped inbox notifications** (Nov 14 2025). Notifications for emoji reactions on comments (3.1) — [R6](https://www.notion.com/releases/page/6)
- **[NEW 2026]** Mute discussion replies (Apr 8 2026) — [R4](https://www.notion.com/releases/page/4)
- **[NEW 2026]** Notion notifications in **Microsoft Teams**, with replies from Teams (3.8, Oct 2026) — [R1](https://www.notion.com/releases)
- Mobile notifications can be replied to, archived or marked read (2.38) — [R8](https://www.notion.com/releases/page/8)

**Publishing: Notion Sites**
- **[NEW 2024]** **Notion Sites** (2.42, Jun 25 2024) grew out of Publish. It offers custom domains or notion.site, a custom header, light/dark theme, favicon, custom SEO metadata and Google Analytics — [R8](https://www.notion.com/releases/page/8)
- SEO indexing on all plans and a Sites settings tab (2.36, Dec 2023). Custom site URLs (2.46, Oct 2024) — [R8](https://www.notion.com/releases/page/8); [R7](https://www.notion.com/releases/page/7)
- Plans: notion.site domains 1 (Free) or 5 (paid). SEO is basic on Free and advanced on paid. Site customizations on Plus+. **Custom domains are $8/month/domain annually or $10 monthly** — [Notion Pricing](https://www.notion.com/pricing)
- Enterprise owners can disable sites, forms and public links in Settings → Security — [Notion Help: Sharing & permissions](https://www.notion.com/help/sharing-and-permissions)

**Enterprise and admin controls**
- From the pricing table: SAML SSO (Business+), SCIM provisioning (Enterprise), advanced workspace and teamspace security controls, organization-level controls, granular admin roles, **audit log** (Enterprise), admin content search, domain management (domain verification on Business), workspace consolidation, managed-users dashboard, **SIEM and DLP connections** (Enterprise), guest invite requests, custom data retention, deletion of AI Meeting Notes transcripts, and 2-step verification — [Notion Pricing](https://www.notion.com/pricing)
- **[NEW 2025]** Centralized admin controls across multiple workspaces and the Enterprise organization setup (2.49, Mar 2025). **Passkeys**. Admins can restrict which workspaces employees use on the company network (2.49) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2025]** **Legal hold** for organization owners (2.52, Jul 2025) — [R7](https://www.notion.com/releases/page/7)
- **[NEW 2025]** **IP address login restrictions** (3.1, Nov 2025) — [R6](https://www.notion.com/releases/page/6)
- **[NEW 2026] [Enterprise]** AI usage analytics (3.2). MCP activity in audit logs (3.2). **Custom Agents audit log** recording runs, changes and triggering users (3.6, Jul 2026) — [R6](https://www.notion.com/releases/page/6); [R2](https://www.notion.com/releases/page/2)
- **[NEW 2026]** AI Meeting Notes consent enforcement: a workspace-wide consent message at transcription start (Mar 12 2026) — [R4](https://www.notion.com/releases/page/4)
- Admins can disable Export (Enterprise, also per teamspace). Enterprise owners can stop members inviting guests — [Notion Help: Export](https://www.notion.com/help/export-your-content); [Notion Help: Sharing & permissions](https://www.notion.com/help/sharing-and-permissions)

### Inferences
- A clone needs a full ACL model with principals (user, group, guest, restricted user, agent/bot, public link, workspace-wide), resources (workspace, teamspace, page, database, data source, row, property) and inheritance with override. "Agent" is now a first-class principal: column permissions apply to "each person or agent", and restricted members can't access agents.
- Enterprise parity features (SCIM, SAML, audit log, SIEM export, legal hold, retention policies) are table stakes for organizations choosing self-hosting.

### Gaps
- **Guest limits per paid plan**: the pricing capture showed only Free = 10. The 2024 Plus announcement said "up to 100 free guests" on Plus — [R8](https://www.notion.com/releases/page/8). Business and Enterprise guest limits were not captured **[UNVERIFIED]**.
- Real-time presence (avatars, live cursors) and the comment/reaction data model were not documented in sources fetched.
- Workspace-level analytics specifics (what metrics Enterprise gets) were not detailed.
- No primary source found for an "allow duplicate as template" toggle on public pages in 2026 **[UNVERIFIED]**, though it historically existed.

## Notion AI & agents: features, models, agents, connectors, pricing

### Takeaway
Notion AI was rebuilt as **Agents** in Notion 3.0 (Sep 18 2025). Since then it has added autonomous **Custom Agents** (3.3, Feb 24 2026, billed in Notion credits from May 4 2026), Skills (Mar 2026 / 3.7 Sep 2026), External Agents such as Claude and Cursor (3.6, Jul 2026), 150+ tool connections (3.8, Oct 2026), and a rebuilt agent harness with its own computer and browser plus an auto model router (Oct 19 2026). AI is **included only on Business and Enterprise**. Free and Plus get limited trials. Usage is metered through per-member Standard and Select credits plus purchasable workspace Notion credits at $10 per 1,000.

### Cited Findings

**Timeline of AI capabilities**
- **2023-11** Q&A (beta), plus a desktop shortcut Cmd/Ctrl+Shift+K (2.35) — [R8](https://www.notion.com/releases/page/8)
- **2024-06** AI search across Slack, Google Drive, Salesforce, Jira and others. @-mentions in searches. GPT-4 chat with citations (2.41) — [R8](https://www.notion.com/releases/page/8)
- **2024-07** One-click AI skills (brainstorm, diagram, summarize, translate). AI chat plus search over Notion and Slack. Global AI shortcut Cmd/Ctrl+Shift+J (2.43) — [R7](https://www.notion.com/releases/page/7)
- **2024-09** AI connectors beta for Google Docs, Sheets and Slides. Style-matched writing via @-mentioned page. PDF and image analysis. Model choice between GPT-4 and Claude. Suggested actions on selection (2.45) — [R7](https://www.notion.com/releases/page/7)
- **2024-12** GitHub answers (beta). HIPAA-compliant PDF analysis (2.47) — [R7](https://www.notion.com/releases/page/7)
- **2025-03** Jira connector beta (2.49) — [R7](https://www.notion.com/releases/page/7)
- **2025-05** **AI Meeting Notes**, **Enterprise Search**, **Research mode** and AI Home. Connectors for Teams, SharePoint, OneDrive, Gmail and Linear. Searchable PDFs. Business and Enterprise get "unlimited Notion AI" (2.51, May 13 2025) — [R7](https://www.notion.com/releases/page/7)
- **2025-07** Research-mode database chat. Gmail search and Linear search on Business+ (2.52) — [R7](https://www.notion.com/releases/page/7)
- **2025-09-18 Notion 3.0 Agents**: multi-step actions for over 20 minutes, memory built on pages and databases, edits across dozens of pages, create or update hundreds of DB rows, an instructions page and memory, AI formulas. Bundled models Claude Sonnet 4 and GPT-5 — [R6](https://www.notion.com/releases/page/6); [Notion 3.0 release](https://www.notion.com/releases/2025-09-18)
- **2025-11** CSV analysis (≤1,000 rows). 3.1 adds agent context over comments, calendars, private Slack and CSVs. Meeting summaries linked to transcript moments. Gemini 3 Pro — [R6](https://www.notion.com/releases/page/6)
- **2025-12** Agents build forms — [R6](https://www.notion.com/releases/page/6)
- **2026-01 Notion 3.2**: AI Notes on mobile with background transcription, Agent on mobile, GPT-5.2, Claude Opus 4.5, Gemini 3, an "Auto" model option, AI analytics (Enterprise) — [R6](https://www.notion.com/releases/page/6)
- **2026-02** Asana AI connector. Claude Opus 4.6 — [R6](https://www.notion.com/releases/page/6); [R5](https://www.notion.com/releases/page/5)
- **2026-02-24 Notion 3.3 Custom Agents**: autonomous agents that run 24/7 on schedule or trigger. Use cases include Q&A, triage and status reports. Built from natural language with editable triggers, instructions, sources and model. Templates. Integrations with Slack, Notion Mail, Calendar and MCP tools (Linear, Figma, HubSpot). Every run is logged, and admins control who creates agents. Free until May 3 2026, then credits from May 4 2026 — [R5](https://www.notion.com/releases/page/5); [Notion 3.3 release](https://www.notion.com/releases/2026-02-24)
- **2026-03**: MiniMax M2.5, GPT-5.4, image generation, Salesforce connector, meeting-notes consent and custom instructions. **Custom AI skills** (pages saved as reusable commands, Business+, Mar 20). 3.4 part 1 adds AI generation of images, charts, slides and diagrams — [R5](https://www.notion.com/releases/page/5); [R4](https://www.notion.com/releases/page/4)
- **2026-04**: desktop voice input. **Shared AI chats as read-only links**. 3.4 part 2: Custom Agents cost 35-50% less, cheaper models (GPT-5.4 Mini/Nano, Haiku 4.5, MiniMax M2.5) use up to 10× fewer credits, a credits dashboard, **AI Autofill via Custom Agents**, inline doc edits, voice dictation, Salesforce and Box connectors, n8n MCP. Agent scheduling of meetings (Apr 15). Opus 4.7. Mail & Calendar settings tab — [R4](https://www.notion.com/releases/page/4); [R3](https://www.notion.com/releases/page/3)
- **2026-05**: Custom Agents in private Slack channels. Admin controls covering creation access, per-agent and workspace credit limits, a credit dashboard, disabling, and auto-pause guardrails. **Custom Agent Directory** (Library → Agents). **Plan Mode** (clarifying questions plus a plan before large changes) — [R3](https://www.notion.com/releases/page/3); [R2](https://www.notion.com/releases/page/2)
- **2026-07-01 Notion 3.6**: **External Agents** (Claude and Cursor assignable on boards and @-mentionable). Speaker labels. Interactive HTML blocks. Agents read and write PPTX, XLSX, DOCX and PDF. Outlook Mail and Calendar connection. New MCP connections (Mercury, Mixpanel, Miro, Box, ClickHouse). Models Opus 4.8, Grok 4.3, GLM 5.2. Custom Agent audit log (Enterprise). Slack Enterprise Grid. One-click agent duplication — [R2](https://www.notion.com/releases/page/2)
- **2026-07/08**: Notion Agents iOS app (Jul 8). Agent calendar tools (Jul 16). Meeting notes trigger Custom Agents (Jul 31). Share context with agents from the Share menu (Aug 7). Simplified model picker with scorecards and an effort setting (Aug 14). **Agents "suggest edits"** instead of editing directly (Aug 28) — [R2](https://www.notion.com/releases/page/2); [R1](https://www.notion.com/releases)
- **2026-09-09** Model controls: owners choose allowed models for Notion Agent and Custom Agents separately and set a default (Business+) — [R1](https://www.notion.com/releases)
- **2026-09-15 Notion 3.7 Skills**: a skills library (skills stored in databases). `/` runs a skill in chat. Skills export as `SKILL.md` for Claude Code, Codex, Cursor, Gemini and Grok. Custom MCP connections (beta, Business+). **Sub-agents** (Custom Agents calling Custom Agents). Embed a Custom Agent in any doc. Custom Agent Insights API. Agent SDK in public beta. AI Search 50% faster. Inbox catch-up. Models Opus 5, GPT-5.6 Sol, Kimi K3 — [R1](https://www.notion.com/releases)
- **2026-10-13 Notion 3.8**: MCP connections for any MCP-compatible agent, with admin visibility and per-connection revocation. **150+ tool connections** (HubSpot, Stripe, Cloudflare…). **Skills API**. **Bring a ChatGPT Plus/Pro plan** to cover AI usage (Business+). Seven new models (GPT 6.1-Sol, GPT-6 Luna, Opus 5.5, Sonnet 5.5…). An admin allowlist of Notion-verified tools — [R1](https://www.notion.com/releases)
- **2026-10-19 "Agents rebuilt"**: agents get their **own computer and browser**, a "Review for me" safety mode, **tasks of up to 24 hours**, an **auto model router** (claimed ~90% lower cost, credits going ~10× further), BYO ChatGPT Plus/Pro/Go for personal agents, **BYOK in private alpha for Enterprise**, MCP memory sync, an AI Command Center "coming soon", and a new unified credit system — [R1](https://www.notion.com/releases)

**AI connectors (Enterprise Search sources, cumulative)**
- Slack (including private channels and DMs from Nov 2025), Google Drive (Docs, Sheets, Slides, plus .docx/.pptx/.xlsx/.csv/PDF), GitHub, Jira, Microsoft Teams, SharePoint, OneDrive, Gmail, Linear, Notion Mail, Notion and Google Calendar, Asana, Salesforce, Box, Outlook — [R7](https://www.notion.com/releases/page/7); [R6](https://www.notion.com/releases/page/6); [R5](https://www.notion.com/releases/page/5); [R3](https://www.notion.com/releases/page/3); [R2](https://www.notion.com/releases/page/2)

**Pricing and bundling**
- Pricing table: Notion Agent (chat, generate, autofill, translate), Meeting notes and Enterprise Search (beta) are **included on Business and Enterprise**, with "Limited trial" on lower plans. AI data retention is 30 days, while Enterprise has **zero data retention**. Custom Agents are "free to try, then $10 per 1,000 monthly Notion credits" — [Notion Pricing](https://www.notion.com/pricing)
- Credit model: Business and Enterprise members get monthly **Standard** credits (Notion picks the model) and **Select** credits (user-chosen model or a Custom Agent). These are non-pooled and reset monthly without rollover. Workspace **Notion credits** are off by default and enabled by owners. Rough cost is ~5/10/25 credits for simple, medium and complex tasks. A one-hour meeting note is ~10 Standard credits (AI Meeting Notes draws on Standard credits "starting in December"). The API and connected apps don't consume credits unless they run an agent — [Notion Help: Custom Agent pricing](https://www.notion.com/help/custom-agent-pricing)
- Third-party sources conflict on whether Plus can buy credits. Most say only Business and Enterprise can — [Coworker blog](https://coworker.ai/blog/notion-ai-pricing) vs [Matthias Frank, Oct 2026](https://matthiasfrank.de/en/notion-ai-pricing-2026/) (third-party, not verified)

### Inferences
- An open-source clone can't match the hosted frontier-model catalog. A realistic parity target is a pluggable LLM layer (BYOK, local models), an agent runtime with tools (read/write pages, DB queries, web, MCP clients), triggers (schedule, DB events, meeting-summarized, Slack message), skills stored as pages, and per-agent audit logs and credit limits. Notion's own move to BYOK and BYO ChatGPT plans shows the market wants this.
- "Agents as principals" with permissions, audit logs, credit budgets and suggest-edit mode is now part of the core collaboration model, not an add-on.

### Gaps
- Exact Standard and Select credit allowances per seat were not published on the fetched help page.
- Whether Plus retains any ongoing AI beyond a "limited trial" is not confirmed by Notion directly.
- The "AI Command Center" was announced as "coming soon" (Oct 19 2026). Its shipping status as of Nov 2026 is unverified.
- No release entries after Oct 19 2026 were found. Any late-Oct or early-Nov 2026 releases are unknown.

## Adjacent products: Calendar, Mail, Forms, Sites, Charts/Dashboards, offline, apps, web clipper, Marketplace

### Takeaway
Around the core workspace Notion ships Notion Calendar (Jan 2024), Forms (Oct 2024), Sites (Jun 2024), Charts (Aug 2024) and Dashboards (Mar 2026), offline mode (Aug 2025), a template Marketplace with payments (Oct 2024), a Notion Agents iOS app (Jul 2026), and Workers and a CLI (May 2026). **Notion Mail launched in Apr 2025 and shut down on Sep 22 2026**, replaced by agent-run email via Gmail and Outlook connectors.

### Cited Findings

**Notion Calendar**
- **[NEW 2024]** Launched Jan 17 2024 (2.37). It syncs Notion databases, links events to pages, supports multiple Google accounts and event blocking, has a scheduling tool ("S"), multi-timezone views, a command menu, Zoom/Meet integrations and a menu-bar app. Available on desktop, web and mobile — [R8](https://www.notion.com/releases/page/8)
- Android app (Jul 2024). RSVP notes (Jul 2024). Focus time and OOO events (Jun 2024). Scheduling with multiple bookings and links in email (Dec 2024). Birthday event type (Dec 2024). Link expiration and booking windows (Feb 2025). **iCloud Calendar** (Mar 2025). Shared login with Notion via passkeys (Mar 2025) — [R8](https://www.notion.com/releases/page/8); [R7](https://www.notion.com/releases/page/7)
- **[NEW 2025]** Tasks in Notion Calendar from any dated database (Aug 2025). **Two-way sync** including text properties (Nov 2025). "Open in Calendar" for date databases (Jul 2025). Option to hide the Calendar context panel (Jan 2026) — [R6](https://www.notion.com/releases/page/6); [R7](https://www.notion.com/releases/page/7)

**Notion Mail (discontinued)**
- **[NEW 2025]** Launched Apr 15 2025 (2.50) with AI auto-labeling, custom inbox views, snippets, `/schedule` availability, and `/` blocks with AI drafting. iOS in Aug 2025. Multi-account in Jan 2026. Save emails to Notion pages — [R7](https://www.notion.com/releases/page/7); [R6](https://www.notion.com/releases/page/6)
- **Shut down Sep 22 2026** for all plans; the last day to save data was Sep 21. Email remains in Gmail because sync was two-way. Mail blocks in pages and the Gmail AI connector continue. Notion's stated reason is that more than half of Mail users manage email through agents without opening the inbox — [Notion Help: Notion Mail inbox is going away](https://www.notion.com/fr/help/notion-mail-inbox-is-going-away-what-to-do-next); [Engadget](https://engadget.com/2201940/notion-mail-is-shutting-down)

**Forms**
- Questions map to properties, so question types follow property types. Options include required, description, list or dropdown, long text, and max selections. **Conditional logic is Business+**. Respondents can be anyone in the workspace or anyone on the web; web forms are always anonymous. A "Respondent" property records the submitter. Respondent access to their own submission can be set from No access to Full access. Customization covers title, description, icon, cover, submit-button color and text, confirmation screen and email copy. Notion branding can be removed on paid plans. Creation works on desktop and web only — [Notion Help: Forms](https://www.notion.com/help/forms)
- Respondents can see only their own submissions (Feb 2025). Option limits for multi-select questions (Feb 2025) — [R7](https://www.notion.com/releases/page/7). Plans: Free = basic, Plus = custom, Business+ = custom plus conditional logic — [Notion Pricing](https://www.notion.com/pricing)

**Charts and Dashboards**: see the Databases section. **Sites**: see the Collaboration section.

**Offline**
- **[NEW 2025]** Offline mode (2.53, Aug 19 2025) on desktop and mobile. Users download pages, create new pages offline, and manage downloads in Settings → Offline. Recents and Favorites **auto-download on Plus+**. "The most essential blocks" work offline — [R6](https://www.notion.com/releases/page/6); Free = "choose pages to download" — [Notion Pricing](https://www.notion.com/pricing)

**Apps and other surfaces**
- Desktop tabs: reorder, restore session (2.39), pin tabs (2.40). Feature search via Cmd+P (2.46). iOS Shortcuts for create, search, append and open (2.47) — [R8](https://www.notion.com/releases/page/8); [R7](https://www.notion.com/releases/page/7)
- **[NEW 2026]** Mobile Home tab with Home, Chats, Meetings and Inbox (May 4 2026). **Notion Agents iOS app** (Jul 8 2026) — [R3](https://www.notion.com/releases/page/3); [R2](https://www.notion.com/releases/page/2)
- Web Clipper saves web content to Notion in one click — [Notion: Features](https://www.notion.com/product/features)
- **[NEW 2024]** **Marketplace** with native payments, analytics, reviews and ratings for selling templates (2.46). Redesigned template gallery with 20k+ templates (2.43) — [R7](https://www.notion.com/releases/page/7)
- Localization: Indonesian, Thai and Vietnamese added Jul 2025 across Notion, Calendar, Notion Faces and Marketplace — [R6](https://www.notion.com/releases/page/6)

### Inferences
- The Mail shutdown shows Notion is cutting standalone surfaces and favoring agent-mediated workflows. A clone should prioritize calendar and DB date sync and an email *connector* over a full mail client.
- "Notion Projects" isn't a separate product in 2024-2026 releases. It is the bundle of databases, sub-items, dependencies, timeline, sprints and charts.

### Gaps
- No offline-mode help page was reachable (404), so limits such as downloadable pages, DB rows offline and conflict resolution are unverified. A third-party claim that databases can't be created offline is unverified.
- Notion Calendar's current feature list (post-Mail shutdown) and Notion Faces details were not fetched.
- Sprints (Notion Projects, 2023) were not verified in this session.

## Automations: buttons, database automations, webhooks, Slack/Gmail, integrations

### Takeaway
Notion automation has three layers. **Buttons** (10 action types, all plans, some actions paid), **database automations** (page added, property edited, or recurring triggers with 8 action types, paid plans) and **webhooks** (Mar 2025 API webhooks plus "Send webhook" actions). Since 2026 these are supplemented by Custom Agents with triggers and by **Workers** for code-based tools, syncs and webhook triggers.

### Cited Findings
- **Button actions (10)**: Insert blocks (above, below, top or bottom of page), Add page to (DB), Edit pages in (DB), Send notification (≤20 people), Send mail via Gmail (paid), Send webhook (paid), Show confirmation, Open page or URL, Send Slack notification (Plus+), Define variables (mentions plus formulas). Buttons are available on all plans. Using or creating them needs Full access or Can edit — [Notion Help: Buttons](https://www.notion.com/help/buttons)
- Database button property: assign, approve, upvote or automate actions, with configurable colors (2.38, Feb 2024) — [R8](https://www.notion.com/releases/page/8)
- **DB automation triggers**: Page added. Property edited (choosing the kind of edit for name, person, number, text, select and relation). **Every [frequency]** recurring, with time, start/end and timezone. Any or All trigger combinations, where edits must occur within ~3 seconds. Recurring triggers can't be combined — [Notion Help: Database automations](https://www.notion.com/help/database-automations)
- **DB automation actions**: Edit property (add or remove values for multi-select, people and relation), Add page to, Edit pages in, Send notification (≤20), Send mail (Gmail; CC/BCC, reply-to, display name), Send webhook (HTTP POST), Send Slack notification, Define variables — [Notion Help: Database automations](https://www.notion.com/help/database-automations)
- Rules: automations need a paid plan (Free can create Slack-notification automations only). Only paid users with Full access can create them; guests can't. Automations can't trigger other automations, though buttons can. They can be scoped to a whole database or one view. A failed automation is paused. A locked database blocks automations — [Notion Help: Database automations](https://www.notion.com/help/database-automations)
- History: DB automations notify (2.39, Apr 2024). Multiple trigger conditions and add/remove assignee (2.44, Aug 2024). Gmail email actions and **formula actions** (2.46, Oct 2024). **Webhook actions** and button notifications/emails (2.47, Dec 2024). **Recurring automations** and automation error tips (2.48, Feb 2025). Confetti effect (Jan 2026) — [R8](https://www.notion.com/releases/page/8); [R7](https://www.notion.com/releases/page/7); [R6](https://www.notion.com/releases/page/6)
- Pricing: Free = "Basic (buttons only)", Plus+ = custom database automations — [Notion Pricing](https://www.notion.com/pricing)
- **[NEW 2026]** Workers: webhook triggers (beta), custom agent tools (beta), and DB sync from any API (beta) (3.5, May 2026). Workers are shareable as "Can connect" or "Full access" (Jul 9 2026). Billed in credits from Aug 11 2026 — [R2](https://www.notion.com/releases/page/2)
- Integrations: Slack shortcut to create tasks, GitHub PR linking via magic keywords and PR activity in comments (2.39). Zendesk link previews (2.36). Replit integration (2.52). Importers listed in the next section — [R8](https://www.notion.com/releases/page/8); [R7](https://www.notion.com/releases/page/7)

### Inferences
- A parity automation engine needs an event bus (page created, property changed), a cron scheduler, an action library (notifications, outbound email, HTTP webhook, Slack, DB mutations, variables via the formula engine), loop prevention ("automations can't trigger automations"), view-scoped filters, and pause-on-failure.

### Gaps
- No run history or log UI for automations is documented on the help page.
- The full integrations gallery (number and names of connections) was not enumerated.

## Developer platform: REST API, webhooks, MCP, Workers/CLI, import/export

### Takeaway
The public REST API (versions 2025-09-03 and 2026-03-11) covers pages, blocks, databases, data sources, comments, users, search, meeting notes, Markdown read/write, templates and views. Rate limits are **180 req/min per connection (~3 rps) on Free and Plus and 600 req/min (~10 rps) on Business and Enterprise**, with payloads ≤1,000 blocks and 500 KB. There is a hosted **Notion MCP** (Aug 2025), plus Workers, CLI, Agent SDK, External Agents API and Skills API (2026). Import covers about 15 sources and formats. Export is Markdown+CSV, HTML or PDF, and **a workspace export can't be re-imported to recreate a workspace**.

### Cited Findings

**API limits**
- Per connection: Business and Enterprise 600 req/min (~10/s), other plans 180 req/min (~3/s). Data-source query has a separate ~3 rps limit. A per-workspace limit also exists. Over-limit responses are 429 `rate_limited`, and overload is 529 `service_overload` — [Notion API: Request limits](https://developers.notion.com/reference/request-limits)
- Sizes: rich text 2,000 chars, URL 2,000, equation 1,000, email and phone 200. Arrays are capped at 100 elements, including relations, people and multi-select per request. A whole payload is ≤1,000 blocks and 500 KB — [Notion API: Request limits](https://developers.notion.com/reference/request-limits)
- **[NEW 2026]** The Free-plan block limit is enforced on certain API connections from **Sep 8 2026**. The write that hits the limit succeeds and starts a 3-day grace period, after which block-creating requests fail — [Notion API: Request limits](https://developers.notion.com/reference/request-limits); [Notion API: Workspace block limits](https://developers.notion.com/reference/workspace-block-limits)

**API surface and versions**
- 2025-09-03 introduced `/v1/data_sources`, and `/v1/databases` became the container. The change isn't backward-compatible — [Upgrade guide](https://developers.notion.com/docs/upgrade-guide-2025-09-03)
- 2026-03-11 renamed `transcription` to `meeting_notes` — [Notion API: Block object](https://developers.notion.com/reference/block)
- A "Working with views" guide exists, implying a Views API — [developers.notion.com guide](https://developers.notion.com/guides/data-apis/working-with-views) (title only from search; contents not fetched)
- **[NEW 2026]** Markdown Content API (3.4, Mar 2026) and Markdown API (3.5, May 2026). Meeting Notes API with transcripts and summaries (Apr 2026). Any member can build connections, plus workspace-scoped OAuth and **personal access tokens** (May 2026). Developer Portal and AI-assisted docs. Developer bar showing page, DB, block and user IDs and copying API objects (Aug 19 2026). **Custom Agent Insights API** (Sep 2026) — [R4](https://www.notion.com/releases/page/4); [R3](https://www.notion.com/releases/page/3); [R2](https://www.notion.com/releases/page/2); [R1](https://www.notion.com/releases)
- **[NEW 2026] Notion Developer Platform (3.5, May 13 2026)**: External Agents API (alpha), Workers (hosted sandboxed runtime deployed via CLI), **Notion CLI** (`ntn.dev`, also npm on Windows), Agent SDK (alpha in May, public beta in Sep 2026), and a unified Connections tab — [R2](https://www.notion.com/releases/page/2); [R1](https://www.notion.com/releases)
- **[NEW 2026]** Skills API (3.8). Public API endpoints became twice as fast (2.49, Mar 2025). Admin API for scheduled workspace exports (Enterprise beta) — [R1](https://www.notion.com/releases); [R7](https://www.notion.com/releases/page/7); [Notion Help: Export](https://www.notion.com/help/export-your-content)

**Webhooks**
- **[NEW 2025]** Webhooks shipped in 2.49 (Mar 2025) — [R7](https://www.notion.com/releases/page/7)
- Events named include `page.content_updated` (aggregated, ~1-2 min delay), `comment.created`, `page.locked`, `data_source.schema_updated` (and the deprecated `database.schema_updated`). Payloads carry only IDs, so clients must re-fetch. Requests are signed with `X-Notion-Signature` HMAC-SHA256 using a `verification_token`. The URL is fixed after verification — [Notion API: Webhooks](https://developers.notion.com/reference/webhooks)

**MCP**
- **[NEW 2025]** Notion MCP (2.53, Aug 2025) lets ChatGPT, Claude and Cursor reference, create and update content. 3.0 added first-party partners Lovable, Perplexity, Mistral and HubSpot — [R6](https://www.notion.com/releases/page/6)
- Capabilities: search, create and edit pages, manage databases, **personal memories** across clients, and **Notion Skills** usable via MCP. Admins manage MCP clients and usage — [Notion API docs: MCP](https://developers.notion.com/docs/mcp)
- MCP additions: Enterprise audit logs and multi-DB queries (Jan 2026). Comments, meeting transcripts, Sites, admin auditing and approved-tool controls (Apr 2026). Meeting Notes, block comments, and DB create/update 91% more token-efficient (May 2026). Per-connection admin visibility and revocation (Oct 2026). Memory sync live and skills via MCP coming (Oct 19 2026) — [R6](https://www.notion.com/releases/page/6); [R3](https://www.notion.com/releases/page/3); [R2](https://www.notion.com/releases/page/2); [R1](https://www.notion.com/releases)

**Import**
- Apps: Evernote (≤~5,000 notes; standard edition only), Trello (≤~5,000 cards and 10 boards per import), Quip, Dropbox Paper (via .docx), Hackpad, WorkFlowy, Google Docs (5 MB on Free, 50 MB on paid), plus separate importers for **Confluence, Asana (2.43), Monday.com (2.38/2.47), Granola** — [Notion Help: Import](https://www.notion.com/help/import-data-into-notion); [R8](https://www.notion.com/releases/page/8); [R7](https://www.notion.com/releases/page/7)
- Files: .txt, .md/.markdown, .docx, CSV (can't create relations, rollups or formulas; adds rows only), Excel via CSV, HTML (H4+ becomes H3), PDF (5 MB on Free, 20 MB on paid), and **ZIP ≤5 GB** containing DOCX, CSV, text, MD, HTML, XLSX/XLS, TSV/DSV, ODS, EPUB and OPML. Markdown and HTML are rate-limited to ~120 imports per 12 hours — [Notion Help: Import](https://www.notion.com/help/import-data-into-notion)
- PDF and Google Docs importers (2.39, Apr 2024). Workspace CSV importer for files over 1,000 rows (Nov 2025) — [R8](https://www.notion.com/releases/page/8); [R6](https://www.notion.com/releases/page/6)

**Export**
- Formats: PDF (paper size, scale, include files; **subpages Business+**), HTML (subpages, comments), and **Markdown & CSV** (pages to MD, full-page DBs to CSV, callouts written as raw HTML). DB export covers the current or default view only, never all views. Form views can't be exported — [Notion Help: Export](https://www.notion.com/help/export-your-content)
- Workspace export (HTML, Markdown/CSV, plus files) can take up to 30 h, and the link expires after 7 days. **"A workspace export can't be reuploaded to recreate the workspace"**. Windows paths over 260 chars break unzip — [Notion Help: Export](https://www.notion.com/help/export-your-content)
- "Export entire workspace as PDF" is a pricing row, not mapped to a plan in the capture — [Notion Pricing](https://www.notion.com/pricing)

**Architecture hints (engineering blog)**
- The data model is block-centric. Postgres is sharded by workspace ID into 480 logical shards (schemas), which ran on 32 physical instances in 2021 — [Notion blog: Sharding Postgres](https://notion.com/blog/sharding-postgres-at-notion)
- Over 200 billion blocks by 2024, and ~90% of upserts are updates. The CDC pipeline runs Debezium → Kafka → S3/Hudi. Permissions are computed on the fly by tree traversal — [Notion blog: Data lake](https://www.notion.com/blog/building-and-scaling-notions-data-lake); [ByteByteGo summary](https://blog.bytebytego.com/p/storing-200-billion-entities-notions)
- Clients cache in SQLite (desktop and mobile native). The browser uses WASM SQLite on OPFS with a SharedWorker "active tab" pattern and Web Locks, giving ~20% faster navigation — [Notion blog: WASM SQLite](https://www.notion.com/blog/how-we-sped-up-notion-in-the-browser-with-wasm-sqlite)

### Inferences
- For a self-hosted clone, offering a Notion-compatible REST shape (blocks, pages, data sources) and an MCP server would ease migration and AI-tool adoption.
- Export is lossy: CSV per view, callouts as HTML, and no re-import. That is the main lock-in vector and a differentiator for an open-source alternative with a lossless, documented format.

### Gaps
- The full endpoint list (file uploads, users, search, comments, views) and page_size maximum (historically 100) were not fetched from the reference this session **[UNVERIFIED]**.
- The full MCP tool list, hosted endpoint URL (historically `mcp.notion.com`), transport and OAuth details were not on the fetched overview page **[UNVERIFIED]**. The "Notion-flavored Markdown" spec was not retrieved.
- The complete webhook event list (e.g. `page.created`, `page.deleted`, `page.moved`) is on a separate "Event types & delivery" page that wasn't fetched.

## Search, shortcuts, command palette, favorites, trash, duplication, templates

### Takeaway
Navigation centers on Cmd/Ctrl+P/K search (pages, plus feature search since Oct 2024), favorites, recents, the Library (2026), 30-day trash, duplication, and a template gallery with 20k+ templates and a paid Marketplace.

### Cited Findings
- Search: Cmd/Ctrl+P or K for search or jump-to-recent, Cmd/Ctrl+F for in-page find. Back and forward: Cmd/Ctrl+[ and ]. Cmd/Ctrl+L copies the link. Cmd/Ctrl+Shift+L toggles dark mode. Peek navigation goes to the next or previous DB page — [Notion Help: Keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts)
- Search improvements: popular and verified pages, previews (2.36). Cmd+F finds content hidden in toggles and databases (2.42). **Feature search**, e.g. "Settings" (2.46). Settings search (2.48). Accent-insensitive in-page find (Jan 2026). Archived pages hidden from search (Mar 2026) — [R8](https://www.notion.com/releases/page/8); [R7](https://www.notion.com/releases/page/7); [R6](https://www.notion.com/releases/page/6); [R4](https://www.notion.com/releases/page/4)
- **Trash**: kept 30 days, then permanently deleted. Permanently deleted items are retained another 30 days (Enterprise can configure this). Trash can be filtered by editor, location or teamspace. No "empty all" exists. Support restores from 30-day backups — [Notion Help: Duplicate, delete & restore](https://www.notion.com/help/duplicate-delete-and-restore-content)
- Duplication: Cmd/Ctrl+D, `/duplicate`, Alt+drag. Move with `/moveto` — [Notion Help: Keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts)
- Templates: redesigned gallery with 20k+ templates (Jul 2024). Marketplace selling with payments (Oct 2024). Agent templates "hall of fame" from Ramp, Clay, Vercel (May 2026) — [R7](https://www.notion.com/releases/page/7); [R2](https://www.notion.com/releases/page/2)
- Favorites, Private and Teamspaces sidebar sections are reorderable (2.39). Library tabs for Recents, Favorites, Private and Shared (Feb 2026) — [R8](https://www.notion.com/releases/page/8); [R5](https://www.notion.com/releases/page/5)

### Inferences
- Search parity requires full-text search over blocks and properties with permission filtering, ranking boosts (verified, popular), and the ability to exclude archived pages. AI search (semantic plus connectors) is a separate layer.

### Gaps
- Search filters (by creator, teamspace, date, "only titles") were not verified this session.
- Retention of trash in non-Enterprise plans beyond 30 days isn't configurable, per the help page; whether this differs per plan was not confirmed.

## Plan tiers, limits, and common user complaints

### Takeaway
There are four plans: **Free** (unlimited for one member; 1,000 lifetime blocks once 2+ members), **Plus** (€9.50 / $10 per seat), **Business** (€19.50 / ~$20 per seat; includes Notion AI, SSO, private teamspaces), and **Enterprise** (custom; SCIM, audit log, zero data retention). Custom Agents and Workers are billed via credits at $10 per 1,000. Recurring complaints are slowness with large databases and relations, limited offline, lossy export and lock-in, and an increasingly complex AI credit model.

### Cited Findings

**Prices**
- Captured page (EUR): Free €0, Plus €9.50/seat/mo, Business €19.50/seat/mo ("Recommended"), Enterprise custom. Annual billing saves "up to 20%". Plus is free for students and educators (1 member). Add-ons: Custom Agents at $10 per 1,000 monthly credits, Workers in beta priced in credits, custom domains at $8/mo/domain annually or $10 monthly — [Notion Pricing](https://www.notion.com/pricing)
- Plus repricing on Jun 26 2024: $10 annual or $12 monthly; €9.50 or €11.50; £8.50 or £10. Plus then included unlimited blocks and uploads, 30-day history, up to 100 guests, synced databases, automations, unlimited teamspaces and the site builder — [R8](https://www.notion.com/releases/page/8)
- Business at $20/seat/mo annual or $24 monthly appears in third-party 2026 pricing roundups surfaced by search (e.g. [Breeze](https://www.breeze.pm/articles/notion-ai-pricing), [Sentra](https://www.sentra.app/articles/notion-ai-pricing)). Not fetched, and the official page capture showed only EUR, so treat as **[UNVERIFIED]**
- The new Business plan with AI included launched with 2.51 (May 2025) — [R7](https://www.notion.com/releases/page/7)

**Plan limits (comparison)**

| Limit | Free | Plus | Business | Enterprise |
|---|---|---|---|---|
| Blocks | Unlimited solo; **1,000 lifetime** with 2+ members (deletions don't reduce the count) | Unlimited | Unlimited | Unlimited |
| File upload per file | 5 MB | 5 GB | 5 GB | 5 GB |
| Page history | 7 days | 30 days | 90 days | Custom |
| Guests | 10 | 100 (2024 announcement) | ? | ? |
| Teamspaces | Open/closed | Open/closed | + Private | + Private, advanced perms |
| Charts | 1 | Unlimited | Unlimited | Unlimited |
| Forms | Basic | Custom | + Conditional logic | + Conditional logic |
| Dashboards | – | – | Yes | Yes |
| Automations | Buttons only | DB automations | DB automations | DB automations |
| notion.site domains | 1 | 5 | 5 | 5 |
| Custom domains | – | Paid add-on | Paid add-on | Paid add-on |
| Offline auto-download | Manual only | Recents and Favorites | Same | Same |
| Notion AI (Agent, Meeting Notes, Enterprise Search) | Limited trial | Limited trial | Included | Included |
| Custom Agents / credits | – | – | Credits add-on | Credits add-on |
| AI data retention | 30 days | 30 days | 30 days | Zero data retention |
| API rate limit/connection | 180/min | 180/min | 600/min | 600/min |
| SAML SSO | – | – | Yes | Yes |
| SCIM, audit log, SIEM/DLP, workspace analytics | – | – | – | Yes |
| Page analytics | Basic | Basic | Advanced | Workspace analytics |
| PDF export with subpages | – | – | Yes | Yes |
| Row-level permissions / "Can create" | – | – | Yes | Yes |
| Presentation mode | – | Yes | Yes | Yes |

Sources: [Notion Pricing](https://www.notion.com/pricing); [Notion Help: Understanding block usage](https://www.notion.com/help/understanding-block-usage); [Notion Help: Duplicate, delete & restore](https://www.notion.com/help/duplicate-delete-and-restore-content); [Notion API: Request limits](https://developers.notion.com/reference/request-limits); [Notion Help: Forms](https://www.notion.com/help/forms); [Notion Help: Intro to teamspaces](https://www.notion.com/help/intro-to-teamspaces); [Notion Help: Export](https://www.notion.com/help/export-your-content); [R5](https://www.notion.com/releases/page/5); [R8](https://www.notion.com/releases/page/8)

- Block-limit mechanics: a lifetime count, so deleting or emptying trash doesn't help. At the limit, existing content stays editable but no new blocks can be added. Guests don't count as members — [Notion Help: Understanding block usage](https://www.notion.com/help/understanding-block-usage)

**Common complaints**
- Performance: Capterra 2026 reviews criticize speed, including slow page loads on heavy databases — [Capterra 1-star reviews](https://www.capterra.com/p/186596/Notion/reviews/?rating=1). A Jan 2026 Reddit thread (via a mirror) reports 3-5 s page loads that worsen as data grows, and relations to large DBs causing lag — [Reddit snapshot mirror](https://reddit.sentinel-team.org/posts/1qd4ooe/snapshots/2026-01-16T00%3A20%3A32.080735Z) (secondary mirror; treat with caution)
- Notion itself repeatedly ships speed work: pages 15-20% faster (Dec 2023), 15% faster (Nov 2025), Windows 27% faster (Jan 2026), initial render 28% faster (Apr 2026), and faster grouped views (Jun 2024). This confirms performance is a chronic concern — [R8](https://www.notion.com/releases/page/8); [R6](https://www.notion.com/releases/page/6); [R3](https://www.notion.com/releases/page/3)
- Offline: launched only in Aug 2025, with "the most essential blocks" offline and manual downloads on Free — [R6](https://www.notion.com/releases/page/6). A third-party claim that offline doesn't cover all blocks or new databases is from a review site I didn't fetch — [Hackceleration review](https://hackceleration.com/notion-review/) **[UNVERIFIED]**
- Lock-in and export quality: Notion's own docs state workspace exports can't recreate a workspace, DB export is per-view CSV, and callouts become HTML in Markdown — [Notion Help: Export](https://www.notion.com/help/export-your-content). Third-party guides say relations and proprietary blocks break on export — [Glitter blog](https://www.glitter.io/blog/knowledge-sharing/best-notion-alternatives) (vendor-adjacent)
- Product churn and pricing complexity: Mail was discontinued 17 months after launch — [Engadget](https://engadget.com/2201940/notion-mail-is-shutting-down). Custom Agents moved from free to credit-based on May 4 2026, followed by a new unified credit system on Oct 19 2026 — [R3](https://www.notion.com/releases/page/3); [R1](https://www.notion.com/releases)
- Data privacy: AI data retention is 30 days except Enterprise zero retention, and an AI Meeting Notes consent control was added (Mar 2026) — [Notion Pricing](https://www.notion.com/pricing); [R4](https://www.notion.com/releases/page/4)

### Master release timeline (for dating features)
- 2023-11 2.35 AI Q&A · 2023-12 2.36 faster pages, Sites settings, descriptions · 2024-01 2.37 Notion Calendar · 2024-02 2.38 Home, DB buttons · 2024-04 2.39 sidebar, PDF/GDocs importers, request edit · 2024-06 2.40 Home calendar, floating TOC, pinned tabs · 2024-06 2.41 AI connectors search · 2024-06 2.42 **Notion Sites** · 2024-06 Plus repricing · 2024-07 2.43 **Suggested edits**, Asana importer · 2024-08 2.44 **Charts**, smarter automations · 2024-09 2.45 AI connectors (Google), model choice · 2024-10 2.46 **Forms**, layouts, Gmail and formula automations, Marketplace, custom emojis · 2024-12 2.47 webhook actions, GitHub AI · 2025-02 2.48 recurring automations, Build with AI · 2025-03 2.49 **verified pages, API webhooks, conditional forms, tabbed layouts, passkeys, admin console** · 2025-04 2.50 **Notion Mail** · 2025-05 2.51 **AI Meeting Notes, Enterprise Search, Research mode, Business plan with AI** · 2025-07 2.52 DB redesign, **Feed view**, legal hold · 2025-08 2.53 **Offline mode, Notion MCP**, Calendar tasks · 2025-09-03 **API data sources** · 2025-09-18 **3.0 Agents, row permissions, AI formulas** · 2025-11 3.1 **Map view, Place property**, IP restrictions · 2026-01 3.2 mobile AI, People Directory, Jira write-back · 2026-02 Library, **3.3 Custom Agents** · 2026-03 presentation mode, **Dashboards**, "Can create pages", skills, **3.4 tabs block, H4, archive, Markdown API** · 2026-04 3.4 pt 2 AI Autofill via agents, rollup formats · 2026-05 agent admin controls, Plan Mode, **3.5 Developer Platform (Workers, CLI, Agent SDK, External Agents API)**, merge cells · 2026-07 **3.6 External Agents, HTML blocks**, Agents iOS app · 2026-08 developer bar, agent suggest-edits · 2026-09 model controls, **3.7 Skills**, sub-agents · 2026-09-22 **Notion Mail shut down** · 2026-10 **3.8 column permissions, rebuilt synced DBs, version history redesign, 150+ connections** · 2026-10-19 rebuilt agents (computer and browser, 24 h tasks, auto router, unified credits) — [R1](https://www.notion.com/releases) to [R8](https://www.notion.com/releases/page/8); [Engadget](https://engadget.com/2201940/notion-mail-is-shutting-down); [Whalesync](https://docs.whalesync.com/connectors/notion/multiple-data-sources)

### Inferences
- An open-source alternative's strongest differentiators against this landscape: (1) no block or member caps, (2) a documented lossless export and import format, (3) local-first or offline by default rather than opt-in downloads, (4) BYO LLM without credit metering, (5) full admin features (SSO, SCIM, audit) without an Enterprise paywall.
- The paywall boundaries Notion has drawn mark the features that matter to organizations: private teamspaces, row and column permissions, SSO, conditional forms, dashboards and AI at Business; SCIM, audit and zero retention at Enterprise.

### Gaps
- Official USD prices for Business and Enterprise and the annual vs monthly split weren't in the captured pricing text.
- Business and Enterprise guest limits and Plus's current guest limit (2026) weren't captured.
- Many pricing rows (API, webhooks, embeds, 2FA, workspace export, priority support) came back without per-plan values.
- I found no reliable 2026 Hacker News thread data, and Reddit evidence is thin or via mirrors.
- The Wikipedia "stable release" (3.7, Sep 15 2026) lags the changelog, which shows 3.8 (Oct 13 2026) and an Oct 19 2026 agent release — [Wikipedia](https://en.wikipedia.org/wiki/Notion_(productivity_software)); [R1](https://www.notion.com/releases)
