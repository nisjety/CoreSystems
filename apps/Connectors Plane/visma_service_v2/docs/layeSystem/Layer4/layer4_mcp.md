# Layer 4 — MCP server (`vsv2-mcp`)

An MCP (Model Context Protocol) server that lets an AI assistant work with the Aquatiq data
warehouse (BigQuery) and Visma itself, on top of Layers 1-3. Transport: stdio. Crate:
`core/crates/vsv2-mcp`, built on the official Rust SDK `rmcp` 3.5.

## Tools

Everything is **read-only** unless the write tools are explicitly enabled (see below).

| Tool | Kind | What it does |
|---|---|---|
| `visma_init` | read | **Call first.** What this instance can do now (and why anything is disabled), whose authority it uses, limits, the **known data issues**, and which skills to read first. Local only. |
| `visma_list_skills` | read | The bundled usage skills, filtered by `tool` (the skill to read first comes first), `tag` or free-text `query`. |
| `visma_get_skill` | read | One skill in full, by `id` or best-matching `query`: conventions, pitfalls and worked, tested examples. |
| `visma_list_endpoints` | read | Catalog of synced endpoints (source API, BigQuery table, key, tier); `endpoint=` returns one endpoint's full detail (paging, defaults, required parameters, DataMart mapping). Local config only. |
| `visma_read_result` | read | Pages through a result that was too large to return (`result_truncated` + `result_id`), by JSON pointer / offset / limit. Handles live in memory for 10 minutes. |
| `visma_list_tenants` | read | Tenants this server may read: the configured ones, or, once you have signed in (`vsv2-mcp login`), the companies you connected (id, name, status, DataMart company id). |
| `visma_sync_status` | read | Freshness of the feed: last sync + age per endpoint, recent runs/errors, 24h run counts. |
| `visma_erp_query` | read | Live GET against a catalog ERP REST endpoint: a page (`page` is 1-based for every API) or one record by `key`. A filter name the API reference does not declare for the endpoint is **refused** (Visma would ignore it). For an endpoint the reference does not cover, a first page with filters also carries **`filter_check`** (total with and without the filters: `narrowed`, `no_effect_detected` or `unknown`). |
| `visma_salesorder_get` | read | One sales order (SalesOrder V3) with its current ETag. |
| `visma_datamart_schema` | read | Explore the DataMart GraphQL schema (root fields, a type's fields). Service credentials only. |
| `visma_datamart_query` | read | Read-only GraphQL against DataMart (mutations/subscriptions refused). Service credentials only. |
| `warehouse_list_tables` | read | Readable BigQuery datasets, or the tables of one dataset. |
| `warehouse_describe_table` | read | Columns (incl. nested), types, descriptions, row count/size. |
| `warehouse_overview` | read | Per dataset: table/view count, total rows, size, newest modification (metadata only). With `dataset=`: every table. |
| `warehouse_query` | read | One SELECT/WITH query, typed results, row and byte limits. |
| `visma_salesorder_patch_plan` | plan | **Write step 1.** Builds the exact change (with fresh ETag), audits it, returns `plan` + `confirm_token`. Changes nothing in Visma. |
| `visma_salesorder_patch_execute` | write | **Write step 2.** Applies a reviewed plan **after the user approves it in their client** (MCP elicitation). |
| `visma_salesorder_create_plan` | plan | **Create step 1.** Checks `order` against the SalesOrders v3 request schema and builds the POST to `/api/v3/SalesOrders`. Every order is created **on hold**. Changes nothing in Visma. |
| `visma_salesorder_create_execute` | write | **Create step 2.** Creates the reviewed order **after the user approves it in their client**. The result names the new order (`created_order`: type and number, taken from the `Location` Visma answers with) and carries that `location`. |
| `visma_salesorder_status_plan` | plan | **Hold/release step 1.** Reads the order's status and version and builds the PATCH that puts an `Open` order on hold or releases an order that is `On Hold` (nothing else: no cancel, complete or back-order). Changes nothing in Visma. |
| `visma_salesorder_status_execute` | write | **Hold/release step 2.** Applies the reviewed change **after the user approves it in their client**. |
| `visma_search_api` | read | Offline search of Visma's own API references (the Finance ERP API, SalesOrders v3 and JournalTx v1: 558 operations): the endpoint for a question, its query parameters and the catalog endpoint that reads it. No credentials, no call to Visma. |
| `visma_submit_feedback` | plan | Stores a report (wrong skill, missing skill, wrong result, bug or suggestion) in a local, owner-only file for the operator. Nothing is sent anywhere. |

**Every read result carries `meta`** (source, fetched_at, tenant, endpoint/path, `coverage`, `complete_collection`): one
response is never proof of a complete collection, and an empty or failed read means *unknown*, not zero. A result over
200 KB is parked and returned as a `result_id` with its `shape`; nothing is silently truncated.

**Prompts** (MCP `prompts/list`): `financial_overview`, `sales_overview`, `receivables_payables`,
`project_profitability`, `period_end_review`, `data_freshness_check`, `reconcile_gl_vs_projects`, `edit_sales_order`.
They are guided workflows over these tools that encode this warehouse's known pitfalls (several companies, base
currencies, hashed date keys, the 2025-12-31 project move, three ledgers by design) and end with the skills to read
first (a test checks that every named skill exists). They call nothing and grant nothing; arguments are length- and
control-character-checked.

