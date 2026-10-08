# Contextual retrieval runbook

Status: **proposed**. Written on 2026-10-06 (task CR-T16 of the contextual retrieval build), before most of the build it describes is deployed. Nothing in this file has been exercised against the new pipeline. The decisions behind it are the four entries of 2026-10-03 in the [decision ledger](../../Model%20Plane/docs/decisions/ledger.md), the ownership rows and rule 16 in the [master ownership matrix](../../master-ownership-matrix.md), and the owner rulings of 2026-10-03.

## 0. How to read this runbook

- **Today** is what is true on the developer stack and in the working tree on 2026-10-06. The numbers are in [`live-reads-contextual-retrieval-2026-10-06.txt`](../../../docs/compliance/evidence/live-reads-contextual-retrieval-2026-10-06.txt) (read-only reads, no secrets, no document text).
- **After CR-Tn** is the design of 2026-10-03 as the build manifest corrected it. Names of variables, columns, flags and metrics are the ones in that design. Where the shipped name differs, the shipped name wins, and the docs finalisation task (X-DOCS-FINAL) replaces the name here and the target with the measured result.
- A procedure that needs something that is not built says so in its first line. Where the design promised a tool that no build task delivers, section 7 says so instead of describing the tool.
- Never edit the live `.env` in place to change a default. Change a value for an incident, record why, and put the new default in `.env.example` through a normal change.
- Containers are recreated through the launcher stage copies (`apps/Frontend Plane/verevonv3/build-verevon-services.sh`), never with a hand-built `docker compose`. An env-only change recreates with `--skip-build`.
- The Data Plane Postgres superuser role is `dataplane` in the container `data-plane-v2-postgres-1`. Row-level security does not apply to that role, so every query below names the org. Query counts and statuses only. Never select `chunk_context` or `text`: they are customer content.

## 1. What the system does

For each indexed chunk (a knowledge unit), contextual retrieval asks a small model for one or two sentences that place the chunk in its document (the situating context). The context is stored beside the chunk (`knowledge_units.chunk_context`), is prepended to the chunk before the chunk is embedded, and reaches the keyword arms through the same stored column (`content_tsv` weight B in Postgres, `context_body` in Quickwit). The model is `gpt-4o-mini` on Azure OpenAI, called through `inference-core` with a service token for the document's organisation.

