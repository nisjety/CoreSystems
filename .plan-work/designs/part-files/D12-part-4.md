
## 5. Dependencies and ordering

### 5.1 Dependencies

| Slice | Hard dependency | Soft dependency | Notes |
|---|---|---|---|
| S0 | none | none | Pure library; can start today. |
| S1 | S0 | D3 slice S3 (`cdp_call` deadline table in `Qbr/chromiumoxide.rs`): land after it or carry a local `tokio::time::timeout`. D3 slice S10 (F-t2-015 actionability pipeline) edits `execute_native_target_action` 1232-1374, which S1 only reads for the frame-context lookup: rebase, do not duplicate | Q10 recipe runs before S1 merges (D12-6). |
| S2 | S0, S1 (the stamp reads `projection.redaction`) | D2 slice S2 (agent_routes split) for the `driver_err` hunk; D3 slice S7 (`challenge`) and D6/D8 for the seven `BrowserObservation {..}` literal edits | Single "additive `BrowserObservation` fields" PR if two streams are in flight. |
| S3 | S0, S2 (`typed_field_kind` needs `field_kind` on targets from S1) | D2 slice S2 (the `step.rs` module), D2 slice S3 (receipt initiator and epoch, migration 0010), D8 slice S8.1 (human metadata receipt: one function `redacted_for_record`), D6 (migration 0011 `cost_source`), D8 (migration 0011 `tab_attach`) | Migration numbers: live latest is `0009_browser_timeline_events.sql`; D2 takes 0010, D6 and D8 name 0011; this design's `0012_step_receipt_redaction_version.sql` is tentative, first-come at merge. |
| S4 | S0 (frozen golden) | S2 (the wire stamp; the guard defaults to off until S2 is deployed) | Runs in parallel with S1 to S3. Model Plane sibling session (b) owns `browser_agent.rs`/`quarry_agent.rs` for D1 (delegated bearer) and D10: coordinate. |
| S5 | S1, S2 | D3 S2 (viewport emulation touches the screenshot path), D8 (human-browse screenshots) | New `png` dependency. |
| S6 | S0 | none | Can ship any time after S0. |
| S7 | S3 | product decision (question 3) | Optional. |
| F-t2-024 epic | **S0 to S5**, F-t2-001, F-t2-013, F-t2-015, custody ADR (9.3 topic 2) for E5 only | F-t2-033 (approvals) | Hard-blocked on this item, per the audit. |
| Downstream consumers | | F-t2-050 (fail-closed PII stage in `quarry-transform`) reuses `shapes.rs`; D10 (run trace envelope) wants the legacy raw `ActionStarted` SSE payloads gone (S2 does it); D8 `human_browse` production enablement needs at least D8's narrow receipt redaction, which S3 generalises; D11 `default_on` is gated on this item per its text | |

Critical path for "no secret in durable evidence": S0 -> S1 -> S2 -> S3. For "no secret in a prompt": S0 -> S4 (parallel). Screenshots last.

### 5.2 Merge-conflict hotspots (live line ranges, shared with other workstreams)

