
## 5. Dependencies and ordering

**Hard.**
- D5 slice 4 (`ScrapeOptions`, the custom `ScrapeFormat` deserializer, `run_with_inline`, `ScrapeResponse`, `FieldError`) before slice 6. Slice 5 asks D5 for two fields on `InlineParts` (`structured`, `json_ld`) and a `want_harvest` switch on `PageRunner`; if D5 declines, slice 6 harvests inside `ai_formats_api` from the decoded HTML it re-reads from the driver response (one more parse of up to 16 MiB; acceptable only for the first release).
- Inside this workstream: 1 before 3 (the golden the fake reads); 3 before 4 before 5 before 6; 2 before slice 6 tier E2; 7 after 6; 8 independent; 9 after D5 Epic E1, F-t2-017, F-t2-040 and D6 S0/S3.
- **DR-15.1 ruling** before slice 2. Nothing else blocks the engine.
- Redaction ordering with F-t2-050 (recorded in D5 section 5): AI formats must read the **post-redaction** markdown. If F-t2-050 is not live when `AI_FORMATS` is enabled, page text goes to the Model Plane unredacted; that is acceptable only if the owner says so (question 4).

**Soft.**
- D6 (F-t2-008): `LimitsResolver` and `BUDGET_EXHAUSTED` are used when present; static floors otherwise. Interface requests to D6, all additive: a dimension value set `llm_tokens | llm_calls | llm_cost` in the `details.dimension` vocabulary (DEC-2), the metric `quarry.extract.llm` in DEC-10's list (its `llm_format` attribute already anticipates it), and a `quarry.extract.llm.tokens` allowance key. D6 owns `quarry.extract.url` in `extract_routes.rs:344-360`; this plan does not touch that block.
- D4 (ZDR): row 23 ("AI formats refused until the request carries ZDR") is satisfied by slices 3 and 6; D4's log-field sweep and lint should cover the new modules (the no-content-in-logs tests here are the local guard).
- F-t2-048 (org-scoped URL cache): an extraction cache would key on `(org, text_fingerprint, spec_sha256, model alias)`; not built here, because a cache of model output is a content-persisting boundary and belongs with the cache decision. The fields are returned in `ai.<fmt>` meta (`schema_sha256`, `text_fingerprint`) so F-t2-048 can add it.
- F-t2-001/D1 (delegated bearer): independent; it is option E of DEC-1 and the alternative for user-initiated calls later.
- F-t2-037, F-t2-050, F-t2-030 (phase 2), F-t2-062, F-t2-065: reuse the lane; each needs its own gateway service route (documents, language/PII) added as a row to `SERVICE_ROUTE_GRANTS` and the golden, one reviewed PR per route. They are not blocked by this plan's engine, only by the same grant decision.
- D7 (policy): `ResolvedPolicy` could one day switch AI formats off per org (tighten-only); the entitlement hook is the D6 resolver key, so no new design here.

**Merge-conflict hotspots** (files and live line ranges shared with other workstreams or with uncommitted work):