How this compares with the Xero MCP and Visma's official MCP: see [`parity.md`](parity.md).

MCP annotations: read tools `readOnlyHint=true`; plan tool `readOnlyHint=false, destructiveHint=false`;
execute `destructiveHint=true`.

## Reference data

`visma_search_api` and the parameter check in `visma_erp_query` read `reference/api_reference.json` (every operation of
the Finance ERP API, the SalesOrders v3 API and the JournalTx v1 API, with its query parameter names), and the create
operation reads `vsv2-interactive/schemas/salesorder_v3_create.json` (the request body of `POST /api/v3/SalesOrders`).
Both are derived from Visma's published OpenAPI/Swagger documents by `scripts/build_api_reference.py` and embedded in
the binary:

```bash
python3 -I scripts/build_api_reference.py --erp SPEC --erp-interactive SPEC --salesorder SPEC --journaltx SPEC --retrieved YYYY-MM-DD
```

The four documents are `api.finance.visma.net/API-index/api/vismanet.erp.service.api-apim` and `...interactive.api-apim`,
`salesorder.visma.net/swagger/v3/swagger.json` and `journaltx.visma.net/swagger/v1/swagger.json`. Regenerate whenever one
changes; `cargo test -p vsv2-mcp -p vsv2-interactive` fails if the embedded data cannot be parsed. Retrieved 2026-10-14:
the Finance service and interactive documents have the same 514 operations, and the reference records that together with
the interactive API's authorization-code endpoints and scopes. The portal's "API base URL" `.../erp/interactive` is the
token's audience, not a route (the gateway answers 404 for it): a signed-in person calls the same host and paths.

## Skills (per-tool usage guides)

A **skill** is a short Markdown guide (`core/crates/vsv2-mcp/skills/*.md`, YAML front matter: `id`, `title`,
`description`, `tags`, `tools`, `primary`, `prerequisites`, `difficulty`, `steps`) embedded at build time by
`build.rs`. Inverting each skill's `tools` list gives **every tool its own skill list**, shown three ways: appended to
the tool's description, as `_meta.skills` on the tool, and as a hint in error messages ("see skill `x`"). The skill a
tool names as `primary` is the one to read first (then the more specific skills, so a skill about two tools comes
before one about thirteen); `visma_list_skills(tool=...)` returns them in that order. Skill search matches whole words
(plural and singular meet) and bounds its work: a query is cut at 200 characters and 12 distinct words. Empty strings
count as "not given". Skills are also MCP resources (`skill://vsv2/<id>`).

**The skills describe the whole product; a given instance may lack some tools** (no credentials means no live tools,
no BigQuery project means no warehouse tools, writes are off by default). So a skill the tool returns names the tools
this instance does not have (`unavailable_tools`, plus an `availability_note` when one skill is read), `visma_init`
lists the tools that exist, and a prompt is offered only if every tool it needs exists (`prompts/get` of another one
is a clear error, not a workflow that sends the model to tools that are not there).

