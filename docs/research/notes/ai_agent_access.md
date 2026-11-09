# AI-native architecture: making AI agents first-class users of a Notion-like workspace (state as of November 2026)

Scope note: research done 2026-11-09. Spec versions and dates are marked inline. The current MCP revision is **2026-07-28**. It supersedes 2025-11-25 and 2025-06-18, and it changes server design in important ways (stateless, no sessions, tasks moved to an extension). Most Notion facts come from Notion's own developer docs (primary). Several product facts (Atlassian, Anytype, Outline, Custom Agents launch date) come only from secondary sources and are flagged as such.

---

## 1. Notion's own approach: hosted MCP, Notion-flavored Markdown, why raw block JSON hurts, Notion AI agents

### Takeaway
Notion tried a 1:1 REST-to-MCP wrapper (April 2025), dropped it, and moved to a hosted remote MCP server (July 2025). Its key tools read and write **Notion-flavored Markdown (NFM)**: CommonMark/GFM plus XML-like tags for blocks Markdown can't express. Edits are a **batched exact-match search-and-replace** (`old_str`/`new_str`). By 2026 the same model reached the public REST API: `PATCH /v1/pages/{id}/markdown`, Notion-Version `2026-03-11`, NFM v2. It ships with guards: a flag is required before child pages can be deleted, large writes run as async tasks, destructive tools need `user_confirmed`, warnings are structured, and rate-limit errors are machine-readable.

### Cited Findings
**History and rationale**
- Notion released the open-source `notion-mcp-server` in early April 2025. Users had to create an API integration and paste a key into MCP headers, or run Docker. Each REST endpoint mapped 1:1 to a tool, which Notion says gave agents a poor experience. A key problem was **high token use from hierarchical block JSON** — [Notion blog, "Notion's hosted MCP server: an inside look", 2025-07-15](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look)
- The hosted server uses Streamable HTTP as its primary transport, with SSE kept for compatibility. OAuth is "one-click", and the server stores the OAuth-issued token to call Notion's public API. Tool definitions are generated from Notion's OpenAPI schemas, converted to Zod — [Notion blog](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look)
- `create-pages` and `update-page` are agent-oriented rewrites of the Create/Update Page APIs and use NFM. `search` does semantic, question-based search across the workspace plus more than 10 connected third-party apps, with a simpler fallback for workspaces without Notion AI. Other v1 endpoints (e.g. `create-comment`) are wrapped with AI-friendly descriptions — [Notion blog](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look)
- Notion's 2022 API launch had **rejected Markdown** because CommonMark can't express rich-text colors or databases. Notion later reversed this and built extended Markdown, arguing it is **denser per token than JSON and needs fewer tool calls**. No token counts were published — [Notion blog](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look)
- Stated lesson: tools should be designed "for agent conversations, not rigid web APIs" — [Notion blog](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look)

