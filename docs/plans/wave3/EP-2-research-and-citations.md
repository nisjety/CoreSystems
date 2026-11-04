# EP-2 - Research and citations (Wave 3 design)

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `MG` = `MPR/services/model-gateway/src`; `EC` = `MPR/services/execution-core/src`; `SC` = `MPR/services/session-core`; `CC` = `MP/go/services/capability-core`; `QE` = `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src`; `DP` = `apps/Data Plane v2`; `FE` = `apps/Frontend Plane/verevonv3`; `FES` = `FE/src`; `BFF` = `FE/apps/gateway/src`. Every `path:line` below was opened in worktree `epic-shaw-4d613b` at HEAD `c18196af2` (branch `model-plane`, Waves 1 and 2 built). `[INF]` = inference, not read in code. `[verify live]` = only a running stack can prove it. `[STALE]` = the audit card (`C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`) or a planning note says otherwise and the code wins.

Plan inputs: `docs/plans/model-plane-chat-build-plan-2026-10-14.md` sections 1.4, 3.3.0-3.3.5 (EP-2 at "### EP-2 Research and citations"), DEC register 2.1, proto table 4.3, migration plan 4.4. Format follows `.plan-work/designs/WS14-hitl.md`.

## 1. Scope, items, owner

**Items (W3, all four in this epic):**

| Item | Audit title | Audit effort | This design |
|---|---|---|---|
| F-t3-054 | Web reading upgrade: chunked long-page reader and batched, reference-id web tool | M | S2, S3, S10 |
| F-t3-053 | Claim-level citations: bind chips by server number, cited-passage viewer, provenance stamp | M | S1, S4 (+ ledger half in S6) |
| F-t3-027 | Deep research v2: plan turn, gap-driven second pass with claim ledger, knowledge and connector sources | XL | S5, S6, S7 |
| F-t3-036 | Wide research: entry point, estimate, spend check, per-item retry, replay after reload | L | S8, S9 |

**Owner.** Model Plane owns the tools, the research pipeline, the fan-out runs and the events (model-gateway `deep_research.rs`, `tool_loop.rs`; execution-core `subagent/research_fan_out.rs`; session-core lineage readback; capability-core policy key). Ingestion Plane (Quarry v2) owns scraping, SSRF vetting, search providers, and the inline-or-artifact return of page text; this epic calls Quarry's existing `/v1/scrape` and `/v1/search` and adds no egress. Data Plane v2 owns knowledge retrieval and passage reads; this epic only calls its existing viewer-ACL read API through the existing BFF and model-gateway clients, never its database. Frontend Plane owns the chip binding, passage viewer, plan card, wide-research entry, estimate card and grid. Control Plane: no change (identity and billing facts only; no new entitlement).

| Slice | Tag | Item | In |
|---|---|---|---|
| S1 | [FINISH] | 053 | Chips bind by the server's `number`, not by array position (FE only) |
| S2 | [BUILD] | 054 | Long-page reader: outline + first chunk + `ref`, `read_page` and `find_in_page` over the gateway handle store |
| S3 | [BUILD] | 054 | Batched `web_search` (`queries[]` up to 4), one merged, de-duplicated list with stable reference ids; `fetch_url`/`read_page` accept a `ref` |
| S4 | [BUILD] | 053 | Cited-passage viewer (knowledge from wire facts and the Data Plane chunk read; web from a quote + text-fragment link) and a provenance stamp on research reports |
| S5 | [BUILD] | 027 | Plan turn: editable sub-queries and up to 3 clarifying questions; run turn re-clamped server-side |
| S6 | [BUILD] | 027 (+053 ledger) | In-process second pass driven by coverage shortfall and a claim ledger, inside the wall clock |
| S7 | [BUILD] | 027 | Knowledge sources in deep research under `retrieval_scope`; connector sources behind a second switch |
| S8 | [BUILD] | 036 | Composer "Bred forskning" entry, estimate card, server-side item clamp from org policy |
| S9 | [BUILD] | 036 | Child budget spend check, per-item retry, grid replay after reload |
| S10 | [BUILD, optional] | 054 | Do-loop reader parity: `web_fetch` outline and find over execution-core run handles |
| S11 | [FINISH] | all | Flip switches on the proof org, ledger entries, taxonomy, evidence |