`vsv2-mcp --print-manifest` prints what **this instance, with its current configuration**, exposes (tools with
schemas and skill lists, the prompts it offers, the skills, capability status) as JSON; run it with the production
environment to see the full surface. A mistyped flag is an error, not a server waiting on stdin.

| Skill | About |
|---|---|
| `start-here`, `honest-results`, `error-triage` | Which tool for which question; completeness, truncation and result handles; what each error means |
| `tenants-and-entities` | Companies, `tenant_id`, entity vocabularies that differ per table, merged (historical) companies, base currencies |
| `warehouse-navigation`, `warehouse-sql-basics`, `multi-currency` | Datasets, joins, hashed keys, sign and currency columns, NOK conversion |
| `general-ledger-reporting`, `account-balance-explanation`, `period-end-review` | The three ledger sources and their signs, explaining a balance, closing a period |
| `receivables-payables`, `sales-reporting`, `project-profitability` | Open items, aging and exposure, and how to compare the snapshot with live Visma; order-type behavior, intercompany, margin; project P&L, the merger move, budget |
| `data-freshness` | What each freshness signal measures and what it cannot show |
| `live-erp-queries`, `sales-orders-live`, `datamart-graphql` | Live Visma reads: filters, paging, keys, expand sections, DataMart's fixed arguments |
| `edit-sales-order`, `create-sales-order`, `hold-or-release-sales-order`, `write-safety` | The audited edit, create (always on hold) and hold/release flows and what protects them |
| `api-reference`, `submit-feedback` | Finding the right endpoint and filter names offline; reporting a wrong or missing skill |

