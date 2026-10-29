# MCP parity: vsv2-mcp vs the Xero MCP vs Visma's official AI Connect MCP

Written 2026-10-09, updated the same day after the skills pass (see "Skills" below) and again 2026-10-14 (API reference for three APIs, create and hold/release on SalesOrders v3, per-person sign-in). Three servers, compared capability by capability.

**What was actually inspected** (so you know what is fact and what is inference):

| Server | Source of this comparison | Confidence |
|---|---|---|
| Xero MCP (`apps/xero_service/xero_service_v2/core/crates/xero-mcp`) | `docs/XERO_MCP.md` (626 lines), `XERO_ENDPOINT_PARITY.md`, crate layout | High: read the design doc and file list; did not run it |
| Visma official "AI Connect (MCP) in Visma Net and Business NXT" | The customer-portal article (2026-06-22, edited 2026-09-21) **and the connector itself, called live in this session**: its 11 tools (`init`, `list_skills`, `get_skill`, `introspect`, `execute_query`, `execute_mutation`, `search_tool`, `get_attachment`, `render_chart`, `get_design_guidelines`, `submit_feedback`), its 55 skills and their structure, its identity and its mutation rules | High for what it exposes and how it behaves; its GraphQL schema was not compared field by field against our 96 REST endpoints |
| `vsv2-mcp` (this repo) | The code and tests | High |

Legend: ✅ parity or better · ◐ partial · ❌ gap · ➖ not applicable / deliberately different.

## Verdict in one paragraph