| File | Range | Shared with |
|---|---|---|
| `Q/quarry-edge/src/routes.rs` [M] | `scrape()` 398-530, cache 432-487 | D5 slice 4, F-t2-023, F-t2-048, F-t2-003, F-t2-046 |
| `Q/quarry-edge/src/extract_routes.rs` [M] | 38-58, 138-182, 205-233, 242-331, 344-360; in-flight hunks 205-228 and 512+ | D5 (parse), D6 S2 (meter block), F-t2-050 (redaction before the model), QB-T17 gate work |
| `Q/quarry-edge/src/state.rs`, `config.rs` (163, 391), `main.rs` (246-251, 1190-1220), `test_support.rs` [M] | `AppState` literals, config fields, boot gate | D5 D19 (16 literals), D6 (`limits`), QB-T17 hunks (+187, +188 lines) |
| `Q/quarry-edge/src/model_plane_gate.rs` [U] | `rows()` 92-97, `FEATURES` 53 | F-t2-037, F-t2-050, QB-T17 |
| `Q/quarry-runtime/src/mp_client.rs` [C] | 31-47, 116-201 | F-t2-037 (OCR hop), F-t2-050 (PII hop), planner client |
| `Q/quarry-runtime/src/pipeline.rs` [C] | 506-508, `InlineParts` | D5, F-t2-023, F-t2-048, F-t2-050 |
| `Q/quarry-runtime/src/service_tokens.rs` [C] | 40-63, 718 | D1 (browser-broker audience), F-t2-037, F-t2-050 |
| Manifest, `.sha256`, vendored copy, `zdr-contract-sync.test.sh:117` | whole file / one digest | every QB-T17 consumer, D1, QB-T09 |
| `CP/config/plane-service-principals.json` [C] | `quarry-edge` object | D1 (audience for the broker), D7, F-t2-037/050: hand-merge |
| `QR/scripts/tests/docs-contract.test.mjs` [U] | 213-227, 258-267 | QB-T17 |
| `MP/model-gateway/src/http_routes.rs` [M] | `AiChatRequest` 1813-1829, `ai_chat` 2328-2385, route table 411-448 | sibling (b), in-flight ZDR hunks (+237), F-t2-037 (documents 432-437), F-t2-050 (language/pii 442) |
| `MP/model-gateway/src/auth.rs` [M] | 1367-1401 | F-t2-037/050 (table rows), sibling (b) |
| `MP/model-gateway/src/grpc.rs` [M] | `extract_structured` 2516-2657, `quarry_err_to_status` 2679+ | sibling (b), D5 slice 7 |
| `MP/model-gateway/src/tool_loop.rs` [M] | 2987-3040, 4233, 4359 | D5 slice 7, sibling (b), in-flight hunks at 3039, 3115 |
| `MPX/proto/model_plane/v1/gateway.proto` [C] | 438-507 | sibling (b) |
| `QR/docs/openapi.yaml` [M] | `/v1/scrape`, `/v1/extract` schemas | D5, F-t2-064 |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

**Per rule.**
- **Control Plane** owns identity, orgs, billing, sessions, audit, quotas, entitlements. This plan changes only its **registry file and specs** (a grant is Control's policy); Auth Core audits every mint (`model_service_token_issued`, durable audit required, `model-plane-token.controller.ts`). Quotas and entitlements are read through D6's resolver, never stored in Quarry. Pricing is not decided.
- **Data Plane v2**: untouched. Harvest and extraction output are not ingested; no database access crosses a plane. Durable knowledge from extracted data (a later feature) would go through the existing ingest contract only.
- **Ingestion Plane** captures evidence: fetch, clean, harvest, verbatim verification and budgets are Quarry's. Matrix row 62 gives generation to the Model Plane, and that holds: Quarry owns the format contract and "bridge/output validation", the Model Plane owns inference, provider routing, model choice and cost. Prompt templates live in Quarry because they are bound to the verified output contract; flagged to the Model Plane owner as a DECISION in DEC-1/DEC-3 (a dedicated route B would move them).
- **Model Plane** owns reasoning, inference, capabilities, cost: the lane, rate class, metering and the interim route are its code (owner O1/sibling (b)).
- **Application Plane**, **Channel Plane**, **Support Plane**: untouched. **Frontend Plane**: the gateway only forwards two optional keys and adds no secret, no forged header, no scoping header.
- No cross-plane database access is proposed anywhere. Quarry keeps no second cost ledger (rule 16): the ledger of record is cost-core fed by model-gateway.

**ZDR propagation per content-persisting boundary.**

| Boundary | Under ZDR | Mechanism |
|---|---|---|
| Model token | tenant posture: `zdr:true` for a ZDR org | registry `retentionByAudience: tenant`; 60 s tokens; no body field accepted by Auth Core |
| Model request | `zdr:true` in the chat body whenever Quarry's effective ZDR is on; typed refusal 412 honoured | `claims.effective_zdr_mode`; `ModelHopError::ZdrRefused` |
| Gateway side | no thread, no run, no envelope, no response-cache entry | `ai_chat` is non-persisting; `zdr_or_refuse`; inference-core cache skips ZDR (`cache.rs:105`) |
| Quarry artifacts, scrape record, page cache | none written (D5 rules; AI requests are non-default options and bypass the cache for everyone) | `zdr::guard` |
| Extraction output | returned in the response only; never stored, cached, evented or logged | slice 6 tests assert 0 artifacts and 0 Redis keys |
| Usage metering | content-free allowlist (kind, counts, tokens, zdr flag) | `usage_attrs` of D6 |
| Logs and tracing | field names, kinds and counts only; never schema, prompt, page, output or URL | slice 4 `no_content_in_logs` test |
| cost-core | **no row for ZDR turns** by Model Plane doctrine; cost under ZDR is visible only as Quarry's content-free counters | stated in DEC-10 |
| Crawl, batch, schedules | refused under ZDR (existing guard) | slice 9 never sees ZDR |
| Interim route | `zdr:true` to inference-core, no envelope | slice 8 tests |

`AI_FORMATS_ZDR=refuse` (default until E2) makes the safest reading the shipped one.

**Model proposes, Quarry executes or rejects.** The Model can propose a schema, a question or a prompt (as tool arguments to a Quarry-facing call). Quarry validates every field (limits, `$ref` policy, scopes), rejects with field-level 400s, and the Model's proposal is only data to a schema-constrained call with no tools and no side effects; nothing in a format can reach a browser action, a script, a header, a proxy or a URL. A service principal (the Model Plane's own tokens) **cannot** request an AI format at all without `extract:ai`, which no principal holds: otherwise a Model-driven `fetch_url` could make Quarry spend model money outside the run's budget (the gateway attributes those rows to `service:quarry-edge`, not to the run). In the interim route the roles are the plain ones: the Model proposes the extraction, model-gateway runs the inference step it owns, Quarry only fetches.