**Accuracy gates.** Every example is a ```` ```tool-call ```` block. `cargo test` runs each one through the **real tool code**
with recording fakes (schema, SQL guard, dataset policy, GraphQL guard, endpoint catalog) and fails if a skill names a
tool, skill, table or argument that does not exist, if a tool has no skill or no primary skill, if a prerequisite cycle
appears, or if front matter is invalid. Three ignored tests check against the real systems (see Tests). Facts about the
warehouse were verified against live BigQuery (project `aquatiq-datavarehus`) on 2026-10-12, facts about the live Visma
APIs on 2026-10-09; add a fact only after checking it the same way.

**Known issues** (`known_issues.json`, shown by `visma_init`): dated data problems that give wrong answers if ignored.
Each names the skill that explains it and a phrase that must appear there (a test enforces both), so the list cannot
drift from the guidance. Remove an entry when the issue is repaired; the skill says how to check.

**Adding a skill:** create `skills/<id>.md` (id = file name, kebab-case, at most 16,000 bytes), list the tools it is about,
write examples with queries you have run, then `cargo test -p vsv2-mcp`; run the ignored live tests before relying on it.

## Safety model

**Warehouse queries** (`src/warehouse/`), in order:
1. Static guard (`sqlguard.rs`): single statement, must start with SELECT/WITH, tokenizer blanks
   strings/comments (raw strings handled; `\r` ends a line comment as in BigQuery), DML/DDL/script
   keywords, `EXTERNAL_QUERY` and `ML.*`/`AI.*` function calls (remote models) refused.
2. BigQuery **dry run** is the authority: statement type must be `SELECT`, every referenced table
   must be in the configured project and an allowed dataset, estimated bytes must be under the cap.
3. The real run uses `--maximum_bytes_billed` and a row cap (one extra row is fetched to report `truncated`),
   and a rolling-hour byte budget stops a runaway loop.
   Verified live: region-qualified `INFORMATION_SCHEMA.JOBS*` is reported by BigQuery under dataset
   `region-...`, so the allow-list blocks it.
4. Dataset allow-list default, in project `aquatiq-datavarehus`: `visma_gold_*`, `visma_datamart`, `visma_silver`,
   `visma_silver_common`, `visma_bronze`, `dw_staging_visma`. Everything else in the project is refused: the assertion
   views (`visma_assertions`), the Excel mapping inputs (`visma_silver_excel`, `visma_bronze_excel`) and the Xero and
   currency datasets. BigQuery reports the tables behind a view, so a view is judged by the tables it reads.

**Live Visma reads**: the URL always comes from the endpoint catalog, never from the caller; query
parameter names/values are validated and paging parameters are owned by the tool; only configured
tenants (or, signed in, the companies you connected) are accepted; the service identity uses read scopes only; a
local rate limiter (max 30/min) keeps this server a polite neighbour of the scheduler (note: its bucket is not shared
with the scheduler).

**Identity.** By default the tools act with the integration's service credentials. After `vsv2-mcp login` they act as
the signed-in person, with one Visma token per company ([`sign-in.md`](sign-in.md)): Visma applies that person's roles,
only their connected companies are visible, DataMart is off, and a sign-in that cannot be used turns the live tools off
instead of falling back to the broader service identity. The warehouse tools are not part of this: they use the
computer's own `gcloud` login.

**Writes** go exclusively through Layer 3 (`vsv2-interactive`):
- Deny-by-default tenant + operation allow-list; dry-run then execute; no DELETE exists; the ETag
  always comes from a fresh GET (never a wildcard).
- An HMAC confirm token binds every field of the reviewed plan, *including the summary the approver
  reads*, with length-prefixed encoding. Plans expire (10 min) and run at most once.
- The `executing` audit row is written *before* the request is sent and is an **atomic claim**
  (unique index on `plan_id` where stage = `executing`), so two concurrent executes cannot both send.
- The send and its outcome row run in a detached task: a cancelled caller cannot abandon a write
  half-recorded. A 2xx is always reported as success (`audit_degraded: true` if the outcome row could
  not be written); a timeout/reset after sending is recorded and reported as **outcome unknown**
  (verify the order in Visma before planning again), never as a clean failure.
- `vsv2.write_audit` is append-only (UPDATE/DELETE/TRUNCATE blocked by triggers).

**What a failure means.** A failure before the request leaves (no token, no scope, rate limit) is recorded as `failed` and
reported as `not sent`: nothing happened and a new plan is safe. A gateway or server error (500, 502, 503, 504) or a
timeout to a *create* is recorded as `unknown`, because the order may have been made before the error. A plan is also
checked against the credentials when it is made, so a missing write scope is refused before anyone is asked to approve.

**Human approval (the MCP layer).** The model cannot approve its own write. In the default mode
(`VSV2_MCP_WRITE_APPROVAL=elicitation`) `..._execute` first pre-validates the plan (bad, expired or
replayed plans never bother the user), then asks **the user** through MCP elicitation, showing the
summary, company (name and id), request and the *complete* compact body taken from the *signed plan*; a body over
8,000 characters is refused rather than cut, because a prompt that hid part of the request would not approve it
(a create takes at most 200 lines). Declined, cancelled, timed out, or a
client without elicitation support => nothing is written (fail closed). `permission_prompt` is an
explicit, logged opt-in for clients without elicitation: it relies on the client's own tool-permission
prompt plus a `user_approved` argument the model sets, which is weaker.

Write tools are registered only if everything is verified at startup: flag on, write scope (the service's
`VISMA_OAUTH_SCOPE_SALESORDER_WRITE`, or, signed in, a token that carries the SalesOrders write scope), confirm
secret, and the audit database reachable **with the `vsv2.write_audit` table present**. The SalesOrder
base URL (used for reads *and* writes) and the Finance base URL must be `https` on a `*.visma.net` host (loopback
allowed for tests), otherwise the server refuses to start. With the service credentials the audit `caller` is a
self-declared label; signed in, it is the person Visma Connect authenticated (`visma-user:<e-mail>`).

## Configuration (environment; `.env` is loaded for local development)

| Variable | Purpose |
|---|---|
| `VSV2_MCP_BQ_PROJECT` | Enables `warehouse_*`. Set it to `aquatiq-datavarehus`, the v2 warehouse (the project `VSV2_BIGQUERY_PROJECT` writes to). Unset: the warehouse tools are left out. |
| `VSV2_MCP_BQ_DATASETS` | Comma list of readable datasets; `prefix*` allowed. Default: `visma_gold_*,visma_silver,visma_silver_common,visma_bronze,visma_datamart,dw_staging_visma`. |
| `VSV2_MCP_BQ_MAX_BYTES` | Max estimated bytes scanned per query (default 2 GiB). |
| `VSV2_MCP_BQ_MAX_BYTES_PER_HOUR` | Rolling-hour scan budget for the whole process (default 20 GiB). |
| `VISMA_CLIENT_ID/SECRET`, `VISMA_SCOPES`, `VSV2_DATABASE_URL`, `VSV2_REDIS_URL` | Same as the sync service; enable the `visma_*` tools with the service credentials. Not needed when you sign in (`VSV2_DATABASE_URL` then only for `visma_sync_status` and writes). |
| `VSV2_MCP_IDENTITY` | `auto` (default: you once `vsv2-mcp login` has been run, else the service credentials), `user` (you only; off without a sign-in) or `service` (ignore a sign-in). |
| `VISMA_USER_CLIENT_ID/SECRET`, `VISMA_USER_REDIRECT_URI`, `VISMA_USER_SCOPES`, `VISMA_USER_SCOPES_WRITE`, `VISMA_USER_PROMPT`, `VSV2_USER_STORE_DIR` | The sign-in application and where its tokens are kept; see [`sign-in.md`](sign-in.md). |
| `VISMA_ERP_REST_BASE_URL`, `VSV2_RATELIMIT_ERP_REST_RPM/RPD`, `VSV2_PILOT_TENANT_ID` | Finance API host (https on `*.visma.net`), live-call limits (minute value capped at 30), the default company. |
| `VSV2_TENANT_CONFIG_DIR`, `VSV2_ENDPOINT_CONFIG_DIR` | Default `config/tenants`, `config/endpoints` (relative to the working directory). |
| `VSV2_MCP_ENABLE_WRITES=1` | Register the write tools. |
| `VSV2_MCP_WRITE_APPROVAL` | `elicitation` (default, recommended) or `permission_prompt` (weaker; see above). |
| `VISMA_OAUTH_SCOPE_SALESORDER_WRITE` | `visma.net.erp.salesorder:write` (confirmed from the public swagger). |
| `VSV2_WRITE_CONFIRM_SECRET` | >= 16 bytes; signs plans. |
| `VSV2_WRITE_ENABLED_TENANTS`, `VSV2_WRITE_ENABLED_OPERATIONS` | Allow-lists (e.g. `salesorder.patch,salesorder.create,salesorder.set_status`). Empty = every write denied. |
| `VISMA_OAUTH_SCOPE_ERP_WRITE` | Not used by any operation: every write goes to the SalesOrders v3 API, which needs `VISMA_OAUTH_SCOPE_SALESORDER_WRITE`. |
| `VSV2_MCP_FEEDBACK_FILE` | Where `visma_submit_feedback` appends its reports. Default `~/.vsv2/mcp-feedback.jsonl`, created owner-only. Local only. |
| `VSV2_MCP_CALLER` | Label recorded in the audit log (default `mcp:$USER`); not authenticated. |
| `VSV2_MCP_SALESORDER_BASE_URL` | Override for tests/staging; must be https on `*.visma.net` (or loopback). |

A capability whose settings are missing is left out and named in a startup warning on stderr
(`RUST_LOG` defaults to `info`; stdout carries the protocol only).

Apply `migrations/0012_write_audit.sql` to the database before enabling writes (the server checks for the table at startup).

## Pointing a client at the v2 warehouse

The tools read BigQuery project `aquatiq-datavarehus` (location `europe-north2`) through the `bq` CLI, as whatever
identity `gcloud` is signed in as (it needs read access to the project's datasets and permission to run query jobs).
The settings, in the client's environment (`claude mcp add -e`, the MCP client's `env` block, or `.env`):

| Variable | Value |
|---|---|
| `VSV2_MCP_BQ_PROJECT` | `aquatiq-datavarehus` |
| `VSV2_MCP_BQ_DATASETS` | not set (the default, `visma_gold_*,visma_silver,visma_silver_common,visma_bronze,visma_datamart,dw_staging_visma`) |
| `VSV2_MCP_BQ_MAX_BYTES`, `VSV2_MCP_BQ_MAX_BYTES_PER_HOUR` | not set (2 GiB per query, 20 GiB per rolling hour) |

```bash
claude mcp remove visma-v2 --scope user
claude mcp add visma-v2 --scope user -e VSV2_MCP_BQ_PROJECT=aquatiq-datavarehus -- "$PWD/scripts/run_mcp.sh"
```

Start a new session, call `visma_init` and check that `capabilities.warehouse.project` is `aquatiq-datavarehus`
and `readable_datasets` is the list above. A client that still names another project keeps that project, with a
`warning` in `capabilities.warehouse`, and every query is refused unless `VSV2_MCP_BQ_DATASETS` names datasets of
that project. To read more of the project, add the datasets to the list (for example
`...,visma_assertions`); `visma_datamart.RLS_sales_person` holds the Power BI user names and is readable with the
rest of `visma_datamart`.

## Register with Claude Code

```bash
cargo build --release -p vsv2-mcp
claude mcp add visma-v2 --scope user -- "$PWD/scripts/run_mcp.sh"
claude mcp list        # visma-v2 ... ✓ Connected
claude mcp remove visma-v2 --scope user   # to undo
```

`scripts/run_mcp.sh` fixes the working directory (so `.env` and `config/` resolve) and the PATH for `bq`, and it gives
`VSV2_MCP_BQ_PROJECT` a default that an exported value overrides (set it as described in the next section); it is
read-only unless writes are explicitly enabled. Rebuild the release binary after code changes, then start a new
Claude Code session (a session's tool list is fixed when it starts).

## Tests

```bash
cargo test -p vsv2-mcp -p vsv2-interactive              # offline, hermetic (spawns the real binary over stdio)
cargo test -p vsv2-mcp --test live_warehouse -- --ignored --nocapture --test-threads=1   # BigQuery, read-only
cargo test -p vsv2-mcp --test live_visma     -- --ignored --nocapture --test-threads=1   # Visma + local PG, read-only
cargo test -p vsv2-mcp --test live_skills    -- --ignored --nocapture --test-threads=1   # every skill example: BigQuery dry-run (free), live Visma reads, warehouse examples (about 4 minutes)
# full-stack write path against a SCRATCH Postgres + mock Visma (ignored unless asked for, so a plain run does not claim it):
VSV2_TEST_DATABASE_URL=postgres://.../scratch cargo test -p vsv2-mcp --test write_e2e -- --ignored --test-threads=1
VSV2_TEST_DATABASE_URL=postgres://.../scratch cargo test -p vsv2-interactive --test pg_audit -- --ignored
cargo test -p vsv2-signin                                # the sign-in against a mock Visma Connect; `--test user_mode_e2e` runs the server as a signed-in person
```

## Known limits / next steps

- stdio only. Signing in gives each person their own identity on their own computer; a remote/HTTP deployment still
  needs its own authentication and per-user token handling (the sign-in code is reusable, the hosting is not built).
- The sign-in has not been exercised against the real Visma consent screen yet (see [`sign-in.md`](sign-in.md) for
  what is and is not verified), and whether the SalesOrders API accepts a person's token is unknown.
- Three write operations, all on the SalesOrders v3 API: `salesorder.patch`, `salesorder.create` (always on hold; the
  body is checked against Visma's own schema) and `salesorder.set_status` (Open to Hold and back). None has been run
  against a live Visma write yet, so the first real change belongs on a test company. Cancelling, completing,
  customer/supplier edits and JournalTx vouchers are not offered.
- Not yet covered: Xero/Excel/Moment warehouse data (deliberately outside the default allow-list).
- The server's rate limiter is process-local; coordinate with the scheduler's quota if heavy use is expected.