On **reading and analysing data** `vsv2-mcp` now matches the Xero MCP's contract (provenance, large-result
handles, discovery, overview, prompts) and goes beyond both others for analytics (full read-only SQL over the BigQuery
warehouse, sync-freshness). On **writing** it is narrower than the official Visma MCP (one operation versus "any
mutation"), though its guardrails are at least as strict. The big structural gaps are not features but **identity and
hosting**: the official Visma MCP acts as the *signed-in user* from a hosted endpoint; ours runs locally, on a *service*
identity unless you sign in, and is stdio-only. Documentation search, attachments and chart rendering exist only in the official one.

## Capability matrix

### Reading data

| Capability | Xero MCP | Visma official | vsv2-mcp | |
|---|---|---|---|---|
| Live reads of ERP data | 148 catalogued GETs, each a tool, plus generic `xero_query` | GraphQL `execute_query`, ~100 rows/page, up to 1000 with `first`; `totalCount` + `pageInfo` | `visma_erp_query` over **96** catalogued ERP REST endpoints, `visma_salesorder_get` (with ETag), DataMart GraphQL (`visma_datamart_query`, 38 endpoints) | ◐ Different surface. We cannot match the official GraphQL schema's breadth without a live comparison (needs the connector authorised). |
| Discovery of what exists / parameters | `xero_list_operations` (params, scopes, sensitivity) | `init` (API reference) + `introspect` | `visma_list_endpoints` (+ `endpoint=` detail: paging, defaults, required params, DataMart mapping), `visma_datamart_schema` | ✅ `visma_search_api` searches every operation and query parameter of the Finance, SalesOrders v3 and JournalTx v1 APIs offline, and `visma_erp_query` refuses filter names the reference does not declare for whichever API the endpoint is in. The embedded reference is derived from Visma's published documents (2026-10-14). |
| Multi-tenant selection | `xero_list_connections` | implicit: the user's tenant | `visma_list_tenants`, `tenant` argument on every tool | ✅ |
| Provenance on every result | `meta`: tenant, operation, fetch time, status, spec commit, hash, coverage flags | `pagination` note when partial | `meta`: source, fetched_at, tenant, endpoint/path, `coverage`, `complete_collection:false` | ✅ new |
| "One page is not the whole collection; failed/empty is unknown, not zero" | Explicit contract | Explicit rule: never present a page as complete | Stated in `meta.note` and in prompts; `maybe_more` on pages | ✅ new |
| Results too big for context | In-memory handles (10 min, 32 entries, 5 MiB) + `xero_read_result` (JSON pointer) | Row cap with paging | Same design: `result_id` + `visma_read_result`; verified live (363 KB BigQuery answer parked, 500 rows paged back in order) | ✅ new |
| Bulk analytics / history | `xero_warehouse_overview`: row counts only | none (live system only) | `warehouse_query` (read-only SQL, typed rows, byte/row/hourly budgets), `warehouse_describe_table`, `warehouse_list_tables`, `warehouse_overview` (volume + last-modified) | ✅ exceeds both |
| Data feed health | n/a | n/a | `visma_sync_status` (age per endpoint, failed runs, 24 h counts) | ✅ unique to us |
| Guided workflows | 6 prompts | 55 skills (`list_skills`/`get_skill`) and design guidelines | 8 MCP prompts plus **23 skills with a per-tool skill list** (see "Skills") that encode this warehouse's known pitfalls | ✅ |
| Documentation search | n/a | `search_tool` over Visma Net procedures/field help/reports | none | ❌ Visma-hosted index; not replicable. Use the official connector alongside. |
| Attachments | binary results as private `xero://results/...` resources (sensitive permission) | `get_attachment` | none | ❌ feasible later (SalesOrder attachments endpoints exist); low priority |
| Charts / dashboards | n/a | `render_chart` | none | ➖ client-side; the model can chart returned rows itself |

### Operations

| Capability | Xero MCP | Visma official | vsv2-mcp | |
|---|---|---|---|---|
| Start data extraction / backfill and track it | `xero_extract` (idempotency key, plan, local/GCS), `xero_get_job`, `xero_cancel_job` | n/a | none. vsv2 already has backfill/schedule-trigger endpoints in `vsv2-http`; not exposed over MCP | ❌ deliberate for now: it spends Visma quota and writes to BigQuery, so it should get the same gating as writes |

### Writing data

| Capability | Xero MCP | Visma official | vsv2-mcp | |
|---|---|---|---|---|
| Breadth | 4 actions (create/update contact, draft invoice, payment); "not full parity" with the upstream Xero MCP | any create/update/processing mutation; examples: create sales order, set order status to On hold | **3** operations on SalesOrders v3: `salesorder.patch` (header/line edits), `salesorder.create` (new orders, always on hold, body checked against Visma's own schema) and `salesorder.set_status` (Open to Hold and back, with the version check) | ◐ Still narrow: no cancel/complete, customer/supplier or JournalTx writes (JournalTx needs the Finance write scope). None has been run against a live write |
| Two-step with preview | prepare → confirm | preview, then confirm | `…_patch_plan` → `…_patch_execute` | ✅ |
| Who approves | **server operator out of band** via CLI, with body digest | the user (native elicitation, or preview + `confirmation_token` for clients without it) | the user via MCP elicitation, **fail-closed**; explicit weaker `permission_prompt` fallback | ◐ We match the official model. We lack Xero's operator-approval mode (stronger, but a different workflow). |
| Deletes | none | forbidden by OAuth scope | none; no DELETE method exists in the type system | ✅ |
| Duplicate / replay protection | server `Idempotency-Key` (6 min window) | n/a | one-shot plans (atomic DB claim), If-Match ETag (never the wildcard), outcome-unknown handling | ✅ |
| Audit | private SQLite approval store | n/a | append-only Postgres table (UPDATE/DELETE/TRUNCATE blocked), written *before* sending | ✅ |

### Access, identity and hosting

| Capability | Xero MCP | Visma official | vsv2-mcp | |
|---|---|---|---|---|
| **Whose authority the tools run with** | per-client token bound to a tenant allow-list + flags (extract / sensitive / warehouse / retention) | **the signed-in user**; "does not grant access beyond what the user already has"; per company, Visma Admin roles **AI Connect Read / AI Connect Read & Write** plus **API User** (Visma Net MCP guide, checked 2026-10-13); one company database per connection | the **integration's service credentials** by default (every holder can read all 12 configured tenants); after `vsv2-mcp login`, **the signed-in person**: one Visma token per company they approved, so Visma applies their roles and only their companies are visible ([`sign-in.md`](sign-in.md)) | ◐ built and tested against a mock Visma Connect; not yet run through the real consent screen. The service identity stays the default until someone signs in. |
| Transport | stdio **and** Streamable HTTP, bearer auth, 120 req/min/client, 64 KiB bodies, host/origin allow-lists | hosted remote endpoint; usable from Claude/Copilot/ChatGPT including mobile | stdio only (local trust) | ❌ |
| End-user sign-in | opt-in Xero OIDC/PKCE bridge issuing MCP-only tokens | Visma OAuth | `vsv2-mcp login`: authorization code + PKCE with a loopback redirect, one consent per company chained in the same browser tab until all are connected, per-company tokens kept owner-only on disk; local only (no hosted bridge) | ◐ |
| Quota behaviour | per-tenant 429 cooldowns, optional shared Redis | server-side | local bucket (30/min), not shared with the scheduler | ◐ |
| Zero-data-retention mode | yes (no handles, no persisted jobs) | n/a | result handles are memory-only and expire; no switch | ◐ minor |

## Skills: what the official server does and what we took from it

The official connector's idea is that a model should not guess: `init` returns the API reference and rules, and
`list_skills` / `get_skill` serve 55 short guides (workflows, field help, "how to do X"), each with a description, tags,
prerequisites and a difficulty. Its rules tell the model to read the relevant skill before acting and to ask the user
before any mutation. We adopted the structure and went further where our situation differs:

| | Official Visma MCP | `vsv2-mcp` |
|---|---|---|
| Skill count and scope | 55, about Visma Net procedures and its GraphQL API | 23, about **this** warehouse and these tools: table and column facts, sign and currency conventions, merged companies, verified data gaps |
| Link from tool to skill | The model has to search | **Each tool carries its own skill list** (description, `_meta.skills`, error hints); a `primary` skill is read first |
| Entry point | `init` | `visma_init`: capabilities and why anything is off, identity, limits, **known data issues**, skills to read first |
| Are the examples checked | Not checkable from outside | Every `tool-call` example runs through the real tool code in `cargo test`; ignored live tests dry-run all SQL against BigQuery and run the Visma examples live |
| Can a skill rot silently | Possible | A renamed tool, table or argument, a tool without a skill, an invalid front matter or a known issue pointing at a vanished phrase fails the tests |
| Delivery | tool calls | tools, MCP resources (`skill://vsv2/<id>`), prompts that name the skills, and `--print-manifest` |

Taken from the **Xero MCP** in the same pass: provenance on every result, result handles, `visma_init` as the
`xero_list_connections`-style "what can I do here" call, per-tool discovery, and the prompt library. New and not in
either server: `filter_check` (Visma silently ignores unknown filter names; the tool compares totals with and without
the filters and says whether they did anything) and the dated, tested `known_issues` list.

## What this means

1. **Read-side parity with Xero: achieved** (rows above marked "new" were built in this pass and verified against live
   Visma and BigQuery). We also have capabilities neither other server has.
2. **Parity with the official Visma MCP is partial by nature.** It is a hosted, user-identity GraphQL gateway; we are a
   warehouse-plus-REST toolkit. They are complementary: the official one for "what is true right now, as me, anything I
   may see", ours for history, analytics, freshness and a narrow audited write path. Using both is reasonable.
3. **Do not widen the audience of this MCP without signing in.** With service credentials, anything that can call it
   can read every tenant. Signing in (`vsv2-mcp login`) makes it act as one person with that person's permissions; a
   hosted or shared deployment still needs its own authentication, which is not built.

## Roadmap, in order of value

Done in the skills pass: per-tool skill lists, 19 skills with tested examples (23 now), `visma_init`, known issues,
`filter_check`, skill resources, `--print-manifest`, two new prompts (`sales_overview`, `period_end_review`), live
accuracy tests. Done since: `visma_search_api` (offline API reference of three APIs), refusal of filter names the reference does not declare, `visma_submit_feedback`, `salesorder.create` (on hold), `salesorder.set_status` (hold/release) and the per-person sign-in (`vsv2-mcp login`). Still open:

1. **Write breadth** on the existing Layer 3 pipeline: create and hold/release are done (SalesOrders v3). Still open:
   customer/supplier edits and JournalTx vouchers (the Finance write scope). Each is one file plus tests, and a skill
   per operation. First real run of any write: a test company.
2. **Confirmation-token fallback** for clients without elicitation (official-server style) as a third approval mode,
   and optionally Xero-style **operator approval** via a CLI.
3. **Per-endpoint filter discovery**: done for the Finance, SalesOrders v3 and JournalTx v1 APIs (embedded reference;
   unknown names are refused with the valid list). Endpoints outside those three are still only flagged afterwards.
4. **Repair the data gaps the known-issues list names** (stale open invoices from the 2026-03-25..04-15 sync gap, the
   one-sided ledger batches, the half-flagged merger move). The list shrinks as they are fixed.
5. **Hosted/multi-user mode**: Streamable HTTP, per-client tenant allow-lists and permission flags (the Xero design is
   the template). The per-person sign-in and the audit identity exist for a local server; a hosted one needs them
   behind its own authentication. Prerequisite for any wider rollout.
6. **Extraction/backfill tools** (`start`, `status`, `cancel`) behind the same gating as writes.
7. Attachments (the spec's attachment responses are untyped, so they need a live look first); a sensitivity flag for
   personal-data endpoints (Xero's `sensitive` permission); ZDR switch; shared quota with the scheduler. (`submit_feedback`
   is done: `visma_submit_feedback`.)