| File | Lines | This design | Others touching the same region |
|---|---|---|---|
| `Qrt/observation.rs` [C] | 346-1017 `execute`; 386-396, 614-644, 965-989, 1004-1009, 1428-1464, 2133-2146 | S2 (masking, stamp, event payload, JSON artifact writer, char-boundary fix) | D3 S7 (`challenge` 903-909), D6 (budget/cost fields), D8 (human receipt), D10 (event projection, `emit_for_zdr`), D2 (timeline), D12-S5 (517, 654, 686-834) |
| `Qedge/agent_routes.rs` [C] | 2228-2409 `step`; 3086-3193 `execute_ws_action`; 453-472; 1116-1122 | S2 (`driver_err`), S3 (`typed_field_kind`, builders, `list_receipts`, checkpoint url) | D2 S2 splits the file into `agent_routes/*` (do S2/S3 after the split); D8 (receipt mode), D6, D1 |
| `Qrt/step_receipts.rs` [C] | 182-210, 457-557 | S3 | D2 S3, D6, D8, D10 |
| `Qcore/contracts.rs` [C] | 19-121 (`BrowserObservation`), 313-331 (`ProofBundle`), 640-827 (`AgentAction`) | S2, S3 (additive) | D3 S7, D5, D8 (`redacted_for_human_receipt` is defined by S3's function), D6 |
| `Qbr/chromiumoxide.rs` [C] | 1089-1198, 2392-2446, 2681-2730 | S1, S5 | D3 S2/S3/S5/S10 (D3's own table lists 1053 and 1089-1157 as an AX-wrap region shared with F-t2-021) |
| `Qcore/error.rs` [C] | `ErrorCode` enum, `QuarryError` | S2 (`redacted`), S6 (`log_message`): methods only, no new code | D6 and D2 add codes: textual neighbours only |
| `Q/docs/openapi.yaml` [M] | 2544, 3780 (plus new schema) | S2, S3, S5 | in-flight hunks at 238-250 and 4012-4150: disjoint; D2/D6/D8 edit the same schemas |
| `scripts/tests/zdr-contract-sync.test.sh` [M] | `GOLDENS`, `VENDORED`, `CONSUMERS`, header | S0, S4 | in-flight hunks (105, 117-139, 198-244, 275-288, 380); rebase last, append as one contiguous block per array |
| `MG/http_routes.rs` [M] | 2023-2097, 2099-2160, 2174-2205 | S4 | in-flight hunks 824-830, 1400-1756, 2032-2036, 2106-2112, 2335-2345, 3712-3835; sibling session (b) |
| `EC/browser_agent.rs`, `EC/quarry_agent.rs`, `EC/llm_planner.rs` [C] | 170-186, 738-751; 219-248, 407-474; 200-235 | S4 | D1 (delegated bearer in `step`), D10, sibling (b) |
| `Q/crates/quarry-runtime/migrations/` | `0012_`, `0013_` | S3, S7 | D2 0010, D6 and D8 0011 |
| `apps/Ingestion Plane/docker-compose.yml` [M], `Q/docs/RUNBOOKS.md` [M] | one env line; appended section | S2, S5 | every Quarry workstream adds env lines: append only |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

- **Control Plane owns identity, orgs, billing, sessions, audit, quotas, entitlements.** No change. The stamp and markers carry no identity. Receipt reads are still owner-scoped by the existing actor scope (`0008_step_receipt_actor_scope.sql`). No quota is added.
- **Data Plane v2 owns documents, chunks, embeddings, retrieval, graph, wiki, source traces.** Respected. Durable content that Quarry persists through Data Plane contracts is covered by the Durable profile in the extraction path only through F-t2-050 (explicitly out of scope, D12-13); this design does not write to Data Plane. `[RISK]` until F-t2-050: a secret displayed on a page can still reach a scrape artifact and a Data Plane document through the crawl pipeline (`quarry-transform`), which this design does not cover. State it in `SECRET_REDACTION.md`.
- **Ingestion Plane captures evidence and persists through Data Plane contracts only.** The module lives in `quarry-core`, applies at Quarry's capture boundary, and uses no database of another plane. The Model Plane gets its own copy (`mp-scrub`), shared by duplication plus a golden, precisely so that no code or table crosses planes.
- **Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants and cost.** Respected: Quarry's masking never calls a model; the Model Plane's own scrub is its decision (D12-11) and the stop-criteria logic keeps the raw observation. The grant and approval work for F-t2-024 stays in the Model Plane (BrowserBroker, approvals card).
- **Application Plane / Frontend Plane.** No change: the SPA has no AX consumer (2.1 row 15); the gateway `suggest_action` forwards a Quarry observation that is already masked, and model-gateway scrubs it again (S4). The gateway is not edited.
- **No direct database crossing.** Confirmed: the only SQL is Quarry's own `quarry_step_receipts` column and an optional Quarry-owned log table.
- **Model proposes, Quarry executes or rejects.** Respected and strengthened: the model never receives a typed secret's value back, and the model's own typed value (its proposal) is masked in the durable record by Quarry, not by the model. S5's withheld screenshot and the F-t2-024 executor-side fill extend the same rule. The stop-criteria and `done` decisions in the Model Plane read the raw observation in memory only, never persisted unscrubbed (`grpc.rs:1015-1042`).
- **ZDR must propagate through every content-persisting boundary.** Masking is **independent of the ZDR flag** (D12-10): Live observations are masked on ZDR runs too (they are returned although nothing persists), Durable masking applies to events, logs and errors always and to receipts, artifacts and checkpoints whenever they are written (non-ZDR). Boundaries covered: receipts (S3), checkpoint URL (S3), events (S2), JSON artifacts (S2), image artifacts (S5), logs (S6), errors (S2), planner prompt and events (S4). Boundaries **not** covered here and owned elsewhere: Quarry telemetry spans (D10 S8), the gateway in-memory `last_observation` and session cache (D9/D4), the Model Plane checkpoint (already scrubbed at `grpc.rs:1042`), the live frame stream (open question 4), scrape artifacts (F-t2-050). **Rule at risk:** the live frame stream and an OOPIF credential field in screenshots are the two places this design cannot guarantee (both marked `[LIMIT]`).
- **Channel Plane** is not touched.

## 7. Cross-session interfaces

### (a) Verevon Space (shared workspaces, agent PC/browser in Spaces, ZDR-safe telemetry)

| Shared piece | Proposed owner | What the Space session must know or reconcile |
|---|---|---|
| Masked live frames: the owner's own frame stream is not masked (JPEG screencast needs per-frame decode/encode); a Space member watching another member's agent or human browser would see a typed password | Quarry owns the mask primitive (`screenshot_mask`, the rectangle scan of S5); the Space session decides whether members get a masked view | Reconcile: (1) accept a masked stream for non-owner viewers as a later slice (cost: per-frame scan plus PNG/JPEG masking, or a CSS overlay rendered by the SPA from `rects` in the stamp, cheaper but trust-based), or (2) restrict member viewing to the receipt/evidence view (already masked). Not decided here (question 4). |
| Receipts shown to members | Quarry (masked on read, S3) | Members read masked receipts only; legacy rows are masked on read, so no migration is required before sharing. |
| Telemetry that is ZDR-safe | D10 and D4 own it; this design only guarantees events/logs/errors are masked | `redaction_rule_version` and counts (never content) are safe to export; the stamp contains no lengths or values. Do not add the stamp's `by_rule` to a span attribute if org policy treats counts as sensitive (unlikely; confirm). |
| Secrets scoped to user or org, never widened by Space access (F-t2-024 E0/E5) | custody ADR | The Space session must not introduce a per-Space secret store. |

### (b) Model Plane and chat/agents

| Shared piece | Proposed owner | Reconcile |
|---|---|---|
| Golden `secret-shapes.v1.json` | Quarry (capture authority) owns the canonical file; the Model Plane vendors it (D12-9) | The Model Plane session must not edit the vendored copy; changes go through Quarry with a `RULE_VERSION` bump, then the sync script pins both. |
| `mp-scrub` crate and the three prompt/event call sites | this design (S4) proposes, the Model Plane session owns merge | Rebase against D1 (delegated bearer in `browser_agent.rs`/`quarry_agent.rs`) and D10 (event mapper); `MG/http_routes.rs` has in-flight hunks. |
| `EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP` flip to `1` | Model Plane session, after S2 is deployed | A refusal mode that fails agent runs when Quarry is an old image: schedule with the Quarry rollout. |
| F-t2-024 approval card, `fill_otp` in `allowedSensitiveActions`, `Login` risk gate | Model Plane (broker and orchestration approvals) | Gated on F-t2-001 (grants) and F-t2-013 (takeover) which the Model Plane session already plans; this design fixes only the contract name and the receipt boolean. |
| Chat transcript persistence of typed text | conversation-core / chat session | The model's proposed `type` value appears in the chat/agent transcript on the Model Plane side; `execution-core` scrubs persisted outputs by shape only. A password the **user** tells the model in chat is outside Quarry's reach: product must decide whether chat input needs its own field-semantic guard. Flag, not solved. |
| Stop criteria use raw text | Model Plane | Keep the scrub at the prompt boundary (S4 test `stop_criteria_still_match_the_raw_observation`). |

## 8. What can proceed WITHOUT an ADR/decision, and what is blocked

**Proceeds now (no ADR):** S0, S1 (after the Q10 recipe, which is a measurement, not a decision), S2 (staging in shadow first), S3 with the default receipt policy `sensitive` (reversible by flag), S4, S6. These are tightenings: they only mask, they add optional fields, and every rollback is "previous image".

**Needs a product decision, not an ADR:** the default receipt text policy `sensitive` versus `all` (question 2); URL masking in Live observations (question 1); legacy-row rewrite (question 3, S7); screenshot OOPIF policy and member-visible live frames (question 4). S5 can start with `allow` plus the stamp count, so it does not wait for question 4.

**Blocked by ADR:** F-t2-024 E3 and E5 (custody, audit 9.3 topic 2). E1 and E2 (fill_otp contract and approval card) are blocked by **other items** (F-t2-001, F-t2-013, F-t2-015), not by an ADR. No other part of this design is ADR-gated; D12-1 to D12-13 are engineering decisions with recommendations.

**Not gated but not started here:** F-t2-050 (PII), F-t2-020 (cookie/profile storage encryption).

## 9. Open questions for the user (only real blockers)

1. **Live URL masking.** The Live `BrowserObservation.url`, `ObservationReady.url` and `ProofBundle.source_url` stay raw in S2 except the event URL (masked), because masking the address-bar value would break reload and navigation; the Durable profile and the Model Plane prompt strip query tokens. Accept raw URLs in the returned observation, or require token stripping there too (which breaks magic-link and OAuth-callback flows in the UI)? Recommendation: accept raw in Live, strip everywhere durable.
2. **Receipt typed-text policy.** `sensitive` keeps benign typed text (search queries, form fields that are not classified) in receipts for audit and procedure compile; `all` masks every typed string and makes compiled procedures replay-useless (`RepairRequired` on every typed step). Recommendation: `sensitive` default, `all` per org later. Needs the product owner before S3 merges.
3. **Legacy data.** Mask on read only (default, S3), or also a destructive one-shot rewrite of old receipts (S7), which conflicts with append-only evidence? Recommendation: read-time masking only. Screenshots written before S5 cannot be re-masked: accept retention-based expiry?
4. **Who may see a live browser frame containing a typed secret?** The owner's frame stream is unmasked (a limit of S5). Should Space members get a masked stream (later slice), receipts only, or the same unmasked view as the owner with a visible "secret field" notice?
5. **Strict screenshot mode for cross-origin iframes** (`QUARRY_BROWSER_SCREENSHOT_OOPIF=mask`): default `allow` leaves payment/login fields inside out-of-process iframes visible in screenshots (counted in the stamp, not scannable). Is that acceptable for non-ZDR default deployments, or should the default be `mask`?

## 10. INTERFACE SUMMARY

**Goal.** F-t2-021: deterministic, versioned secret redaction at capture, before artifact write, event emission and the response to the Model Plane, also on ZDR. Epic outline for F-t2-024 (blocked).

**Files touched.** Ingestion Plane / Quarry: new `quarry-core/src/redaction/{mod,shapes,field,luhn,url,json,stamp,action}.rs`; `contracts.rs`, `error.rs`; `quarry-browser/src/{field_probe,sensitive_rects,screenshot_mask}.rs` (new), `lib.rs`, `chromiumoxide.rs`; `quarry-runtime/src/{observation,step_receipts,postgres_step_receipts,browser_procedure,action_runtime}.rs`; `quarry-edge/src/{agent_routes,main,redaction_boot}.rs`; docs `SECRET_REDACTION.md`, `openapi.yaml`. Model Plane: new crate `mp-scrub`; `execution-core/src/{scrub,llm_planner,quarry_agent,browser_agent,browser_events}.rs`; `model-gateway/src/http_routes.rs`. Repo: `scripts/tests/zdr-contract-sync.test.sh`, `.github/workflows/contract-goldens.yml`.

**New contracts.** `RedactionStamp` on `BrowserObservation.redaction`; `ProofBundle.redaction_rule_version`; `StepReceipt.redaction_rule_version` plus SQL column (`0012_step_receipt_redaction_version.sql`, tentative; optional `0013` for S7); golden `secret-shapes.v1.json` plus vendored copy; `RULE_VERSION = "qr-redact/1"`; marker `[redacted:<kind>]` (Quarry) and `[REDACTED]` (Model Plane). Flags: `QUARRY_EDGE__REDACTION_MODE` (enforce default; shadow/off dev only), `QUARRY_BROWSER_FIELD_PROBE`, `QUARRY_EDGE__RECEIPT_TEXT_MODE`, `QUARRY_BROWSER_SCREENSHOT_MASK`, `QUARRY_BROWSER_SCREENSHOT_OOPIF`, `EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP`.

**Effort.** S0 3.5 d, S1 4 d, S2 4 d, S3 4 d, S4 3.5 d, S6 1.5 d (text surfaces about 20.5 d, L); S5 7 d (L); S7 2 d optional. F-t2-024 E1 to E4 after its gates: roughly 20 to 30 d, XL until custody is decided.

**Dependencies.** Hard: S0 first; S1 -> S2 -> S3; S5 after S1 and S2; S4 after S0. Soft: D3 S3 (deadline wrapper), D2 S2 (route split) and S3 (migration 0010), D6/D8 (migration 0011, human receipt function). F-t2-024 hard-blocked on all of S0 to S5, F-t2-001, F-t2-013, F-t2-015.

**ADR gates.** None for F-t2-021. Custody ADR (audit 9.3 topic 2) gates only F-t2-024 E3/E5. Product decisions: receipt text policy, live URL masking, legacy rewrite, live-frame visibility for Space members.

**Parallelisable with.** D4 (ZDR boundary), D5, D7, D9 UI slices, D11, D13; S4 with S1-S3; S6 with anything. **Conflicts with.** D2 S2/S3 (`agent_routes`, `step_receipts`), D3 S3/S7/S10 (`chromiumoxide.rs`, `observation.rs`, `BrowserObservation` literals), D6 and D8 (migrations 0011, receipt builder), D10 (event payloads), sibling (b) in `MG/http_routes.rs` and `browser_agent.rs`.

**Honest limits.** Chromium's own AX masking unverified (recipe D12-6); OOPIF fields in screenshots and the owner live-frame stream are not masked; page text on scrape artifacts waits for F-t2-050.

<!-- COMPLETE -->
