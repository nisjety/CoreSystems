
## 5. Dependencies and ordering

### 5.1 Hard and soft dependencies

| This work | Depends on | Kind | Why and what happens if it slips |
|---|---|---|---|
| Slice 1 (session isolation) | none | none | Self-contained. Overlaps D6's "thread `org_id` into the acquire" (`browser_driver.rs` 160-163) and D3's slot design; slice 1 owns the one-line `org_id = hints.org_id` change so D6 can drop it. |
| Slice 2 (types) | D5 slice 4 (`quarry-core::scrape`, `ScrapeFormat`, `ResolvedScrapeOptions`, `FieldError`) | hard for the `ScrapeFormat` and `ScrapeOptions` edits; soft for `capture.rs` and `scrape_actions.rs` | If D5 slips, land the two new files and the `validate_*` functions first (no D5 symbol needed), wire them when D5 lands. |
| Slice 3 (driver capture) | none; D3 S2 (`resolve_viewport`) and S3 (deadlines) soft | soft | Rebases over `chromiumoxide.rs` 2392-2445 and 194-271; either order works, the second to land rebases. |
| Slice 4a | none | none | Quiet-window mechanical PR; announce 1 day. |
| Slice 4b | slices 2, 3, 4a; D5 slice 4 (handler, `ScrapeResponse`, `InlineParts`); D4 S-1 (`Admission`) soft | hard on D5 | Cannot ship a response shape D5 has not defined. If D5's handler lands first as designed, this is additive. |
| Slice 5 | slices 1, 2, 3, 4a, 4b; D2 S1 (Evaluate deny) soft (this slice carries the two-line fallback); D6 per-org fairness soft | hard on 1 and 4b | Actions must not run on a shared context (slice 1) and reuse the capture plumbing. |
| Slice 6 | slice 3 and 4b | hard | Extends `capture_screenshot`. |
| Slice 7 | slice 4b for the route and SPA capture rendering; slice 5 for the preview strip and timeout | hard on 4b | The gateway route and SPA panel need response fields to exist. D4 S-1 soft. |
| Slice 7b | slice 7 (`pdf` media kind) and sibling (b) agreeing the wire field | hard on both | Optional. |
| Slice 8 | slices 4b, 5, 6 | hard for contract text | Docs and OpenAPI describe shipped behaviour only. |
| F-t2-059 remote providers | this plan's capability flags and an egress boundary proof | follow-on | Browserbase, Browserless and Kernel stay `false` until then. |
| F-t2-002 / D2 | none from here | provides | The Evaluate refusal this plan relies on (A2). |
| F-t2-003 / D4 | none from here | receives | Slice 4b's matrix row 13 text and slice 7's cases for `Admission`. |

Critical path: D5 slice 4 -> 4b -> 5. Slices 1, 3 and 4a have no external prerequisite and can start immediately in W2; slices 2, 3 and 1 run in parallel.

### 5.2 Merge-conflict hotspots (files and ranges shared with other workstreams)