Measured benefit (87 judged queries, 1,164-chunk corpus, 2026-08-26): contextual retrieval beat raw chunks by +0.033 to +0.048 nDCG@10 and +0.040 to +0.061 MRR at every fusion weight tested (the contextual organisation's pinned-mix scorecard, the committed baseline, is nDCG@10 0.7732 and MRR 0.7394), recall unchanged within noise, exact-identifier queries unchanged. The gain held with the reranker on (+0.028 MRR in the pair measured). The reranker itself added +0.0245 nDCG on the raw corpus and only +0.005 on the contextual one, so the two overlap; it reads the raw chunk text and never the context, and whether it should read the context is what the flag `RERANK_INPUT_INCLUDE_CONTEXT` (CR-T15) measures. The flag stays off until the evaluation shows a gain. Source: `retrieval-fusion-evidence-2026-08-26.md`.

### 1.1 Invariants

| # | Invariant | Delivered by |
|---|---|---|
| I1 | The vector of a unit is a pure function of its text and its persisted context. `embedded_context_hash` is the sha256 of the stored context, or NULL for a raw vector. | CR-T6, CR-T8 |
| I2 | A context is written together with its provenance (model, provider, residency, prompt version, input hash, time) in one statement. | CR-T8 |
| I3 | Nothing is silent: every eligible unit has a non-NULL `context_status`, and every non-ready status has a content-free reason. | CR-T6, CR-T8 |
| I4 | Ingest never fails or stalls because of context generation. The inline phase is time-boxed; unfinished units are embedded raw and left `pending`. One tenant cannot hold up another. | CR-T8 |
| I5 | No document text goes to a provider unless the organisation's posture is resolved and either not ZDR or the route is attested. Unresolved posture means defer: never send, never skip for good. | CP wave, CR-T5, CR-T7 |
| I6 | Every provider call is metered with org, feature and exact usage, at least once and idempotently. | CR-T10, CR-T4 |
| I7 | A re-run with unchanged inputs makes zero provider calls. | CR-T8, CR-T9a |
| I8 | The keyword arms are fed from the same persisted context, and a parity check makes convergence measurable. | CR-T9a, CR-T11 |

### 1.2 Status vocabulary (`knowledge_units.context_status`)

The migration is `apps/Data Plane v2/infra/postgres/migrations/20261003130000_context_provenance.sql`. It is in the working tree and not applied to the stack (the live table has no `context_status` column).

| Status | Meaning | Who moves it on |
|---|---|---|
| NULL | Unknown: the row predates the migration or was written by code that records no status. Not `pending`. | The backfill function, then the writers |
| `pending` | Eligible, no usable context yet. | The reconciler fills it. |
| `ready` | A context exists and its provenance was recorded. | Refreshed on a prompt change only when enabled. |
| `legacy` | A context exists and its provenance is unknown (written before provenance was recorded). The vector's coherence with it is unproven. | The legacy verification pass re-embeds the stored context and makes it `ready` (no model call). |
| `skipped_zdr` | Withheld from the provider because the document or the organisation is zero-retention. Carries an error class. | Re-checked per organisation at most hourly. |
| `skipped_policy` | The organisation's processing policy is off. Operator-set, with a reason. | The policy changing. |
| `skipped_budget` | The budget mode is `stop` and the budget is used up. Exists only when an operator sets `stop`. | The next period or a raised budget. |
| `failed_transient` | Retryable failure. Carries an error class. | The reconciler, with backoff. |
| `failed_permanent` | Terminal. Carries an error class. | An operator, after the cause is fixed. |

Error classes are a closed set, enforced by a CHECK constraint of the migration, which is the authority for the list (`scripts/check-cr-docs.py` fails when this list and the migration differ): `rate_limited`, `provider_unavailable`, `timeout`, `auth`, `zdr_unavailable`, `zdr_org`, `zdr_document`, `residency_unavailable`, `posture_unavailable`, `content_filter`, `invalid_output`, `doc_missing`, `too_long`, `internal`.

A zero-retention skip says which kind it was: the organisation is zero-retention (`zdr_org`), the document is classified restricted (`zdr_document`), or no attested route exists (`zdr_unavailable`). The class `posture_unavailable` is a wait, not a skip: Auth Core could not resolve the organisation's posture and answered 503 `retention_posture_unavailable`, so the unit stays `pending` and is retried. The migration accepts all of these today; the code that writes them is CR-T7 and CR-T8, and nothing writes any of them yet.

## 2. What exists today and what each procedure needs

| Capability | Delivered by | Today (2026-10-06) |
|---|---|---|
| `context_status`, provenance columns, `org_processing_policy`, `v_context_coverage`, legacy backfill | CR-T6 | Migration file in the tree, not applied |
| Typed refusal statuses and per-service token counters in `inference-core` | CR-T3 | In the tree, not deployed. `/metrics` answers with one help line and no sample. |
| Shared token client with a tenant posture, bearer cache, typed decoder | CR-T5 | Separate copies of the client in embedding-engine, graph-index and retrieval-engine; static posture `persistent` |
| Contextualizer v2 (outcomes, prompt `ctx-v2`, windowing, validation) | CR-T7 | Live image of 2026-10-06: contexts are generated inline; an empty answer is not logged; the stop reason is ignored; `zdr: false` is hard-coded; no provider or residency floor. Built, tested, not deployed (ships with CR-T8): a closed outcome for every chunk with its status and error class, the provider and residency floors, the stop reason and retry-after honoured, windowing by section, validation, usage captured. See section 5.10. |
| Write-ahead context, vector coherence, reuse by hash, inline deadline | CR-T8 | Context is persisted after the vector, best effort; a re-embed always regenerates (re-pays) |
| Reconciler, `context-backfill`, legacy verification, parity endpoints | CR-T9a | Built and tested, not deployed: the CR-T6 migration, then the embedding-engine image (the reconciler and the tool, shipped dark with `CONTEXT_RECONCILER_ENABLED=0`) and the quickwit-adapter image (the parity endpoints). On the stack today only `backfill-reannounce` and `reembed-switch` exist. Sections 5.2 and 5.7. |
| Budget gate | CR-T9b | Built and tested, not deployed (the embedding-engine image: the gate in the stream consumer's pipeline and in the reconciler, `GET /metrics` with `ctx_budget_exceeded_total`, and the boot checks of `BACKGROUND_BUDGET_*`; the compose file passes the four variables to it). The policy API still accepts only `inherit` and no figure: its flag (`policy.Enforced.Budget` in data-quality-go) is turned on by the change that follows this one, after the embedding-engine and graph-index images are deployed. Section 5.5. |
| Policy admin API, coverage endpoint, release gates `contextual_coverage_ok` and `sparse_index_in_sync` | CR-T11 | Built and tested, not deployed (data-quality-go and embedding-engine images, and the CR-T6 migration first). The keyword-index gate reads the parity endpoint that CR-T9a adds and is `unavailable` until it exists. Section 5.11. |
| Usage ledger v2 and forwarder to cost-core | CR-T10, CR-T4 | The Data Plane's own `cost_events` holds rerank and embedding events only; nothing reaches cost-core |
| Graph extraction state table, poison cap, direct-Azure backend removed | CR-T12 | Live image of 2026-10-06: in-memory suppression, a retry loop was observed, a direct Azure key is present. Built and tested, not deployed (the migration `20261003150000_graph_extraction_state`, then the graph-index image, then data-quality-go, which only then accepts `graph_extraction`): per-chunk state with backoff and a poison cap, typed errors with the gRPC code in the log, the usage ledger (dark until `USAGE_EMIT_ENABLED=1`), the tenant posture (the compose value stays `persistent` until the flip of section 6), the budget gate (alert-only), and no Azure key or endpoint in the container. Its 22 Rust and 7 Go Postgres-gated tests have not been run by their author (no Postgres on the build machine): the CI job `postgres-contract-tests` runs them. Section 5.9. |
| Default ON, kill-switch reason, boot-time checks, `.env.example` | CR-T13 | Built and tested, not deployed (the embedding-engine image and the compose file, last: after the live proof of CR-T8 and the reconciler). The live image and compose still default to false, and the feature is on only because the git-ignored `.env` sets it. Built: on in code and compose; the model defaults to `gpt-4o-mini`; the switch needs `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON` or the service refuses to start; the reason is logged and reported by `/readyz`; the settings and the credential are checked before any consumer starts; every variable is passed through compose and documented in `.env.example`; the three isolated harnesses turn the feature off by name. Section 3, 5.6; `tests/scripts/contextual-retrieval-config-contract-test.sh` keeps the compose file, `.env.example` and the harnesses to it |
| Org retention posture reaches background work (`tenant`, 503 `retention_posture_unavailable`, 60-second token cap) | Control Plane wave (CR-T1) | Static `persistent`. The running Auth Core and org-core images of 2026-10-04 carry the posture code, the organisation-service route and its read grant (string checks), but the running registry has no `tenant` principal, so the organisation's ZDR setting is not consulted by these jobs |
| Quality harness: frozen corpus and its ingest, golden builder, coverage and parity evidence, promotion gate, recorded results | CR-T14 | Built and tested, not run live (section 5.12): the corpus is committed and verified, the two evaluation organisations do not exist (OPS-ORG), `quality_eval_runs` is empty, and the paid re-seed waits for the owner's decision OD8. The data-quality image with `POST /v1/evals/results` is needed for recorded results. |
| Rerank input with context | CR-T15 | Not built |

## 3. Switches

Every switch has a default, a boot-time check and a procedure here. Defaults are as built (CR-T13, CR-T9a, CR-T10): the compose file, the code and `.env.example` carry the same ones (a Rust test compares the compose defaults with the code defaults, and `tests/scripts/contextual-retrieval-config-contract-test.sh` compares the compose file with `.env.example`). Until the CR-T13 image and compose are deployed, the first row is false in the running image and compose.

| Switch | Default after the build | Boot-time check | Procedure |
|---|---|---|---|
| `CONTEXTUAL_RETRIEVAL_ENABLED` | `true`. Incident-only kill switch. An empty value is the default (on), never off. | Boot fails when it is true without a model or credential (the credential is checked before any consumer starts, whenever signed events are on: exit status 1 and a message, not a consumer that ends quietly), and when it is false without `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON`. A value that is not `true`, `1`, `false` or `0` stops the service and names the variable. The startup log says `contextual retrieval is ON` (with the model) or `contextual retrieval is OFF` (with the reason), and `/readyz` reports `contextual_retrieval` as `{"state":"on","model":...}` or `{"state":"kill_switch","reason":...}`; a thrown switch never makes the service not ready. | 5.6 |
| `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON` | empty | Required when the switch is off: 8 to 240 characters, one line, no control character, written for an operator who reads `/readyz` (no customer name, no personal data). Ignored, with a warning in the log, while the switch is on. | 5.6 |
| `CONTEXTUAL_RETRIEVAL_MODEL` | `gpt-4o-mini`, the shared deployment. A dedicated background deployment alias (operator action `OPS-ALIAS`: the deployment is added to inference-core's chat catalog, `AZURE_OPENAI_CHAT_DEPLOYMENTS`) is another value of this one variable and needs no code change. | Boot fails when blank while enabled (unset it for the default). | 5.4 |
| `CONTEXT_INLINE_BUDGET_MS` | `20000`: the most one batch spends generating contexts inline. `0` makes all generation asynchronous (raw first). | Boot fails above `30000` (half of the 60-second ack wait: a batch that runs past it is redelivered and its contexts are paid for twice). The 120-second envelope time to live is the looser bound. | 5.1, 5.2 |
| `CONTEXT_INLINE_DOCUMENT_BUDGET_MS` | `8000`: the same bound for one document; never past the batch's. | Boot fails when it exceeds the batch budget, or is `0` while the batch budget is not. | 5.2 |
| `CONTEXT_INLINE_DOCUMENT_CONCURRENCY` | `2`: how many documents of one batch are contextualised at the same time, so one slow document or tenant cannot hold up the rest. The most provider calls one batch has in flight is this times `CONTEXTUAL_RETRIEVAL_CONCURRENCY` (so 8 by default, twice what the sequential loop asked of a deployment that chat shares). Raise it with the dedicated deployment alias, not before. | Boot fails outside 1 to 32. Passed through the compose file with the other knobs (CR-T13), and documented in `.env.example`. | 5.1 |
| `CONTEXTUAL_RETRIEVAL_PROVIDER_HINT` | `azure-openai` (`azure_openai` matches too). An empty value takes the default: compose passes every variable, empty when unset, and an unpinned route lets chunk text go to whichever provider the chain tries next. | None beyond non-empty after the default. | 5.10 |
| `CONTEXTUAL_RETRIEVAL_MIN_RESIDENCY` | `eu`. An empty value takes the default; the weaker floor is spelled out (`global`). | Boot fails unless it is one of `global`, `eu`, `eea`, `norway`, `no`, `sovereign` (inference-core refuses any other token on every call). It is only as good as inference-core's own region variable: section 5.10. | 5.10 |
| `CONTEXTUAL_RETRIEVAL_PROMPT_VERSION` | `ctx-v1`, the prompt every stored context came from, byte for byte. `ctx-v2` ships only through the evaluation gate (5.4). | Boot fails on any other value. | 5.4 |
| `CONTEXTUAL_RETRIEVAL_WINDOW_CHARS` | `32000`: a document up to this size is read whole, a longer one in fixed sections of this size that overlap by an eighth, each with a header. | Boot fails below `8000` or above `CONTEXTUAL_RETRIEVAL_MAX_DOCUMENT_CHARS`. | 5.10 |
| `CONTEXTUAL_RETRIEVAL_MAX_TOKENS` | `128`: the output cap. A cut-off answer keeps its last full sentence when that is at least 20 characters, and is otherwise `invalid_output`. | Boot fails outside `16` to `1024`. | 5.10 |
| `CONTEXT_RECONCILER_ENABLED` | `0` at first deploy, `1` with the limits below (and `context-backfill` capped) after the live proof | Turned on, the service refuses to start unless every `CONTEXT_RECONCILER_*` value is inside its bounds (the message names the variable), the provenance migration is applied, and signed events are on (`ENABLE_SIGNED_EVENT_CONSUMERS=1` and the event key: a healed document is announced signed, and an unsigned announcement is refused). It logs its limits at start (`context reconciler running`). | 5.2 |
| `CONTEXT_REFRESH_ON_PROMPT_CHANGE`, `CONTEXT_REFRESH_MAX_UNITS` | `0` and `0` | `1` without a `CONTEXT_REFRESH_MAX_UNITS` above zero stops the service at startup: a refresh spends one provider call per unit and runs only inside an explicit cap. The cap counts over the life of the process, so a restart gives a fresh one: write that in the change record. A refresh regenerates only a context made with another prompt version than the one in force for its organisation. | 5.4 |
| `CONTEXT_RECONCILER_INTERVAL_SECS`, `_UNITS_PER_PASS`, `_ORG_UNITS_PER_PASS`, `_CONCURRENCY`, `_ORG_CONCURRENCY`, `_CHUNK_CONCURRENCY`, `_LEASE_SECS`, `_BUDGET_MS`, `_DOCUMENT_BUDGET_MS` | `30`, `32`, `16`, `2`, `1`, `2`, `300`, `30000`, `25000` | Bounds, each refused by name: the interval 1 to 3600 seconds; units per pass 1 to 500, and per organisation 1 to the pass total (so one organisation cannot take a whole pass); documents at once 1 to 16, and per organisation 1 to that; chunks at once within a document 1 to 16; the lease 60 to 3600 seconds (it has to outlast a pass: a lease that ends under a running batch hands its units to a second worker); the time boxes by the inline path's own rule (a document's within its batch's, and the batch's at most 30000). The most provider calls in flight is documents times chunks (4 by default), less than the inline path's, so chat keeps its share. | 5.1, 5.2 |
| `CONTEXT_RECONCILER_MAX_ATTEMPTS`, `_BREAKER_FAILURES`, `_BREAKER_PAUSE_SECS`, `_PAUSE_FILE`, `_ZDR_RECHECK_SECS`, `_SWEEP_SECS` | `8`, `3`, `300`, `/tmp/context-reconciler.pause`, `3600`, `300` | Bounds: attempts 1 to 100, breaker failures 1 to 20, breaker pause 10 to 3600 seconds, ZDR recheck 60 to 86400, sweep 10 to 86400. A unit that fails for its own sake `MAX_ATTEMPTS` times becomes `failed_permanent` with a structured error (`alert="context_units_failed_permanent"`, counts only); a failure that is not the unit's (a rate limit, a provider that is down, an unresolved posture, a refused credential) never counts. After `BREAKER_FAILURES` passes in a row that failed, the loop pauses for `BREAKER_PAUSE_SECS`; a refused credential opens it at once. Creating the pause file stops every pass from the next one on, with no restart, and removing it resumes; an empty path turns the file off. The sweep (the work the queue index does not cover: vectors to repair, `skipped_policy` units of an organisation that is no longer off, a changed prompt, exhausted units) runs at the sweep interval, and the hourly ZDR recheck asks Auth Core for a bearer per organisation that has `skipped_zdr` units. | 5.1, 5.2, 5.3 |
| `CONTEXT_RECONCILER_STALE_ENABLED`, `CONTEXT_RECONCILER_STALE_DOCUMENTS` | `0` and `8` | Off by default. On, each sweep reads up to `STALE_DOCUMENTS` documents (1 to 100) from a cursor and regenerates only the chunks whose input hash changed since their context was made. A chunk that did not change costs nothing. | 5.2 |
| `CONTEXT_ESTIMATE_INPUT_USD_PER_MTOK`, `CONTEXT_ESTIMATE_OUTPUT_USD_PER_MTOK`, `CONTEXT_ESTIMATE_EMBED_USD_PER_MTOK` (`context-backfill` only) | `0.15` and `0.60` (the gpt-4o-mini row of the Model Plane catalogue); no embedding price (no price for that model is in the repository, so the embedding cost is printed as unknown) | A value that is not a number from 0 up stops the command, naming the variable. Not in the compose file: pass them to the one command with `docker exec -e`. | 5.2 |
| Org policy `contextual_retrieval` (`inherit`, `on`, `off`) | no row, meaning on | `off` needs a reason and an operator service principal holding `data:quality:admin` for that organisation (an organisation's own admin is refused); the API refuses an invalid value, and the engine treats an unrecognised stored value as unknown and generates nothing for it. | 5.6, 5.11 |
| Org policy `budget_mode` (`inherit`, `alert_only`, `throttle`, `stop`) | `alert_only` | `stop` is explicit, audited and needs a reason. embedding-engine (CR-T9b) and graph-index (CR-T12) both read it, together with `monthly_budget_usd`: embedding-engine within 30 seconds (its policy cache), graph-index before every document. An organisation with no row, or `inherit` and no figure, has the deployment's `BACKGROUND_BUDGET_*`; an unrecognised stored value makes embedding-engine treat the organisation as unknown and generate nothing for it. The policy API still accepts only `inherit` and no figure until its flag (`policy.Enforced.Budget`) is turned on, in the change that follows, after both images are deployed: a stored value must never be one that one worker honours and another ignores. | 5.5, 5.11 |
| Org policy `graph_extraction` (`inherit`, `on`, `off`) | no row, meaning on | The policy API accepts it since CR-T12 (graph-index is its consumer); `off` needs a reason and an operator service principal holding `data:quality:admin` for that organisation. graph-index reads the row before every extraction, with no cache: an organisation that is `off` has its chunks marked `skipped_policy`, with no call and no bearer, and they are worked at the first pass after the switch is lifted. The gate runs in the order policy, retention posture, budget. A value it does not recognise, or a read that fails, ends that organisation's pass with an error: a gate that cannot be read is never read as open. | 5.9, 5.11 |
| `GRAPH_EXTRACTION_MAX_ATTEMPTS` (graph-index) | `6`: the poison cap. A chunk that fails for its own sake this many times becomes `failed_permanent`, and nothing retries it until an operator requeues it (section 5.9). A failure that is not the chunk's (a refused credential, a throttle, a provider that is down, a posture Auth Core could not resolve) does not count. | The service refuses to start outside `1` to `50`, naming the variable. | 5.9 |
| `BACKGROUND_BUDGET_MODE`, `BACKGROUND_BUDGET_MONTHLY_USD`, `BACKGROUND_PRICE_INPUT_USD_PER_MTOK`, `BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK` (embedding-engine and graph-index: the same four variables, the same defaults, one setting in `.env` for both; `tests/scripts/background-budget-contract-test.sh` holds them together) | `alert_only`, no figure, no prices: nothing is built in (owner default D-E; the owner supplies the figure). Over a figure, `alert_only` logs and keeps working, `throttle` works one chunk at a time, `stop` marks the chunks `skipped_budget`. An organisation's own `budget_mode` and figure win over these. The spend is the month's `llm` tokens in `cost_events` (every background call of the organisation: contextual retrieval, graph extraction, community summaries; never an embedding or a rerank) priced with the two prices, by the same statement in both services (a test holds the two together); with a figure and no prices the spend is unmeasurable, the log says so and the work proceeds. In embedding-engine: `throttle` lets one chunk through per batch and per reconciler pass and leaves the rest `pending` (due at once, no error class); `stop` makes no provider call and asks for no bearer, marks the units `skipped_budget`, and a unit so marked is generated again at the first pass in which the budget allows it (a new month, a raised figure, another mode, a removed row). The gate reads the ledger at most every 30 seconds and adds what this process has spent since, so it stops within one batch of crossing the figure, never to the cent. With the usage ledger off (`USAGE_EMIT_ENABLED=0`) this service's own calls are not in `cost_events`, so a budget that applies is unmeasurable and is alerted, not enforced. | The service refuses to start on an unknown mode, a figure or price that is not a number from 0 up, only one of the two prices, or `stop` with a figure and no prices (a stop that could never fire). embedding-engine also refuses `throttle` with a figure and no prices, and `throttle` or `stop` with a figure while `USAGE_EMIT_ENABLED` is off (and `context-backfill` refuses the same, as it reads the same configuration). | 5.5, 5.9 |
| `COMMUNITY_SUMMARY_ENABLED`, `COMMUNITY_SUMMARY_MAX_PER_RUN` (graph-index) | `0` (off) and `5`. A community summary is a paid call. Turned on, it passes the same gate as extraction (policy, posture, budget), is metered as `graph-community-summary`, and the run ends at the first provider failure. | The service refuses to start unless the first is blank, `0`, `false`, `no`, `1`, `true` or `yes`, and the second is blank or a whole number from `1` to `1000`. | 5.9 |
| `CONTEXT_COVERAGE_THRESHOLD_PERCENT` (data-quality-go) | `99` | The service refuses to start unless it is a number above 0 and at most 100. | 5.11 |
| `SPARSE_INDEX_PARITY_URL` (data-quality-go) | empty, meaning the `sparse_index_in_sync` gate is `unavailable` | The service refuses to start unless it is empty or an absolute http(s) URL without credentials or a fragment. The caller's own bearer is forwarded to it. | 5.7, 5.11 |
| `USAGE_EMIT_ENABLED` (embedding-engine, graph-index) | `0` until the consumer is deployed. `1` or `true` turns it on; `0`, `false` and empty are off; anything else stops the service at startup. | Turned on, the service refuses to start unless signed events are enabled (and the signing key is present: `EMBEDDING_EVENT_PRIVATE_KEY_PATH`, `GRAPH_EVENT_PRIVATE_KEY_PATH`) and the durable stream `DATAPLANE_COST` can be created or verified (a stream with `Interest` or `WorkQueue` retention, or memory storage, is refused). While it is off the embedding engine publishes the version 1 cost event as before, and graph-index records nothing. Turned on, graph-index records one content-free event per paid call (feature `graph-extraction` for an extraction, `graph-community-summary` for a summary), at the call and before the answer is judged, so an answer that is refused or cannot be parsed is still in the ledger. | 5.8 |
| `USAGE_FORWARDER_ENABLED` (data-orchestrator) | `0` until cost-core accepts `feature` and `units` and prices the models. Exactly `1` turns it on. | Turned on, the service refuses to start without `DATA_ORCHESTRATOR_COST_SERVICE_API_KEY` (32+ characters, no placeholder marker), absolute `http(s)` addresses for `USAGE_FORWARDER_COST_CORE_URL` and `USAGE_FORWARDER_TOKEN_URL`, a batch of 1 to 500 and an interval of 1 to 3600 seconds. The consumer and the graph verification key (`GRAPH_EVENT_PUBLIC_KEY_PATH`) are required whenever `SIGNED_COST_EVENTS_ENABLED=1`, forwarder or not. | 5.8 |
| `RERANK_INPUT_INCLUDE_CONTEXT` | `0` | None. | 1 |
| `EVAL_RECORD_RESULTS`, `EVAL_REQUIRE_COVERAGE`, `EVAL_REQUIRE_PARITY` (eval-nightly.sh), `STUDY_CLASSES` (eval-attribution-study.sh), `EVAL_DOCKER` (the Python eval scripts) | `0`, `1`, `1`, `natural norwegian mixed`, `docker`. Recording is off until the data-quality image with `POST /v1/evals/results` is deployed. | Each of the first three must be `0` or `1` or the nightly stops with exit 2 before it touches anything; `STUDY_CLASSES` must name known classes and include `natural` (exit 2); the gate exits 2 on a bad `EVAL_REQUIRE_*`. | 5.12 |
| `MODEL_PLANE_EMBEDDING_INFERENCE_RETENTION_POSTURE` and `MODEL_PLANE_INFERENCE_RETENTION_POSTURE` (graph-index) | `persistent` until the Control Plane image has the tenant code, then `tenant` | The client asserts the token's posture equals this value and fails every mint on a mismatch. With `tenant`, graph-index asks for a bearer per organisation before it extracts for it: the token's `zdr` claim is that organisation's own setting, `zdr: true` ends in `skipped_zdr` with no provider call, and a 503 `retention_posture_unavailable` leaves the chunks pending (never a send, never a skip). | 5.3, 5.9 |
| `GRAPH_EXTRACTION_PROVIDER`, `AZURE_OPENAI_API_KEY`, `AZURE_OPENAI_ENDPOINT`, `AZURE_OPENAI_EXTRACTION_DEPLOYMENT` (graph-index) | Gone. The direct-Azure backend is deleted from the code and from the container's environment: a graph model call goes through `inference-core` with the Auth Core service token or not at all. | Setting `GRAPH_EXTRACTION_PROVIDER=azure_openai` stops the service at startup with the message that says so. `tests/scripts/graph-index-no-azure-contract-test.sh` fails when the compose, or an isolated overlay, brings the variable or the key back. | 5.9 |

Isolated test stacks (`tests/e2e/run-isolated-mvp.sh` and the two real-authority runners) have no `inference-core` that can write a context. Each of the three exports `CONTEXTUAL_RETRIEVAL_ENABLED=false` and a `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON` (CR-T13), so that the feature is off there by name and with a reason; `tests/scripts/contextual-retrieval-config-contract-test.sh` fails when one stops doing it. The non-browser real-authority runner starts no embedding engine today, and carries the line so that a service added later does not meet a switch it cannot pass.

## 4. Standing queries

Run as `docker exec -i data-plane-v2-postgres-1 psql -U dataplane -d dataplane`. Replace `<org>`.

**Coverage, after CR-T6.** One row per organisation. `eligible` is the population the keyword index also uses (embedding done, document not deleted, classification internal, public or sensitive). `vector_incoherent` must be 0. `ready_unverified`, `status_unknown` and `legacy` count as not yet ready.

```sql
SELECT * FROM v_context_coverage WHERE org_id = '<org>';
```

**Backlog and failures, after CR-T6.**

```sql
SELECT context_status, context_error_class, count(*) AS units,
       min(context_next_attempt_at) AS oldest_due, max(context_attempts) AS max_attempts
  FROM knowledge_units
 WHERE org_id = '<org>' AND embedding_status = 'done'
 GROUP BY 1, 2 ORDER BY 1, 2;
```

**Leases held right now, after CR-T9a.** A lease expires by itself; a crashed worker's rows free up when the lease time passes. A unit's lease also ends in the statement that marks it embedded, so a number that stays above zero between passes is a worker that is stuck or dead, not a unit that was finished.

```sql
SELECT count(*) FROM knowledge_units WHERE org_id = '<org>' AND context_lease_until > now();
```

**Today, before the migration.** The only numbers that exist.

```sql
SELECT count(*) FILTER (WHERE chunk_context IS NOT NULL AND btrim(chunk_context) <> '') AS with_context,
       count(*) FILTER (WHERE chunk_context IS NULL OR btrim(chunk_context) = '')      AS without_context,
       count(*) FILTER (WHERE embedding_model IS NULL)                                 AS embedding_model_null
  FROM knowledge_units WHERE org_id = '<org>' AND embedding_status = 'done';
```

On 2026-10-06 the single organisation reads 206 with a context, 1 without, and `embedding_model` NULL on all 207 (`embedding_model` is written only when the legacy verification pass re-embeds a unit).

**Spend by feature, after CR-T10** (column names as designed; check with `\d cost_events`).

```sql
SELECT feature, model, sum(input_tokens) AS input_tokens, sum(output_tokens) AS output_tokens, sum(count) AS calls
  FROM cost_events
 WHERE org_id = '<org>' AND created_at >= date_trunc('month', now())
 GROUP BY 1, 2 ORDER BY 3 DESC;
```

**Keyword index parity, today and after CR-T9a.** Expected is the quickwit-adapter eligibility predicate; present is Quickwit's `knowledge_unit` documents with a `context_body`.

```sql
SELECT count(*) FROM knowledge_units ku JOIN documents d ON d.document_id = ku.document_id
 WHERE ku.org_id = '<org>' AND ku.embedding_status = 'done' AND d.deleted_at IS NULL
   AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive');
```

```
docker exec data-plane-v2-quickwit-1 curl -s -X POST -H 'content-type: application/json' \
  http://localhost:7280/api/v1/dataplane-corpus/search \
  -d '{"query":"entity_type:knowledge_unit AND context_body:*","max_hits":0}'
```

This Quickwit refuses the `cardinality` aggregation, and a delete is eventual, so a count can exceed the distinct chunk count. Compare after merges, and de-duplicate by `knowledge_id` when the numbers matter. The quickwit-adapter that CR-T9a builds has two read-only endpoints that do both (built and tested, not deployed): `GET /admin/index-parity`, the `index-parity-v1` report that the `sparse_index_in_sync` gate reads, and `GET /admin/index-parity/documents`, the ids of the documents that differ (section 5.7).

**Signals that exist as queries, not as metrics.** The design lists embedding-engine and graph-index metrics (`ctx_units_total`, `ctx_backlog`, `usage_forward_lag_seconds` and others) and alert rules. No build task delivers them: only the `inference-core` counters (`inference_requests_total`, `inference_tokens_total`, CR-T3) and the budget counter `ctx_budget_exceeded_total` (CR-T9b) are assigned. The Data Plane compose does carry an optional Prometheus and Grafana behind the `obs` profile (`apps/Data Plane v2/infra/prometheus/`), but it is not running on this stack. It scrapes `retrieval-engine`, four Go services and, since CR-T9b, `embedding-engine` (the one counter it serves on `GET /metrics`: `graph-index` and `inference-core` are not scraped, and `graph-index` has no `/metrics` route), and of its twelve alert rules one concerns contextual retrieval, `BackgroundLlmBudgetExceeded`; none concerns keyword index parity or usage forwarding. Until that gap is closed (section 7), the queries above are the alerts, and the release gates of CR-T11 are the only automatic check.

## 5. Procedures

Each procedure gives the signal, the checks, the action, how to verify, and what not to do.

### 5.1 Provider 429 storm

**Signal.** Today: units embedded raw. `without_context` rises on new documents while `inference-core` answers `RESOURCE_EXHAUSTED`. Nothing records why (an empty answer is not logged), so the count is the only signal. After CR-T8: `failed_transient` with `rate_limited` rising in the backlog query; after CR-T3: the `inference_requests_total` outcome for service `embedding-engine`.

**Why it happens.** Background work shares the Azure deployment with chat. A dedicated deployment alias (OD4, operator action OPS-ALIAS) removes the sharing. Rerank has measured the same pattern: about 19 percent of queries lost the cross-encoder to 429s under eval load.

**Check.** Is chat affected too (model-gateway and `inference-core` logs)? Is it one organisation (a large first ingest) or all?

**Act.**

1. A burst needs nothing. After CR-T7 the retry honours the server's retry-after (`x-mp-retry-after-ms`, capped at 5 minutes), ingest never fails because of it (I4), and the reconciler heals the units. Today the retry is a doubling ladder (2, 4, 8 and 16 seconds, offset per chunk) sized to span the provider's 30-second window, and it does not read the header.
2. If chat is hurt, lower `CONTEXTUAL_RETRIEVAL_CONCURRENCY` (today 4) and recreate `embedding-engine` with `--skip-build`. After CR-T9a also lower `CONTEXT_RECONCILER_CONCURRENCY` and `CONTEXT_RECONCILER_ORG_CONCURRENCY` (2 and 1 by default).
3. If it lasts more than 15 minutes, pause the reconciler. The circuit breaker already pauses it for 5 minutes after 3 passes in a row that failed (a refused credential opens it at once), and the reconciler waits at least as long as the provider's retry-after (capped at 5 minutes); the manual pause is the pause file, which needs no restart: `docker exec data-plane-v2-embedding-engine-1 touch /tmp/context-reconciler.pause` (remove it to resume; section 5.2). `CONTEXT_RECONCILER_ENABLED=0` with a recreate also stops it.
4. If a large ingest is the cause, set `CONTEXT_INLINE_BUDGET_MS=0` so new documents embed raw first and are contextualised in the background.

**Verify.** `failed_transient` falls and `ready` rises in the backlog query; chat latency returns.

**Do not** switch contextual retrieval off to relieve load. The kill switch is for an incident in which content must stop leaving (section 5.6).

### 5.2 Stuck backlog

**Signal.** `pending`, `failed_transient` or `legacy` does not shrink; the oldest `context_next_attempt_at` is far in the past; `contextual_coverage_ok` fails (coverage under 99 percent for 30 minutes).

**Check, in this order.**

1. `context_error_class = 'auth'`: a credential or posture problem. Stop. The reconciler does not burn attempts on it. Fix the mint (is `auth-service` up, is the principal valid, is the posture env equal to the registry value) before anything else.
2. `skipped_zdr` (class `zdr_org`, `zdr_document` or `zdr_unavailable`): the organisation is ZDR and no provider is attested, or the document is restricted. Expected, not a fault (section 5.3). `pending` with class `posture_unavailable` is different: Auth Core could not resolve the organisation's posture. Check that Auth Core and the organisation service are up before anything else (section 5.3).
3. `skipped_budget`: the budget mode is `stop` and the budget is used (section 5.5).
4. `skipped_policy`: an operator turned the organisation off (section 5.6).
5. `failed_permanent` with `too_long` or `content_filter`: content problems. Read the class, do not retry blindly.
6. `failed_permanent`: the reconciler gave up on the unit after `CONTEXT_RECONCILER_MAX_ATTEMPTS` (8) attempts of its own, with the class it failed with, and logged it once per organisation as an error with `alert="context_units_failed_permanent"` and a count. Read the class, fix the cause. Nothing requeues such a unit yet (section 7).
7. Nothing wrong but nothing moving: is `CONTEXT_RECONCILER_ENABLED=1` (the log has `context reconciler running`); is the pause file there (`docker exec data-plane-v2-embedding-engine-1 ls /tmp/context-reconciler.pause`); is the breaker open (the log says `context reconciler paused: the circuit breaker is open` and when it reopens); is `context_next_attempt_at` still in the future (the ladder: 1 minute, 5, 30, 2 hours, 12 hours); is a lease held (queries above; a lease runs out by itself within `CONTEXT_RECONCILER_LEASE_SECS`)? A unit whose vector cannot be built (the embedding provider fails for it) is asked again on the same ladder, from the reconciler's own memory, because a write to the unit would make the stale-embedding detector report it; after `MAX_ATTEMPTS` of those the log says `alert="context_unit_unresolvable"` once and the unit goes on being tried at the slowest step. A restart of the service forgets that memory, so the ladder starts again.

**Act.** Preview, then run, through the one tool that shares the reconciler's code. `context-backfill` is in the embedding-engine image, uses the service's own environment, and leases its units the way the service does, so it can run next to a running reconciler without touching the same unit twice:

```
docker exec data-plane-v2-embedding-engine-1 context-backfill --dry-run --org-id <org>
docker exec data-plane-v2-embedding-engine-1 context-backfill --org-id <org> --max-units <n> [--max-usd <usd>]
```

The dry run changes nothing and prints one JSON line per organisation and a total: the units that need a context, the `legacy` units that will only be verified (no model), the vectors that are not the one their context makes, the units that failed every attempt, the zero-retention skips, the units waiting on their ladder, and the estimated provider calls, tokens and spend. Counts and numbers only, never content. The estimate prices default to the gpt-4o-mini row of the Model Plane catalogue; give `CONTEXT_ESTIMATE_EMBED_USD_PER_MTOK` (`docker exec -e`) to have the embedding cost estimated too, otherwise it is printed as unknown. A real run needs `--org-id` and a cap, `--max-units` or `--max-usd` or both (the unit cap is exact; the spend cap stops before the estimated generation spend passes it). Its report says why it stopped: nothing left to do, unit cap reached, spend cap reached, or paused. The exit status is 0 when it finished or stopped at a cap, 1 when it failed, 2 for a wrong command line and 3 when it was stopped before it could finish (the pause file, the breaker). Run the dry run before any real run, and always with a cap. `--target-version` is the prompt refresh of section 5.4.

**What a pass does**, organisation by organisation, each with its own share of the pass so that one organisation's backlog never starves another's:

1. Gives up on units whose attempts are used (`failed_permanent`, above).
2. For an organisation whose policy is off, records `skipped_policy` once and calls nothing; for one whose policy was lifted, generates the units that were skipped.
3. **Generates** a context for each unit that has none, then embeds the composed text and writes the vector (contexts are written ahead of the vector, so a crash in between leaves a unit the next pass repairs).
4. **Verifies** each `legacy` unit: it embeds the stored context and the chunk again, with no model call, and the unit becomes `ready` only when the hash recorded for the new vector is the hash of the context the unit holds now. A context that changed meanwhile, or a vector that was not built, leaves the unit `legacy`.
5. **Repairs** a `ready` unit whose vector is not the one its context makes (the batch that wrote the context died before it wrote the vector).
6. Regenerates a **stale** chunk (off by default) and **refreshes** a context of another prompt version (off by default, section 5.4).

A unit whose context is right and whose vector matches costs nothing: a second run makes zero provider calls (invariant I7), which the tests prove against a real schema.

`dataplane.documents.indexed` is announced, signed, for a document that had a context written or a vector repaired, so the keyword index and the graph follow the change. A verification changes neither the keyword text nor the stored context, so it announces nothing: an announcement would make graph-index extract the document again and Meilisearch re-index it for no change.

**Pause and resume without a restart.** `docker exec data-plane-v2-embedding-engine-1 touch /tmp/context-reconciler.pause` stops every pass from the next one on (also a running `context-backfill`, which exits 3 at its next pass); `rm` resumes. The file is in the container's `/tmp`, so a recreate removes it. The incident kill switch (`CONTEXTUAL_RETRIEVAL_ENABLED=false`, section 5.6) takes the model away and leaves the reconciler's repairs and verifications running, since they call no model.

**Verify.** `ready / eligible` is at least 99 percent, `legacy` is 0 and `vector_incoherent` is 0 (the coverage query of section 4). The log line `context reconciler pass` carries the counts of a pass (`contexts_written`, `legacy_verified`, `vectors_repaired`, `announced`, `given_up`, `errors`), never text.

**Do not** update `context_status` or `context_attempts` by hand. The reconciler's writes compare the input hash and assume one writer path; a hand edit can leave a vector built from a context that no longer matches. Until the images are deployed there is no reconciler: the one raw unit stays raw. `reembed-switch` (index-engine) replays `knowledge.units.created` for an organisation and re-runs the pipeline, which, after CR-T8, reuses a stored context by its hash and regenerates only what changed; before CR-T8 is deployed it regenerates every context and pays for every one.

### 5.3 A ZDR organisation reports or asks

Use this when an organisation with zero retention switched on says its documents were processed, or asks whether they were.

**Today (what is true).** The organisation's own ZDR setting is not consulted by contextual retrieval or graph extraction. Their service tokens carry the fixed posture `persistent`, contextual retrieval sends `zdr: false`, and only a document classified `restricted` (or a signed event with the ZDR flag) is skipped. No provider is attested for ZDR, so if the organisation's setting were read the calls would be refused, but it is not read. Index-time text embedding is guarded only per document, and query-time embedding and rerank only by the request's own ZDR mode; none of them reads the organisation's setting (owner decision OD-ZDR-INGEST is open; the gap is stated in honest-limits L33). On 2026-10-06 none of the three organisations has ZDR intent set, so nothing has been sent on that basis.

**Check.**

```sql
SELECT zdr_classification, count(*) FROM documents WHERE org_id = '<org>' AND deleted_at IS NULL GROUP BY 1;
SELECT count(*) FILTER (WHERE chunk_context IS NOT NULL AND btrim(chunk_context) <> '') FROM knowledge_units WHERE org_id = '<org>';
```

The second number is the least that was sent (one call per unit; retries are not counted). No ledger records the calls today. For graph extraction, count the organisation's rows in `graph_entities` and `graph_text_units` (both carry `org_id`); only organisation-visible documents are extracted. After the CR-T12 migration the state table answers it directly: `SELECT status, error_class, count(*) FROM graph_extraction_state WHERE org_id = '<org>' GROUP BY 1, 2;` (`skipped_zdr` and `skipped_visibility` are chunks that were not sent, `ready` and `empty` are chunks that were).

**Act today.** There is no per-organisation switch. The only containment is global and needs the owner told first, because it is the kill switch (section 5.6). Do not tell the organisation that its setting was honoured. Use the text of honest-limits L33.

**After the Control Plane wave and CR-T5, CR-T7, CR-T12.**

- The service token carries the organisation's posture, resolved at mint. A ZDR organisation gets a token with `zdr: true`, `inference-core` refuses every non-attested provider before any network call, and the units become `skipped_zdr` with zero provider calls and a distinct class (`zdr_org`, against `zdr_document` for a restricted document).
- When the organisation's posture cannot be resolved, Auth Core returns 503 `retention_posture_unavailable` and issues no token. The Data Plane treats it as `pending` with the class `posture_unavailable` and the next attempt in 5 minutes: never a send, never a permanent skip.
- Staleness, as the Control Plane code states it (in the working tree and in the running Auth Core image of 2026-10-04, which no principal uses yet): after an organisation turns ZDR on, a worker can still hold a `zdr: false` token for up to the 45-second resolver cache plus the 60-second token lifetime, about 105 seconds. While the organisation service is unreachable, a stale non-ZDR answer is served for at most 15 minutes, plus 60 seconds. The Data Plane client reuses a token for at most 45 seconds (refresh margin `min(60 s, ttl/4)`), which is inside that bound. Quote the Control Plane's final figures, not these.
- The check that must read zero: provider calls recorded for an organisation, with `zdr = false`, later than the staleness window after its posture became ZDR (usage ledger rows with the feature `contextual-retrieval` or `graph-extraction`). A row inside the window is the disclosed window, not a defect; a row after it is one.
- A ZDR organisation still ingests: its knowledge base is built, the keyword and graph-local arms work, and context generation is skipped with a visible marker.
- When an organisation stops being ZDR, the reconciler (CR-T9a) notices at most `CONTEXT_RECONCILER_ZDR_RECHECK_SECS` (an hour) later: it asks for a bearer for each organisation that has `skipped_zdr` units, and when the token says the organisation is no longer zero-retention it makes the units of class `zdr_org` and `zdr_unavailable` `pending` and due, and generates them at the next pass. A unit skipped because its document is restricted (`zdr_document`) stays skipped whatever the organisation is. When Auth Core cannot say, the units stay as they are and the organisation is asked again in 5 minutes: a wait, never a skip and never a send. The check is a mint, not a model call. `skipped_zdr` units are found by a scan of the table once an hour; an index for it belongs to the migration's owner (section 7).

**Do not** lower ZDR for an organisation to make a backlog clear, and do not reach for a user's own connection: background work never uses a subscription or bring-your-own route.

### 5.4 Prompt rollout and rollback

**Prerequisites.** The CR-T14 eval harness (section 5.12) and its baseline exist, and the owner has approved the paid re-seed (OD8). A prompt or model change ships only through the quality gate: non-inferiority (nDCG and MRR no worse than the current default by more than 0.01), coverage at least 99 percent on the contextual organisation, and the exact-identifier class unchanged.

**Roll out.**

1. Pin the canary organisation: `context_prompt_version` in its `org_processing_policy` row (operator API of CR-T11).
2. Run the eval cells for the default and the candidate with `eval-variant-cell.sh` (reranker off for attribution; confirm with the reranker on, three runs each, because 429 noise is about ±0.04 nDCG), and let `eval-gate.py --promote` decide (section 5.12).
3. If the gate passes, widen the pin organisation by organisation, then change the global default.
4. Refresh existing units only inside a cost cap: either `CONTEXT_REFRESH_ON_PROMPT_CHANGE=1` with `CONTEXT_REFRESH_MAX_UNITS=<n>` (the service refuses to start with the first and no cap; the cap counts over the life of the process), or one organisation at a time with `context-backfill --org-id <org> --target-version <version> --max-units <n>` after `--dry-run --target-version <version>` has priced it (section 5.2). A refresh re-embeds every affected unit: one Cohere embed per unit, plus the generation. It touches only a unit whose context was made with another prompt version than the one in force for its organisation, and a regeneration that fails changes nothing about the unit and is not asked for again before its wait (the time the failure wrote, else the ladder of section 5.2). The units of an organisation that is zero-retention now, or whose posture Auth Core cannot resolve, are not refreshed at all (a bearer is minted to ask, no model is called), and neither are they regenerated by the stale pass: a regeneration that cannot be made would still embed the unit again.

**Roll back.** Set the policy pin back to the previous version and run a refresh to it. A failed regeneration never overwrites a ready context (the previous context and its coherent vector stay), so a half-finished refresh is safe to stop.

**Verify.** Coverage by `context_prompt_version`; `vector_incoherent = 0`; the eval cell for the restored version matches the stored baseline.

**Do not** change the model alias (`gpt-5` family models need reasoning settings and a higher token cap) outside an eval cell. No model change without the gate.

### 5.5 Budget exhausted

**Owner default (D-E).** The budget is configurable per organisation and globally, and the default is alert-only: exceeding it alerts and does not stop. The owner supplies the figure later. No figure is built in.

**Signal.** In embedding-engine, `ctx_budget_exceeded_total{mode}` on `GET /metrics` (counted at most once per organisation per 30-second spend reading, so it is a rate an alert can read; the Prometheus rule is `BackgroundLlmBudgetExceeded`, which needs the compose `obs` profile) and one structured log line per organisation per five minutes with `alert="ctx_budget_exceeded"`, the organisation, its mode, the spend and the figure (graph-index writes the same words in its log and has no metric). A budget that applies and cannot be measured logs `alert="ctx_budget_unmeasured"` with the cause (`no_prices`, `usage_ledger_off`, `ledger_unreadable`) and the work proceeds: it is never stopped on a guess. When an operator set `stop`, the units are `skipped_budget` (counted apart in `v_context_coverage` and in the dry run), and `context-backfill` of that organisation ends with `stopped: the organization's background budget is used up and its budget mode is stop` and exit status 3.

**Check.** Spend by feature (section 4). Is it a real increase (a large ingest, a refresh) or a loop (section 5.9, or a prompt refresh run twice)? The gate counts the month's `llm` tokens of the organisation in `cost_events`, every feature of them: `SELECT feature, sum(input_tokens), sum(output_tokens) FROM cost_events WHERE org_id = '<org>' AND schema_version = 2 AND event_type = 'llm' AND created_at >= date_trunc('month', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' GROUP BY 1`, priced with `BACKGROUND_PRICE_*`.

**Act.**

1. `alert_only` (default): decide whether to raise the budget or let it run. Nothing stopped.
2. `throttle`: the cap is the intended effect (one chunk per batch and per reconciler pass, the rest `pending`); raise the budget or wait.
3. `stop` (explicit, audited): raise `monthly_budget_usd` or set the mode back. Nothing else is needed: a `skipped_budget` unit is work again at the first reconciler pass in which the gate lets the organisation through (the policy is read within 30 seconds), and a new month does the same by itself. A `skipped_budget` unit is an explicit state with a marker, never a silent raw embed, and a stopped organisation's units are embedded from their raw text once (to be marked) and not again at every pass. Whether `stop` may be used on customer organisations is open (OD-PER-ORG-OFF); until the owner rules, it is for eval organisations and incidents.

**What holds, and what it does not.** The incident kill switch comes first: with it thrown nothing is generated, the gate is not consulted and nothing is marked. An organisation that is `off` is `skipped_policy` whatever its budget says. A stop is decided before a bearer is minted (it makes no call), so a zero-retention organisation over a stopped budget is marked `skipped_budget` and, once the budget lets it through, `skipped_zdr`: nothing is sent in either case. A restricted document keeps its `skipped_zdr` mark. A stop withholds regenerations too (the context a unit has stays in force) and never removes anything.

**What works the held units afterwards.** A `throttle` leaves the chunks it did not let through `pending`, and a lifted `stop` leaves `skipped_budget` units raw until something generates them. That something is the reconciler (`CONTEXT_RECONCILER_ENABLED=1`, which is off until its live proof, section 5.2). With it off, a deferred or marked unit waits until its document is embedded again (which asks the gate afresh) or an operator runs `context-backfill` for the organisation (which does not need the flag); nothing is lost and the unit's status says why it waits, but nothing works it at the throttle's pace. Turn the reconciler on before relying on `throttle` or on a stop lifting by itself.

**Do not** read the cost-core chat budget for this. Data Plane spend is read from `cost_events` and the policy table; cost-core's chat budget check excludes these features (CR-T4) so that background spend cannot downgrade a person's chat model.

### 5.6 Kill switch and purge

**What the switch is.** Contextual retrieval is on and stays on (owner ruling of 2026-10-03). The global flag is an incident-only kill switch; the finer controls are, from finest to coarsest: a document classified `restricted` (existing), an organisation's `contextual_retrieval = off` (operator-only, audited, reason required, a visible `skipped_policy` status, no org-admin UI), and the global flag.

**When to use it.** Only when content must stop going to the provider: a confirmed posture failure, a provider incident, a legal instruction. Not for load (5.1) and not for cost (5.5). Tell the owner before using the global flag.

**Today.** Set `CONTEXTUAL_RETRIEVAL_ENABLED=false` in the Data Plane `.env`, recreate `embedding-engine` through the launcher with `--skip-build`, and write down why. This stops new generation. It removes nothing: existing contexts stay in `knowledge_units.chunk_context`, in `content_tsv` and in Quickwit `context_body`, and are folded into the stored vectors. There is no reason text and no readiness report in the running image.

**After CR-T13 is deployed (built and tested, not deployed).** Set the flag to `false` and `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON` to the reason (8 to 240 characters, one line, for an operator to read: no customer name, no personal data), then recreate `embedding-engine` with `--skip-build`. Boot fails without the reason (exit status 1, naming both variables), logs it (`contextual retrieval is OFF` at warning level, with the reason) and `/readyz` reports `{"state":"kill_switch","reason":...}` under `contextual_retrieval` without making the service not ready. Check: `docker exec data-plane-v2-embedding-engine-1 curl -s http://127.0.0.1:9202/readyz`. To switch back on, remove both variables (an empty switch is on) and recreate. Per organisation, use the operator API (CR-T11) instead of the global flag whenever one organisation is the problem.

**Purge: not built.** The design says a purge reuses the reconciler (clear `chunk_context` and change the status in the same statement, re-embed raw, re-announce `documents.indexed`). No task in the build delivers it, so there is no supported purge. What does exist: deleting a document deletes its units and their contexts (`ON DELETE CASCADE`), an organisation erasure removes the organisation's rows, and the keyword indexes are not purged by that path (honest-limits L22 and L35). A context-only purge is a follow-up (section 7). Do not clear `chunk_context` by hand: once the CR-T6 migration is applied the table refuses it for `ready` and `legacy` rows (a constraint says those statuses need a context), and today nothing does, and an edit that also changes the status leaves the vectors contextual and the keyword indexes on the old text.

**Verify.** Provider calls for the affected scope stop (usage rows, `inference-core` counters); coverage shows `skipped_policy` rather than a drop in `eligible`.

**To switch back on.** Remove the policy row or set `inherit`, or set the flag to `true`; run `context-backfill --dry-run`, then a capped run.

### 5.7 Quickwit parity repair (the keyword index)

**Signal.** The parity gate `sparse_index_in_sync` fails, or the counts in section 4 differ. **Today:** Quickwit holds 52 knowledge-unit documents (51 with a `context_body`) against 207 units in Postgres, so the keyword arm reaches at most 25 percent of the chunks. The 155 missing units sit in 138 live documents (136 private SharePoint, 1 organisation SharePoint, 1 Quarry), all classified internal and embedded, and every one already has its stored context. Why they never reached the index is not known: the embedding-engine log of that period has rolled over, and the adapter log holds one admin-job claim failure (error `Storage`, 2026-10-01). `quickwit_admin_jobs` has no rows, so the rebuild path has never run here.

**Repair: re-announce, do not rebuild.** `documents.indexed` rides the JetStream stream `DATAPLANE_GRAPH`, and `quickwit-adapter` consumes it with a durable pull consumer that deletes the document's chunks and re-indexes them from Postgres, with their stored `chunk_context`. The repair is to re-publish that event for the documents that are missing:

```
docker exec -e RUST_LOG=warn data-plane-v2-embedding-engine-1 \
  backfill-reannounce --org-id <org> --document-id <doc> [--document-id <doc> ...] [--dry-run]
```

Run `--dry-run` first. Name documents: the tool refuses a whole organisation unless you say `--all-documents` (and refuses `--all-documents` together with named documents). The reconciler (CR-T9a) announces `documents.indexed` itself for a document whose context it writes or whose vector it repairs, so the keyword index follows those; it does NOT announce a document that is merely missing from the index, because it has no access to Quickwit. Which documents are missing is what the parity endpoints say.

**Measure (CR-T9a; built and tested, not deployed).** Both endpoints are read-only, answer with `Cache-Control: no-store`, and need a bearer with the scope `data:quality:admin` for the organisation measured (the same operator token as the policy API, section 5.11; the organisation is the token's, a query parameter is not read, and no principal needs a new grant). A measurement that cannot be taken is `503 index_parity_unavailable` and never a partial number. They are reached from inside the network (the adapter's port 9204 is not published):

```
docker exec -e TOKEN data-plane-v2-quickwit-adapter-1 sh -c \
  'curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9204/admin/index-parity'
docker exec -e TOKEN data-plane-v2-quickwit-adapter-1 sh -c \
  'curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9204/admin/index-parity/documents'
```

- `/admin/index-parity` is the `index-parity-v1` report the `sparse_index_in_sync` gate reads (section 5.11): Postgres' eligible units and those with a stored context; Quickwit's documents (as the index counts them, so a delete that has not merged yet counts twice), its distinct units and those with a `context_body`; and the four differences `missing_units`, `unexpected_units`, `context_body_missing` and `context_body_unexpected`. The index is read by terms, not by a count, because this Quickwit refuses `cardinality`; the read is split by id prefix when it is too big for one request, and the endpoint answers 503 rather than a number if the parts do not add up to the whole.
- `/admin/index-parity/documents` (`index-parity-documents-v1`) is for the operator: the ids of the documents whose units are missing from the index or whose `context_body` differs, with counts per document (`missing_units`, `context_body_missing`, `context_body_unexpected`), at most 500, with `documents_with_drift` and `truncated` to say when the list is cut. Ids and counts only, never text. Units that are in the index and should not be (`unexpected_units`, stale after a deletion or a classification change) name no document and are only counted here; section 7.

**Repair.** Take the ids from `/admin/index-parity/documents` and announce exactly those, `--dry-run` first, then without it:

```
docker exec -e RUST_LOG=warn data-plane-v2-embedding-engine-1 \
  backfill-reannounce --org-id <org> --dry-run --document-id <doc> [--document-id <doc> ...]
```

Then measure again after the merges. A context-body difference is the same repair: the adapter re-indexes the document from Postgres with its stored context. Every document announced is also re-indexed by Meilisearch, and extracted again by graph-index when it is organisation-visible (paid; see below), so name only the documents the measurement lists.

**Do not** run `scripts/rebuild-quickwit.sh` for this. It clears the index by default (`CLEAR=true`; only `--no-clear` keeps it), it sends no bearer, the route needs a scope and a second approval scope that no principal holds, and it has never run here (the script now says so at its head). **Do not** re-announce a whole organisation until CR-T12 is deployed: a re-announce also triggers graph extraction for organisation-visible documents (paid, and today with no memory of earlier results) and a Meilisearch re-index. Of the 138 documents above, 2 are organisation-visible; the 136 private ones are not extracted by the graph. Once CR-T12 is deployed this caution lifts for the graph: the legacy backfill marks every chunk that already has graph rows `ready`, an extraction that came back empty is remembered as `empty`, and neither is extracted again for the same prompt version, so a re-announce makes no graph call for them (the Meilisearch re-index still happens). A chunk whose earlier extraction came back empty left no trace before CR-T12: it is extracted once more, then remembered.

**Verify.** Re-run the two counts of section 4 after the merges (Quickwit applies deletes at the next merge, so for a while a chunk can appear twice; retrieval de-duplicates by `knowledge_id`). The gate passes when expected equals present after de-duplication and every present unit with a stored context has a `context_body`.

### 5.8 Cost spike triage

**Signal.** Spend by feature (section 4) or cost-core's feature aggregate (CR-T4: `GET /api/v1/cost/aggregate` with the feature filter) rises; `inference_tokens_total` for service `embedding-engine` or `graph-index` rises faster than ingest.

**Today.** There is nothing to triage with: calls to the model for contextual retrieval and graph extraction are not metered anywhere. The Data Plane's own ledger holds rerank (265 events, latest 2026-10-05) and embedding (17 events, latest 2026-09-28) only, and cost-core holds rows from the producer `model-gateway` only (1,716 on 2026-10-06). The estimate is units with a context times about $0.00013 per chunk at gpt-4o-mini prices (design estimate, not measured): the live index of 207 chunks is about three cents.

**Triage, after CR-T10 and CR-T4.** In order:

1. Which feature, which organisation, which model (query above).
2. A loop: the same `request_id` or unit paying repeatedly. Check `context_attempts`, and whether a prompt refresh or `reembed-switch` ran twice. I7 says an unchanged input makes zero calls; a violation is a defect.
3. A backfill without a cap: `context-backfill` was run without `--max-usd`.
4. Graph extraction: a poison chunk retried. Section 5.9.
5. Prices: until sourced rows exist for `embed-v-4-0` and the rerank model, cost-core would price them at its default (3.00/15.00 per million, about 25 times too high for embeddings). The forwarder stays dark until those rows are supplied (OD-PRICES, operator action OPS-PRICE). Do not invent a price.

**What the ledger is (CR-T10).** Every paid call is an event on the durable stream `DATAPLANE_COST` (subject `dataplane.cost.ledger`, retention Limits, 7 days, 512 MiB; `infra/nats/SUBJECTS.md`), signed by its producer (embedding-engine, graph-index, retrieval-engine each have their own key) and read by data-orchestrator's durable consumer `data-orchestrator-cost`, which writes `cost_events` after verifying the signature and acknowledges only after the row is written. An event published while the consumer is down, restarting or cannot reach Postgres is read later, exactly once into the table: an authentic event whose own two-minute lifetime has passed in the stream is accepted (and counted in `dpv2_data_orchestrator_go_usage_events_stale_accepted_total`); a forged one never is. Events that are not authentic, or that the table refuses for what is in them, are terminated and stay in the stream for 7 days (`nats stream get DATAPLANE_COST <sequence>`; the log names the sequence and nothing else about the event, and `dpv2_data_orchestrator_go_usage_events_consumed_total{result="invalid"|"permanent_failure"}` counts them). The table's columns are the migration `20261003140000_cost_events_v2`: `feature`, `provider_used`, `input_tokens`, `output_tokens`, `cache_read_tokens`, `units`, `cost_basis` (`metered` or `estimated`), `estimated`, `zdr`, `request_id` (the digest of the calls' request ids), and the forwarder's own `forwarded_at`, `forward_attempts`, `forward_next_at`, `forward_lease_until`, `forward_error`. `count` is the number of items the event covers: provider calls for contextual retrieval and graph extraction, texts for embedding, candidates for rerank. Embedding tokens are an estimate (bytes / 4: an embedding request returns vectors and no usage) and the row says so (`estimated = true`); contextual retrieval and graph tokens are exactly what the provider reported. Spend is booked to the organization, never to a user.

**Zero-retention organizations are metered.** Their rows carry counters only (organization, model, provider, feature and the numbers) and `zdr = true`; no document, chunk, text or user is in an event, and the table refuses a zero-retention row that names a user. This reverses the earlier convention (the consumer used to drop those events, which made their spend invisible), for the Data Plane only. Spend by feature for one organization, with the zero-retention part apart:

```sql
SELECT feature, model, zdr, sum(count) AS items, sum(input_tokens) AS input_tokens, sum(output_tokens) AS output_tokens,
       bool_or(estimated) AS any_estimate
  FROM cost_events
 WHERE org_id = '<org>' AND schema_version = 2 AND created_at >= date_trunc('month', now())
 GROUP BY 1, 2, 3 ORDER BY 4 DESC;
```

Version 1 rows (the count-only event of the producers that predate the ledger, 282 on 2026-10-06) keep their numbers, gain only a feature of the same name (`embedding`, `rerank`), and are never forwarded. What is waiting to be forwarded, and for how long:

```sql
SELECT count(*) AS waiting, coalesce(extract(epoch FROM now() - min(created_at)), 0)::int AS oldest_seconds,
       count(*) FILTER (WHERE forward_error IS NOT NULL) AS failing
  FROM cost_events WHERE forwarded_at IS NULL AND schema_version >= 2;
```

The same two numbers are the gauges `dpv2_data_orchestrator_go_usage_forward_pending` and `dpv2_data_orchestrator_go_usage_forward_lag_seconds`. `forward_error` is one of `rejected` (cost-core read the row and refused it: a deploy or a person, not a retry; the pause goes to six hours), `unauthorized` (cost-core refused the bearer; the next pass asks Auth Core for a fresh one), `unavailable` (cost-core or the network; retried, pauses of 30 seconds doubling to an hour) or `token_unavailable` (Auth Core would not mint for the organization: the principal's credential, or the organization is not in the directory).

**Enable metering in this order.** Consumer and verifier deployed first, then `USAGE_EMIT_ENABLED=1`, then (after cost-core accepts `feature` and `units`, and the prices are in) `USAGE_FORWARDER_ENABLED=1`. A forwarder that runs before cost-core ships is rejected by its strict decoder. For ZDR organisations, usage is recorded as content-free counters with `zdr = true` (no document ids); this reverses the earlier suppression for the Data Plane only and is recorded in the ledger.

**Verify.** One call gives exactly one cost-core row with feature, organisation and exact tokens; replaying it leaves one row; the chat budget aggregate is unchanged.

### 5.9 Graph extraction retry loop

**Signal.** The same chunk or document extracted again and again. Seen on 2026-09-29: one single-chunk document retried at about six-minute intervals at least ten times, then succeeded with no entities, was remembered only in process memory, and was paid for again after the restart. The log hid the real gRPC status.

**On the live image of 2026-10-06 (until CR-T12 is deployed).** `GRAPH_RECONCILE_ENABLED=1` (interval 300 seconds, batch 5). The direct-Azure backend (`GRAPH_EXTRACTION_PROVIDER=azure_openai`) is selectable and the container holds the key and endpoint; do not select it, it sends no service token, no ZDR claim and no usage record. To stop a loop there, set `GRAPH_RECONCILE_ENABLED=0` and recreate `graph-index`.

**After CR-T12 is deployed (migration `20261003150000_graph_extraction_state` first, then the graph-index image, then data-quality-go).** Every chunk has one row in `graph_extraction_state`, and the reconciler and the event consumer both choose their work from it. Nothing is remembered in process memory, so a restart pays for nothing twice.

| Status | What it says | Worked again |
|---|---|---|
| **`pending`** | never tried, or waiting for something to resolve (class `posture_unavailable`: Auth Core could not say what the organisation's retention posture is) | when `next_attempt_at` is empty or past (5 minutes for the posture wait) |
| **`ready`** | extracted, at least one entity, relationship or claim stored (rows of the legacy backfill are `ready` with prompt version `legacy` and no model) | never |
| **`empty`** | extracted cleanly, nothing to store; durable across restarts | only when the extraction prompt version changes (`graph-v1` today) |
| **`skipped_zdr`** | zero-retention: the organisation (`zdr_org`), a restricted document (`zdr_document`) or no attested route (`zdr_unavailable`); no provider call | at the next pass for the first two (the settings lift them), hourly for the third |
| **`skipped_policy`** | the organisation's `graph_extraction` policy is `off` | at the first pass after it is lifted |
| **`skipped_visibility`** | the document is private or shared, not organisation-visible (graph entities are organisation-shared) | at the next pass |
| **`skipped_budget`** | the organisation is over its budget and its mode is `stop` | at the first pass after the budget is raised or the month turns |
| **`failed_transient`** | a retryable failure of the chunk, with its class | after the wait: 5 minutes, then 15, 45, 135 and 360 minutes (the provider's own retry-after wins when it is longer, up to 6 hours) |
| **`failed_permanent`** | given up: the sixth counted attempt failed (`GRAPH_EXTRACTION_MAX_ATTEMPTS`), or the failure cannot be fixed by trying again (`too_long`, an invalid request) | never, until an operator requeues it |

Only a failure of the chunk's own work counts toward the cap. A refused credential (`auth`), an organisation whose posture cannot be resolved, a zero-retention or budget block and a residency refusal hand the attempt back, so a deployment problem cannot walk healthy chunks to `failed_permanent`. A pass stops after three failures in a row, or at the first failure that is about the deployment or the organisation (credential, throttle, provider down, posture, policy, budget), instead of asking once per remaining chunk. Two workers cannot extract the same chunk at once: a claim takes a lease (twice `MODEL_PLANE_EXTRACTION_TIMEOUT_MS` and a minute: 3 minutes by default), and a worker that dies mid-call leaves a lease that expires, with its attempt already counted.

Each completed extraction is stamped with the model and provider that served the call (`InferResponse.model_used`, never what was asked for), the prompt version, the time and the counts. To see which model and prompt produced an entity, join `graph_text_units.knowledge_id` to `graph_extraction_state` (an entity is merged from many chunks, so `graph_entities.provenance` stays the constant `extracted`).

**Read it.** The coverage view counts the chunks graph extraction may look at, by state:

```sql
SELECT * FROM v_graph_extraction_coverage WHERE org_id = '<org>';
SELECT status, error_class, count(*), max(attempts) AS most_attempts, min(next_attempt_at) AS next_due
  FROM graph_extraction_state WHERE org_id = '<org>' GROUP BY 1, 2 ORDER BY 3 DESC;
SELECT model, prompt_version, status, count(*) FROM graph_extraction_state
 WHERE org_id = '<org>' AND status IN ('ready', 'empty') GROUP BY 1, 2, 3;
```

`no_state` in the view is the backlog graph-index has not recorded yet; `ready_legacy` are the chunks extracted before provenance was kept.

**The log.** A failed attempt is one `warn` line, `graph extraction failed`, and a chunk given up on is one `error` line, `graph extraction GIVEN UP on a chunk`, both with the typed fields `failure`, `stage` (`mint` for Auth Core, `call` for inference-core), `grpc_code`, `provider_error` (the `x-mp-provider-error` trailer), `error_class`, `status`, `attempts` and `retry_in_secs`, and never the chunk's text. The gRPC code is in the line: that is what was hidden on 2026-09-29. A chunk that was not sent is `graph extraction deferred or skipped`; an organisation whose bearer cannot be had is `graph extraction waits: no bearer for this organization`. A chunk given up on is also published to `dataplane.dlq.graph-index` (ids and class only), whatever the envelope verifier mode.

**A loop, or a failing chunk.** Group by class first (the second query above). `auth` on many chunks is a credential or a registry problem (the principal `graph-index` on the audience `inference-core`), not a chunk problem: fix it, the chunks need nothing else. `posture_unavailable` is Auth Core or org-core being unreachable: nothing was sent, and the chunks wait. `rate_limited` and `provider_unavailable` are the provider; the pass stops early and the ladder spaces the retries. To stop all graph extraction for one organisation, set its `graph_extraction` policy to `off` with a reason (section 5.11): it takes effect the next time a document is picked up (the policy is read for every document, with no cache), in the reconciler and in the event consumer alike. `GRAPH_RECONCILE_ENABLED=0` stops the reconciler only (new documents still arrive by event), and there is no global switch for both: stopping everywhere is the policy per organisation, or stopping the container.

**Requeue what was given up on**, after the cause is fixed (as the table owner; the table's row-level security applies to the application role):

```sql
UPDATE graph_extraction_state
   SET status = 'pending', attempts = 0, next_attempt_at = NULL, lease_until = NULL, error_class = NULL
 WHERE org_id = '<org>' AND status = 'failed_permanent' AND error_class = '<class>';
```

Do not delete rows to force a retry of `ready` chunks: a chunk without a row is extracted again, and that is a paid call per chunk, so state the count first.

**Budget.** `alert_only` (the default) logs `the background budget is exceeded; alert only, nothing is stopped` on every pass of an over-budget organisation and keeps working. `throttle` works one chunk at a time; `stop` marks the chunks `skipped_budget`. With a figure and no prices the log says the spend cannot be measured and the work proceeds. The organisation's own policy beats the deployment's `BACKGROUND_BUDGET_*`, and community summaries are inside the same gate.

**ZDR.** An organisation whose token says `zdr: true` gets `skipped_zdr` / `zdr_org` and no call; the proof that none was made is the ledger (no `graph-extraction` row with `zdr = false` after the staleness window of section 5.3) and the state table. The compose value stays `persistent` until the Control Plane flip (section 6), so today's behaviour for an organisation with ZDR on is the one section 5.3 describes.

**Rolling back.** The graph-index image of before CR-T12 ignores the table and goes back to selecting chunks that have no graph rows (and, with it, the old loop and the Azure key it needs): do not roll the image back without a reason, and never past the migration while the new image runs (the new image refuses to start without the table). The migration's `.down.sql` drops only what this change recorded; the graph itself is untouched.

### 5.10 Contextualizer v2: what each chunk becomes, the floors, and the live proof

**What changed.** Every chunk of a call ends in one explicit outcome, never in a silent nothing. The mapping is one table (`src/context/outcome.rs`), tested against the pinned copy of every status inference-core can emit:

- Zero-retention organisation (the bearer's `zdr` claim): `skipped_zdr` / `zdr_org`, and **no call at all** is made. A restricted document: `skipped_zdr` / `zdr_document`, refused before any of it is read. A route with no attested zero-retention provider (inference-core's typed refusal): `skipped_zdr` / `zdr_unavailable`.
- Auth Core could not resolve the posture or could not be reached: `pending` / `posture_unavailable`, due in 5 minutes. Auth Core refused the credential, the principal or the organisation: `failed_transient` / `auth`, 5 minutes, and it does not count against the unit.
- No provider meets the residency floor: `failed_transient` / `residency_unavailable`, one call for the whole document (the rest are not asked), re-checked hourly, and not counted against the unit.
- Throttled: the call is retried after exactly the wait the provider named (`x-mp-retry-after-ms`), or after the doubling ladder when it named none, and never past the deadline; what is left over is `failed_transient` / `rate_limited` carrying the provider's wait. Unavailable or timed out: `failed_transient` / `provider_unavailable` or `timeout`.
- The inline time box ran out: `pending` / `timeout`, due at once; the unit is embedded raw and the reconciler fills it in.
- The answer was empty, too short (under 12 characters), too long (over 600), markup only, or had a finish reason this code does not know: `failed_transient` / `invalid_output`, and it is not retried inline. The finish reason `stop` (Azure OpenAI) and `end_turn` are complete; `length` and `max_tokens` keep the last full sentence; `content_filter` is `failed_permanent` / `content_filter` and never retried.
- Input too long: one smaller window around the chunk, once, then `failed_permanent` / `too_long`.

**Zero-retention organisations are skipped, not sent to an unattested route.** When a provider is attested for zero retention, attestation is data in inference-core, and this client can be told to send the flag instead of skipping; that is a later change and needs an owner ruling on the attested route (D-A). Until then a zero-retention organisation gets no contexts and a visible `skipped_zdr` marker, and still ingests.

**A content-filter error that arrives as an HTTP error is not typed.** inference-core turns a provider's HTTP 400 into an untyped UNAVAILABLE, so a blocked prompt looks like `provider_unavailable` and is retried up to the attempt limit before the reconciler's ladder ends it. Typing it is a Model Plane change (not in this build); a finish reason `content_filter` on a 200 answer is typed and final.

**The residency floor needs a region.** The floor asks inference-core to refuse a route that cannot prove it stays inside it, and its proof is its own `AZURE_OPENAI_REGION` (and, optionally, `AZURE_OPENAI_DEPLOYMENT_TYPE`). With the region unset the Azure deployment reads as unproven and every call is refused. Today the Model Plane compose file passes the variable through with an empty default, its `.env.example` does not name it, and the running stack has it only because the git-ignored `.env` says `swedencentral`: a fresh environment would refuse every call, which is now a loud `residency_unavailable` instead of a silent raw embed. Check before enabling anywhere new:

```
docker exec model-plane-inference-core-1 printenv AZURE_OPENAI_REGION AZURE_OPENAI_DEPLOYMENT_TYPE
```

On 2026-10-07 the region reads `swedencentral` and the deployment type is empty, so the `eu` claim is label-based. Do not set the deployment type without reading the real Azure deployment: a wrong value is a false residency claim (OPS-ALIAS).

**Live proof of contextualizer v2.** Four paid `gpt-4o-mini` calls (about $0.002) on one synthetic document of an invented company, against a throwaway organisation (OPS-ORG). It is shared with the CR-T8 and CR-T10 proofs. Run the free preflight first:

```
CR_LIVE_ORG_ID=<throwaway org id> CR_LIVE_PREFLIGHT_ONLY=1 bash tests/scripts/contextual-retrieval-live-proof.sh
CR_LIVE_ORG_ID=<throwaway org id> CR_LIVE_WRITE_CAPTURE=1 bash tests/scripts/contextual-retrieval-live-proof.sh
```

Pass: the bearer's `zdr` is false; `ListModels` shows the model served by an Azure OpenAI deployment that declares a residency of at least `eu`; three chunks give three `Ready` outcomes from exactly three provider calls, with `provider_used` naming Azure OpenAI, `residency` meeting the floor and `model_used` starting with the configured model; every accepted finish reason is `stop` and the 16-token probe's is `length`; one request reached Auth Core for all four calls. `cache_read_input_tokens` is **measured and recorded, not a gate**: Azure's automatic prefix cache needs a 1,024-token prefix and is best effort, so zero is a result. With `CR_LIVE_WRITE_CAPTURE` the real answers are written to `services/embedding-engine-rs/tests/fixtures/contracts/infer-response-azure-live.json` and replayed through the validation tests under `REQUIRE_CONTRACT=1`; commit the file and its pin. State the number of paid calls in the report: four.

**Do not** pin `CONTEXTUAL_RETRIEVAL_PROMPT_VERSION=ctx-v2` before CR-T8 is deployed: the legacy batch path labels every persisted context `ctx-v1`, so it refuses a service pinned to another version rather than write the wrong provenance.

### 5.11 Policy API, coverage endpoint and release gates (CR-T11)

**Status.** Built and tested in the working tree; not deployed. The policy table, the coverage view and the provenance columns come from the CR-T6 migration, which must be applied first: without it the endpoints answer `503` (`policy_store_unavailable`, `coverage_unavailable`) and the gates are `unavailable`, never a pass.

**The policy is an operator control.** Per owner ruling 2 contextual retrieval is not switched off; a per-organisation off exists for the evaluation baseline organisations and for incidents. It needs a service principal (for example the `corpus-seeder` principal for the two corpus organisations) holding `data:quality:admin` with a token minted for THAT organisation. A user token (an organisation's own owner or admin also holds the scope) is refused with `403 operator_only`; a token of another organisation is refused with `403 tenant_mismatch`. Whether `off` may be used on customer organisations is open (OD-PER-ORG-OFF): until the owner rules, do not.

**Set, read and reset** (`$TOKEN` is the operator's token; never paste one into a ticket or a log):

```
# off, with a reason (required for off; a PUT states every switch)
curl -s -X PUT http://data-quality:8013/v1/admin/orgs/<org>/processing-policy \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit","reason":"<why>"}'

# read (the organisation's own admin may read too)
curl -s http://data-quality:8013/v1/admin/orgs/<org>/processing-policy -H "Authorization: Bearer $TOKEN"

# back to the default (every switch inherit): the row is removed
curl -s -X PUT http://data-quality:8013/v1/admin/orgs/<org>/processing-policy \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit"}'
```

Every change writes one row to `admin_audit_log` in the same transaction (`org_processing_policy.set` or `.reset`, the principal, the request id, what it replaced and what it became). Check it with a count, never the reason text in a ticket: `SELECT action, actor, created_at FROM admin_audit_log WHERE org_id = '<org>' AND target_kind = 'org_processing_policy' ORDER BY audit_id`.

**What the API refuses.** `contextual_retrieval` and `context_prompt_version` are enforced by the embedding engine, and `graph_extraction` by graph-index (CR-T12, section 5.9). The budget fields still accept only their defaults: any other value is answered 400 and says why. An operator who sets a budget must never believe it stopped work when nothing reads the column. Both consumers read the budget columns now (graph-index, CR-T12; embedding-engine, CR-T9b), and the flag (`policy.Enforced.Budget`) is turned on by the change that follows, which is deployed after both images (data-quality-go last): until then a budget is set by writing the table by hand, which both services honour, or deployment-wide with `BACKGROUND_BUDGET_*`. The change that makes a consumer read a column flips its flag (`policy.Enforced`) with its test. An invalid body, an unknown field, a missing switch and an `off` without a reason are refused as well (400).

**What an off does.** Within 30 seconds (the embedding engine's policy cache) the engine makes zero provider calls for the organisation, asks for no bearer, and marks its units `skipped_policy` (no error class: it is a decision). They are still embedded and searchable from their raw text. Contexts a unit already has stay in the store and in the vectors; an off stops generation and removes nothing. A restricted document keeps its own `skipped_zdr` mark. `on` never overrides the global kill switch (5.6).

**What a graph `off` does.** graph-index reads the policy for every document it picks up (no cache): it makes no provider call and asks for no bearer for that organisation, and marks the chunks `skipped_policy` in `graph_extraction_state`. The graph that already exists stays and keeps being served; an off stops extraction and removes nothing. A lift (`inherit` or `on`) makes those chunks due at the next pass, with no hand-made requeue. Section 5.9.

**What a lift does.** When the row is removed or the switch set to `inherit` or `on`, a unit that is `skipped_policy` is generated again the next time it passes through the pipeline (a redelivery, a re-embed). The reconciler (CR-T9a) also selects `skipped_policy` units of an organisation whose policy is no longer off: the work queue index does not include that status, so it finds them in its sweep (every `CONTEXT_RECONCILER_SWEEP_SECS`, 5 minutes by default), and `context-backfill` takes them at once. Until the reconciler is deployed, requeue by re-embedding (`reembed-switch`) for the organisation. Do not update `context_status` by hand (5.2).

**When the policy cannot be read.** The engine keeps the last answer it has for an organisation for as long as the table is unreachable (an organisation that is off stays off). With no answer, a database that has no policy table (the migration is not applied, or was rolled back) gets the deployment default, which is on; any other failure (a timeout) generates nothing for that organisation and records nothing, so the units stay as they are and are picked up on a later pass. Look for the log line `org_processing_policy could not be read` (content-free: the failure kind and how many organisations).

**Coverage.** `GET /v1/quality/context-coverage` (scope `data:quality:admin`, the organisation is the token's) returns the counts of `v_context_coverage`, the policy that applies and how the `contextual_coverage_ok` gate judges them. The numerator is `ready` units whose vector is known to be built from the stored context (`ready` minus `ready_unverified` minus `vector_incoherent`); the denominator is `eligible` minus `skipped_zdr`. `legacy` counts as eligible but not ready until the verification pass runs. An organisation whose policy is `off` passes with status `skipped_policy`, said out loud; `skipped_policy` units while the policy is NOT off are stale and count against coverage.

**Gates.** `GET /v1/quality/gates` answers 200 when every gate passes and 412 otherwise. The two new gates:

- Gate `contextual_coverage_ok` passes when the share above is at least `CONTEXT_COVERAGE_THRESHOLD_PERCENT` (99), or when the organisation's policy is off. It is `unavailable` when the coverage view or the policy table does not exist.
- Gate `sparse_index_in_sync` passes when the adapter's parity report (`index-parity-v1`) is for the caller's organisation, adds up, agrees with this service's own count of eligible units, and shows no missing unit, no stale unit and no context difference. It is `unavailable` when `SPARSE_INDEX_PARITY_URL` is unset, when the endpoint does not exist yet (CR-T9a), refuses the credential, errors, times out (5 seconds) or answers something that is not the contract.

An `unavailable` gate fails the report (412): unread evidence is never a pass. The endpoint now exists in the tree (CR-T9a, section 5.7: built and tested, not deployed). **Until the quickwit-adapter image carrying it is deployed and `SPARSE_INDEX_PARITY_URL` is set in the data-quality block of the compose file, `/v1/quality/gates` answers 412 for every organisation because of this gate.** The variable is not set by the CR-T9a change (the data-quality block was outside it): set it to `http://quickwit-adapter:9204/admin/index-parity` after checking that the two services share a network, and recreate data-quality. Nothing automated reads it today. The contract files and the drift check (`scripts/check-parity-contract.sh`, also run in CI) pin the shape both services use.

**Deploy order.** CR-T6 migration, then the CR-T12 migration and the graph-index image (the reader of `graph_extraction`), then the data-quality-go image (the endpoints and gates: from CR-T12 on it accepts `graph_extraction`, so it must not run before graph-index does, or an operator could store an `off` that nothing honours), then the embedding-engine image (the policy reader, and with CR-T9a the reconciler, dark), then the quickwit-adapter image (the parity endpoints of CR-T9a), then `SPARSE_INDEX_PARITY_URL`. Rolling back the engine image makes every organisation on again (the old engine does not read the table); remove an organisation's policy only after confirming which image is running.

### 5.12 Evaluating retrieval quality: the frozen corpus, the cells, the gate and the promotion (CR-T14)

**Status.** Built and tested (Python, Go and shell; `make eval-harness-test` runs them, and the CI job `eval-harness` runs the same), not run against the live stack. The paid re-seed is the owner's decision OD8 and has not been taken. Nothing here runs on its own except the nightly job, which stops at its first step when the corpus is not there.

**Why it exists.** On 2026-10-06 the two evaluation organisations were gone, `quality_eval_runs` was empty, and the corpus could not be rebuilt: it had been assembled once by a process that no file recorded, and the judgments keyed on document ids that documents-api had assigned. A quality gate that nobody can re-run protects nothing, and a prompt or model change ships only through it (section 5.4).

**The corpus.** `scripts/eval-corpus/` is a frozen snapshot, pinned file by file by sha256 in `scripts/eval-corpus-manifest.json` (corpus id `dpv2-eval-corpus-v1`). `python3 scripts/eval-ingest-corpus.py verify` costs nothing and fails on a changed byte, a missing or an extra file, a wrong digest or a path that leaves the snapshot. It holds 104 documents, 1,853,135 bytes: 96 markdown documents of this repository, byte for byte the git blobs of commit `da6fa0d45d8d` (2026-08-26, the time the judgments were written; the one document that was not yet committed then, `video-temporal-retrieval-gap-2026-08-25.md`, comes from its first commit), and 8 synthetic Norwegian documents (hygiene routines in food production, written for this corpus and saying so, on topics no English document touches). The selection is `apps/*.md`, `apps/*/*.md` and everything under `apps/Data Plane v2/docs/`, without agent-instruction files; `eval-ingest-corpus.py freeze` records it and reproduces it.

The corpus of 2026-08-26 had 95 documents, and its file list was never recorded, so it cannot be recovered. This one is the reconstruction the evidence allows, and the numbers measured on it are a new baseline. The committed `contextual` cell (nDCG@10 0.7732, MRR 0.7394) stays the reference: `eval-gate.py --update` refuses to record a cell lower than its baseline beyond its tolerance unless `--accept-lower` says that is the decision. The English corpus holds two chunks with Norwegian characters, which is why Norwegian morphology could not be measured at all; the synthetic documents exist to make it measurable.

**Provisioning the organisations (OPS-ORG).** The two organisations, `org-corpus-baseline` and `org-corpus-contextual`, are the only ones the `corpus-seeder` principal may act for (`config/plane-service-principals.json`). They must exist in org-core, created through the organisation service: with the `tenant` posture an organisation org-core does not know gets no token (503 `retention_posture_unavailable`) and so no embedding and no context. `scripts/eval-fixture-membership.sql` adds the Better Auth half (the seeder's membership and a placeholder owner that cannot sign in) and does not create the org-core half. An operator does this once.

**Ingest, in this order.**

1. `python3 scripts/eval-ingest-corpus.py verify` and `plan` (reads, mints tokens, writes nothing): counts, the policy of each organisation, and the estimate.
2. `python3 scripts/eval-ingest-corpus.py apply --set-policy --yes-paid-reseed OD8 --max-chunks 1500`. Without the approval flag and the cap it does nothing and says why. It sets the baseline organisation's policy to `off` (operator-only, audited, with a reason) and reads it back BEFORE its first document, because contexts generated for a baseline would be paid for and would make the A/B compare contextual retrieval with itself; it refuses a baseline that is not raw and a contextual organisation whose policy is off. Documents go in private (an org-visible document is fed to graph extraction, a paid call per chunk that is not in this budget) and carry their manifest key in metadata. It is idempotent: a second run creates nothing and sends nothing; a document that exists with other content is drift and is refused, never overwritten.
3. `python3 scripts/eval-ingest-corpus.py status --wait 3600` until every unit is embedded and the contextual organisation's units are `ready`.

**What it costs.** An estimate, not a measurement: about 1,234 chunks, so about 1,234 contextualization calls in the contextual organisation (about 0.16 USD at the design price of gpt-4o-mini) and one embedding per chunk in each organisation. The estimate is `ceil(bytes / 1,580)` per document: 1,580 characters per chunk is what the corpus of 2026-08-26 measured (1,164 chunks from 1,840,513 bytes). OD8 approved about 1,164 chunk calls and about 0.15 USD; this is 6 percent more, so say the number when asking. The first run measures it: the usage ledger's rows for the two organisations (CR-T10), or failing that the provider's own count, are the figure.

**The golden sets** are keyed by document title and written for the organisations as they are: `python3 scripts/eval-build-golden-set.py --out <dir>` (the nightly and `make eval-golden` do it first). The ids are never committed state any more; the `golden-v2*.json` files committed under `scripts/` name documents of organisations that no longer exist and are not authoritative. The builder refuses a title that is not exactly one document of the corpus and a document that is not in the organisation (and says to ingest it). The 87 natural-language judgments equal the committed ones query for query, with one documented exception (a test proves it). 28 Norwegian-morphology queries use another form than their text (an inflection, a definite or plural form, a compound), and 20 mixed-language queries mix Norwegian and English, against the Norwegian documents and against English ones.

**The cells and the gate.** `eval-attribution-study.sh` writes `st-rr-on-<cell>.json` for `baseline`, `contextual`, `norwegian-baseline`, `norwegian-contextual`, `mixed-baseline` and `mixed-contextual` (`STUDY_CLASSES`), `eval-lexical-cell.sh` the two lexical ones, and `eval-gate.py` judges them against `eval-baseline.json`: no metric below its baseline by more than the noise floor (recall 0.030, nDCG 0.045, MRR 0.045), a cell must score at least 90 percent of its baselined queries (a cell of 20 or fewer, all of them), at most 2 percent of queries may return nothing, a baselined cell with no file fails, and two organisations of one class must have been asked the same queries. A cell without a baseline only warns until `--update` records it; `--update` refuses a cell of fewer than 10 queries, a cell in which queries returned nothing, and a cell it would lower. Record the first baseline after the first successful run, in one commit that says why: the four cells already in `eval-baseline.json` measured the corpus of 2026-08-26, so where the new corpus scores lower `--update` needs `--accept-lower`, and the commit says that the corpus changed (a lower number there is not a regression of the code).

Beside the numbers the gate reads evidence, fetched by `eval-service.py coverage` and `gates` into the results directory, and judged whenever it is present: the contextual organisation must have a verified context on at least 99 percent of the units that are expected to have one (recomputed from the counts, and compared with the service's own verdict so that the rule cannot drift), with no unit whose vector was not built from its context; the baseline organisation must be raw (policy `off`, no unit holding a context); both must hold the same number of eligible units; and the keyword index must be in sync (`sparse_index_in_sync`, `unavailable` is never a pass). Evidence that could not be fetched is removed, not left stale.

**The promotion gate.** A prompt version, a model or a rerank input becomes the default only through `eval-gate.py <dir> --promote <candidate> --against <default>`. For each variant, in this order: put the contextual organisation in the variant's state (a policy pin and a refresh, section 5.4; a model change and a refresh; `RERANK_INPUT_INCLUDE_CONTEXT=1` for CR-T15), then `scripts/eval-variant-cell.sh <dir> <variant> --prompt <v> [--model <m>] [--rerank-input-context 1]`, which declares the state, records what the organisation really is, and measures: the natural-language class with the reranker off once (attribution, noise about 0.008), with it on three times (confirmation, noise about +/-0.04), and the exact-identifier class. A variant run is 371 retrieval requests, 261 of them reranked (paid); the organisation must not change under it (its coverage is read before and after). The gate promotes only when: nDCG and MRR with the reranker off are no worse than the default's by more than 0.01 on the same queries; the candidate's coverage is at least 99 percent with no incoherent vector; its units carry the declared prompt version and model (otherwise it measured the default under the candidate's name); no exact-identifier query is lost; and the three reranker-on runs are complete and their mean is no worse than the default's by more than 0.045. Anything less, including absent evidence, is HOLD (exit 1). The verdict is `promotion-<candidate>-vs-<default>.json`. `breadcrumb` is refused: it is not built (CR-T18). Wait five minutes after a refresh has finished before measuring: the retrieval service caches a result for 300 seconds per organisation, query, `top_k` and mix, and the cache is invalidated by the organisation's version, which only a document change bumps and a context refresh does not; the coverage and provenance checks of the variant run cannot see a cached answer from the previous state. Re-running a variant in the same directory removes that variant's files first, so a slice that writes nothing leaves its cell missing (the gate then holds) and the run exits 1.

**Recorded results.** With `EVAL_RECORD_RESULTS=1` the nightly stores each cell and each verdict in `quality_eval_runs` through `POST /v1/evals/results` (operator service principal only, the token's organisation, immutable: a replay is a 200, other content under the same key a 409). History is a query: `SELECT created_at, org_id, strategy, scorecard->'harness'->'gate'->>'verdict' AS verdict, scorecard->>'mean_ndcg_at_10' AS ndcg FROM quality_eval_runs WHERE strategy LIKE 'harness:%' ORDER BY created_at DESC;`, and `GET /v1/evals/retrieval/{eval_id}` reads one back. The wire contract is `harness-record-v1`, pinned on both sides (`scripts/check-eval-contract.sh`).

**Switches** (the table of section 3 lists them too):

| Switch | Default | Boot-time check |
|---|---|---|
| `EVAL_RECORD_RESULTS` (eval-nightly.sh) | `0`: the route exists once the data-quality image of CR-T14 is deployed | `0` or `1`, else the nightly stops with exit 2 before it touches anything |
| `EVAL_REQUIRE_COVERAGE`, `EVAL_REQUIRE_PARITY` (eval-nightly.sh, read by eval-gate.py) | `1` in the nightly, off when the gate is run by hand (a missing report then only warns) | `0` or `1`, in both places (the gate exits 2 on anything else) |
| `STUDY_CLASSES` (eval-attribution-study.sh) | `natural norwegian mixed` | each word must be a known class and `natural` must be there (exit 2); a class whose golden file is absent is skipped out loud |
| `EVAL_DOCKER` (the Python scripts) | `docker` | split like a shell; for a podman or a `--context`, and for the tests |

**Live proof (the orchestrator, with OD8).** In this order, saying how many paid calls were made: (1) `verify`, `plan`; (2) `apply --set-policy --yes-paid-reseed OD8 --max-chunks 1500`, then `status --wait 3600` (about 1,234 contextualization calls and about 2,468 embeddings: report the ledger's number); (3) `eval-build-golden-set.py --out <dir>`, `eval-mine-lexical-queries.py <dir>`, `eval-seed-lexical-judgments.py <dir> --apply`; (4) `eval-service.py coverage <dir>` and `gates <dir>`: pass when contextual coverage is at least 99 percent, the baseline is raw and `sparse_index_in_sync` passes; (5) `eval-attribution-study.sh <dir>` and `eval-lexical-cell.sh <dir>` (about 1,360 retrieval requests: 270 for the three classes, 1,044 for the six attribution variants and 46 for the exact-identifier class, the reranker on in all but `rr-off`), then `eval-gate.py <dir> --update` once and a second `eval-gate.py <dir> --require-coverage --require-parity` that must pass; the contextual cell must not undercut nDCG@10 0.7732 beyond its tolerance; (6) `EVAL_RECORD_RESULTS=1`, `eval-gate.py <dir> --records-out <dir>/records --run-id <id>` and `eval-service.py record <dir>/records`: pass when `quality_eval_runs` holds one `harness:` row per cell and a replay answers 200; (7) `eval-variant-cell.sh` for `ctx-v1` and then `ctx-v2` on the canary policy, and `--promote ctx-v2 --against ctx-v1`: the verdict is the decision, and a HOLD is a result.

## 6. Rollout order and the checks before each switch

Each step is additive and can be undone alone. Contextual retrieval stays on throughout: no step sets the flag to `false`.

1. Documents (this change), no runtime effect.
2. Control Plane wave: the organisation read path and the 503, then the `tenant` posture in the Auth Core image, then the registry. The image comes before any registry change (an old validator rejects a registry that says `tenant`).
3. Model Plane: `inference-core` typed statuses and counters (CR-T3); then cost-core accepting `feature` and `units` and the budget exclusion (CR-T4), after the other session's cost-core work is committed and deployed.
4. Data Plane: migrations first (`context_provenance`, `cost_events` v2, `graph_extraction_state`), then the graph-index image before the data-quality-go image (section 5.11), then images with every new switch dark (`CONTEXT_RECONCILER_ENABLED=0`, `CONTEXT_REFRESH_ON_PROMPT_CHANGE=0`, `USAGE_EMIT_ENABLED=0`, `USAGE_FORWARDER_ENABLED=0`). Take a backup of `knowledge_units` first. Re-run the idempotent legacy backfill (`SELECT * FROM context_provenance_backfill();` as the table owner) at the deploy that first writes statuses.
5. Enable by env, in this order: posture to `tenant` (after the Control Plane image and registry); `USAGE_EMIT_ENABLED=1` (after the consumer is deployed); `CONTEXT_RECONCILER_ENABLED=1` with a cap (after the live proof is green); `USAGE_FORWARDER_ENABLED=1` (after cost-core is deployed and priced).
6. Defaults flip last: contextual retrieval on in code and compose, after the live proof and the reconciler exist.

**Live proof (tiny paid, state the counts).** About three `gpt-4o-mini` calls (about $0.002) on one synthetic document in a throwaway organisation; one call to heal the raw unit; one extraction call; about 206 Cohere embed calls for the legacy verification (about 60,000 tokens; no price for that model is in the repository, so state the cost by the number of calls). Pass: 207 of 207 units have a non-NULL status; keyword index parity 0 after named re-announces; a forced re-embed with unchanged inputs makes 0 provider calls; a captured real Azure response with finish reason `stop` passes validation; a ZDR test organisation makes 0 provider calls and records `skipped_zdr`; a graph-index restart makes no new call for an empty document.

**Live proof of CR-T9a (reconciler, tool, verification, parity; the commands and what must be seen).** Run on the one organisation of the stack, after the CR-T6 migration and its idempotent backfill, with the embedding-engine and quickwit-adapter images deployed and `CONTEXT_RECONCILER_ENABLED=0` (the tool needs no flag):

1. `docker exec data-plane-v2-embedding-engine-1 context-backfill --dry-run --org-id <org>`: no model call and no write. Expect 206 units to verify (`verify_legacy`), 1 to generate, nothing to repair and nothing exhausted.
2. `docker exec data-plane-v2-embedding-engine-1 context-backfill --org-id <org> --max-units 300`: exit 0, `stopped` is "nothing left to do". About 1 `gpt-4o-mini` call (the raw chunk) and about 206 Cohere embed calls (state the count; no price is in the repository), 0 further model calls. One document is announced (the healed one), by id and signed.
3. The coverage query of section 4: 207 of 207 `ready`, `legacy` 0, `vector_incoherent` 0, `embedded_context_hash` not NULL on 207.
4. Run step 1 and step 2 again: the dry run shows nothing to do, and the run makes 0 provider calls (compare `inference_requests_total` for `embedding-engine` and the Cohere call count before and after) and announces nothing.
5. `GET /admin/index-parity` and `/admin/index-parity/documents` (section 5.7) with an operator token: the report adds up and names the documents missing from the index; `backfill-reannounce --org-id <org> --document-id ...` for exactly those; measure again after the merges: `missing_units`, `unexpected_units`, `context_body_missing` and `context_body_unexpected` all 0.
6. Switch the reconciler on with its defaults and the pause file in mind (`CONTEXT_RECONCILER_ENABLED=1`, recreate with `--skip-build`): the log says `context reconciler running`, and an idle pass logs nothing at info level. `touch /tmp/context-reconciler.pause` and `rm` it: the next pass is skipped and then runs, with no restart.

Announce documents by id only until CR-T12 is deployed (section 5.7).

**Live proof of CR-T13 (defaults and the kill switch; no paid call, nothing written).** Deployed last (step 6), after the live proof of CR-T8 and CR-T9a is green: the embedding-engine image and the compose file, recreated through the launcher with `--skip-build`. Two steps of `tests/scripts/contextual-config-live-proof.sh`, each with its pass criteria in the script's header:

1. `CR_LIVE_STEP=defaults bash tests/scripts/contextual-config-live-proof.sh` on the running container, recreated with `CONTEXTUAL_RETRIEVAL_ENABLED` and `CONTEXTUAL_RETRIEVAL_MODEL` absent from a copy of the environment file, or set to the empty string in the launcher's environment (compose takes the default for an empty value, and the process environment wins over `.env`); the live `.env` is not edited in place. Pass: every variable has its documented value inside the container (D1), the startup log says `contextual retrieval is ON` with `gpt-4o-mini`, `azure-openai`, `eu` and `ctx-v1` and the consumer says `contextual retrieval ENABLED` (D2), `/readyz` is 200 with `contextual_retrieval` on and the model (D3). Before the new image runs, this step fails on D1 to D3 against the stack of 2026-10-07: that is the proof that it can fail.
2. `CR_LIVE_STEP=boot-contract IMAGE=<the new embedding-engine image> bash tests/scripts/contextual-config-live-proof.sh`, which runs the image with no network and dummy endpoints seven times (the default, the switch off with and without a reason, a reason that is too short, a typo, an empty switch, a blank model) and checks how each one ends: the refusals come before the database and name their variable, the accepted cases log ON or OFF (with the reason) and then fail at the database, which they cannot reach.

The kill switch on a running service is proved in a disposable stack, never on the live one: the three isolated harnesses run with the switch off and a reason, so `docker compose -p <project> exec embedding-engine curl -s http://127.0.0.1:9202/readyz` in a stack started with the environment of `tests/e2e/run-isolated-mvp.sh` answers 200 with `"contextual_retrieval":{"reason":"isolated test stack: ...","state":"kill_switch"}` and `"checks":{"contextual_retrieval":"kill_switch"}`.

## 7. Known gaps and open decisions

Not built, and not assigned to any task in the build manifest:

- **A context-only purge** (section 5.6).
- **A requeue for `failed_permanent` units.** The reconciler gives up on a unit after its own attempts are used (section 5.2) and the pipeline leaves such a unit alone; nothing returns it to work after the cause is fixed except an operator's database write, which section 5.2 forbids. A requeue tool (an explicit, audited, capped `context-backfill` mode) is a follow-up of CR-T9a.
- **Index parity repair is by named re-announce, not automatic.** The reconciler has no access to Quickwit and holds no `data:quality:admin` principal, so it cannot see which documents are missing from the index; the operator reads `/admin/index-parity/documents` and runs `backfill-reannounce` (section 5.7). Units that are in the index and should not be (`unexpected_units`) are counted and cannot be named, so nothing repairs them short of the document's deletion event; an authenticated rebuild with a dry run is the follow-up, and the old `scripts/rebuild-quickwit.sh`, which clears the index and sends no bearer, is not it (do not run it, section 5.7).
- **The hourly `skipped_zdr` discovery is a scan of `knowledge_units`.** A partial index for it belongs with the CR-T6 migration, which this task did not change. It reads one status column once an hour per service.
- **The reconciler runs on the consumer's task.** It is a branch of the service's `select!`, not a task of its own (the pipeline's future is not provably `Send`), so a panic in a pass ends the process the way one in the consumer does. The compose restart policy brings it back; the pause file and `CONTEXT_RECONCILER_ENABLED=0` stop it.
- **Metrics and alert rules** for embedding-engine and graph-index (the `ctx_*` and `usage_*` families of the design), and a Prometheus to scrape them. The queries in section 4 stand in for them.
- **The organisation's ZDR setting for index-time text embedding, page-image embedding and rerank.** They use the Foundry Cohere endpoint, which the code treats as retaining, and are guarded only per document (`DEF-CR-EMBED-ZDR-ORACLE`, owner decision `OD-ZDR-INGEST`). Until the owner decides, the evidence documents state the gap.
- **Converging the third copy of the token client** in retrieval-engine (`DEF-CR-RE-CLIENT`).
- **The deterministic title-and-heading breadcrumb** as a context for organisations that cannot use a model (CR-T18, owner decision `OD-BREADCRUMB`).
- **Graph extraction is sequential per document.** CR-T12 gives a pass a bound on failures (three in a row) and every call a deadline, but chunks of one document are still extracted one at a time, and the request does not ask the model for structured output; an answer that is not an extraction is `invalid_output` and goes through the ladder. Bounded concurrency per document and a structured-output request are follow-ups, not part of CR-T12.
- **The harness (CR-T14, section 5.12) has not been run.** Its corpus is a reconstruction of the one of 2026-08-26 (96 repository documents cut from git history and 8 synthetic Norwegian ones; the 95 of that day cannot be recovered), so its first numbers are a new baseline. The committed `golden-v2*.json` still name documents of organisations that no longer exist and should be deleted after the first successful nightly (the builder writes fresh ones every run). The new query classes are not seeded into `eval_golden_judgments`, so the in-service scorecard (`by_class`) still shows only natural language and lexical identifiers. One of the 87 judgments (the browser-automation question) is judged on the Model Plane overview because the committed file named a document it does not identify. The estimate of what the re-seed costs is an estimate until the usage ledger is deployed.
- **The budget gate is built and not yet reachable through the policy API.** Both consumers read it (graph-index since CR-T12, embedding-engine since CR-T9b), alert-only by default with no figure and no prices built in, and it works for the deployment-wide settings and for rows written by hand; the policy API still refuses budget values until its flag is turned on after both images are deployed (the follow-up in data-quality-go). Three limits of the gate itself: the spend is read from the usage ledger, so without `USAGE_EMIT_ENABLED=1` and the data-orchestrator consumer the gate can only alert that it cannot measure; the ledger lags a call by the time an event takes to reach `cost_events`, which the gate bridges with its own accounting inside one process (a second replica or `context-backfill` running beside the service only learns of the first's calls at the next 30-second reading); and the price table is one pair of prices for every background model, so a dedicated background deployment with other prices needs the prices changed with it.

Open owner decisions that change this runbook: `OD-PER-ORG-OFF` (whether `off` or `stop` may be used on customer organisations), `OD-PRICES` (the price sheet), `OD-ZDR-INGEST`, `OD-BREADCRUMB`. Operator actions: `OPS-ORG` (throwaway organisations for the proofs), `OPS-PRICE`, `OPS-ALIAS` (a dedicated background deployment and the real deployment type), `OPS-SECRETS` (rotate two credentials that were printed in an audit transcript; this includes rotating `GRAPH_INDEX_GDPR_NATS_PASSWORD`, which is done in the secret store and in the broker's user list, not in code: nothing in the CR-T12 change reads or sets it).

## 8. How to re-check this file

- `python "apps/Data Plane v2/scripts/check-cr-docs.py"` checks the shape of this runbook, its status and error-class lists against the CHECK constraints of the provenance migration, the matrix rows and rule 16, the four ledger entries, the stale statements and the line endings. `--selftest` proves each check can fail; `--citations` also runs the citation checker on the four evidence documents.
- `node docs/compliance/evidence/tools/check-citations.mjs docs/compliance/evidence/data-flow-map.md` (and `honest-limits.md`, `subprocessors-from-code.md`, `claims-vs-code.md`).
- The numbers in this file were read on 2026-10-06 and are in the live-reads file named at the top. After the rollout, re-run those reads and replace every figure and every "today" with what the stack shows (X-DOCS-FINAL).