**Current tool surface (Notion MCP docs, Nov 2026)**
- The tool categories are: search/read (including teammates and teamspaces); "work together" (track changes, comments, **suggest edits for review**); create/organize pages; databases and views; files (upload, folders, download links); Skills, Custom Agents and private memories; and calendars — [Notion MCP supported tools](https://developers.notion.com/docs/mcp-supported-tools)
- **`notion-create-pages`** takes `pages[]` (properties, content, icon, cover, `is_skill`), a `parent` (page, database, `teamspace_id`, or `"memory"`), `creation_mode: "draft"` and `allow_async`. Markdown that is too large or complex fails with `validation_error`, and **no pages are created in that call**. The response includes a per-page `warnings[]` with an `index` — [Notion MCP pages reference](https://developers.notion.com/guides/mcp/tools-pages-reference.md)
- **`notion-update-page`** commands: `update_content` (search-and-replace), `replace_content`, `update_properties`. Each `old_str` must be non-empty and **match exactly one location** unless `replace_all_matches: true`. Edits that don't match are skipped and reported as `no_match` or `indentation_mismatch` warnings. If no content-changing edit matches, **the call fails and the page is unchanged** — [Notion MCP pages reference](https://developers.notion.com/guides/mcp/tools-pages-reference.md)
- Large pages: content beyond about **20,000 records** can't be loaded and appears as `<unknown>` tags. Edits that touch unloaded content fail with `validation_error` — [Notion MCP pages reference](https://developers.notion.com/guides/mcp/tools-pages-reference.md)
- `notion-delete-pages` moves up to 20 pages to Trash and **requires `user_confirmed: true`** (otherwise nothing is moved). It is marked destructive, and locked pages can't be trashed. There are matching `notion-restore-pages`, `notion-move-pages` (moved items inherit the new parent's permissions) and an async `notion-duplicate-page`. `notion-get-async-task` reports `queued|running|retrying|succeeded|failed` with `poll_after_seconds` — [Notion MCP pages reference](https://developers.notion.com/guides/mcp/tools-pages-reference.md)

**Notion-flavored Markdown v2 spec**
- `markdown_version: "v2"` is recommended and "becomes the default in the next Notion-Version". v1 ("enhanced markdown") is deprecated. Base syntax is CommonMark plus GFM tables, task lists and strikethrough, plus `:emoji:`, `$inline$` and `$$` math, and `[^URL]` citations (no footnotes) — [NFM spec](https://developers.notion.com/guides/data-apis/notion-flavored-markdown.md)
- Blocks are XML-like tags: `<callout icon="💡" color="yellow_bg">` (indented children), `<toggle heading="h1">`, `<columns><column ratio="50">`, `<table>/<tr>/<td>` for styled tables, `<database url=… data-source-url=… inline="false">`, `<page url="…">Title</page>`, `<mention url=…>`, `<span color=…>`, and also `synced-block`, `meeting-notes`, `custom-block`, `tab`, `mail` and others. **Omitting a `<page>` tag in an update deletes that subpage only if `allow_deleting_content` is true.** Unknown HTML (`<div>`, `<details>`) is saved as literal text, and literal tags must be escaped (`\<tag>`) — [NFM spec](https://developers.notion.com/guides/data-apis/notion-flavored-markdown.md)
- `developers.notion.com/llms.txt` lists a v2 migration guide and the deprecated v1 spec, along with "Retrieve a page as markdown" and "Update a page's content as markdown" REST endpoints — [Notion llms.txt](https://developers.notion.com/llms.txt)

**Markdown REST endpoint (Notion-Version 2026-03-11)**
- `PATCH /v1/pages/{page_id}/markdown` supports these operations:
  - `replace_content` (recommended; whole page)
  - `update_content` (recommended; **up to 100** `content_updates` of `{old_str,new_str,replace_all_matches}`)
  - legacy `insert_content` and `replace_content_range`, which use **ellipsis selections `"start text...end text"`, now deprecated**
- The endpoint **refuses by default** to delete child pages or databases. The `validation_error` lists them, and `allow_deleting_content: true` is the override. A 200 response returns the **full post-edit markdown**, `truncated`, `unknown_block_ids` (≤100) and `warnings`. With `allow_async` it returns 202 plus an `async_task`. Errors cover multiple matches, synced pages (not updatable), and content too large ("Retrying the same request fails again; split content…") — [Update page markdown](https://developers.notion.com/reference/update-page-markdown.md)

**Why the raw REST/block JSON is painful**
- Rate limits are **180 req/min per connection (~3 rps)** on most plans and 600 req/min on Business/Enterprise. Data-source queries have a separate limit of about 3 rps, and a per-workspace budget is shared across connections. Rich-text `content` is capped at **2,000 chars**, block arrays at **100 elements per request**, and payloads at **1,000 blocks / 500 KB** — [Notion request limits](https://developers.notion.com/reference/request-limits.md)
- Appending block children allows **at most 100 children per request and two levels of nesting**. Deeper trees need several sequential calls — [Append block children](https://developers.notion.com/reference/patch-block-children)
- Every MCP tool call counts against these API limits. Notion MCP retries once if the wait is ≤2 s; otherwise it returns a tool error whose `structuredContent.error` holds `code: "rate_limited"`, `retry_after_seconds` and `rate_limit_reason` — [Notion MCP rate limits](https://developers.notion.com/guides/mcp/rate-limits.md)

**Security guidance from Notion**
- "Treat content returned by tools as untrusted." Turn on confirmation for content-changing actions. A connected client can access anything the user can. `notion-fetch` on connector results can return full Slack, Drive or email items. Restricting which clients members may connect requires Enterprise. The Admin API can list and revoke members' MCP connections — [Notion MCP security best practices](https://developers.notion.com/guides/mcp/mcp-security-best-practices.md)

**Notion AI agents**
- Notion 3.0 (2025-09-18) introduced agents that can do 20+ minutes of multi-step work across many pages, with memory stored as workspace pages, on Business/Enterprise plans — [Notion release 3.0](https://www.notion.com/releases/2025-09-18); [Datamation](https://www.datamation.com/?p=26679)
- Custom Agents went to public beta on 2026-02-24 (Notion 3.3) and moved to credit-based billing from 2026-05-04. Both facts are secondary — [Matthias Frank](https://matthiasfrank.de/en/notion-custom-agents/)
- How Custom Agents work, from Notion help:
  - Triggers: recurring; Notion events (comment added, page added to or removed from a database, property updated, AI meeting note finished); Slack.
  - **An agent sees only the pages, databases and apps granted in "Tools and access". Linking a page in the instructions does not grant access.**
  - Permissions on the agent itself: Full Access, Can Edit, or Can View and Interact.
  - An Activity tab logs each run (trigger, actions, errors). Version history allows restore.
  - A web-access toggle controls browsing. Admins can restrict which models are allowed, and Enterprise audit logs record changes to instructions, permissions and integrations — [Notion help: Custom Agents](https://www.notion.com/help/custom-agent)

**Security incidents**
- **Sept 2025 (CodeIntegrity):** a PDF carrying a hidden prompt injection made the Notion 3.0 agent put page contents into a web-search tool call aimed at an attacker server. Simon Willison called it a "lethal trifecta" case and suggested removing URL support from the search tool — [Simon Willison](https://simonwillison.net/2025/Sep/19/notion-lethal-trifecta); [Schneier](https://www.schneier.com/?p=70883)
- **Dec 2025 / Jan 2026 (PromptArmor):** a resume containing an injection got Notion AI to insert an external image whose URL carried hiring-tracker data. **AI edits were saved before the user approved them**, and external Markdown images rendered automatically, so the data leaked whether or not the user accepted. The HackerOne report was first closed as N/A. Notion then said a fix for "insecure AI document updates" went live on 2026-01-07. A fix for image rendering was not confirmed. PromptArmor also notes that Notion's document scanner is itself an LLM, so injection can bypass it — [PromptArmor](https://www.promptarmor.com/resources/notion-ai-unpatched-data-exfiltration)

### Inferences
- The market leader tried both approaches. It ended on **Markdown with XML-ish extension tags for content, plus exact-match string replacement for edits**, not block-JSON CRUD. Notion's own markdown API deprecating the ellipsis-range and "insert after selection" operations in favour of `old_str/new_str` is a strong signal that str_replace semantics are what agents handle best.
- The guardrails worth copying:
  - a deletion guard (`allow_deleting_content`)
  - `user_confirmed` on destructive tools, plus `destructiveHint`
  - all-or-nothing validation when parsing fails
  - per-operation warnings instead of hard failure when only some edits match
  - the **full post-edit content in the response**, so the agent can check its own work
  - async tasks with `poll_after_seconds`
  - structured rate-limit errors carrying `retry_after_seconds`
  - "unknown/unloaded" placeholders (`<unknown>` plus IDs) instead of silent truncation
- Illustrative estimate (my own, not a Notion figure): a Notion API paragraph block holding "Hello world" carries IDs, parent, timestamps, created_by/last_edited_by objects and a rich_text array with six annotation booleans. That is roughly 600–700 characters of JSON for 11 characters of text. Markdown costs about 11 characters. This order-of-magnitude gap, the 100-child and 2-level nesting limits, and ~3 rps together explain why agents struggle with raw REST.
- The PromptArmor case shows that "suggest/approve" UX gives no protection if the suggestion is already persisted and rendered with network side effects. Pending AI content must be inert: no external fetches until a human accepts it.

### Gaps
- Notion has published no token or cost comparison between JSON and NFM.
- I couldn't confirm whether NFM output includes block IDs, or whether NFM round-trips losslessly. The spec page I read says neither.
- I didn't fetch the technical references for files, skills, agents or calendars, or `notion-fetch`/`notion-search`, so the full count of tool names is unverified.
- How Notion's own agents apply edits internally (real-time transactions or a suggestion layer) isn't documented in what I read. The "suggest edits for review" tool category exists, but I didn't read its details.

---

## 2. Other products' agent interfaces (Linear, Atlassian, Google Docs, Anytype, Outline, Obsidian, AFFiNE, AppFlowy, Tiptap/Liveblocks, Ink & Switch)

### Takeaway
What works across products:
- hosted remote MCP with OAuth (Notion, Linear, Atlassian)
- read-only endpoints and scopes (Linear)
- agents modelled as **app actors distinct from humans** (Linear `actor=app`, delegate ≠ assignee)
- AI edits shown as tracked changes or branches (Tiptap, Ink & Switch)
- optimistic revision control (Google Docs `requiredRevisionId` / `targetRevisionId`)

What fails:
- auto-generated 1:1 OpenAPI wrappers (Notion v1, and Anytype's current design)
- lossy format conversion (Confluence ADF ↔ Markdown)
- ambiguous structural anchors (Obsidian heading paths)
- persisting AI edits before approval (Notion)

### Cited Findings
**Linear**
- Endpoints: `https://mcp.linear.app/mcp` (Streamable HTTP, read-write), `…/mcp/readonly` (read tools only), and `…/sse` (deprecated).
- Auth: OAuth 2.1 with dynamic client registration, or a Bearer token (OAuth token or API key). Requesting only the `read` scope blocks write APIs. Enterprise options include Okta-managed authorization and Claude-issued managed authorization (beta).
- Tools cover finding, creating and updating issues, projects, comments and documents — [Linear MCP docs](https://linear.app/docs/mcp)
- The MCP server launched on 2025-05-01 — [Linear changelog](https://linear.app/changelog/2025-05-01-mcp)
- **Linear for Agents** — [Linear developers: agents](https://linear.app/developers/agents):
  - Agents install through OAuth with `actor=app`, so the agent is its own identity rather than the installing user.
  - The `app:mentionable` and `app:assignable` scopes let agents be @mentioned and **delegated** issues. Delegation sets `delegate`, not `assignee`, **"so humans keep ownership"**. Agents can't be project members or leads.
  - Agents don't count as billable users.
  - Agent Sessions track each task and start on mention or delegation. The agent must emit a `thought` activity within **10 seconds**.
  - `PermissionChange` webhooks exist, and admins can revoke an agent's team access at any time.

**Atlassian (Confluence/Jira)**
- The Rovo MCP Server reportedly reached GA in February 2026 with 12 Confluence tools grouped into read, write and search permission sets, plus beta `searchAtlassian`/`fetchAtlassian` cross-product tools. Secondary source — [Scalekit](https://www.scalekit.com/blog/confluence-mcp-vs-api)
- Microsoft says the Confluence tools create and update pages from Markdown and counts 46+ tools across products — [Microsoft Tech Community](https://techcommunity.microsoft.com/blog/AppsonAzureBlog/get-started-with-atlassian-rovo-mcp-server-in-azure-sre-agent/4497122). **This conflicts with** an Atlassian Community article saying Confluence expects ADF and that Markdown behaves inconsistently, e.g. flattening tables — [Atlassian Community](https://community.atlassian.com/forums/Rovo-articles/Should-I-Use-a-Third-Party-MCP-Server-Instead-of-Rovo-MCP/ba-p/3223144)
- Repository: [atlassian/atlassian-mcp-server](https://github.com/atlassian/atlassian-mcp-server)

**Google Docs API (prior art for concurrency)**
- `WriteControl.requiredRevisionId` **rejects** a write if the document changed since it was read. `targetRevisionId` instead **applies the edits on top of collaborators' changes**, with the server doing the merge ("The Docs server is responsible for merging the content").
- Within one `batchUpdate`, Google recommends ordering requests in **descending index** so earlier edits don't shift later indexes. Index-based edits go stale under collaboration — [Google Docs API best practices](https://developers.google.com/workspace/docs/api/how-tos/best-practices)

**Anytype**
- The official `@anyproto/anytype-mcp` turns Anytype's OpenAPI spec into MCP tools at runtime. The API key and `Anytype-Version: 2025-05-20` go in `OPENAPI_MCP_HEADERS`. The local API is desktop-only. Directory listings only — [LobeHub listing](https://lobehub.com/mcp/anyproto-anytype-mcp); [Glama CLAUDE.md](https://glama.ai/mcp/servers/@anyproto/anytype-mcp/blob/7af1b97b91764dd1000db390520fb2e59a4a0f7e/CLAUDE.md)
- A community server targets `http://localhost:31009/v1` — [Qwinty anytype-mcp](https://lobehub.com/ru/mcp/qwinty-anytype-mcp)

**Outline**
- My searches found no official Outline MCP server. The community `outline-mcp-server` (huiseo) covers documents, collections, comments and search, offers RAG Q&A (`ask_wiki`, `summarize_document`), and has a read-only mode plus a switch that blocks delete tools — [LobeHub: outline-wiki-mcp](https://lobehub.com/mcp/huiseo-outline-wiki-mcp); [outline-smart-mcp](https://lobehub.com/en/mcp/huiseo-outline-smart-mcp)

**Obsidian**
- The Local REST API plugin can "read, write, or patch a particular heading, block reference, or frontmatter field" using append, prepend or replace operations — [Obsidian plugin page](https://community.obsidian.md/plugins/obsidian-local-rest-api)
- MCP wrappers expose `targetType` (heading/block/frontmatter) plus `operation`. Block targets use `^block-id` — [Glama patchNote tool](https://glama.ai/mcp/servers/@takuya0206/obsidian-mcp/tools/patchNote)
- Known failure: heading targets below H1 fail unless given as fully qualified heading paths — [Obsidian forum](https://forum.obsidian.md/t/patch-content-with-target-type-heading-fails-on-all-h2-headings/114680)

**AppFlowy / AFFiNE (open-source Notion alternatives)**
- AppFlowy documents local AI through **Ollama**. It is off by default and turned on in desktop settings, and `ollama serve` must keep running — [AppFlowy docs](https://docs.appflowy.io/docs/appflowy/product/appflowy-ai-ollama)
- AFFiNE self-hosting lets admins set a custom base URL for the OpenAI provider. Since v0.25 an `oldApiStyle` flag switches to `/chat/completions`, but "some AI feature may not work", and only specific hosted models were tested — [AFFiNE self-host AI docs](https://affine-docs.pages.dev/self-host-affine/administer/ai)
- AFFiNE's documented providers are OpenAI, Anthropic, Google and Perplexity — [AFFiNE docs mirror](https://www.mintlify.com/toeverything/AFFiNE/features/ai-assistant)

**Tiptap AI Toolkit (editor-level agent editing)**
- The "Tiptap Edit" workflow — [Tiptap Edit workflow](https://tiptap.dev/docs/content-ai/capabilities/ai-toolkit/workflows/tiptap-edit):
  - `tiptapRead` returns content "optimized for fast, precise edits".
  - The LLM's output is constrained by a Zod schema to an `operations` array (replace, insert before, insert after **nodes**), which is streamed and applied as it arrives.
  - Review modes: `trackedChanges` ("Changes persist as part of the document and are visible to other users") or `review` (ephemeral decorations visible only to the current user).
  - `range` limits edits to part of the document, and `tiptapReadChunks` processes large documents in parallel chunks.
- With tracked changes on, AI edits are written as tracked changes, explanations can attach as linked comments, and reads return the document **as if all tracked changes were applied**. Fixes were needed for suggestions disappearing during undo/redo with collaboration on — [Tiptap AI Toolkit changelog](https://tiptap.dev/docs/resources/changelog/pro-ai-toolkit); [Tracked changes extension](https://tiptap.dev/docs/editor/extensions/functionality/tracked-changes.md)
- Tiptap's roadmap lists native redlining (suggestion mode) for humans and AI as a pilot feature — [Tiptap roadmap](https://tiptap.dev/roadmap)

**Liveblocks / Yjs ecosystem**
- Liveblocks advertises server-side document editing by AI agents, either live over WebSocket or through HTTP for simple updates — [Liveblocks Yjs](https://liveblocks.io/yjs); [Liveblocks text editing](https://liveblocks.io/docs/products/sync/text-editing)
- The agent-native framework applies surgical search-and-replace on `Y.XmlText` server-side, so only the changed text produces a Yjs update — [agent-native docs](https://agent-native.com/docs/real-time-collaboration)
- Electric's demo runs the agent as a **server-side Yjs peer** with its own cursor and presence — [Electric demo](https://electric.ax/streams/demos/collaborative-ai-editor)

**Ink & Switch Patchwork**
- An AI bot can propose a **branch**, and its edits appear as a diff the user accepts or rejects. After acceptance, history shows which edits came from the bot. Bots helped with style-guide editing and transcript cleanup but "can also give bad suggestions". The project ran 2024–2026 — [Ink & Switch notebook 07](https://www.inkandswitch.com/patchwork/notebook/07/); [Patchwork 2024 version control](https://www.inkandswitch.com/patchwork/notebook/2024-version-control/)

### Inferences
- Linear's model is the clearest precedent for "agents as workspace members": a separate app identity, scopes for being mentioned or delegated, humans keep ownership, no seat cost, plus session and activity telemetry. A Notion-like workspace should copy it for pages and database rows (e.g. an `ai_delegate` field next to a human owner).
- Anytype's spec-to-tools approach is exactly what Notion abandoned. Expect token bloat and weak tool choice unless curated tools sit on top of it.
- Converting between the agent format and a richer internal format (Confluence ADF) causes silent fidelity loss. The agent format should be designed together with the block model, not bolted on.
- Structural anchors (heading paths) are fragile. Content-matching (str_replace) or stable IDs are both more robust.
- Google's `requiredRevisionId` (reject) vs `targetRevisionId` (server merges) maps directly onto the two concurrency modes an agent API should offer.

### Gaps
- Not researched due to budget: Coda (Packs/MCP), Craft, Mem, Google Docs/Workspace Gemini agents, AFFiNE or AppFlowy MCP servers.
- Atlassian's official supported-tools page wasn't opened, so the Markdown-vs-ADF conflict is unresolved.
- I couldn't confirm whether Outline shipped an official MCP server by 2026.
- Tiptap doesn't document how the AI Toolkit resolves an AI and a human editing the same node at once.

---

## 3. Content representation for LLMs (Markdown vs Markdown+IDs vs MDX/XML vs JSON)

### Takeaway
The evidence (mostly from code editing, plus Notion's product choice) favours a **plain-text, Markdown-like format with light XML-style tags** over JSON for content. **Short content-addressed or ID anchors** help edit precision and stale-edit detection, especially for weaker or local models. No single edit format wins for every model, so offer two edit styles (anchor-based and str_replace) behind one tool.

### Cited Findings
- Notion moved from JSON to extended Markdown because Markdown is denser per token and needs fewer calls — [Notion blog](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look)
- NFM puts Markdown-inexpressible blocks in XML-like tags with attributes (`<callout icon color>`, `<columns>`, `<database url>`), treats unknown HTML as literal text, and requires escaping literal tags — [NFM spec](https://developers.notion.com/guides/data-apis/notion-flavored-markdown.md)
- **Aider, unified diffs (GPT-4 era):** on an 89-task "laziness" benchmark, GPT-4 Turbo scored 20% with SEARCH/REPLACE and **61% with unified diffs** (lazy comments fell from 12 tasks to 4). GPT-4-0613 went from 26% to 59%. Stated reasons:
  - the format is familiar from `git diff`
  - it avoids JSON escaping
  - it avoids line numbers, which models handle poorly
  - high-level hunks (30–50% more errors without them)
  - flexible patch application (disabling it gave **9× more editing errors**)

  — [Aider: unified diffs](https://aider.chat/docs/unified-diffs.html)
- Aider's earlier benchmarks found plain-text edit formats beat JSON function-calling formats — [Aider benchmarks](https://aider.chat/docs/benchmarks.html). Aider picks a different edit format per model — [Aider edit formats](https://aider.chat/docs/more/edit-formats.html)
- **Diff-XYZ (JetBrains, arXiv 2510.12487):** search-replace did best for stronger models, udiff variants were competitive, and no format dominates across models and tasks. Summarised from search results — [arXiv 2510.12487](https://arxiv.org/html/2510.12487v2)
- **Hashline ("The Harness Problem", Can Bölük, 2026-02-12)** — [stencil.so/blog/the-harness-problem](https://stencil.so/blog/the-harness-problem), redirected from blog.can.ac:
  - Setup: 16 models, 3 edit tools (apply_patch, str_replace, hashline), 180 tasks × 3 runs, with only the edit tool changing.
  - Hashline tags each line with a 2–3 character content hash on read, and edits reference those tags. **If the file changed since the read, hashes don't match and the edit is rejected** (an "optimistic" check).
  - Results: hashline beat patch in 14 of 16 models, by +15 points on average. Grok Code Fast 1 went from 6.7% to 68.3%. Output tokens fell by up to 61%, though some models used more (GPT-5.2 Codex +26%, DeepSeek V3.2 +20%).
  - Failure modes: patch failed 50.7% of the time for Grok 4 and 46.2% for GLM-4.7. str_replace commonly failed with "String to replace not found".
- **Replication pushback:** a small follow-up found hashline vs replace was "not a clear winner", with the effect depending on model and language. It concluded edit format isn't the bottleneck for current frontier models — [dev.to/nwyin](https://dev.to/nwyin/hashline-vs-replace-does-the-edit-format-matter-15n2)
- Obsidian uses inline `^block-id` markers as stable block anchors inside plain Markdown, and REST/MCP patch tools target them — [Obsidian Local REST API](https://community.obsidian.md/plugins/obsidian-local-rest-api); [patchNote](https://glama.ai/mcp/servers/@takuya0206/obsidian-mcp/tools/patchNote)
- Notion represents unloaded or unsupported regions with `<unknown>` tags and returns their IDs (`unknown_block_ids`) instead of dropping them — [Update page markdown](https://developers.notion.com/reference/update-page-markdown.md)
- Anthropic: return high-signal, natural-language fields instead of low-level identifiers like UUIDs. Mapping cryptic IDs to names reduced hallucinations — [Anthropic: Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)

### Inferences
- **Recommended content format:** "Workspace Markdown". CommonMark + GFM for prose, lists, tables and code; XML-like tags with attributes for everything else (callout, toggle, columns, page, database, mention, embed, synced block); and an explicit escape-hatch tag (`<block type="…" id="…"/>` or `<unknown id=…/>`) so unsupported blocks survive a round trip and are never silently deleted.
- **Block anchors:** full UUIDs cost many tokens per block, and Anthropic advises against exposing them. Better options, compact and opt-in:
  - (a) Obsidian-style trailing markers (`^k3f`)
  - (b) an `id` attribute only on tag-based blocks
  - (c) hashline-style short content hashes generated at read time

  The server maps a short **page-local alias → real block UUID plus content hash** and resolves it on write. A changed hash means a stale edit, so the server returns a conflict and doesn't guess. Make anchors a read option (`anchors: none|blocks|lines`) so strong models can use pure str_replace and weaker or local models can use anchors.
- JSON is still the right choice for **structured data**: database properties, schemas, filters. Mixing gives the best of both: Notion returns rich-text properties as Markdown strings inside JSON rows (see §8).
- Round-trip fidelity is a design requirement. Every block attribute must have a text serialisation or be preserved through an opaque tag, and the server should run property tests (block tree → markdown → block tree = identity).

### Gaps
- I found no rigorous public benchmark of Markdown vs JSON vs XML for editing **rich documents** (as opposed to code). All format evidence comes from code-editing benchmarks plus Notion's qualitative claim.
- No published token counts compare Notion JSON with NFM.
- No evidence was found on how reliably models preserve inline anchors like `^id` or HTML comments when rewriting text. That should be tested in-house.

---

## 4. Edit operations for agents, and merging with live human collaboration

### Takeaway
Use **small, targeted edit operations**: batched str_replace and/or anchor-based block operations. Keep full-document replace for new or small pages, behind a deletion guard. Add optimistic concurrency (revision or hash). Apply agent edits **as CRDT transactions from a server-side peer with its own origin**, so they merge with concurrent human typing instead of overwriting it. Default untrusted or background agents to **suggestion mode** (tracked changes or a branch) with accept/reject, per-agent undo, attribution, and no side effects until accepted.

### Cited Findings
- **Notion's operations:** `update_content` takes up to 100 `{old_str,new_str}` edits, each with a unique match or `replace_all_matches`. Non-matching edits come back as warnings, and the call fails only if nothing matched. `replace_content` replaces the whole page, guarded by `allow_deleting_content`. The ellipsis-range operations are deprecated. The full post-edit Markdown is returned — [Update page markdown](https://developers.notion.com/reference/update-page-markdown.md); [Notion MCP pages reference](https://developers.notion.com/guides/mcp/tools-pages-reference.md)
- **Google Docs concurrency modes:** `requiredRevisionId` fails if the document changed; `targetRevisionId` has the server merge the edits on top of collaborators' changes — [Google Docs API best practices](https://developers.google.com/workspace/docs/api/how-tos/best-practices)
- **Hashline:** content hashes catch stale edits before corrupting the file — [stencil.so](https://stencil.so/blog/the-harness-problem)
- **"When an AI agent joins your Yjs room"** — [DEV Community](https://dev.to/norfolkd/when-an-ai-agent-joins-your-yjs-room-three-assumptions-break-50h8):
  - Throughput: agents write 25–100× faster than humans (1,000–4,000 wpm) and can starve human operations, so put an **app-level token bucket** between the LLM stream and Yjs writes, scoped to the agent origin (illustrative numbers: 50 queued ops, refill 10 ops per 100 ms).
  - Origin: agent edits are normal `ydoc.transact()` calls tagged with an **`agentOrigin`**. A server-side peer is preferred over posting diffs over REST, so there is one convergence model instead of two.
  - Undo: `Y.UndoManager` scopes history by origin, so give the agent **its own UndoManager** (`trackedOrigins`) and expose "reject AI suggestion" as a separate action from normal undo.
  - Presence: coalesce agent awareness updates (about 300 ms) and add `type: 'agent'` and `streaming` flags to the awareness state.
- **Tiptap:** AI edits can be tracked changes (persisted, shared) or ephemeral review suggestions (local to one user). Accept/reject comes with explanations as linked comments — [Tiptap Edit workflow](https://tiptap.dev/docs/content-ai/capabilities/ai-toolkit/workflows/tiptap-edit); [Tiptap AI Toolkit changelog](https://tiptap.dev/docs/resources/changelog/pro-ai-toolkit)
- **Liveblocks / agent-native / Electric:** agents can be server-side Yjs peers, apply surgical search-and-replace on `Y.XmlText`, and show their own cursor — [Liveblocks](https://liveblocks.io/yjs); [agent-native](https://agent-native.com/docs/real-time-collaboration); [Electric](https://electric.ax/streams/demos/collaborative-ai-editor)
- **Ink & Switch:** bot edits arrive as branches or diffs to accept or reject, and history attributes accepted edits to the bot — [Ink & Switch notebook 07](https://www.inkandswitch.com/patchwork/notebook/07/)
- **Anti-pattern:** Notion AI saved edits before approval, and the rendered external image exfiltrated data even when the user rejected the edit — [PromptArmor](https://www.promptarmor.com/resources/notion-ai-unpatched-data-exfiltration)
- Custom Agents runs are logged and reversible through version history — [Notion help: Custom Agents](https://www.notion.com/help/custom-agent)
- **Batching and async:** Notion gives each create call all-or-nothing validation, offers async tasks for large writes, and caps a request at 100 content updates — [Notion MCP pages reference](https://developers.notion.com/guides/mcp/tools-pages-reference.md); [Update page markdown](https://developers.notion.com/reference/update-page-markdown.md)

### Inferences
- **Recommended merge pipeline:**
  1. `fetch` returns content plus a `revision` (the CRDT state-vector digest) and optional anchors.
  2. The agent sends operations with `base_revision`.
  3. The server resolves each operation against the *base* snapshot: str_replace matches against the base text, and anchors resolve to block IDs and hashes.
  4. Each resolved target becomes a Yjs/Loro relative position or block ID, so concurrent human edits elsewhere don't invalidate it.
  5. If a target region was itself changed by a human since `base_revision` (hash mismatch), return a **conflict tool error with the current snippet**. This is like `requiredRevisionId`, but scoped to the touched blocks rather than the whole document.
  6. Otherwise apply the operations as one CRDT transaction with `origin = agent:<id>, run:<run_id>`. This is like `targetRevisionId` with automatic rebase.
- **Write modes per agent and per page:** `direct` (an interactive, trusted agent writing as the user), `suggest` (tracked-change marks in the CRDT, the default for background or untrusted agents and for pages the agent can only "comment/suggest" on), and `branch` (a forked doc or Patchwork-style branch for large rewrites, merged on accept).
- Suggestions must be **inert**: no external image or embed fetches, link unfurls, mention notifications or automation triggers until accepted.
- **Attribution:** store `origin` per CRDT transaction or item, plus authorship metadata (agent ID, model, run ID, on-behalf-of user). Render it in the UI (an "AI" badge, blame view) and in history. Per-run undo means reverting every transaction with that run's origin.
- **Backpressure:** stream LLM output into a buffer and commit at block granularity, not per token. Rate-limit per agent origin and coalesce presence updates.
- Keep `replace_content` but require `allow_deleting_content` for child pages and databases, and preferably refuse whole-page replace on pages with active human collaborators (`409 page_in_active_edit`, telling the agent to use targeted edits).

### Gaps
- I found no published evaluation of AI-suggestion UX (acceptance rates, reviewer burden) or of conflict frequency between agents and humans in real workspaces.
- Tiptap's handling of simultaneous edits to the same node is undocumented.
- I didn't research Loro or Automerge specifics, such as their suggestion and mark support.

---

## 5. Protocols (MCP 2025-06-18 → 2025-11-25 → 2026-07-28, MCP Apps, A2A, OpenAPI, llms.txt, WebMCP) and MCP tool-design best practices

### Takeaway
MCP is the de facto agent protocol, now under the Linux Foundation's Agentic AI Foundation. As of November 2026 the current revision is **2026-07-28**: **stateless** (no `initialize`, no `Mcp-Session-Id`), state passed in explicit server-minted handles, Tasks and Apps as official extensions, Sampling/Roots/Logging deprecated, and Dynamic Client Registration deprecated in favour of Client ID Metadata Documents. A self-hosted workspace should expose a **stateless Streamable HTTP MCP server with OAuth 2.1** and about 10–15 consolidated, token-efficient tools. Alongside it: a REST/OpenAPI API for deterministic integrations, `llms.txt` for docs, and A2A only if external agents need to delegate tasks *to* workspace agents. WebMCP is still a draft.

### Cited Findings
**MCP 2025-06-18**
- Removed JSON-RPC batching.
- Added **structured tool output** (`structuredContent`, `outputSchema`).
- Classified MCP servers as **OAuth Resource Servers**, with protected resource metadata used to discover the authorization server.
- Required clients to send **RFC 8707 Resource Indicators**.
- Added **elicitation** and **resource links** in tool results.
- Required the `MCP-Protocol-Version` header and added `title` fields.

— [MCP changelog 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/changelog)

**MCP 2025-11-25**
- OpenID Connect Discovery.
- Icons on tools, resources and prompts (SEP-973).
- **Incremental scope consent via `WWW-Authenticate`** (SEP-835).
- Tool-name guidance (SEP-986).
- **URL-mode elicitation** (SEP-1036).
- Tool calling inside sampling (SEP-1577).
- **Client ID Metadata Documents** as the recommended registration method (SEP-991).
- **Experimental Tasks** for durable requests with polling (SEP-1686).
- **Input validation errors returned as Tool Execution Errors, so the model can self-correct** (SEP-1303).
- JSON Schema 2020-12 as the default dialect (SEP-1613).

— [MCP changelog 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/changelog)

**MCP 2026-07-28 (current)** — [MCP changelog 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
- Removes protocol sessions and `Mcp-Session-Id`. "Servers that need cross-call state use explicit, server-minted handles passed as ordinary tool arguments" (SEP-2567).
- Removes the `initialize` handshake. Every request carries its protocol version and client capabilities in `_meta` (SEP-2575).
- Adds a mandatory `server/discover` call.
- `subscriptions/listen` replaces the GET stream and `resources/subscribe`.
- Removes `ping` and `logging/setLevel`.
- Moves Tasks into the official extension `io.modelcontextprotocol/tasks`, with polling via `tasks/get` and a new `tasks/update` (SEP-2663).
- Introduces **Multi Round-Trip Requests**: servers return `resultType: "input_required"` with `inputRequests`, replacing server-initiated elicitation and sampling requests (SEP-2322).
- Removes SSE resumability.
- Adds an `extensions` capability.
- Adds OpenTelemetry trace context in `_meta`.
- Says servers **SHOULD return tools in deterministic order** to improve LLM prompt-cache hit rates.
- Requires `Mcp-Method`/`Mcp-Name` headers.
- Requires `ttlMs`/`cacheScope` on list and read results.
- Adds RFC 9207 `iss` validation.
- Loosens `inputSchema`/`outputSchema` to any JSON Schema 2020-12 (SEP-2106).
- **Deprecates Roots, Sampling and Logging** (SEP-2577). The suggested migration for Sampling is to "integrate directly with LLM provider APIs".
- **Deprecates Dynamic Client Registration** in favour of CIMD (PR #2858).
- Adopts a 12-month deprecation policy.

**Extensions and MCP Apps**
- Vendor summary of 2026-07-28: extensions get reverse-DNS IDs and their own repositories (SEP-2133). Tasks (SEP-1686) and Apps (SEP-1865: interactive HTML that the host renders in a sandboxed iframe) are official. "Skills over MCP" (SEP-2640) is said to be next. Single vendor source — [Alpic blog](https://alpic.ai/blog/mcp-2026-07-28-makes-the-protocol-stateless-and-the-app-extensions-official)
- MCP Apps was proposed on 2025-11-21 by MCP-UI's creators with Anthropic and OpenAI maintainers. Secondary sources date its production release to January 2026, with extension ID `io.modelcontextprotocol/ui`. The sources disagree on whether "official" means January or July 2026 — [MCP blog: MCP Apps](https://blog.modelcontextprotocol.io/posts/2025-11-21-mcp-apps/); [MCP blog apps tag](https://blog.modelcontextprotocol.io/tags/apps/)

**Tool spec details (2025-11-25 tools page)**
- Annotations exist, but clients **MUST treat tool annotations as untrusted** unless the server is trusted.
- If an `outputSchema` is declared, servers MUST return conforming `structuredContent`, and SHOULD also return the serialised JSON as text.
- Tool execution errors (`isError: true`) carry "actionable feedback that language models can use to self-correct". Protocol errors are for malformed requests.
- "There SHOULD always be a human in the loop with the ability to deny tool invocations."
- Servers MUST validate inputs, enforce access control, **rate limit tool invocations** and sanitise outputs. Clients SHOULD confirm sensitive operations and log tool use for audit.
- Tool names: 1–128 characters from `[A-Za-z0-9_.-]`.

— [MCP spec: tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)

**Anthropic tool-design guidance** — [Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)
- Consolidate around workflows instead of thin API wrappers; too many overlapping tools mislead agents.
- Namespace tools by service and resource.
- Return high-signal fields and names rather than UUIDs and MIME types.
- Offer a `response_format` of `concise`/`detailed` (concise used about a third of the tokens).
- Paginate, filter and truncate with sensible defaults. Claude Code caps tool responses at **25,000 tokens** by default. When truncating, tell the agent how to narrow its next query.
- Write actionable error messages with examples of correct input.
- Use unambiguous parameter names (`user_id`), strict schemas, and annotations for destructive or open-world tools.
- Evaluate on realistic multi-call tasks.

**Code execution with MCP (Anthropic, 2025-11-04)** — [Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)
- Loading every tool definition and routing intermediate results through the model costs heavily (e.g. a 2-hour transcript adds about 50k tokens).
- Exposing MCP tools as code APIs in a sandbox cut one workflow from about **150,000 to about 2,000 tokens (−98.7%)**. Data can be filtered in code (5 of 10,000 rows shown), `search_tools` gives progressive disclosure, and PII can be tokenised so it never enters the model context.
- This requires secure sandboxing. Cloudflare calls the same idea "Code Mode".

**A2A, governance, llms.txt, WebMCP**
- A2A was donated to the Linux Foundation in June 2025 — [PureAI](https://pureai.com/Articles/2025/06/23/Linux-Foundation-Adopts-Google-Backed-Protocol-for-AI-Agent-Interoperability.aspx). By April 2026 it had reached v1.0 with more than 150 supporting organisations and production use in Azure AI Foundry and Amazon Bedrock AgentCore. The Linux Foundation describes MCP as complementary (tools vs agent-to-agent) — [Linux Foundation press release](https://linuxfoundation.org/press/a2a-protocol-surpasses-150-organizations-lands-in-major-cloud-platforms-and-sees-enterprise-production-use-in-first-year)
- MCP sits in the Agentic AI Foundation, created December 2025 with MCP, Goose and AGENTS.md. Secondary — [FlowHunt](https://www.flowhunt.io/de/blog/agentic-ai-foundation-a2a-mcp-standards/)
- `llms.txt` in practice: both developers.notion.com and modelcontextprotocol.io publish an llms.txt documentation index, which their doc pages point agents to — [Notion llms.txt](https://developers.notion.com/llms.txt); [MCP changelog page header](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
- WebMCP is a W3C Web Machine Learning Community Group **draft report, not on the Standards Track**. The API reportedly moved from `navigator.modelContext` to `document.modelContext` (`registerTool`, `getTools`, `executeTool`) in an October 2026 draft. Chrome origin-trial and deprecation dates conflict across sources — [Daniel Vaughan, 2026-10-15](https://codex.danielvaughan.com/2026/10/15/webmcp-browser-native-model-context-protocol-navigator-modelcontext-agent-web/); [StudioMeyer reality check, May 2026](https://studiomeyer.io/en/blog/webmcp-reality-check-may-2026); [MCP Playground](https://mcpplaygroundonline.com/blog/what-is-webmcp)
- Notion also uses MCP **resources for long format specs**: the view DSL (`notion://docs/view-dsl-spec`) and page-layout format (`notion://docs/database-page-layouts`) are resources, which keeps tool descriptions short — [Notion MCP databases reference](https://developers.notion.com/guides/mcp/tools-databases-reference.md)

### Inferences
- Target **2026-07-28 semantics** and keep 2025-11-25 compatibility, since many clients lag. The SDKs and gateways (e.g. Alpic) bridge versions.
  - Make every tool call self-contained. Pass `revision`, `cursor` and `run_id` handles as arguments rather than relying on sessions.
  - Use the Tasks extension for bulk imports, duplications and long agent runs.
  - Use MRTR / `input_required` for human approval of destructive actions. On older clients, fall back to a `user_confirmed` argument as Notion does.
- **Optional MCP App** for a diff/suggestion review panel and database pickers. It degrades gracefully because hosts opt in.
- Don't build on MCP Sampling for built-in AI (deprecated). The workspace's own provider-agnostic LLM layer should call providers directly (§7).
- Exposure surface:
  - MCP (agents)
  - REST + OpenAPI (deterministic integrations, and the source of tool schemas, as Notion does with OpenAPI→Zod)
  - webhooks/events (automations)
  - `llms.txt` plus Markdown docs
  - A2A Agent Card only if workspace agents should accept delegated tasks from external agent platforms
  - WebMCP: wait.
- For bulk work, consider a "code mode" endpoint (a sandboxed TypeScript/Python runtime with a typed SDK of the same operations). It saves a lot of tokens for multi-hundred-row operations but needs real sandboxing.

### Gaps
- The MCP blog post "Tool Annotations as Risk Vocabulary" appeared in search results but wasn't read.
- The exact official date MCP Apps became "official" is inconsistent across sources.
- "Skills over MCP" (SEP-2640) status rests on a single vendor source.
- WebMCP's Chrome status conflicts across sources.

---

## 6. Agent identity, permissions, audit, and safety (prompt injection, exfiltration, sandboxing)

### Takeaway
Treat each agent as a **distinct principal**: an app or bot identity, optionally acting on behalf of a user. Give it least-privilege, per-resource grants (default none), progressive OAuth scopes, its own rate limits, an audit trail of every tool call and edit, and human approval for destructive or outbound actions. Prompt injection from page content is a demonstrated, exploited risk in Notion. Defend by design: apply the **Rule of Two** / lethal-trifecta analysis to each agent configuration, keep pending AI output inert, enforce CSP and egress allowlists, and never rely on an LLM scanner as the security boundary.

### Cited Findings
- **Linear:** separate `actor=app` identity, mention and delegate scopes, humans keep ownership, admins revoke team access, `PermissionChange` webhooks — [Linear developers: agents](https://linear.app/developers/agents). A read-only endpoint and a read-only OAuth scope are available — [Linear MCP docs](https://linear.app/docs/mcp)
- **Notion Custom Agents:** no access by default, explicit page/database/app grants (linking in instructions doesn't grant), three permission levels on the agent, per-run Activity logs, version history, a web-access toggle, admin model restrictions, and Enterprise audit logs of instruction, permission and integration changes — [Notion help: Custom Agents](https://www.notion.com/help/custom-agent)
- **Notion MCP:** an MCP client inherits everything the user can access. Notion advises treating tool output as untrusted and enabling confirmations. Enterprise can allowlist clients, and the Admin API can revoke connections — [Notion MCP security](https://developers.notion.com/guides/mcp/mcp-security-best-practices.md). Destructive tools require `user_confirmed: true` and carry `destructiveHint` — [Notion MCP pages reference](https://developers.notion.com/guides/mcp/tools-pages-reference.md)
- **MCP security best practices (2025-11-25)** — [MCP security best practices](https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices):
  - **Token passthrough is forbidden.** "MCP servers MUST NOT accept any tokens that were not explicitly issued for the MCP server."
  - Proxy servers MUST implement per-client consent to prevent confused-deputy attacks.
  - "MCP Servers MUST NOT use sessions for authentication."
  - Scope minimisation: a minimal initial scope (e.g. `mcp:tools-basic`), incremental elevation through `WWW-Authenticate scope=` challenges, and logging of elevation events. Avoid omnibus scopes and avoid publishing every scope in `scopes_supported`.
  - Client-side SSRF protections apply too.
- Auth evolution:
  - CIMD recommended and incremental scope consent: [MCP 2025-11-25 changelog](https://modelcontextprotocol.io/specification/2025-11-25/changelog)
  - DCR deprecated, RFC 9207 `iss` validation, credentials bound to their issuer: [MCP 2026-07-28 changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
  - RFC 8707 resource indicators: [MCP 2025-06-18 changelog](https://modelcontextprotocol.io/specification/2025-06-18/changelog)
- **Lethal trifecta / Rule of Two:** Meta's "Agents Rule of Two" (around 2025-10-31) says that until prompt injection can be reliably detected, an agent should combine no more than two of: (A) processing untrustworthy inputs, (B) access to sensitive data, (C) changing state or communicating externally. Willison calls it the best practical advice available and notes it extends his trifecta by adding state change — [Simon Willison, 2025-11-02](https://simonwillison.net/2025/Nov/2/new-prompt-injection-papers/)
- **Real exploits against a Notion-like product:**
  - The web-search exfiltration via PDF injection (Sept 2025) — [Simon Willison](https://simonwillison.net/2025/Sep/19/notion-lethal-trifecta)
  - Image-URL exfiltration through AI edits persisted before approval (disclosed 2026-01-07). PromptArmor recommends:
    - blocking automatic rendering of external images in AI updates without approval
    - a strict CSP
    - making sure the image CDN can't act as an open redirect
    - letting admins turn off AI web search and restrict connectors to sensitive or untrusted data
    - requiring confirmation for AI web requests

    It also notes LLM-based scanners can be talked past — [PromptArmor](https://www.promptarmor.com/resources/notion-ai-unpatched-data-exfiltration)
- Tool annotations are hints, untrusted unless the server is trusted. Servers MUST rate-limit tool calls. Clients SHOULD show tool inputs to users before calling, to avoid exfiltration, and log tool usage for audit — [MCP spec: tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
- Code-execution approaches can tokenise PII so it never reaches the model, but they require sandboxing, resource limits and monitoring — [Anthropic: Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)

### Inferences
- **Principals:** `user`, `agent` (a workspace-owned bot such as a custom or background agent), and `agent-on-behalf-of-user` (an external MCP client acting under OAuth). Effective permission = intersection of the agent's grants, the user's permissions (if on-behalf-of), and the token's scopes.
- **Grants** go on teamspace, page subtree and database, at levels `read | comment/suggest | edit | full`. **Default none** for workspace agents. "Suggest" is a first-class level: agent writes become tracked changes only.
- **OAuth scopes** (progressive): `content:read`, `content:suggest`, `content:write`, `db:read`, `db:write`, `schema:write`, `comments:write`, `trash:write`, `members:read`, `outbound:web`. Ask for elevation through `WWW-Authenticate` when a write tool is first used. Offer a separate `/mcp/readonly` endpoint, as Linear does.
- **Audit log:** every tool call, with principal, on-behalf-of, client ID, tool, arguments hash, affected object IDs, revision before and after, run ID, model, and decision (auto, confirmed, denied). Link CRDT transactions to audit entries via the origin.
- **Trifecta enforcement per agent configuration:** if an agent reads untrusted sources (connectors, inbound email, public web, uploaded files) *and* has private-data access, then disable outbound channels (web fetch, sending email or Slack, rendering external URLs) or require human approval for each outbound action. Show this as a configuration-time warning.
- **Content-level defences:**
  - Mark untrusted spans when sending them to the model (provenance tags in tool output, e.g. `<untrusted source="upload:resume.pdf">`).
  - Strip or neutralise external image and embed URLs in AI-authored pending content.
  - Enforce CSP on rendered pages.
  - Use URL allowlists for agent-created links.
  - Never put a URL-fetching capability inside a "search" tool.
- **Rate limits:** per agent and per token, separate from humans, plus CRDT write backpressure (§4) and per-run budgets (tool calls, tokens, wall clock).

### Gaps
- I didn't read Meta's primary post. Its details come through Willison.
- I found no public data on how well "spotlighting" or provenance tagging works against injection in document workspaces.
- Notion's per-tool OAuth scopes for MCP aren't documented in what I read. Notion's MCP grants roughly the user's full access.

---

## 7. Built-in AI in the product (provider-agnostic LLM layer, embeddings plus hybrid search, Q&A with citations, AI-filled properties, background agents, event-triggered automations)

### Takeaway
Open-source competitors support local models (AppFlowy via Ollama) or an OpenAI-compatible base URL (AFFiNE, with caveats). Notion lets admins choose models per agent type. Retrieval should be **hybrid** (BM25 plus embeddings plus reranking, with contextual chunk enrichment). For small corpora, skip RAG and put the whole knowledge base in context with prompt caching. Background agents should be **event-triggered with explicit grants and run logs**, as with Notion Custom Agents.

### Cited Findings
- AppFlowy runs local AI through Ollama (`ollama serve` must run); it is disabled by default and enabled in desktop settings — [AppFlowy docs](https://docs.appflowy.io/docs/appflowy/product/appflowy-ai-ollama). AppFlowy's blog promotes a workspace "powered by local AI" — [AppFlowy blog](https://appflowy.com/blog/A-Simple-Way-to-Build-a-Workspace-Powered-by-Local-AI)
- AFFiNE self-hosting uses a custom OpenAI base URL, with an `oldApiStyle` flag (v0.25+) for `/chat/completions`. Non-official APIs and models aren't guaranteed — [AFFiNE self-host AI](https://affine-docs.pages.dev/self-host-affine/administer/ai)
- Notion Custom Agents can use a specific model or "Auto". Admins can restrict models, and agents on a disabled model fall back to the default. Notion notes that Anthropic may retain prompts and responses for a period for some models — [Notion help: Custom Agents](https://www.notion.com/help/custom-agent). From 2026-09-09, owners can reportedly set separate models for Notion Agent and Custom Agents. Secondary — [Matthias Frank](https://matthiasfrank.de/en/notion-custom-agents/)
- Custom Agent triggers: recurring schedules; Notion events (comment added, page added or removed in a database, property updated, meeting note finished) with filters; Slack events — [Notion help: Custom Agents](https://www.notion.com/help/custom-agent)
- Notion's MCP `search` is semantic and question-based, spans the workspace plus 10+ connected apps, and has a simpler keyword path without Notion AI — [Notion blog](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look)
- **Contextual Retrieval (Anthropic, 2024-09-19):** with top-20 retrieval, contextual embeddings cut retrieval failures by 35% (5.7% → 3.7%), adding contextual BM25 cut them by **49%** (→ 2.9%), and adding reranking (top 150 → 20) cut them by **67%** (→ 1.9%). Context generation costs about $1.02 per million document tokens with prompt caching. **For knowledge bases under about 200k tokens, skip RAG** and put everything in the prompt, using caching (more than 2× lower latency, up to 90% lower cost) — [Anthropic: Contextual Retrieval](https://www.anthropic.com/news/contextual-retrieval)
- MCP Sampling is deprecated in 2026-07-28, with guidance to "integrate directly with LLM provider APIs instead" — [MCP changelog 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
- The community Outline MCP adds RAG `ask_wiki` (an OpenAI key is optional) — [outline-smart-mcp](https://lobehub.com/en/mcp/huiseo-outline-smart-mcp)

### Inferences
- **LLM gateway:** one internal interface with adapters for (1) OpenAI-compatible Chat Completions/Responses, covering OpenAI, Ollama, vLLM, LM Studio, OpenRouter and similar; (2) the native Anthropic Messages API, for prompt caching and tool features; (3) optionally Google. Make capabilities explicit per model (tool calling, JSON schema output, context length, vision) and degrade features when they're missing. AFFiNE's "some features may not work" caveat shows why capability flags matter.
- **Admin policy per workspace:** which providers and models are allowed, where data may go (local-only mode), retention notices, and per-agent model selection with a default.
- **Search:** Postgres full-text or BM25 (e.g. Tantivy/pg_search) plus pgvector embeddings, with reciprocal-rank fusion and an optional reranker. Chunk at **block granularity** with page-title and breadcrumb context prepended (a cheap form of contextual retrieval). Every hit carries `page_id + block anchor`, so answers can cite and deep-link to blocks. Enforce permissions at query time with an ACL filter in SQL, never after retrieval.
- **Q&A with citations:** return answer spans that reference block anchors. The UI renders footnotes that scroll to the block. For small workspaces or scoped questions, load full pages instead of chunks.
- **AI-filled database properties:** model them as a property type `ai` with a prompt template over other columns, run by background jobs, clearly marked "AI-generated" with refresh or lock controls, and written through the same agent principal and audit path.
- **Background agents and automations:** an event bus (page created/updated, property changed, comment added, schedule, webhook) triggers agent runs with a stored config (instructions, grants, model, write mode). Each run is logged (trigger, tool calls, diffs) and revertible by run ID.

### Gaps
- Not researched: Notion AI autofill / Airtable AI fields as precedents for AI-filled properties, details of Ollama's OpenAI-compatible API, and embedding-model choices or multilingual (French) retrieval quality.
- I found no source quantifying hybrid search for block-structured notes specifically.

---

## 8. Structured data for agents (querying and updating database rows, schema discovery, validation errors)

### Takeaway
Notion's 2026 MCP gives agents **three query modes**: JSON filter "rows", saved "view", and **read-only SQL (SQLite dialect) across up to 10 data sources with bound `?` parameters**. Rich text inside rows is returned as Markdown, results are keyed by property name, limits are small (default 50, max 100), and mentions and names must be resolved to IDs. Schema updates follow "list every option you want to keep" semantics. A workspace should offer a compact typed schema, JSON filters, an optional read-only SQL view, and validation errors that list allowed values.

### Cited Findings
- **`notion-query-data-sources`** — [Notion MCP databases reference](https://developers.notion.com/guides/mcp/tools-databases-reference.md):
  - Modes are **SQL, rows and view**.
  - Rows mode takes `data_source_url`, an optional `filter` with **two levels of and/or**, and `sort`. `limit` defaults to **50, max 100**. Responses have `results` keyed by property name and `has_more`, with no cursor in rows mode.
  - **Rich-text properties come back as Notion Markdown strings.**
  - A query can resolve at most 1,000 mention targets; exceeding that is an error, not partial output.
  - SQL output may drop formatting or link targets, so read via rows mode or fetch before modifying.
- **`notion-query-multiple-data-sources`**: a **read-only SQLite query across 2–10 data sources**. Each `collection://` URL is a quoted table name, and untrusted values are bound with `?` placeholders. Saved-view filters don't apply. Results include a truncation flag. Requires Business/Enterprise with Notion AI — same source.
- **`notion-update-data-source`**: for select, multi-select and status properties, *list every option to keep*. Omitted options are removed, and existing ones keep their IDs. Don't change properties and `page_layout` in the same call (`validation_error`). The page-layout format lives in the resource `notion://docs/database-page-layouts` — same source.
- **`notion-create-view` / `notion-update-view`**: 10 view types (table, board, list, calendar, timeline, gallery, form, chart, map, dashboard). Configuration uses a **view DSL** documented in the resource `notion://docs/view-dsl-spec`. Filter values must be IDs or URLs ("Page and people names aren't accepted; resolve them to URLs or IDs first"), with `"me"` allowed for person filters — same source.
- **`notion-delete-databases`**: requires `user_confirmed: true`. Without it, the error lists the databases and their **row counts**. Marked `destructiveHint: true` — same source.
- Notion REST data-source queries have their own ~3 rps limit — [Notion request limits](https://developers.notion.com/reference/request-limits.md)
- MCP: input validation errors should be Tool Execution Errors so the model can self-correct (SEP-1303) — [MCP changelog 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/changelog). `outputSchema` plus `structuredContent` support typed results — [MCP spec: tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
- Anthropic: actionable errors with examples of correct input, names over opaque IDs, and `response_format` concise vs detailed — [Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)
- Code execution lets agents filter large tables in a sandbox (5 of 10,000 rows reach the model) — [Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)

### Inferences
- **Schema discovery:** `get_database` returns a compact typed schema. For example, a TypeScript-like or SQL-DDL-like block: `status: select("Todo"|"Doing"|"Done")`, `owner: person`, `due: date`, `estimate: number(min 0)`, `tags: multi_select(...)`, `parent: relation(→Projects)`. Include row count and default view, keyed by property *names* with stable IDs available.
- **Querying:**
  - (1) a JSON filter (`{and:[{prop:"Status",eq:"Done"},{prop:"Due",before:"2026-12-01"}]}`), limited to two nesting levels, with sorts, `limit` ≤100 and a cursor (fixing Notion's no-cursor rows mode)
  - (2) read-only SQL over a virtual view (SQLite or Postgres dialect, one view per database, bound parameters, row and time limits)
  - (3) "run saved view"

  Return rows keyed by property name, rich text as Markdown, and relations and people as `{id, name}` pairs.
- **Writing:** `upsert_rows` with an array of `{row_id?, properties:{Name:…}}`, validated against the schema. Return **per-row results** (some succeed, others fail with reasons) and errors such as `"Status" must be one of Todo|Doing|Done (got "In progress")`. Accept people and relation values by name *or* ID: resolve names server-side when unambiguous, and otherwise return candidates in the error. This removes Notion's "resolve names first" round trip.
- **Schema changes** are a separate higher-privilege tool (`schema:write`) with explicit add, remove and rename option operations rather than "list everything to keep", to prevent accidental option deletion.

### Gaps
- I didn't find the full Notion property-value format for row writes in MCP: create-pages with a database parent is mentioned, but its property syntax wasn't in the pages I read.
- I found no evaluation comparing SQL with JSON filters for LLM query accuracy on workspace databases.

---

## 9. Recommended "agent interface" design for a self-hosted, open-source Notion-like workspace (synthesis)

### Takeaway
Build the agent interface as **Workspace Markdown plus targeted edits plus a stateless MCP server, with agents as least-privileged principals whose writes are CRDT transactions, in suggestion mode by default**. About 14 consolidated tools, specs exposed as MCP resources, OAuth 2.1/CIMD with progressive scopes, audit and per-run undo, and hybrid search with block-level citations.

### Cited Findings
The design is synthesised from the sources above. The decisive inputs:
- Notion's move from 1:1 REST tools and block JSON to Markdown and str_replace — [Notion blog](https://www.notion.com/blog/notions-hosted-mcp-server-an-inside-look); [Update page markdown](https://developers.notion.com/reference/update-page-markdown.md)
- Edit-format evidence: unified diffs and plain text beat JSON, anchors with stale detection help many models, and no format dominates — [Aider](https://aider.chat/docs/unified-diffs.html); [Harness problem](https://stencil.so/blog/the-harness-problem); [Diff-XYZ](https://arxiv.org/html/2510.12487v2); [nwyin replication](https://dev.to/nwyin/hashline-vs-replace-does-the-edit-format-matter-15n2)
- Concurrency precedents: Google's reject vs merge revisions, and Yjs agent-peer patterns — [Google Docs best practices](https://developers.google.com/workspace/docs/api/how-tos/best-practices); [Yjs agent article](https://dev.to/norfolkd/when-an-ai-agent-joins-your-yjs-room-three-assumptions-break-50h8)
- Suggestion and branch precedents — [Tiptap](https://tiptap.dev/docs/content-ai/capabilities/ai-toolkit/workflows/tiptap-edit); [Ink & Switch](https://www.inkandswitch.com/patchwork/notebook/07/)
- Current MCP spec — [MCP 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/changelog)
- Tool-design guidance — [Anthropic](https://www.anthropic.com/engineering/writing-tools-for-agents)
- Identity precedents — [Linear agents](https://linear.app/developers/agents); [Notion Custom Agents](https://www.notion.com/help/custom-agent)
- Security incidents and frameworks — [PromptArmor](https://www.promptarmor.com/resources/notion-ai-unpatched-data-exfiltration); [Rule of Two](https://simonwillison.net/2025/Nov/2/new-prompt-injection-papers/); [MCP security best practices](https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices)
- Retrieval — [Contextual Retrieval](https://www.anthropic.com/news/contextual-retrieval)

### Inferences
**A. Content format: "Workspace Markdown" (WMD v1)**
- CommonMark + GFM (tables, task lists, strikethrough) plus `$math$`.
- Non-Markdown blocks as XML-like tags with attributes, aligned with NFM names to ease migration and model familiarity: `<callout icon color>`, `<toggle>`, `<columns>/<column ratio>`, `<page id title>`, `<database id view>`, `<mention type id>Label</mention>`, `<embed url>`, `<synced ref>`, `<span color>`.
- **Lossless escape hatch:** `<block type="x" id="…" data="…"/>` for anything without a mapping. Agents must leave it untouched, and deleting it requires `allow_deleting_content`.
- **Anchors, opt-in on read** (`anchors: "none" | "block"`): a short page-local alias at block end (`^b12`) or as an `id` attribute on tags. The server maps alias → (block UUID, content hash at read time). Lines stay human-readable.
- Every read returns `revision` (an opaque handle derived from the CRDT state vector), `truncated`, and `unloaded[]` (placeholders such as `<unloaded id=… />`) for very large pages.
- A round-trip property test suite (tree → WMD → tree) is a release gate.

**B. Edit operations (one tool, several operation types, batch ≤100, atomic per call unless `partial_ok`)**
- `str_replace {old, new, replace_all?}`: the default for strong models. The match must be unique; on failure, return the closest candidates.
- `replace_block {anchor, markdown}`, `insert {after|before: anchor | position: start|end, markdown}`, `delete_block {anchor}`, `move_block {anchor, after: anchor}`: anchor operations for precision and for weaker or local models.
- `replace_page {markdown}`: only for new or small pages. Child pages and databases are protected unless `allow_deleting_content: true`. Refused with `409` while humans are actively editing.
- `set_properties {…}` for page properties (§E rules).
- Every write takes `base_revision`. With `on_conflict: "rebase"` (default), the server rebases automatically if the touched blocks are unchanged and otherwise returns a conflict error with fresh snippets. With `"fail"`, any change rejects the write (Google `requiredRevisionId` semantics).
- The response carries the **post-edit Markdown of touched blocks** (or the full page if small), a new `revision`, `warnings[]` (`no_match`, `ambiguous_match`, `stale_anchor`), and the `suggestion_ids` created.

**C. Merge strategy with live collaboration**
- The MCP server runs agent writes as a **server-side CRDT peer** (Yjs/Loro), one transaction per tool call, `origin = {agent_id, run_id, on_behalf_of}`. Operations resolve to block IDs and relative positions, so concurrent human edits elsewhere merge cleanly.
- **Write modes:** `suggest` (default for workspace or background agents and for "suggest"-level grants; edits stored as tracked-change marks that collaborators see, with accept/reject per suggestion or per run), `direct` (interactive agent with edit rights; still attributed and undoable per run), and `branch` (fork for big rewrites, diff review, merge).
- **Pending AI content is inert**: no external image or embed loading, no unfurls, no mentions or notifications, and no automation triggers until accepted.
- Per-agent UndoManager / "revert run" (all transactions with that `run_id`). Visible attribution (AI badge, blame, history entries naming agent, model and user).
- Backpressure: commit at block granularity, token bucket per agent origin, presence coalesced about every 300 ms with `type: agent`.

**D. MCP server surface (target 2026-07-28, compatible with 2025-11-25)**
- Transport and auth:
  - stateless Streamable HTTP at `/mcp` plus `/mcp/readonly`
  - OAuth 2.1 with protected resource metadata (RFC 9728), CIMD for client registration (DCR only for legacy), RFC 8707 audience binding, and incremental scopes via `WWW-Authenticate`
  - personal access tokens for local models and CLI use (stdio bridge)
  - no token passthrough
- **Tools (≈14, namespaced, deterministic order, annotated):**
  1. `search` (hybrid; filters: teamspace, type, date, author; returns titles, breadcrumbs, block-anchored snippets; `response_format`)
  2. `fetch` (page, database or block by ID or URL → WMD plus properties, `revision`, anchors option, paging for long pages)
  3. `create_pages` (batch; parent; WMD content; properties; `allow_async`)
  4. `update_page` (operations from §B plus properties; `mode: suggest|direct`)
  5. `manage_pages` (move, duplicate, trash, restore; trash requires confirmation via MRTR, or `user_confirmed` on older clients)
  6. `query_database` (rows / view / read-only SQL; cursor; `limit` ≤100)
  7. `upsert_rows` (batch, per-row validation results)
  8. `get_database` (typed schema, views, counts)
  9. `update_database_schema` (add, remove, rename properties and options; higher scope)
  10. `manage_views`
  11. `comments` (list, create, resolve; anchored to blocks)
  12. `suggestions` (list, accept, reject, by run or ID; human-only accept unless the policy allows)
  13. `workspace_info` (self, members, teamspaces; name ↔ ID resolution)
  14. `get_task` (or the Tasks extension) for async work.
- **Resources:** `workspace://docs/wmd-spec`, `…/filter-spec`, `…/sql-views` (schema of SQL virtual tables), `…/view-dsl`. Specs live in resources, not bloated tool descriptions, following Notion's pattern.
- **Prompts:** a few curated workflows (e.g. "summarise page with citations", "triage database").
- **Optional:** an MCP App for diff and suggestion review, and a sandboxed "code mode" SDK for bulk operations.
- **Output hygiene:** default responses under a few thousand tokens. Hard cap well under 25k tokens with a truncation notice explaining how to narrow the query. Names instead of UUIDs wherever possible. `isError` tool errors with fix instructions and examples. Structured rate-limit errors with `retry_after_seconds`.
- **Also expose:** REST + OpenAPI (the same operations; generate MCP schemas from it), webhooks/events, `llms.txt` plus Markdown docs. Add an A2A Agent Card only if external agents need to delegate tasks to workspace agents. Skip WebMCP until it stabilises.

**E. Identity and permission model**
- **Principals:** human users; **workspace agents** (bot members, not billable seats, created by admins or permitted users, **zero access by default**); external MCP clients acting on behalf of a user (the token's scopes ∩ the user's permissions).
- **Grants:** on teamspace, page subtree and database, at levels `read < suggest < edit < full`. An agent's effective access = its grants ∩ (on-behalf-of user's permissions, if any) ∩ token scopes.
- Linear-style **delegation**: rows and pages can have a human `owner` plus an AI `delegate`, so humans keep accountability.
- **Policies per agent:** allowed models, write mode, outbound channels (web, email, Slack), max tool calls, tokens and time per run, rate limit. A configuration-time **Rule-of-Two check** warns or blocks when untrusted input, private data and outbound/state-change capabilities are combined without approval gates.
- **Approvals:** destructive operations (trash, schema delete, permission changes, bulk edits above N blocks or rows) and all outbound sends require human confirmation (MRTR `input_required`, a UI prompt, or the suggestion queue).
- **Audit:** an immutable log of every tool call and resulting transaction (principal, on-behalf-of, client, model, run, objects, revisions, decision), exportable, with per-run revert.
- **Injection hygiene:** provenance-tag untrusted content in tool outputs, set a CSP and egress allowlist for rendered AI content, never let search fetch arbitrary URLs, and don't treat an LLM classifier as the security boundary.

**F. Built-in AI**
- A provider-agnostic gateway (OpenAI-compatible incl. Ollama/vLLM, native Anthropic, others) with capability flags and an admin allowlist, including a local-only mode.
- Hybrid search: BM25 + vectors + rerank, block-level chunks with breadcrumb context, ACL filtering in the query.
- Q&A with block-anchored citations, and full-context mode for small scopes.
- AI properties as background jobs through the agent principal.
- Event-triggered background agents with run logs and reverts.
- Workspace AI and external MCP agents share the same tool layer, so permissions, audit and merge behave identically.

### Gaps
- This design is a synthesis and hasn't been validated by benchmarks on document editing. Before freezing WMD v1, run an internal eval (e.g. 100–200 realistic editing tasks across Claude, GPT and 2–3 Ollama models) comparing str_replace, anchor operations and whole-block replace on WMD, and measure round-trip fidelity.
- The cost of conflict detection based on CRDT state vectors and the UX of suggestion marks in Yjs/Loro need prototyping.