| File (provenance) | Range | Shared with | Resolution |
|---|---|---|---|
| `Q/quarry-edge/src/routes.rs` [M] `scrape` | 398-530 (handler), 81 and 303-305 (timeout layer) | D5 slice 4 (rewrites the whole handler), D7 (`ResolvedPolicy` gate), D4 S-4 (usage meter metadata at 491-509), D6 (limits), D2/D1 | D5 first; this plan adds three bounded insertions (gate/ensure_capable/plan_for call, the runner's `capture` hint and safe mode, response fields). **The timeout layer change is mine but D5 D17 caps `timeout_ms` at 55 s "below the 60 s `TimeoutLayer`"**: D5 must state the cap is 55 s for plain requests and 115 s when actions are enabled. |
| `Q/quarry-runtime/src/browser_driver.rs` [C] `do_fetch` | 131-297; constructors 82-105; lease block 144-170 | D3 (slot design, deadlines), D6 (org id and per-org permits at 138-142), D4 (ZDR leases), D8 (`tab_pin` on `BrowserSession`, struct-literal edit at 348), D5 (documents never reach a browser plan, 281-285 synthesized body) | Slice 1 first (it is small); D8's `tab_pin: None` literal edit is mechanical. Agree one owner for the lease block: slice 1 here. |
| `Q/quarry-runtime/src/pipeline.rs` [C] | 524-705 (page hash, artifacts, `NormalizedOutput`), 780-799, 846 | D5 (content-kind sniff, `InlineParts`, formats), D4 (ZDR logging at 821-829), D7, D3 | One new function `persist_captures` plus a two-field literal edit at 702-703; keep it self-contained to ease the rebase. |
| `Q/quarry-browser/src/chromiumoxide.rs` [C] | 194-271, 371-557 (D3 viewport and pages), 2392-2445 (screenshot and pdf), 2800-3060 (actions), 1873-1889 (capabilities) | D3 (S2, S3), D8 (tab pin, capabilities), D2 (evaluate), D9/D12 (UI-side) | Slice 3 touches 2392-2445, 2800-3040 (typed reasons) and 1873-1889; D3's `cdp_err` helper becomes the home for the reasons when it lands. |
| `Q/quarry-browser/src/lib.rs` [C] | 269-286 (`BrowserDriverCapabilities`), 409- (trait), 445-451 | D3 (`trusted_input`), D8 (`background_tabs`, `tab_pinning`, `per_*`) | All additive fields with `#[serde(default)]`; literals use `..Default::default()` except the chromiumoxide one (1873-1889) which three designs edit: land them in order and rebase. |
| `Q/quarry-runtime/src/driver.rs` [C] | `RenderHints` 70-, trait `Driver` | D5, D6, D8 | Additive fields with defaults. |
| `Q/quarry-runtime/src/action_runtime.rs` [C] | 62-110, 192-210, 235-268, 380-394 | D2 (Evaluate arm), D10 part 1 (event hygiene), D12 | The 380-394 and 192-210 edits are the same ones D2 and D10 want; whoever lands first, the others drop theirs (state it in each PR). |
| `GW/domains/browser.rs` [C] | 1463-1465 (comment), 3922-3938, 3427-3453 | D9 (frontend), D4 S-1, sibling (a) Space | Comment and content-type edits are small; keep `owned_browser_artifact_run` untouched. |
| `GW/domains/knowledge/quarry.rs` [C] | `scrape` 28-60, `scrape_preview` 62-203, helper 724 | D4 S-1, D5 slice 7 (gateway cleanup), D3, D6 | Rebase over D5 slice 7 which edits the same functions. |
| `SPA/shared/api/knowledge-client.ts`, `SPA/features/dashboard/home/KnowledgeScrapePreview.tsx` [C] | `ScrapeResult` 140-160, `scrapeUrl` 366; whole preview component | D5 slice 7 (SPA types), D9, sibling (a) | D5 slice 7 lands types first; this plan's `captures` fields extend them. |
| `Q/quarry-edge/src/{main,config,state,lib,test_support}.rs` and `tests/zdr_claim_floor.rs` [M] | QB-T17 hunks listed in section 2.2 | uncommitted QB-T17 work | No new `AppState` or `EdgeConfig` field here; sequence the shared test harness after its owner commits. |
| `QR/docs/openapi.yaml` [M], `QR/docs/*.md` [M] | `/v1/scrape` 46-95, `ScrapeRequest` about 4331 | D5 slice 8 (OpenAPI from types), D3 (`openapi.yaml` 2432-2458) | D5 slice 8 generates; this plan's slice 8 supplies the schemas it must include. |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

**Plane ownership.** Everything that executes in a page, captures, validates or stores lives in the Ingestion Plane (Quarry v2: `quarry-core` types, `quarry-browser` driver, `quarry-runtime` pipeline and adapter, `quarry-edge` route). The frontend plane owns only the same-origin gateway route and the UI. No new database, no cross-plane table access, no new Control Plane or Model Plane state: Control Plane identity and entitlements are consumed through the existing claims, and `plane-service-principals.json` is deliberately not edited. Artifacts are Ingestion-owned objects in Quarry's artifact store; the gateway reads them only through Quarry's HTTP surface with the user's org token. **At risk: none.** One thing to flag, not a violation: sibling (b)'s Model Plane has its own `Evaluate` and `browser.act` vocabulary; this plan's wire enum is separate on purpose (D4).

**Model proposes, Quarry executes or rejects.** Quarry validates every field, caps counts, waits, bytes and viewport, refuses the denied actions and runs the list itself; no raw script ever runs (D8). The deviation to state plainly: the card title says "grant-gated", but `/v1/scrape` carries no BrowserBroker grant and no run, so a broker grant cannot gate it. The gate that exists is flag plus principal kind plus the new `scrape:interact` scope (D9). **At risk (needs the ADR 6 ruling):** a Model-originated action list. It is closed by construction (no principal holds `scrape:interact`, model-gateway's `quarry.rs` builds scrape bodies without actions and its comment already warns against `signals.screenshot`); opening it later is an owner decision plus a grant story that belongs to sibling (b).

**Zero Data Retention.** Statement per boundary, all of which hold only if the tests in slices 4b, 5 and 7 pass:
- *Artifacts:* non-ZDR only, through `zdr::guard(.., WriteKind::Artifact)` in `persist_captures` (and the defensive guard added to `ActionRuntime`'s two `store.put` arms). ZDR (including the floor raised by the user token's org posture) returns bytes inline and writes nothing.
- *Cache:* any request with `actions`, a capture format or a viewport is not cache-admissible, on both the Quarry page cache and the gateway preview cache (D15, slice 7 strip); capture refs therefore never exist in a cached `NormalizedOutput`.
- *Events, logs, spans, receipts:* captures emit only a content-free `ArtifactWritten` (non-ZDR); `write.text` is never logged, echoed or placed in errors; the action event path of `ActionRuntime` is not used in attach mode; usage-meter metadata carries counts only. The existing meter fields `url` and `fingerprint` (`routes.rs` 491-509) are D4 S-4's defect and are not widened here.
- *Scrape record (D5 slice 6):* stores only the options echo, never bytes, never `text`.
- *Durable boundaries:* `scrape_stream`, `internal_run_page`, crawl, batch and schedules reject actions and capture formats, so inline bytes cannot ride Temporal history or a schedule record.
- *Safe mode:* actions disable ingest, the local index write-back, the visual-RAG renderer, source registration and the Model title hop (D9 (4)), so an action-mutated page is never persisted as evidence of its URL.
- *Gateway:* `Cache-Control: private, no-store` on artifact responses; ZDR orgs get `withheld`.
- **Rule at risk: none, with one honest limit.** The capture inline cap (6 MiB) means a large ZDR screenshot is omitted rather than returned (D14); this loses data, not privacy.

**Other rules.** *Provider actions through integration-corev2:* not involved. *No independent embeddings:* not involved; the visual-RAG renderer's PNG is not reused. *Tenant isolation:* artifacts are stamped with the claim org (`pipeline.rs` 596-600) and read through the org-bound `get_artifact`; the gateway route never trusts a client-supplied org. *Evidence captures secrets (risk 4 in the audit):* screenshots and PDFs of authenticated pages stay inside the org, can never be written under ZDR, and credential entry is refused heuristically (D17, ADR 2 soft); a scrape session starts with no cookies, so authenticated pages are only reachable if the caller's actions log in, which D17 blocks for password fields.

## 7. Cross-session interfaces

**(a) Verevon Space (shared workspaces, agent PC/browser in Spaces, ZDR-safe telemetry).**

| Shared piece | Proposed owner | What the user must reconcile |
|---|---|---|
| Scrape artifacts shown inside a Space | This plan owns producing and serving them (org-bound route); Space owns where they appear and who inside the org sees them | Slice 7 serves any org member who knows the id. If a Space needs per-user or per-Space visibility, that is D18 option (b) plus Space grants, and it belongs to the run-sharing taxonomy (ADR 8). Decide before the Space UI embeds scrape artifacts. |
| `KnowledgeScrapePreview.tsx`, `knowledge-client.ts`, `browser-session.ts` | Shared files: this plan adds the capture panel and the `pdf` media kind; Space edits layout and routes | Sequence by file, not by feature; section 5.2. |
| ZDR-safe telemetry for captures | Space/telemetry session owns the trace envelope (F-t2-014); this plan promises counts-only metadata | Capture metadata (`{format, bytes, width, height, tiles}`) is safe to put in a trace; bytes and URLs are not. Keep the attribute names from `capture_meter_metadata`. |
| Agent PC/browser inside a Space | Space owns the surface; Quarry owns execution | A Space browser session uses `/v1/agent/*` and the broker, not scrape actions. Do not route Space browsing through scrape `actions`. |

**(b) Model Plane + chat/agents (BrowserBroker grants, agent browser path, delegated bearer, approvals).**

| Shared piece | Proposed owner | What the user must reconcile |
|---|---|---|
| Model-originated scrape actions | Model Plane would own the proposal; Quarry owns validation and execution (this plan) | Closed today. Opening it needs the ADR 6 ruling, a principal with `scrape:interact` in `plane-service-principals.json` (Control Plane), and an approval story (F-t2-033) for form-submitting clicks. This plan does not decide it. |
| Evaluate | D2/F-t2-002 owns the deny; any evaluator is a new action variant under ADR 6 | Confirm one vocabulary: this plan's wire enum has no `evaluate`; Model Plane's `quarry_agent.rs` contract does. They must not merge. |
| `screenshot`/`pdf` on the agent path | Quarry (existing `observation.rs` arms) | Slice 7b needs an additive field through execution-core's step projection; sibling (b) must agree the field name (`artifact_ids.pdf`). |
| model-gateway's `BROWSER_SCREENSHOT_MAX_BYTES` (6 MiB) and its comment on `signals.screenshot` | model-gateway (sibling b) | The capture inline cap deliberately equals it. If model-gateway later consumes scrape screenshots as image input, it needs the JPEG form and the scrape response fields; today it ignores them. |
| Delegated bearer forwarding | sibling (b) / D1 | Scrape calls from the gateway use the user's org token; nothing here changes how the bearer is forwarded. |

## 8. What can proceed WITHOUT an ADR/decision, and what is blocked

**Proceeds now (no ADR, flags off by default):** slice 1, 2, 3, 4a, 4b (formats), the plain-user half of slice 5 (`QUARRY_EDGE__SCRAPE_ACTIONS=user` behind the flag, no principal granted the scope), slice 6, slice 7, slice 8. D12's report-only fidelity behaviour, D14's inline-only ZDR default and D17's heuristic are proceed-with-default choices (DECISION level), listed in section 9 only for confirmation.

| Blocked item | Decision | Unblocked by |
|---|---|---|
| A Model-originated or service-principal action list (`scrape:interact` granted to any principal) | ADR topic 6 (script execution model) | owner ruling; recommendation in the audit is declarative capped lists, which this contract already is |
| A read-only evaluator action | ADR topic 6 and F-t2-002 | a future new variant, not this plan |
| Third-party subresource allow-list for faithful screenshots | ADR topic 1 (web policy) | D7's `BrowserEgressPolicy` extension |
| Treating inline-only ZDR capture as the permanent ZDR semantics | ADR topic 4 (soft: the default proceeds, a different ruling changes only `persist_captures`' ZDR branch) | the ZDR matrix owner |
| A structural (non-heuristic) credential-entry check, and password references | ADR topic 2 (soft) | secret-custody ADR and F-t2-024 |
| Actions or captures on Browserbase, Browserless or Kernel | ADR topic 10 and F-t2-059 | an egress boundary for remote browsers |
| Per-user artifact privacy inside an org | ADR topic 8 (resource taxonomy) | Space sharing design |

## 9. Open questions for the user (only real blockers)

1. **Who may send `actions`?** The default here is: user principals when the flag is `user`, no service principal until it is granted `scrape:interact`. Confirm that is the intended first release, and say whether the Model must ever send action lists (ADR 6). Nothing in slices 1 to 4 and 6 to 8 waits on the answer; slice 5 ships dark either way.
2. **Is Chromiumoxide-only acceptable for this feature?** Browserbase is the one registered remote lane (`main.rs` 316-328) and fails closed for every scrape today (finding B, `[INF]`, needs one live run to confirm); Browserless and Kernel are unwired. Confirm that scrape actions and captures are local-driver-only until F-t2-059, and that a 501 is the right answer elsewhere.
3. **Heavy lane size and time budget.** One action request at a time per process (`QUARRY_SCRAPE_HEAVY_PERMITS=1`) and a 125 s router deadline for action scrapes. Confirm that a 125 s HTTP request is acceptable through any front proxy in the target environments (`[UNVERIFIED]`: proxy read timeouts); otherwise the cap falls to about 20 s of total wait.
4. **Is a faithful-looking screenshot required, or is host-only egress fidelity acceptable for v1?** A page whose CSS or images are on another domain will look unstyled (finding E, decision D12). If product wants faithful renders, that is a policy decision (ADR 1) that adds a subresource allow-list; it is not a code-only change.

## 10. INTERFACE SUMMARY

**Items.** F-t2-023 only (WS17, W2). L, about 33 engineer-days in 9 slices, two shippable halves (formats, then actions) plus stitching, UI, docs.

**Files touched.** *quarry-core:* `capture.rs`, `scrape_actions.rs` [NEW]; `scrape.rs` (D5), `artifact.rs`. *quarry-browser:* `lib.rs`, `chromiumoxide.rs`, `stitch.rs` [NEW]. *quarry-runtime:* `browser_driver.rs`, `action_runtime.rs`, `driver.rs`, `fetch.rs`, `pipeline.rs`, `artifact_store.rs`, `capture.rs` [NEW]. *quarry-edge:* `scrape_actions.rs` [NEW], `routes.rs` [M], `auth.rs`. *Docs/config:* `openapi.yaml` [M], parity and driver-matrix docs, compose files, `e2e-scrape-capture.sh` [NEW]. *Frontend:* gateway `knowledge/quarry.rs`, `knowledge.rs`, `browser.rs`; SPA `knowledge-client.ts`, `action-registry.ts`, `KnowledgeScrapePreview.tsx`, `browser-session.ts`, `errors.ts` [M]. Model and Control Plane: none (7b: one additive field).

**New contracts.** Request `formats[]` capture objects, `actions[]`, `actionsOnError`. Response `formats.screenshot|pdf`, inline `screenshot|pdf`, `captures`, ordered `actions[]`, warnings, error codes (`feature_disabled`, `action_denied`, `action_failed`, `heavy_browser_busy`, `browser_capability_unavailable`, `actions_deadline`). Gateway `GET /api/v1/knowledge/scrape-artifacts/{id}`. Artifact kind `screenshot_jpeg`. Scope `scrape:interact` (granted to nobody). Flags `QUARRY_EDGE__SCRAPE_CAPTURE|ACTIONS|STITCH`, `QUARRY_SCRAPE_SESSION_ISOLATION`, `QUARRY_SCRAPE_HEAVY_PERMITS`. No SQL, proto or migration.

**Dependencies.** Hard: D5 slice 4 for 4b and 5. Soft: D2 S1, D3 S2/S3, D4 S-1, D6 fairness. Driver slices need a live Chromium.

**Effort.** 1: S 2d. 2: M 3d. 3: M 4d. 4a: S 1d. 4b: M 5d. 5: L 7d. 6: M-L 5d. 7: M 4d. 8: S-M 2d. Optional 7b: 2d.

**ADR gates.** 6 (Model-facing actions; blocks only opening them), 1 (fidelity), 4 and 2 (soft defaults), 10 (remote providers).

**Beyond the card.** Concurrent same-host scrapes share one CDP context (fixed in slice 1); Browserbase scrape fails closed; agent PDFs discarded; `ActionRuntime` ZDR and event traps; host-only egress degrades fidelity; constant `final_url`; filesystem artifact key aliasing; gateway cannot serve PDFs; a 60 s wait is unreachable today.

**Parallel with:** D2, D4, D6, D8, D12; D3 after slice 1; slices 1, 3, 4a start now. **Conflicts with:** D5 (`routes.rs`, `pipeline.rs`, `knowledge/quarry.rs`), D3 and D8 (`chromiumoxide.rs`, capability struct, `browser_driver.rs`), D9 and sibling (a) (`KnowledgeScrapePreview.tsx`, `browser.rs`).

<!-- COMPLETE -->