**Rules at risk.**
1. **Ownership split of AI formats** (matrix rows 61-62): defensible as written, but the prompt templates in Quarry should be confirmed by the Model Plane owner (DEC-1).
2. **Delegated-identity doctrine** (G1 gives quarry-edge `inference:invoke` on `inference-core`, bypassing the gateway if the network allows): mitigated by tenant binding, network policy, rate class, and the retrieval-engine precedent; named in DR-15.1.
3. **ZDR**: no ledger row under ZDR means a ZDR org's extraction spend is unbilled by the Model Plane; the owner may want a content-free envelope for ZDR (open in the DR text).
4. **Org safety policy** (`injection_defense`, required PII redaction) does not apply on `ai_chat` (section 2.2, DEC-7): until F-t2-050 or a gateway change closes it, this lane relies on Quarry-side framing and verification only.
5. **Tenancy**: a directory-bound principal can mint for any real org; a leaked quarry credential could therefore spend model money for any org within the service rate bucket. Same exposure class as the existing `documents:write` grant, now with spend.

## 7. Cross-session interfaces

**(a) Verevon Space (shared workspaces, agent PC and browser in Spaces, ZDR-safe telemetry).**
- Shared piece: ZDR-safe telemetry. This plan's rule (names, kinds, counts, tokens; never schema, prompt, page, output, URL) and the content-free `quarry.extract.llm` attributes; Space must reuse them for any extraction telemetry it adds. Owner: D4 for the rule, this plan for the new call sites.
- Shared piece: extraction in Space chat goes through the interim tool (slice 8, runs inside the Model Plane run, ZDR from the run's resolved posture) until the Quarry path is live; it never takes a Space-scoped Quarry token.
- Not shared on purpose: extraction results are returned inline and persisted nowhere; sharing a result with Space members would need the resource-taxonomy decision (ADR 8); nothing here widens an ACL.
- User must reconcile: product copy for `ZDR_UNAVAILABLE` and `BUDGET_EXHAUSTED` (i18n keys in `SPA/shared/i18n/errors.ts` [M], the same file D5 edits).

**(b) Model Plane and chat/agents (BrowserBroker grants, agent browser path, delegated bearer forwarding, approvals).**
- Shared piece, proposed owner sibling (b): the **service lane** of model-gateway (`/v1/ai/chat`, the `SERVICE_ROUTE_GRANTS` table, the golden `model-gateway-service-routes.json`, the service rate bucket, metering). Slices 1 and 8 live in its files; this plan supplies the spec, tests and the Quarry client. F-t2-037 and F-t2-050 add rows to the same table.
- Shared piece: the second bearer. Any service calling `ai_chat` must present `x-inference-authorization` (`aud=inference-core`, `models:invoke`, and `inference:invoke` downstream): worth a line in the Model Plane docs so the next service does not rediscover it.
- Shared piece: the `extract_structured` tool and the `ExtractStructured` fixes (slice 8), owned by sibling (b); the corpus is shared byte for byte.
- Delegated bearer forwarding (F-t2-001/D1): not needed by this plan; option E of DEC-1 if a user-initiated, non-service path is wanted later.
- Approvals (F-t2-033): extraction is read-only and is not approval-gated; its cost is bounded by budgets, not by consent. Run budgets (F-t2-008) see interim-route spend (inside the run) but **not** Quarry-side AI-format spend (attributed to the service), which is the reason for the `extract:ai` default-deny.
- User must reconcile: (1) the owner slot for DR-15.1 (O1/O4); (2) who commits the in-flight `http_routes.rs`, `grpc.rs`, `zdr_posture.rs`, `zdr_features.rs` hunks before slices 1 and 8 edit the same functions; (3) whether the route table should name `service:quarry-edge` for `/v1/ai/chat` (tightening; today any service holding `models:invoke` passes); (4) whether a content-free ZDR usage envelope is wanted.

## 8. What can proceed WITHOUT an ADR/decision, and what is blocked

**Proceed now:** slice 0 (docs; the ledger entry is a proposal, not a decision); slice 1 (behaviour-preserving for existing callers, needs only an owner slot); slices 3, 4, 5 (library and fake, no route exposed, flag off); slice 7 (optional keys); slice 8 behind its flag (and its bug fixes unflagged); slice 6 as dark code behind `QUARRY_EDGE__AI_FORMATS=0`, after D5 slice 4.

**Blocked, and by what:**
- Slice 2 and everything that mints the two tokens: **DR-15.1** (new decision; adjacent audit topics 1 and 4).
- Slice 6 tier E2 and any production enablement of `QUARRY_EDGE__AI_FORMATS=1`: slices 1 and 2 deployed, D5 slice 4, and question 4 (org safety policy and redaction ordering).
- `AI_FORMATS_ZDR=allow`: the E2 ZDR proof plus the owner's reading of D4 row 23 (audit ADR 4: "what does ZDR mean for this feature").
- `USAGE_LLM_METRICS=on`: Lago billable-metric codes and the pricing decision (question 2).
- `AI_FORMATS_HARVEST=prefill`: the slice 5 precision gate (a measurement, not a decision).
- Any service principal using AI formats: a ruling to grant `extract:ai` (default deny).
- Slice 9: D5 Epic E1, F-t2-017, F-t2-040, D6 S0 and S3; and ADR 4 for what ZDR means for durable jobs (existing refusal stands).
- A dedicated route B (DEC-1): the Model Plane owner's choice; the Quarry client is one line away from it.

## 9. Open questions for the user (real blockers only)

1. **DR-15.1: approve the grant shape and name the owner.** Is G1 (two audiences, `tenant` retention, two new manifest rows, route table unchanged, `model-invoke`/`model-stream`/`model-audio` stay `granted:false`) acceptable, or should extraction wait for G3 (interim route only)? And which owner option (O1 to O4) takes DEF-QB-T17-PART2? Blocks slice 2 and every production use of the Quarry path; the Model-proposed interim route does not wait.
2. **Do AI formats cost the customer anything?** Meter only (`shadow`), or meter and bill (needs Lago metric codes and a per-format price; Firecrawl charges four credits per page per format)? Blocks `USAGE_LLM_METRICS=on` and any entitlement check; not the build.
3. **ZDR for AI formats:** allowed with the explicit flag and typed refusal when no attested provider exists (recommended), or refused for ZDR orgs until a Model Plane ledger doctrine for ZDR spend exists? Blocks flipping `AI_FORMATS_ZDR=allow`; the default ships as `refuse`.
4. **Org safety policy on this lane.** The org's `injection_defense` and required PII redaction are applied on `invoke`, not on `ai_chat`. Must they apply before this lane serves production traffic (then F-t2-050, or a gateway change, goes first and the order is 050 then 019), or is Quarry-side framing plus verbatim verification enough for the first release? Blocks production enablement, not the build.

Confirmations that do not block: default routing aliases (`verevon-balance` for `json`, `verevon-budget` for the rest); `extract:ai` as the new scope name; a 60_000-character default input cap.

## 10. INTERFACE SUMMARY (for the integrating architect)

**Files touched.** Ingestion (Quarry-v2): quarry-core (`scrape.rs` from D5, `json_schema.rs`, `contracts.rs` deletions), quarry-runtime (`mp_client.rs`, new `ai_schema.rs`, `ai_extract.rs`, `harvest_map.rs`, `ai_verify.rs`, `service_tokens.rs`, `pipeline.rs`, `usage.rs`, delete `structured_extract.rs`), quarry-edge (`routes.rs`, `extract_routes.rs`, new `ai_formats_api.rs`, `scrape_contract.rs` from D5, `model_plane_gate.rs`, `config.rs`, `main.rs`, `state.rs`, `auth.rs`), compose, docs, tests and fixtures. Model Plane: model-gateway (`http_routes.rs` ai_chat, `auth.rs` route table, `rate_limit.rs`, `grpc.rs`, `tool_loop.rs`, new `web_extraction.rs`, `schema_check.rs`), `gateway.proto`, `api/openapi.yaml`, new `api/contracts/*`, ledger. Control Plane: `plane-service-principals.json`, auth-core fixtures and policy spec. Frontend: gateway `runs.rs`.

**New contracts.** `/v1/ai/chat` additive keys (`temperature`, `max_tokens`, `purpose`; response `provider_used`, `usage.cost_usd`); golden `model-gateway-service-routes.json`; two manifest rows (`model-extract`, `model-extract-inference`); registry audiences `model-gateway`, `inference-core` (retention `tenant`); scrape formats `json|summary|question|highlights`, response fields `json`, `summary`, `answer`, `highlights`, `ai`; scope `extract:ai`; metric `quarry.extract.llm`; proto fields `ExtractStructuredRequest` 10-14, `ExtractStructuredResponse` 11. No SQL migrations anywhere. Flags: `QUARRY_EDGE__AI_FORMATS` (+ `_ZDR`, `_HARVEST`, `_REPAIR`, model, budget keys), `USAGE_LLM_METRICS`, `MODEL_GATEWAY_AI_CHAT_METERING`, `MODEL_GATEWAY_SERVICE_RPM`, `MODEL_GATEWAY_EXTRACT_TOOL`.

**Dependencies.** Hard: D5 slice 4, DR-15.1, slices 1+2 for E2. Soft: D6, D4, F-t2-050 ordering, F-t2-048.

**Effort.** S0 1d, S1 5, S2 3, S3 5, S4 5, S5 5, S6 8, S7 2, S8 7, S9 8-10 unscheduled; 41d without S9; item revised M to L.

**ADR gates.** New decision DR-15.1 (adjacent topics 1 and 4); topic 4 for ZDR semantics; none other.

**Parallelisable with:** D5 slices 1-3, D1, D2, D3, D6 S0/S1/S5/S8. **Conflicts with:** D5 slice 4 and slice 7 (`routes.rs`, `extract_routes.rs`, `quarry.rs`), D6 (`state.rs`, `extract_routes.rs` meter), F-t2-037/050 (route table, manifest, registry object), sibling (b) and the in-flight Model Plane diffs (`http_routes.rs`, `grpc.rs`, `tool_loop.rs`).

<!-- COMPLETE -->