**Explicitly OUT (with owner):**
- W4/W5 members and neighbours: F-t3-093 grow the corpus from research (W5, Data Plane, needs 054); F-t3-038 research-to-deck (EP-5, consumes S6's report); F-t3-092 Model Council (W5); paid per-claim support check and web authority labels (deferred by the audit card; no domain-class data exists).
- Mid-run steering of deep research (027 part): demoted per the plan; queued input already lands only at tool-round boundaries and the in-stream pipeline has none. Not built in W3.
- Moving deep research onto a durable run (`WS03` fold). Decided against for W3 (D1); deep research stays one in-stream turn.
- A confirm gate for wide-research spend (DEC-40 / OD7 recommends none in v1); a renewable delegated bearer (DEC-02 / OD8; open, owned by Control security review).
- Quarry changes of any kind: inline return, SSRF vetting, provider routing, ZDR provider gating (`QE`), and the Quarry-side stored-page viewer.
- Data Plane changes: no new endpoint, no `chunk_index` filter on `GetChunks` (S4 uses what exists), no embeddings or reranking outside Data Plane.
- The Go `WideResearchWorkflow` (orchestrator-core): not used (audit card; hardcoded run ids).
- Channel Plane, sharing, export (EP-12).

## 2. Verified current state

**Web reading (054).**
- `fetch_url` cuts the inline page at `MAX_FETCH_CHARS = 4_000` (`MG/tool_loop.rs:139`). The arm (`MG/tool_loop.rs:3772-3870`) calls `quarry.scrape_readable` (`:3782`), screens the COMPLETE body before cutting (`assess_external_payload`, `:3812-3840`), then keeps the 4,000-char head inline.
- [STALE] The audit says nothing holds the rest of the page. Since WS25-S4 (`ffb738224`, queue `partial`), when `RESULT_HANDLES_EXTENDED` is the literal `true` (`MG/tool_result_handles.rs:77,87`) the full screened page is parked under a handle (`:3841-3870`, `park_extended_result` `:4780-4818`) and the model can read character ranges back with `result_query` (`:4622-4700`; tool definition text at `~:5425`). Handles are keyed `(org_id, user_id, handle_id)`, live 900 s (`tool_result_handles.rs:63`), at most 512 per process (`:67`), in gateway memory only. Parking is refused for ZDR or waived turns (`external_parking_allowed`, `tool_loop.rs:4771-4773`) and for unscreened external content (`tool_result_handles.rs:96-104`). A read back replays the parking-time screening verdict (`read_back_provenance`, `tool_loop.rs:~4825`).
- What is still missing: an outline, a chunk addressed by section, find-in-page ranked by relevance (`grounding::select_passages` exists at `MG/grounding.rs:438`), and a model-legible name for "read more of this page". `result_query` is a generic row/range tool; on live turns the model is not known to use it for pages [verify live].
- `web_search` takes one `query` (`MG/tool_loop.rs:3633-3640`; definition `:5383-5386`), returns `web_search_item` rows (`MG/quarry.rs:419`) with url/title/snippet and rerank data, no reference id. Several `web_search` calls in one round already run concurrently (`join_all`, `tool_loop.rs:8627`), so `queries[]` saves rounds and tokens, not latency (audit right).
- Quarry inline return: [STALE] overlap-quarry note 10 ("F-t2-005 inline return before F-t3-054's ZDR buffer path") is satisfied in this tree. The client always asks for `formats: ["markdown"]` (`MG/quarry.rs:181-202`); the edge returns the text inline up to `QUARRY_EDGE__SCRAPE_INLINE_MAX_BYTES` (default 1 MiB, test `QE/scrape_contract.rs:467`; ceiling 8 MiB `:50`); over the cap it omits the field and stores an artifact outside ZDR, and truncates inline under ZDR with no artifact (`QE/routes.rs:2596-2622`). The gateway resolves an artifact reference only outside ZDR (`MG/quarry.rs:878`, `artifact_text` `:925`). A per-scrape record is stored only outside ZDR (`QE/routes.rs:865-890`). Whether the deployed quarry-edge carries this build is [verify live].
- [STALE] The "turn-scoped buffer under ZDR" the audit proposes is moot: a ZDR turn is refused before any tool runs when web search is asked for (`MG/zdr_features.rs:98` `browse_web` -> `web_search`; `:103` `retrieval_scope` web-only; `:346-353`). D1-815 (resolved 2026-10-28) made the composer stop sending web search under org ZDR.
- Execution-core Do loop: `web_fetch` caps at 8,000 chars (`EC/web_tools.rs:38`). [STALE] The audit says the Do loop has no handle store; WS25-S5 (`6ce0f6ba6`, queue `partial`) added run-scoped handles behind `EXECUTION_RESULT_HANDLES` (`EC/runtime_loop/result_handles.rs:1-21,35`) and `web_fetch` is parkable (`:461-465`).

**Citations (053).**
- The server numbers citations and emits `number`, `claim_id`, `start`, `end`, `origin` (`MG/sse_events.rs:194-230`, serialised `:695-720`). `RecordedCitation` persists `number` into assistant message metadata (`MG/sse_events.rs:1245-1262`; `turn_evidence_metadata`, `MG/session_flow.rs:1939-1961`). The post-stream anchor pass resolves by number, never by position (`MG/citation_anchors.rs:24-28,106`).
- The SPA drops `number`: `ChatCitationEvent` has no `number` (`FES/shared/api/chat-client.ts:407-419`) and `emitCitation` does not read it (`:1371-1387`), although the generated payload type has it (`FES/shared/api/chat-event-payloads.generated.ts:27-39`). `normalizeCitation` has no `number` (`FES/features/chat/components/chat-normalizers.ts:1707-1722`); `Citation` type `chat-types.ts:96`. Chips bind `[n]` to `citations[n - 1]` (`FES/features/chat/components/chat-media-markdown.tsx:1360-1373`). Audit right.
- Knowledge grounding carries `document_id`, `chunk_index`, fact `text` (`MG/retrieval.rs:137-147`) and a stable `number` per citation (`:86-115`). The Data Plane has a viewer-ACL chunk read: `POST /v1/retrieve/chunks` (`DP/services/retrieval-engine-rs/src/api/mod.rs:474`, viewer grants resolved before any fetch `~:2086-2107`), contract `GetChunksRequest {org_id, knowledge_ids, document_id}` with no `chunk_index` filter (`DP/proto/retrieval_v2.proto:172-180`); the BFF already exposes it as `POST /api/v1/knowledge/retrieve/chunks` (`BFF/domains/knowledge.rs:113-116`).
- Citation hrefs are scheme-guarded since Wave 1 depth (D2-010, D2-011 resolved 2026-10-26). D2-013 (decide the chip sink) is still `open` in the register although the guard landed: stale register row.
- Deep research citations carry `number` for read sources and none for unread ones, `claim_id: None` (`MG/deep_research.rs:1956-2005`). The report is a `document` artifact with id `research-{request_id}` (`:2054`); the tool receipt carries counts only (`:2081-2136`). No provenance record exists.

**Deep research (027).**
- `run_deep_research` (`MG/deep_research.rs:2413`) is one in-stream pipeline: plan (up to 9 sub-queries, `:91`), concurrent search, read (16 pages target, `:131`; 4,000 chars per page, `:174`, tied to `MAX_FETCH_CHARS`), synthesis (80,000-char corpus, `:200`), wall clock 410 s (`:233`). It dies with its stream by design (module doc `:18-23`). Audit right.
- `sse.rs:1612-1615` rejects `knowledge_only` with deep research; `:1608-1611` rejects a non-auto scope with agentic; `:1582-1586` rejects deep research under `conversation_only`. The call site is `MG/sse.rs:3532-3610`.
- Coverage shortfall reasons already exist (`shortfall_reason`, `MG/deep_research.rs:1720-1750`): the inputs a gap-driven second pass needs.
- `PLAN_MODEL = "verevon-budget"` (`:263`); D1-807 (resolved 2026-10-28) made auxiliary models configurable.
- Deep research is refused under ZDR (`MG/zdr_features.rs:100,326-328`; `MP/docs/ZDR.md:97`).
- Ledger 2026-09-03 (`MP/docs/decisions/ledger.md:~1868-1874`): `ResearchExecutor::run_search` in deep research had no ZDR threading. Moot while deep research is refused under ZDR; S7 must not change that.
- Wall clock 410 s versus the ~300 s delegated bearer: the inline pipeline uses the turn's inference bearer for plan and synthesis calls. Whether a long research turn already hits `Unauthenticated` near the end is [verify live].

**Wide research (036).**
- Engine: `subagent.research_fan_out` (`EC/subagent/research_fan_out.rs:36`), kill switch `WIDE_RESEARCH_FAN_OUT_ENABLED` defaults ON (falsey opt-out, `:42,67-72`), concurrency 4, cap 16 (`:57,62`), item count bounded by the run's child ceiling (`:245`), digest budget 22-24 k chars (`:421-425`). Dispatch at `EC/runtime_loop/agent.rs:4856-4857,4928+`; tool removed when switched off (`:1039-1040`).
- The child ceiling is `resolve_child_ceiling(request, org)` (`EC/subagent/mod.rs:84-91`; default 8, hard 100, `:63,71`), but the call site passes `org_ceiling = None` (`EC/runtime_loop/agent.rs:1779`). The request side is `RunAgentRequest.max_subagents = 15`. So nothing org-side clamps the count today. Audit right.
- No cost-core client in execution-core (only `run_budget.rs`, `compaction_budget.rs`). `RunDeadline` bounds a run by wall clock and earliest bearer `exp` (`EC/run_budget.rs:1-30`). The per-run ceiling of WS27-S6 is gateway-side (`MG/run_ceiling.rs:1-60`) and covers the Ask loop, not execution-core children.
- Lineage: `AttachSubagentRequest` carries `goal_summary = 5`, `batch_id = 6` (`MP/proto/model_plane/v1/orchestration.proto:1184,1192`); session-core stores both in `subagent_edges.metadata` JSONB (`SC/src/orchestration_grpc.rs:1194-1217`, `4386-4405`), but `LineageEdge` has only fields 1-4 (`orchestration.proto:529-542`) and the readback drops them (comment at `orchestration_grpc.rs:4386-4392`; query `:1879-1910` also selects `status` but does not return it). The read path already exists end to end: MG `GET /v1/orchestration/threads/:thread_id/lineage` (`MG/http_routes.rs:448,1486-1506`), BFF `BFF/domains/orchestration.rs:94`, SPA `getLineage` (`FES/shared/api/orchestration-client.ts:548`).
- `ChatWideResearchPanel` renders live cards only and starts empty after reload (`FES/features/chat/components/ChatWideResearchPanel.tsx:16-20`), mounted at `ChatPage.tsx:980`. Audit right.
- No composer entry point and no action contract for research (`FES/shared/actions/action-registry.ts`, grep). The composer sends `deep_research` (`FES/shared/api/chat-client.ts:109,1604`).
- Under ZDR the whole agentic path is refused (`MG/zdr_features.rs:329-334`). [STALE] The audit says the grid "stays live-only under ZDR"; under ZDR there is no grid at all.
- Org agent policy (WS25-S7 `done`): closed key schema in `MPR/crates/mp-contracts/src/org_agent_policy.rs:69-149` (keys `mode_cap`, `rung_cap`, `lockdown`, `ext.*`, `mcp_tool_approval`, `user_hooks`, `reference_uploads`), stored as JSONB in `CC/migrations/0023_org_agent_policy.up.sql`. No research key. Adding a key needs no migration.

**Content safety.** Every fetched page and search result is screened through `assess_external_payload` before it is cut or parked (`MG/tool_loop.rs:3663-3680,3812-3840`). Connector results are `ConnectorContent` (`MPR/crates/mp-content-safety/src/trust.rs:40-41`). [STALE] Register row D2-172 ("trust.rs rows not applied, do not enable connector reads") is still `open` but the rows are in the tree.

**Migration and proto heads (verified).** session-core latest `0071_run_question_consume.sql`; capability-core latest `0030_tool_policy_expiry`. `RunAgentRequest` highest tag 24 (`org_policy`, `execution.proto:271`), next free **25**. `LineageEdge` highest 4, next free **5**. Neither message has a `reserved` range.

**Prerequisite status (`.plan-work/execution/queue.json`) and the register rows that matter.**

| Prereq | Status | What this epic needs from it | Open register rows that matter | Start anyway? |
|---|---|---|---|---|
| WS25-S4 gateway handles | partial | Handle store, `insert_external`, read-back provenance (S2, S3) | WS25-S12 (flip) `blocked-external`: `RESULT_HANDLES_EXTENDED` is off everywhere | Yes: S2 builds dark; enabling S2 needs the handles switch on for the proof org |
| WS25-S5 execution-core handles | partial | Run-scoped handles for S10 | same | Yes (S10 optional) |
| WS26-S2.code / S6 / S7 screening | partial | Screening of every chunk (all slices) | D2-109 (`CONTENT_SAFETY_NORMALIZE` default-on decision) open; D2-112 live proof open | Yes: the screening call already sits on the fetch path; no new screen is added |
| WS23-S1.code, S3.code-S6.code connector reads | partial; external halves `blocked-external` | S7 connector half | D2-172 (stale, see above); DEC-26 flags | S7 knowledge half yes; connector half no (gated) |
| WS27-S1 pricing primitives | done | Estimate card USD (S8) | none | Yes |
| WS27-S3/S4/S6/S9 meter, ceiling, Do progress | partial | Spend shown during wide research (S9 uses its own child budget) | D2 rows on S6/S9 live proofs | Yes |
| WS03-S1..S7 durable loop | done | Wide research runs are Do runs (S8, S9) | S8 parking `partial`, S9 live runner not run | Yes |
| WS04-S1..S9 replica-safe control | mostly done; S3, S6, S8, S9 partial | Cancel of a fan-out run across replicas | S9 flip pending | Yes |
| WS02 context/cache | mostly done | none specific | none | Yes |
| WS25-S7 / S9 org policy | done | S8 policy key and enforcement client | WS25-S12 blocked | Yes |
| WS14-S4 ask_user | partial | Not used (D7: clarifying questions live in the plan card) | none | n/a |

Depth register rows that touch this epic directly: D1-762 (older paged turns show no citations in the Sources tab; S4's viewer inherits it), D1-107 (citations have no golden payload; S1 adds one), D1-102/D1-104 (citation live evidence owed; S11 covers), D2-321 (web_search sends model-chosen text to a third-party provider; S3 widens this surface, see D9).

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | Where research state lives (plan, ledger, provenance) | (a) in-stream + assistant message metadata; (b) new session-core table; (c) move deep research onto a durable run with the WS03 fold | **(a).** The plan turn and run turn are two ordinary turns: the plan travels to the client in an event and back in the run request, where the server re-clamps it; nothing must wait inside a stream. The ledger and provenance are evidence of one answer and ride `turn_evidence_metadata` beside `citations` and `artifacts` (`MG/session_flow.rs:1939-1961`), erased with the message by the existing GDPR path. No migration. (c) is the right move only for unattended research, which needs OD8. New ledger entry | S5, S6, S4 provenance | S1-S3, S7-S10 |
| D2 | Claim ledger schema and erase contract | free text / closed JSON | **Closed JSON `research_ledger` v1:** `{version:1, claims:[{id, text (<=300 chars), status: supported|single_source|conflict|unsupported, sources:[number]}] (<=24), gaps:[sub-query text] (<=4), pass_count}`. Persisted only on non-ZDR, non-waived turns; deleted with the message; never indexed, never fed to memory or learning. Owner of ZDR acknowledges | S6 | all others |
| D3 | `knowledge_only` with deep research | keep refusing / allow | **Allow.** `knowledge_only` = no web phase, KB retrieval per sub-query only; `web_only` = today; `auto` = both, KB first. Removes `sse.rs:1612-1615` only behind S7's switch | S7 | all others |
| D4 | OD7 wide-research spend gate (DEC-40) | confirm gate / separate child budget / nothing | **Separate child budget, no confirm gate in v1** (DEC-40 rec). The estimate card is shown before Start, so the user sees the cost; Start is the entry, not a gate. A token budget for all children, checked between items; items not started when it is spent end `budget_reached`. OWN/PROD to confirm | S9 budget half | S8, S9 retry/replay |
| D5 | Where the org item cap lives | (a) new key in capability-core org agent policy; (b) org-core quota (Control) | **(a)** `research.max_fan_out_items` (integer, tighten-only, default 8 = `DEFAULT_MAX_CHILDREN`, hard max 16 for this key). Agent policy is Model-owned (DEC-14, WS25); it is a run-shape limit, not billing. No migration (JSONB doc) | S8 clamp | S8 entry/estimate |
| D6 | Reader tool surface | extend `result_query` / new `read_page` + `find_in_page` | **New named tools** implemented on the same handle store. Page reading needs section addressing and relevance search; the names say what they do in the transcript and audit. `result_query` is unchanged | S2 | yes |
| D7 | Clarifying questions for deep research | WS14 question store / in the plan card | **In the plan card**: the plan inference may return up to 3 questions; answers travel in the run request like the edited sub-queries. No run exists to pause, so the WS14 store does not fit | S5 | yes |
| D8 | Web passage locator | (a) quote + text-fragment link to the live page; (b) open Quarry's stored copy via a new BFF route | **(a) in W3.** No new read path into Quarry artifacts, works the same on every non-ZDR turn, and Quarry keeps retention. (b) is a Quarry-session item if wanted later | S4 web half | yes |
| D9 | Batched search and query exfiltration (D2-321) | offer everywhere / Ask and interactive Do only | **Ask loop and attended Do only; never the scheduled lane** (WS20 lane keeps its contract-approved single query). Max 4 queries per call, each one billed by Quarry as today; `allow_paid_providers` is applied per query | S3 | yes |
| D10 | Execution-core Do-loop reader in scope | yes / no | **Yes, optional S10**, after S2: wide-research children read through `web_fetch`, so their evidence improves only if the Do loop gets the same outline/find | S10 | yes |
| D11 | Deep research inside the bearer window | none / guard | **Guard:** the second pass (S6) starts only if `min(wall-clock deadline, earliest bearer exp - 30 s)` leaves room for one search round, one read batch and synthesis; otherwise it is skipped and the coverage line says so. Mirrors `EC/run_budget.rs` | S6 enablement | yes |
| D12 | Waived (R17) turns at the new persisted keys | persist like Ask / refuse | **Refuse (DEC-32 rule):** `research_plan`, `research_ledger`, `provenance` are not persisted on a waived turn; the turn still streams them | S4, S5, S6 | yes (default refuses) |

Ledger entries this epic adds (`MP/docs/decisions/ledger.md`, newest first): "Deep research state lives in the stream and in message metadata, not on a durable run" (D1, D2); "Deep research may run knowledge-only" (D3); "Wide research: separate child budget, no confirm gate; org item cap in agent policy" (D4, D5). Nothing in the ledger is reversed.

## 4. Slices

Test commands: `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --lib <filter>` (also `-p execution-core --lib`, `-p session-core --lib`, `-p mp-contracts`); on Windows use `--lib` plus a named filter (two targets fail pre-existing) and a dedicated `CARGO_TARGET_DIR` cleaned per member (memory: stale warm targets); `cargo fmt --all -- --check`; SSE taxonomy `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; ZDR contract `bash scripts/tests/zdr-contract-sync.test.sh` and `cargo test -q -p model-gateway --test zdr_posture_matrix --test zdr_refusal_inventory --test zdr_feature_table`; proto in `apps/Model Plane/proto`: `buf lint`, `buf generate`, Go bindings under `MP/go/gen/model_plane/v1/`; capability-core `cd "apps/Model Plane/go" && go test ./services/capability-core/...` (`GOWORK=off` if needed); FE from `FE`: `pnpm exec vitest run <files>`, `pnpm lint`, `tsc6 -p tsconfig.app.json` (memory: `tsc -p tsconfig.json` checks nothing; typecheck has pre-existing failures, diff against HEAD); BFF `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets`; e2e on a second dev server (`:5173` serves the main checkout).

Every switch below is read with the strict literal-`true` parser (vocabulary C in `.plan-work/common/flags-and-config.md`), defaults off, constant `pub const ..._ENV` in the owning module, compose pass-through `${NAME:-false}`.

### S1 - Chips bind by server number [FINISH]
**Goal.** `[n]` opens the source the server numbered `n`. Cheapest user-visible fix in the epic; no server change.
**Files.**
- `FES/shared/api/chat-client.ts` - `ChatCitationEvent` (`:407-419`) gains `number?: number`; `emitCitation` (`:1371-1387`) reads `num(payload.number)`.
- `FES/features/chat/components/chat-types.ts` - `Citation` (`:96`) gains `number?: number` with a doc line ("server-assigned; bind by this, never by position").
- `FES/features/chat/components/chat-normalizers.ts` - `normalizeCitation` (`:1707`) carries `number`; the history read (`metadataArray(serverTurn.citations, ...)`, `:1031`) already gets `number` from `RecordedCitation`, so reload binds the same way.
- `FES/features/chat/components/chat-media-markdown.tsx` - at `:1360-1373` resolve each index by `citations.find(c => c.number === index)` when ANY citation in the turn has a `number`; fall back to `citations[index - 1]` only when none has one (older gateway, older stored turns). A marker whose number has no source stays plain text (today's rule at `:1368-1370`).
**Contract/flag.** None. Pure client fix, reversible by revert (server-controlled behaviour does not use VITE flags, flags-and-config 8.4).
**ZDR.** Nothing stored. ZDR turns carry KB citations (knowledge grounding is honoured under ZDR, `zdr_features.rs:101-103`) and bind the same way.
**Tests.** NEW `chat-media-markdown.citation-number.test.tsx`: gap case (citations numbered 1, 3; `[3]` binds to the second element), unread deep-research rows without number never bind, mixed KB + web turn, no-number fallback. UPDATE `chat-normalizers.connector-citation.test.ts` (number carried), `chat-client` parse test for `number`. Add a `citation` golden payload row so the keys are checked (closes D1-107's citation half; edit the golden in the WS12 S6 harness file, not a new harness). `pnpm exec vitest run src/features/chat src/shared/api`.
**Live proof.** A turn with KB grounding plus web search where the server numbering has a gap [verify live: needs a KB with two providers]; screenshot the chip popover and the Sources tab, and the SSE frame showing `number`.
**Effort.** S, 1-2 d. **Deps.** none. **Risks.** `ChatMessages.tsx`/markdown hot file (WS09, WS14); keep the change inside the citation branch.

### S2 - Long-page reader [BUILD]
**Goal.** A fetched page returns an outline, the first chunk and a `ref`; the model reads any section or finds passages without another fetch.
**Files.**
- NEW `MG/page_reader.rs` - pure: `outline(text) -> Vec<Section{id:"s1".., heading, level, start, end, chars}>` from Markdown headings (fallback: fixed 4,000-char chunks `c1..`); `chunk(text, section_id|chunk_no) -> &str` bounded by `MAX_TEXT_READ_CHARS`; `find(text, query, max_chars) -> Vec<Passage{section_id, offset, text}>` built on `grounding::select_passages` (`MG/grounding.rs:438`) plus section mapping. No I/O.
- `MG/tool_loop.rs` - `fetch_url` arm (`:3841-3870`): when `WEB_READER` is on AND a handle was parked, the output becomes `{final_url, title, ref: handle_id, outline:[{id, heading, chars}] (<=40 rows), content: first chunk, note}` instead of `{content: head, handle}`. With the switch off, or when parking is refused (ZDR, waived, unscreened), output is byte-identical to today.
- `MG/tool_loop.rs` - two arms `read_page {ref, section?|chunk?}` and `find_in_page {ref, query, max_passages<=5}`: resolve through `state.tool_results.resolve(org, user, ref)` (same tenancy as `result_query`), apply `page_reader`, return provenance via `read_back_provenance` so a flagged page stays flagged. Unknown or expired ref: the same error text style as `result_query` (`:4630-4640`). Tool definitions next to `fetch_url` (`~:5439`), offered only when `fetch_url` is offered and the switch is on; add both names to `WEB_CLASS_TOOLS` (`:5288`) so source-scope filtering and the web trust class apply.
- `MG/zdr_features.rs` - no change (web tools never reach a ZDR turn); add the two names to any closed tool list the ZDR feature table test enumerates [INF: check `tests/zdr_feature_table.rs`].
- `MP/docs/decisions/ledger.md` - D6 entry.
**Contract/flag.** `MODEL_GATEWAY_WEB_READER` (default off). Only effective with `RESULT_HANDLES_EXTENDED=true`; the proof turns both on for the proof org's gateway. No proto, no migration, no SSE change (tool frames already carry arbitrary tool names).
**ZDR.** No new store: pages live in the existing 900 s in-memory handle store, which ZDR and waived turns never use. Golden cases (Section 5).
**Tests.** `page_reader` unit tests (headings, no-heading chunking, UTF-8 boundaries, find ranks the planted passage first, an injection line past chunk 1 is still in the screened payload). `tool_loop` tests: switch off is byte-identical (snapshot of the current output), switch on returns outline + ref, `read_page` on another user's ref fails, expired ref fails, flagged page keeps `[source...] NOTE` label on every read, waived turn returns the old head. `cargo test -q -p model-gateway --lib page_reader tool_loop::tests::reader`.
**Live proof.** Rebuild model-gateway (`apps/Model Plane/scripts/compose.sh`, then `recreate-with-live-env.sh` per memory); both switches on; ask a question whose answer sits past 4,000 chars of a long public page (a standards document or annual report); capture the SSE transcript showing `fetch_url` -> `find_in_page` -> answer citing the passage, and the gateway log showing no second Quarry scrape.
**Effort.** M, 4-6 d. **Deps.** WS25-S4 code (in tree). **Risks.** handle store is per process: a reconnect to another replica mid-turn loses refs [INF; the turn runs in one process, so only a resumed stream is exposed]; memory cap 512 handles under load evicts early, the error text tells the model to re-fetch.

### S3 - Batched search with reference ids [BUILD]
**Goal.** One `web_search` call takes up to 4 queries and returns one merged list; every hit has a stable `ref` the model can open.
**Files.**
- `MG/tool_loop.rs` - `web_search` arm (`:3633`): accept `queries: [string] (1-4)` alongside `query` (either, not both; `query` stays valid). Run the searches concurrently (`join_all`), each with the same `web_search_options` and `allow_paid_providers`; merge interleaved by rank, de-duplicate by `deep_research::normalize_url_key` (`MG/deep_research.rs:947`), cap at 8. Each item gains `ref: "w{n}"` where `n` is a per-turn counter held in the tool loop's turn state (a `TurnRefs` map ref -> final URL, in memory, dropped at turn end). Screen the merged payload once (as today).
- `MG/tool_loop.rs` - `fetch_url` (and S2's tools via the URL they hold) accept `{ref}` in place of `url`; a ref not issued this turn is an error, never a guess. The fetched page's own handle id stays separate from the search ref.
- `MG/tool_loop.rs` tool definitions (`:5383-5386`) - schema with `queries` and the rule "use refs from the results, do not retype URLs".
- `MG/quarry.rs` - none (one `search_reply` per query).
- Scheduled lane: no change (D9). `EC` not touched.
**Contract/flag.** `MODEL_GATEWAY_WEB_SEARCH_BATCH` (default off). With it off the schema and output are unchanged.
**ZDR.** None new; turn-scoped refs hold URLs only and are never persisted. Web search is refused under ZDR.
**Tests.** merge order, de-dup across queries, cap 8, `queries` of 5 rejected with a model-readable error, `ref` resolves only this turn, `allow_paid_providers=false` passed on every query (mock Quarry asserts), switch off byte-identical. `cargo test -q -p model-gateway --lib tool_loop::tests::batched_search`.
**Live proof.** Switch on; a comparison question ("compare X and Y pricing in Norway"); capture one `web_search` call with 3 queries, the merged list with refs, a `fetch_url {ref}` call, and the Quarry edge log showing 3 searches; round count lower than the same question with the switch off (record both).
**Effort.** M, 3-5 d. **Deps.** none (S2 optional). **Risks.** more provider calls per round: Quarry bills per search; a model may fire 4 queries where 1 would do. Tool description says "only when the question has distinct parts".

### S4 - Cited-passage viewer and provenance stamp [BUILD]
**Goal.** Clicking a chip opens the passage that supports the claim; a research report carries a stamp saying what it was built from.
**Files.**
- `MG/sse_events.rs` - `ChatEvent::Citation` gains optional `quote: Option<String>` (<= 400 chars, the passage the source contributed) and `locator: Option<CitationLocator{document_id, chunk_index}>` for knowledge sources; serialised only when set (`:695-720`), so existing frames are unchanged. Additive SSE keys; regenerate `chat-event-payloads.generated.ts` with the taxonomy script.
- `MG/sse.rs` (`:3427,3487` KB citation emit) - set `locator` from `GroundingFact` and `quote` from the fact text. `MG/deep_research.rs` `citation_events` (`:1956`) - set `quote` from the source's best passage (`select_passages` over the extract it already holds). `RecordedCitation` (`:1245`) gains the same two optional fields so history shows them.
- `MG/deep_research.rs` - provenance: when the report artifact is emitted, attach `provenance {question, sub_queries, sources:[{number, url|document_id, title, read_at}], model, coverage:{found, read, unread, deadline_hit}, generated_at, pipeline:"deep_research_v1"}`. `RecordedArtifact` (`MG/sse_events.rs:~1266`) gains optional `provenance`. Not persisted on waived turns (D12).
- `FES/features/chat/components/CitedPassageViewer.tsx` (NEW) - side sheet opened from `InlineCitationMarker` and the Sources tab. Knowledge: show `quote`; "Vis mer" calls the existing BFF `POST /api/v1/knowledge/retrieve/chunks` with `document_id` and picks `chunk_index` client-side (Data Plane decides what the viewer may read; a 403/empty shows "Du har ikke tilgang til dette dokumentet"). Web: show `quote` and open the live page at `url#:~:text=<encoded first 8 words>` (guarded by `isSafeLinkHref`). Connector: quote only.
- `FES/features/chat/components/ChatPanels.tsx` (`~:476`) - Sources tab row opens the viewer; knowledge rows show the passage, not only a count.
- `FES/features/chat/components/ArtifactProvenance.tsx` (NEW) - footer on a research report artifact card.
- `FES/shared/actions/action-registry.ts` - `chat.open_cited_passage` (ownerPlane `frontend`, risk `low`, read-only) before wiring (repo rule).
**Contract/flag.** SSE keys additive (taxonomy gate). Server half behind `MODEL_GATEWAY_CITATION_PASSAGES` (default off: no `quote`/`locator`/`provenance` keys). FE renders the viewer only when the keys exist.
**ZDR.** Under ZDR: no web citations exist; KB citations stream `quote`/`locator` live and nothing is persisted (ZDR turns write no thread). The chunk read is a read through Data Plane's ACL, no new store. `provenance` and `quote` in message metadata only on non-ZDR, non-waived turns; erased with the message.
**Tests.** MG: citation serialisation with and without the keys (switch off byte-identical), deep research quote selection, provenance shape and the waived skip. FE: `CitedPassageViewer.test.tsx` (knowledge quote, chunk fetch 403 message, web text-fragment href passes `isSafeLinkHref`, `javascript:` refused), `ChatPanels.test.tsx`, `ArtifactProvenance.test.tsx`. BFF unchanged. Commands as above plus the taxonomy check.
**Live proof.** KB question with two providers: open a chip, see the fact text, "Vis mer" returns the chunk; a second user without access to the document gets the refusal (Data Plane ACL) [verify live]; a deep research report shows the stamp; screenshots at 1280 and 375 px.
**Effort.** M, 5-7 d. **Deps.** S1. **Risks.** D1-762: turns older than the local snapshot carry no citations, so the viewer is empty there until WS09 paged reads carry metadata; `GetChunks` returns all chunks of a document (no `chunk_index` filter), so a large document is a large response; the BFF already returns it today, cap rendering client-side.

### S5 - Deep research plan turn [BUILD]
**Goal.** "Dyp research" first shows an editable plan (sub-queries, up to 3 clarifying questions); Start runs exactly the edited plan.
**Files.**
- `MG/http_routes.rs` / request struct used by `sse.rs` - JSON fields `research_phase: "plan" | "run"` (absent = today's one-shot) and `research_plan: {sub_queries:[string], answers:[{question, answer}]}` on the run turn.
- `MG/deep_research.rs` - split `run_deep_research` at the plan phase: `plan_only(...)` returns `ResearchPlan{sub_queries, questions (<=3)}` using the existing `plan_sub_queries` (`:3078`) with a prompt variant that may add questions (JSON); `run_with_plan(...)` skips phase 1 and takes the client plan after re-clamp: `normalize_sub_query`, `is_plausible_query`, `MAX_SUB_QUERY_CHARS` (`:337`), count `<= max_sub_queries()`, de-dup; answers become one bounded context line ("Brukerens presiseringer: ...", <= 1,000 chars, screened as user text).
- `MG/sse_events.rs` - new control event `research_plan {request_id, sub_queries, questions, caps:{max_sub_queries}}`; taxonomy row.
- `MG/sse.rs` (`:3532`) - branch on `research_phase`; the plan turn ends after the event with a short assistant line ("Her er planen ...") and persists `research_plan` in message metadata (non-waived).
- `FES/features/chat/components/ResearchPlanCard.tsx` (NEW) - editable list (edit, remove, add up to the cap), question fields, Start and Cancel; docks with the WS14 trays (`ComposerTrays`) as a `plan`-class item. `use-chat-controller.ts` sends the run turn with `research_phase: "run"` and the plan. `FES/shared/actions/action-registry.ts`: `chat.research_plan_start`, `chat.research_plan_cancel`.
**Contract/flag.** `DEEP_RESEARCH_PLAN_TURN` (default off): off means `research_phase` is ignored and the one-shot runs as today. HTTP JSON only, no proto.
**ZDR.** Deep research is refused under ZDR before either phase (`zdr_features.rs:326`): add the golden case. Plan persistence skipped on waived turns (D12).
**Tests.** re-clamp (11 queries -> 9, 500-char query cut, duplicates removed, empty plan -> model-planned fallback), plan turn ends without search calls (mock Quarry sees nothing), run turn performs no plan inference, switch off ignores both fields; FE card edit/add limits, Start payload, reload shows the persisted plan card as read-only once a run turn exists. `cargo test -q -p model-gateway --lib deep_research::tests::plan_turn`, `pnpm exec vitest run src/features/chat/components/ResearchPlanCard.test.tsx`.
**Live proof.** Plan turn then edited run turn on the dev stack; capture both SSE transcripts, show the removed sub-query never searched (Quarry log), the added one searched.
**Effort.** M, 4-6 d. **Deps.** D1, D7; WS14-S1 trays (done). **Risks.** two turns cost two model calls for the plan + answer line; a user may never press Start (nothing is held, so nothing leaks).

### S6 - Gap-driven second pass and claim ledger [BUILD]
**Goal.** After the first read, a cheap pass lists claims and gaps; up to 4 gap queries are searched and read inside the same wall clock; the report and its stamp carry the ledger.
**Files.**
- `MG/deep_research.rs` - after phase 3 (read): if `shortfall_reason` (`:1720`) or the ledger shows `single_source`/`conflict`/`unsupported` claims, and D11's guard leaves room: `ledger_pass` (one inference on the plan tier, input = numbered extracts, output = closed JSON `research_ledger` v1, D2) -> `gap_queries` (<=4, re-clamped like S5) -> search + read with the REMAINING page and attempt budget (never above the existing ceilings `:131-165`) -> recompute ledger for new sources -> synthesis prompt (`synthesis_prompt`, `:1872`) gains the ledger (claims with status) and a rule: a claim marked `unsupported` goes to "Kunne ikke verifiseres". Page extracts for synthesis use `select_passages` over the full parked body when S2's switch and handles are on, instead of the first 4,000 chars.
- `research_receipt` (`:2081`) - adds `passes`, `gap_queries`, `claims_by_status` counts.
- `MG/sse_events.rs` - steps `deep-research-gaps` and `deep-research-pass2` (existing `step` events, no new variant).
- Provenance (S4) gains `ledger_digest` (sha256 of the canonical ledger JSON) and `passes`.
- `FES` - Sources tab groups by pass; the report footer shows claim counts by status. No new card.
**Contract/flag.** `DEEP_RESEARCH_SECOND_PASS` (default off). Inference cost: +1 plan-tier call, +up to 4 searches, reads within the existing caps. Ledger persisted in message metadata only on non-ZDR, non-waived turns.
**ZDR.** Refused under ZDR (deep research refused). Golden case for the waived skip.
**Tests.** ledger JSON parser (closed schema, over-length claims cut, unknown status rejected), gap queries re-clamped, page/attempt budgets never exceeded across passes (property test on `plan_read_batch`), guard skips pass 2 when the deadline or bearer `exp` is near, coverage statement names the skip, synthesis prompt contains the unsupported rule. `cargo test -q -p model-gateway --lib deep_research::tests::second_pass`.
**Live proof.** A question with a known conflicting figure across sources; capture steps, receipt counts, the report's "Kunne ikke verifiseres" section, metadata row in session-core (`psql` on the message metadata, non-ZDR org), and the turn timing showing it ended inside the wall clock.
**Effort.** L, 7-9 d. Reason: two new inference stages with closed-schema parsing, budget accounting across passes and the synthesis contract change; it cannot be split further without shipping a ledger nothing reads. **Deps.** S5 (shared re-clamp), D1, D2, D11; S4 for the stamp half. **Risks.** cost (one more inference, more reads); the ledger model may mislabel claims, so it only routes claims to the hedged section, never deletes evidence; bearer window [verify live].

### S7 - Knowledge and connector sources in deep research [BUILD]
**Goal.** One report cites web, org knowledge and (gated) connector sources; `knowledge_only` research works.
**Files.**
- `MG/sse.rs:1612-1615` - when `DEEP_RESEARCH_KNOWLEDGE_SOURCES` is on, allow `knowledge_only` with deep research; pass `retrieval_scope` into `run_deep_research`.
- `MG/deep_research.rs` - per sub-query, a knowledge retrieval through the existing gateway Data Plane client used by grounding (`MG/retrieval.rs`, viewer bearer, same `org_id` pin) when scope is `auto` or `knowledge_only`; results become `ResearchSource{origin:"knowledge", document_id, chunk_index}` and are numbered with web sources; skip the web phases under `knowledge_only`. Citations carry `origin` (`knowledge`) and S4's `locator`.
- Connector half (second switch `DEEP_RESEARCH_CONNECTOR_SOURCES`, default off): only when the turn carries a non-empty `connector_scope` the user selected (DEC-26), never on a Space thread, through `MG/connector_reads.rs`'s read path (WS23-S5 code); results are `ConnectorContent` (`trust.rs:40-41`), screened, counts-only in step records (WS23-S1 rule).
- `FES` - composer: Dyp research no longer disabled under "Kun kunnskap" when the server advertises the capability [INF: via the existing feature/config read the composer uses for source scope]; Sources tab shows knowledge/connector rows with the S4 viewer.
**Contract/flag.** two switches above, default off. No proto, no migration.
**ZDR.** Deep research stays refused under ZDR (do not let `knowledge_only` slip past `zdr_features.rs:326`: test). Connector reads refused under ZDR (`connector_scope`, `zdr_features.rs:104`).
**Tests.** `knowledge_only` research issues zero Quarry calls; ACL: a KB result the viewer cannot read never appears (mock Data Plane returns none for that viewer; real ACL is Data Plane's); numbering across origins is stable; connector switch off ignores `connector_scope`; Space thread never reads connectors. `cargo test -q -p model-gateway --lib deep_research::tests::sources`.
**Live proof.** Knowledge-only research on an org KB; mixed auto research citing both; connector half only after WS23's external live proof on a real mailbox [verify live].
**Effort.** M, 5-7 d (knowledge half 3-4 d). **Deps.** D3; S4 locator; connector half on WS23-S1/S3/S5 external halves (`blocked-external`) and DEC-26. **Risks.** Data Plane latency per sub-query inside the wall clock (do them concurrently, budgeted in the search phase); connector content is a prompt-injection surface (screened, `ConnectorContent`).

### S8 - Wide research entry, estimate and clamp [BUILD]
**Goal.** A user starts wide research from the composer with an item list or a count preset, sees an honest estimate first, and the server clamps the count to the org cap.
**Files.**
- `MP/proto/model_plane/v1/execution.proto` - `RunAgentRequest.wide_research = 25` (`WideResearchRequest`, new message: `uint32 item_cap = 1; repeated string items = 2; string batch_id = 3; string retry_of_child_run_id = 4;`), comment "EP-2 S8/S9; additive". Next free verified 25; integrator assigns.
- `MPR/crates/mp-contracts/src/org_agent_policy.rs` - key `research.max_fan_out_items` (`Kind` integer, tighten-only, default `"8"`, fail-closed value `"0"` = fan-out off, status active); Go twin in `CC` (the WS25 closed-schema mirror) and its golden.
- `EC/runtime_loop/agent.rs:1779` - pass the org cap from `OrgPolicyContext` (WS25-S9 client) as `org_ceiling`; when `wide_research` is set, `max_subagents = min(item_cap, org cap)`, the first round offers `subagent.research_fan_out` with the items pre-filled in the goal context (items over the cap are dropped and named in the answer), and `WIDE_RESEARCH_FAN_OUT_ENABLED` off refuses with the existing `disabled_refusal`.
- `MG/http_routes.rs` - `POST /v1/research/wide/estimate` (pure compute, no persistence, non-ZDR only): input `{item_count, model}`; output `{items_after_cap, concurrency, rounds_per_item (item_round_share), minutes_low, minutes_high (from RunDeadline and concurrency), usd_low, usd_high (only on a metered, priced route, WS27-S1 primitives; null otherwise), cap_source}`. `MG/sse.rs` agentic request mapping sets `wide_research` from the JSON body.
- `BFF/domains/agents_runs.rs` (or `chat.rs`, whichever holds the agentic send) - pass-through route `POST /api/v1/agents/wide-research/estimate`.
- `FES/features/chat/components/WideResearchStart.tsx` (NEW) - composer entry "Bred forskning" (presets 5/10/16, or paste a list, one item per line), estimate card, Start. `FES/shared/actions/action-registry.ts`: `chat.wide_research_start` (risk `medium`), contract first.
**Contract/flag.** `WIDE_RESEARCH_ENTRY_ENABLED` (gateway, default off; off = estimate route 404 and `wide_research` ignored). Proto additive. No migration.
**ZDR.** The agentic path is refused under ZDR (`zdr_features.rs:329`); the estimate route refuses ZDR callers with feature `agentic` (no new key). Items are user text sent as the run goal: stored like any Do goal (see D2-127: Do goals are stored raw; same rule, nothing new).
**Tests.** `resolve_child_ceiling` with an org cap (request 16, org 8 -> 8); fail-closed `0` refuses; items over cap named; estimate arithmetic (subscription route -> usd null), ZDR refusal; proto `buf lint`; Go mirror golden; FE entry/estimate/start tests. `cargo test -q -p execution-core --lib subagent::tests`, `-p mp-contracts`, `go test ./services/capability-core/...`.
**Live proof.** Org cap 5, request 10 items: estimate says 5, the run fans out 5, answer names the 5 dropped; screenshots of the card at 1280 and 375 px; capability-core policy row (`psql`, keys only).
**Effort.** M, 5-7 d. **Deps.** D5; WS25-S7/S9 (done). **Risks.** `agent.rs` hot file (WS03/05/24/25 edit-first order applies); estimate honesty: minutes are a range, not a promise; the card says so.

### S9 - Spend check, per-item retry, replay [BUILD]
**Goal.** A fan-out stops starting items when its child budget is spent, a failed item can be retried alone, and the grid survives reload.
**Files.**
- `EC/runtime_loop/agent.rs` `spawn_research_fan_out` (`:4928+`) - a child token budget (`WIDE_RESEARCH_CHILD_TOKEN_BUDGET`, default 400,000 metered tokens; subscription children count 0 spend but still count rounds): before starting each item, compare the summed `RunUsage` of finished children (WS02-S3 usage) to the budget; items not started end `FanOutItemResult::refused(item, "budget_reached")`; the merge says how many items ran.
- Retry: `wide_research.retry_of_child_run_id` set -> one-item fan-out with the original item text read from the child run's goal (session-core `GetRun`, owner-checked), same `batch_id`; lineage edge links to the same parent thread.
- `MP/proto/model_plane/v1/orchestration.proto` - `LineageEdge.goal_summary = 5`, `batch_id = 6`, `status = 7` (string, the `subagent_edges.status` value), comment "EP-2 S9 readback of fields AttachSubagentRequest 5/6 already stores"; `SC/src/orchestration_grpc.rs` `fetch_lineage_for_thread` (`:1879-1910`) selects `metadata` and `status` and fills them; `MG/http_routes.rs` `lineage_value` emits them.
- `FES/features/chat/components/ChatWideResearchPanel.tsx` - on mount, seed cards from `getLineage(threadId)` (edges with the run's `batch_id`), then join live events as today; status from `status` + `getRun` for terminal children; "Prøv igjen" on failed/refused/budget_reached cards calls `chat.wide_research_retry_item` (new action contract).
**Contract/flag.** `WIDE_RESEARCH_CHILD_BUDGET_ENFORCED` (EC, default off: budget logged, not enforced); retry and replay ride `WIDE_RESEARCH_ENTRY_ENABLED`. Proto additive. No migration (metadata JSONB exists).
**ZDR.** No fan-out under ZDR. Lineage `goal_summary` is already stored today; this slice only reads it back to the run owner (owner check in session-core is unchanged). Erasure: `subagent_edges` are in the GDPR summary (`SC/src/gdpr.rs:260`).
**Tests.** budget refuses the remaining items and never a started one; subscription children do not trip the token budget; retry reads the goal only for the owner; lineage readback includes 5-7 and stays byte-identical for old edges with empty metadata; panel replay after a simulated reload; real-PG `#[ignore]` test for the lineage query (pattern `SC/src/grpc.rs` DATABASE_URL tests). `cargo test -q -p execution-core --lib research_fan_out`, `-p session-core --lib orchestration_grpc`, `pnpm exec vitest run src/features/chat/components/ChatWideResearchPanel.test.tsx`.
**Live proof.** 10 items with a small budget: 6 run, 4 `budget_reached`; retry one; reload the page mid-run and after: grid restored; capture SSE, lineage JSON and screenshots.
**Effort.** M, 5-7 d. **Deps.** S8; D4. **Risks.** the 300 s delegated bearer bounds long fan-outs (DEC-02; RunDeadline stops new rounds) [verify live]; usage of a child that failed mid-round may be unreported, so the budget can be slightly exceeded (state in docs).

### S10 - Do-loop reader parity [BUILD, optional]
**Goal.** Wide-research children and Do runs read long pages the same way as chat.
**Files.** `EC/web_tools.rs` / `EC/runtime_loop/result_handles.rs` - when `EXECUTION_RESULT_HANDLES` and `EXECUTION_WEB_READER` are on, `web_fetch` returns outline + first chunk + ref and `read_page`/`find_in_page` are offered, sharing `page_reader` logic moved into `mp-contracts` (pure, no I/O) so both loops use one implementation (the two loops stay separate; only the pure helper is shared). Capability policy: the two tool names map to `cap.tool.http` read class (`EC/capability_policy.rs:1571`).
**Contract/flag.** `EXECUTION_WEB_READER` (default off). **ZDR.** Do runs refused under ZDR; waived runs never park (`result_handles.rs:18-20`).
**Tests.** as S2 for execution-core; `cargo test -q -p execution-core --lib result_handles web_tools`. **Live proof.** A wide-research run where an item needs a fact deep in a long page. **Effort.** M, 3-5 d. **Deps.** S2, D10. **Risks.** run-scoped store memory per run; capability-core class seed for two new tool names (WS05 rule: unmapped tool ids fail closed).

### S11 - Flip, document, prove [FINISH]
Turn on, on the proof org only, in order: S1 (no switch), `RESULT_HANDLES_EXTENDED` + `MODEL_GATEWAY_WEB_READER`, `MODEL_GATEWAY_WEB_SEARCH_BATCH`, `MODEL_GATEWAY_CITATION_PASSAGES`, `DEEP_RESEARCH_PLAN_TURN`, `DEEP_RESEARCH_SECOND_PASS`, `DEEP_RESEARCH_KNOWLEDGE_SOURCES`, `WIDE_RESEARCH_ENTRY_ENABLED`, `WIDE_RESEARCH_CHILD_BUDGET_ENFORCED`, `EXECUTION_WEB_READER` (S10 if built). Each flip only after its slice's live proof. Defaults stay off in compose until the owner says otherwise. Ledger entries (D1-D5), `MP/docs/ZDR.md` rows, `flags-and-config.md` rows, SSE taxonomy, ZDR golden cases. Close D1-107 (citation half), D1-102/D1-104 with the S1/S4 transcripts, and mark D2-172 and D2-013 as stale-resolved with the code citations. **Effort.** S, 2-3 d.

## 5. ZDR clause

**New stores and egress, and the behaviour under ZDR and waived turns.**

| Slice | New store or egress | ZDR turn | Waived turn (DEC-32) |
|---|---|---|---|
| S1 | none | binds KB chips the same way | same |
| S2 | none new: page text in the existing 900 s gateway handle store | web tools never offered (`browse_web`/web-only refused, `zdr_features.rs:98,103`) | no parking (`external_parking_allowed`): old head output, no `ref`, no reader tools |
| S3 | turn-scoped ref -> URL map in memory; more Quarry searches per call (Quarry is the egress owner; ZDR provider gating lives there, ledger 2026-09-03) | refused (web search) | allowed, refs in memory only |
| S4 | `quote`, `locator`, `provenance` keys in message metadata; a Data Plane chunk read (no store) | KB `quote`/`locator` stream live, nothing persisted; chunk read allowed (a read) | streamed, not persisted |
| S5 | `research_plan` key in message metadata | refused (`deep_research`) | streamed, not persisted |
| S6 | `research_ledger` key in message metadata; one more plan-tier inference | refused | streamed, not persisted |
| S7 | Data Plane retrieval per sub-query; connector reads (gated) | refused (deep research; connector reads also refused) | knowledge allowed; connector reads follow WS23 (refused on waived for new stores) |
| S8 | estimate route (no store); `wide_research` on the run request | refused (`agentic`) | follows Do-run rules today |
| S9 | lineage readback of data already stored | no fan-out | follows Do-run rules |
| S10 | run-scoped handle store (exists) | Do refused | no parking |

**Refusal behaviour.** No new refusal feature key: every refused path reuses `web_search`, `deep_research`, `agentic`, `connector_read` from `MP/api/contracts/zdr-refusals.json`, so that file and its `.sha256` do not change. A test must prove `knowledge_only` + deep research is still refused under ZDR after S7 lifts the scope conflict for non-ZDR turns.

**Golden matrix cases** to add to `MP/api/contracts/zdr-posture-matrix.json` (enforcement rows, `e_` prefix, beside `e_browser_do_tools_refused_under_zdr`) and the live inventory tests (`tests/zdr_posture_matrix.rs`, `tests/zdr_refusal_inventory.rs`):
- `e_zdr_token_web_reader_not_offered` (S2)
- `e_waived_web_reader_keeps_inline_head` (S2)
- `e_zdr_token_batched_web_search_refused` (S3)
- `e_zdr_turn_kb_citation_passage_streams_not_persisted` (S4)
- `e_waived_research_metadata_not_persisted` (S4, S5, S6: plan, ledger, provenance)
- `e_zdr_token_deep_research_plan_turn_refused` (S5)
- `e_zdr_token_knowledge_only_deep_research_refused` (S7)
- `e_zdr_token_deep_research_connector_sources_refused` (S7)
- `e_zdr_token_wide_research_estimate_refused` (S8)
- `e_zdr_token_wide_research_run_refused` (S8)
Each new metadata key (`research_plan`, `research_ledger`, `provenance`, `quote`, `locator`) gets a row in `MP/docs/ZDR.md` (what, where, erased by the message GDPR path) and the plan's 6.6 list. Owner of ZDR acknowledges D2 and D12.

## 6. Proof plan

1. Per slice: unit tests named above, `cargo fmt`, taxonomy and ZDR contract checks; FE vitest and lint; typecheck diffed against HEAD's failing set.
2. Rebuild for each live proof: model-gateway (S2-S7, S8 estimate), execution-core (S8-S10), session-core (S9), capability-core (S8 key mirror), BFF (S8 route), FE on a second dev server (memory: `:5173` is the main checkout container and needs a restart for host edits). Use `.plan-work/execution/tools/recreate-with-live-env.sh` after a worktree build so the live env is kept.
3. Live flows (non-ZDR proof org, Anthropic key per the owner's env file; stop and summarise at the usage line):
   - F1 chips and viewer: KB + web turn, chip -> passage, Sources tab, reload binds the same.
   - F2 long page: `fetch_url` -> `find_in_page` -> answer citing past 4,000 chars; no second scrape.
   - F3 batched search: one call, 3 queries, refs, `fetch_url {ref}`.
   - F4 deep research v2: plan card edited, run, second pass on a conflicting fact, ledger in metadata, stamp on the report.
   - F5 knowledge-only research: zero Quarry calls.
   - F6 wide research: org cap clamp, estimate card, budget stop, retry, reload replay.
   - F7 ZDR org: each refused path returns the typed refusal with zero side effects (session-core row counts before/after), the KB viewer works on a ZDR turn without persisting.
4. Evidence: SSE transcripts (curl -N through the BFF), Quarry edge logs (counts only), `psql` keys-only reads of message metadata and policy rows, screenshots at 1280 and 375 px, appended to the depth register as `D3-nnn` rows.
5. Regression guards (audit section 8 "Research honesty"): read-versus-not-read stays (unread sources never get a number: S1, S4, S6 tests), the mandatory "could not be verified" section stays (S6 test), the `source_scope_applied` receipt stays on every scoped turn (S7 test).

## 7. Risks and what the owner must decide

**Risks.**
1. Cost: S3 (more searches), S6 (one more inference plus reads), S8/S9 (N children). Bounded by existing ceilings, the new child budget and the org item cap; nothing raises a ceiling.
2. Prompt injection: more page text reaches the model (S2, S6) and connector content (S7). Every payload is screened before parking or reading; read-backs replay the verdict; connector content is `ConnectorContent`. Depends on WS26 live proof (D2-112) before the reader is turned on for real orgs.
3. Credential window: deep research (410 s) and wide research versus the ~300 s bearer (DEC-02 open). S6 guards; S9 relies on `RunDeadline`. Real behaviour [verify live].
4. Hot files: `MG/sse.rs`, `MG/tool_loop.rs`, `EC/runtime_loop/agent.rs`, `FES/.../chat-media-markdown.tsx`, `ChatPanels.tsx`. Follow the plan's edit-first order (4.5); S2/S3 touch the same arms and should land in one train.
5. Per-process handle store: refs do not cross gateway replicas; fine for one stream, not for a resumed stream on another replica [INF].
6. The ledger is model-labelled: it may mislabel; it only routes claims to the hedged section and never removes evidence.
7. Quarry inline return and provider gating are Quarry's; if the deployed edge predates the inline build, pages arrive through the artifact path (works outside ZDR) [verify live].

**Owner decisions (block only the slices named).**
- OD-1 (D1, D2): research state in message metadata with the closed ledger schema, no durable run, no new table. Blocks S5, S6 (and S4's stamp half).
- OD-2 (D3): allow knowledge-only deep research. Blocks S7.
- OD-3 (D4, DEC-40/OD7): separate child budget, no confirm gate in v1; default budget 400,000 metered tokens per fan-out. Blocks S9's enforcement half.
- OD-4 (D5): org item cap as an agent-policy key in capability-core (default 8, max 16). Blocks S8's clamp half.
- OD-5 (D9, register D2-321): batched search only in the Ask loop and attended Do, never the scheduled lane. Blocks S3 enablement.
- OD-6 (D12, DEC-32): waived turns do not persist plan, ledger, provenance or passages. Blocks S4-S6 enablement (defaults already refuse).
S1, S2, S3 (build), S8 (entry and estimate), S9 (retry and replay) and S10 do not wait on any decision. Suggested order: S1, S2, S3, S4, S5, S8, S9, S6, S7, S10, S11; S8/S9 can run in a second lane in parallel with S4-S6 (different services).
