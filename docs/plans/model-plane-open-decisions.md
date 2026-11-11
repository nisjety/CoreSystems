# Model Plane: open decisions (code-completion phase, 2026-11-11)

This file lists every decision that blocks or shapes Model Plane code in waves 1 to 5 and is not settled by the
owner. Each entry names the question, the options, the recommended option (best practice, sovereignty first) and
what happens to the code.

There are two classes:

- **Assumed, owner to confirm.** The recommended option stays inside the standing rules: no frozen contract changes,
  no ownership rule is reversed, and no user content is kept without a ZDR clause. The code is built dark behind a
  default-off switch, so nothing changes until the owner confirms and the switch is turned on. If the owner chooses
  another option, the switch stays off and the unit is reworked.
- **Owner must decide; not built.** The recommended option needs something only the owner can give: a change to a
  frozen contract or an ownership rule, a Channel Plane runtime, real money, credentials, an external attestation or
  legal wording. Nothing is built until the owner rules.

Standing rules referred to below: ZDR is a monotonic floor and a waived run counts as ZDR for new stores (DEC-32);
the owning plane enforces and fails closed with typed refusals (ADR-0005 pattern); audit rows carry no content; keys
stay in integration-corev2 custody; the Channel Plane is docs-only (DEC-40); `docs/actions-surface-operations.md`
is frozen.

The wave 3 decisions taken at their recommended option (105 rows) are in `docs/plans/wave3/OWNER-DECISIONS.md` and
are not repeated here. Decision rows in `docs/plans/model-plane-depth-register.md` (kind `decision`) are triaged in
step F of the handoff; the ones that block code are copied here as they are reached.

Unit ids name the build units of the code-completion programme (branch `mp-w345`); each unit's result is in
`docs/plans/wave3-5-code-completion-2026-11-10.md`. Nothing listed here is tested or live-proven yet.

## 1. Assumed, owner to confirm (built dark)

| ID | Item | Question | Recommended and built | Basis | Units |
|---|---|---|---|---|---|
| OD-01 | P0 D2-698 run ceiling | May a request cap loosen the org's per-run ceiling? | No. The effective cap is the smaller of the request cap and the org ceiling. Switch `MODEL_GATEWAY_CAP_MIN`. Owner-visible: a request that asked for more than the org allows now gets the org cap. | budget.rs's own contract says a caller may only tighten; the org decides (rule 6). | P0-D2-698 |
| OD-02 | P0 D2-460 graph grounding in a Space | Should a Space-scoped turn get org-wide graph entities? | No. The turn sends its Space decision; the Data Plane refuses it on `/v1/retrieve/graph` today (403, no Space predicate yet), and the gateway treats that as "no graph grounding in a Space". When the Data Plane adds the predicate, scoped graph grounding starts with no Model Plane change. Switch `MODEL_GATEWAY_GRAPH_SPACE_SCOPE`. | Fail closed; the Data Plane owns retrieval (rule 11). | P0-D2-460 |
| OD-03 | P0 D2-461 follow-up turns in a Space | A follow-up turn names a thread but no Space: refuse or resolve? | Resolve the thread's Space on the server (the BFF already has the lookup) and mint the scoped decision; refuse with a typed error when the Space cannot be read. Never fall back to org-wide. Switch `SPACE_THREAD_SCOPE_RESOLVE` (BFF AppState). | Fail closed; Control signs the decision. | P0-D2-461 |
| OD-04 | P0 D2-480/481 memory content policy | How do capability-core and orchestrator-core memory writes pass the content policy? | A new additive session-core RPC `CheckMemoryWrite` runs the one policy; capability-core and orchestrator-core ask it and fail closed (503) when it is unreachable. Erasure is never gated. Switches `MEMORY_GATE_CAPABILITY_WRITES`, `MEMORY_GATE_ORCHESTRATOR_WRITES`. | ADR-0005: the owner of the policy enforces, others ask. | P0-D2-480, P0-D2-481 |
| OD-05 | P0 D2-1149/1178, D2-1153 waived approvals | Does a waived run write approval cause and decision rows? | No. Approval create and decide refuse a waived caller before any write, with the catalogue refusal. Switch `APPROVAL_WAIVER_REFUSE`. | DEC-32. | P0-D2-1149 |
| OD-06 | P0 D2-769 list tool in Space runs | Refuse or scope `knowledge_list_documents` in a Space-scoped Do run? | Refuse with a typed error until a Space-scoped listing exists. Switch `OWNER_LIST_SPACE_REFUSE`. The same refusal in the gateway tool loop rides `AGENTIC_SPACE_RETRIEVAL_ENABLED` (W2-WS18-S5-b). | Fail closed. | P0-D2-769, W2-WS18-S5-b |
| OD-07 | P0 D2-509 org skills by members | May a member create an org-wide skill directly in session-core? | No: org scope needs the org admin role from the verified identity. Switch `SKILL_ORG_ADMIN_CHECK`. | Matches capability-core's existing rule. | P0-D2-509 |
| OD-08 | P0 D3-956 org skill edit | May any member PATCH or DELETE an org skill in capability-core? | No: owner or org admin only. Switch `SKILL_ORG_OWNER_CHECK`. | The model and members may not widen what the org set. | P0-D3-956 |
| OD-09 | W5-071-b coordinator resume (DEC-GAP-01) | After a browser pause, does a coordinator keep the narrowed tool offer? | Keep the complete offer in the pause snapshot and recompute the narrowing on resume; an old snapshot keeps the stored narrowed offer. | Removing the browser family from coordinators would drop a capability the browser flow uses. | GAP-W5-071-b-coordinator-resume |
| OD-10 | W4-CC-8 custom REST connection route (DEC-GAP-03) | Can the integration-corev2 route that creates a custom_rest connection be built while the frozen actions-surface doc's custom_rest section awaits review? | Yes: it is HTTP connection creation, not an action on the actions surface. Built without touching the frozen doc. | CLAUDE.md: only the operation/capability contract is frozen. | GAP-W4-CC-8-custom-rest-route |
| OD-11 | W2 WS17-S9 message versions (DEC-W2-WS17-S9-b) | The gateway and session-core each have a versions switch. Derive one from the other? | Keep two switches with an operations check that they are equal; the stream now tells the SPA `versions_enabled`. | Deriving would change an existing contract. | W2-WS17-S9-a, W2-WS17-S9-b |
| OD-12 | W2 WS17-S9 rewind under ZDR (D2-577, D2-580; R2-DEC-OD-12) | Do thread-bound rewind and SetActiveLeaf need rows in `zdr-posture-matrix.json`? | Yes: two enforcement rows with distinct inputs in the existing feature-row pattern (no new table, no format change), whose notes name the refusing code sites; a waived SetActiveLeaf caller is refused. The matrix is authored and pinned, not frozen; FUGEN re-pins and re-vendors it. Supersedes DEC-W2-WS17-S9-c. | ZDR.md: every durable mutation has a matrix row; DEC-32. | R2-OD-12-a-versions-refusals, R2-OD-12-b-matrix-rows |
| OD-13 | W2 WS18-S5 Do knowledge receipt (DEC-W2-WS18-S5-b) | What does the Do run's knowledge receipt read? | The same Space decision the search uses; mode and count only. | No content in frames. | W2-WS18-S5-a |
| OD-14 | W2 WS23-S3 egress hold cause (D-WS23-S3-a) | New cause code `tainted_egress`, or keep `risky_effect_under_ask`? | New code in the closed vocabulary, so the person sees the right gate; old readers degrade unknown codes to `unclassified_gate`. | Approvals carry closed cause codes (rule 6). | W2-WS23-S3-a |
| OD-15 | W2 WS23-S4 connector scope on the run (D-WS23-S4-a) | Record the selected connector ids on the run record? | Yes, ids only, never under ZDR or a waiver, erased with the run. Switch `CONNECTOR_SCOPE_RECORD_ENABLED`. | Provenance without content. | W2-WS23-S4-a |
| OD-16 | W2 WS26-S2 content-safety normalisation (D-WS26-S2-a) | `CONTENT_SAFETY_NORMALIZE` is on in code; the rule says new switches default off. | Register it at its existing default (on). It is not new behaviour, and off would loosen the scan. | Tighten, never loosen. | W2-WS26-S2-a |
| OD-17 | W2 WS14-S6 ask_user in Ask turns (D-1) | Which Ask turns may get `ask_user`? | Non-ZDR, non-waived turns that are not Space-scoped and not plan turns. Switch `MODEL_GATEWAY_ASK_USER_ASK_MODE_ENABLED`. | DEC-32; matches the Do path. | W2-WS14-S6 |
| OD-18 | ZDR Z-17 thread receipt (D-ZDR-04) | How do existing threads read without a receipt? | NULL reads as persistent; no backfill, because a backfill would assert past retention without evidence. Switch `SESSION_THREAD_RECEIPT_ENFORCE`. | Evidence, not assumption. | ZDR-Z-17-thread-retention-receipt |
| OD-19 | Q-ERASE request snapshots (D-ZDR-06) | On user erasure, which snapshot blobs go? | The user's snapshot rows; a shared blob only when no snapshot references it any more, in the same transaction. | Blobs are shared across runs and users. | ZDR-Q-ERASE-request-snapshots |
| OD-20 | DEF-Z-06b ZDR spend in the ledger | Should ZDR turns be checked against budgets and ledgered? | Yes: the budget read runs before the provider call, and one content-free ledger row (ids, counts, amount) is written after. Switch `MODEL_GATEWAY_ZDR_LEDGER_ROWS`. Turning it on is the owner's call under DEC-18(d). | A ZDR turn must not exceed a ceiling it was never checked against; counts are not content. | ZDR-DEF-Z-06b-ledger-zdr-spend |
| OD-21 | Z-05a silent ZDR drops | Web search and file output are dropped silently under ZDR. | Typed `feature_not_supported` refusals from the catalogue, before any call. Switch `ZDR_TYPED_REFUSALS_ENABLED`. | Refusals are typed and visible (rule 2). | ZDR-Z-05a-typed-refusals, ZDR-Z-05b-spa-refusal-copy |
| OD-22 | FU-13 snapshot grants on org purge | Who revokes Control view grants when an org is purged? | user-core deletes the org's resource grants in its own org purge. A service never acts as the owner to revoke them. | Each plane erases its own data; Control owns grants. | FU-22-control-org-purge-grants, FU-21-thread-delete-delegated-credential |
| OD-23 | W5-095 priority tier pricing | Where is the priority-tier multiplier applied for streamed answers? | In one place only, from the tier the provider reports on the stream; displayed and ledgered cost agree. | Money paths have one source of truth. | FU-24-stream-priority-tier-pricing |
| OD-24 | Goal judge principal (W3-EP-7-S3/S6; R2-DEC-OD-58-a, R2-DEC-OD-58-b) | Which principal records goal verdicts, and what do verdict rows keep? | A dedicated persistent principal `model-gateway-goal-judge` on session-core with `session:goal-judge:read` and `session:goal-verdict:write` only; ZDR and waived threads are refused before any read. Verdict rows are content-free by default: `verdict_reasons` and `evidence_ids` are written as NULL (verdict, judge model and token counts kept). Owner alternative: keep reasons on persistent threads only. Control parity checks run in the verification phase. | Narrowest scope (PRINCIPAL-TRAIN rule 4); audit rows carry no content; DEC-32. | R2-OD-58-a-judge-principal, R2-OD-58-b-session-judge-gate, R2-OD-58-c-gateway-judge-credential |
| OD-25 | GDPR: thread-bound memory rows (R2-DEC-OD-59) | What does a user-subject erasure do to memory bound to threads? | Delete rows bound to the person's own threads; on Space threads apply the EP-8 S7 attribution rule (never delete other people's content); user_anonymize keeps Space rows; a legal hold on any affected thread refuses the whole event before any store. Switch `SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE`. | Erasure is complete for what the person owns; holds win. | R2-OD-59-a1-switch-attribution-helper, R2-OD-59-a2-thread-memory-erasure, R2-OD-59-a3-zdr-doc-row |
| OD-26 | W5-072 resilient routing, platform-only part (R2-DEC-OD-40-a) | What may inference-core do when a platform provider fails? | A closed failure vocabulary; per-replica benching of (provider, model), 30 s doubling to 300 s, half-open recovery; server, timeout and empty answers bench only after 3 consecutive failures from at least 2 orgs (one org cannot bench a provider for everyone); `providers_benched` (UNAVAILABLE plus retry-after) when every eligible provider is benched; a same-tier fallback behind its own switch that never leaves the first hop's privacy tier and never reaches a user route. Switches `INFERENCE_PROVIDER_BENCHING`, `INFERENCE_SAME_TIER_FALLBACK`. | Ownership-matrix rule 17 kept; sovereignty tier never loosened. | R2-OD-40-a-failure-classes-bench, R2-OD-40-b1-switches-guard-anchor, R2-OD-40-b2-providers-benched-status, R2-OD-40-b3-chain-benching, R2-OD-40-c-dataplane-status-cases |

The round-2 design scopes (WS13-S7/S6 "Always" chain and grants, WS14-S5 input pause, the zdr2 waiver issuer
chain, and r2) carry their own decision ids; they are in section 3 with their classes and units.

## 2. Owner must decide; not built

| ID | Item | Question | Options | Recommended | Why it is not built |
|---|---|---|---|---|---|
| OD-40 | W5-072 resilient routing, cross-route part (R2-DEC-OD-40-b) | May a turn fall back from a platform model to the user's subscription or BYO route (or back)? | (a) reverse ownership-matrix rule 17 for fallbacks; (b) no cross-route fallback | (b). The platform-only parts are built dark (OD-26). | Rule 17 reversal. |
| OD-41 | W5-082 voice input | Which speech-to-text provider, and how is ZDR met? | (a) an EU streaming STT provider with a ZDR attestation; (b) local in-browser STT for ZDR orgs; (c) wait for demand | (c), then (a) for non-ZDR with (b) as the ZDR route | Provider choice, money and an external attestation. |
| OD-42 | W5-084 per-Space environment specs, W5-085 sandbox extensions | Build without a concrete user need? | (a) build; (b) wait for demand (plan rule, audit 10.3) | (b). 085 also needs a billing decision. | Plan rule: not scheduled without demand; billing. |
| OD-43 | W5-088 code edit quality | Build post-write diagnostics (H11) and apply_patch? | (a) build; (b) keep H11 in wave 6 (OD18 default skip) | (b) | Owner decision OD18 on record. |
| OD-44 | W5-089 outward MCP endpoint and channel adapters, F-t3-064 channel adapters and CLI | Build? | Only after a Channel Plane ADR and a Control Plane OAuth authorization server | Write the Channel Plane ADR first | DEC-40: the Channel Plane is docs-only. |
| OD-45 | W4 desktop backend (F-t3-024 C2+) and desktop viewer | Which sandbox desktop backend? | WS22-S8 microVM spike and ADR-0009 (provider, cost, guest image owner, metering) | Run the spike when a second attested executor host exists (DEC-22) | DEC-22; money and provider. |
| OD-46 | W4 continuous walkthrough clip (F-t3-050) | Record continuous browser video? | Needs a Quarry recording plus a retention and redaction policy | Keep the screenshot filmstrip (built); decide the policy first | Retention policy for user content. |
| OD-47 | W4 public links, cross-org sharing, live shared threads, fork from a snapshot (F-t3-046) | Widen sharing beyond org-only view snapshots? | A new share-model ADR | Keep org-only (share-model ADR) | ADR on record forbids. |
| OD-48 | W4 C2PA signing, image edit and mark tool (F-t3-051) | Sign generated images? | Needs a Control Plane signing certificate and the binary-artifact-home decision | Decide the binary artifact home first (also unblocks the Do-loop `generate_image`) | External certificate; retention decision. |
| OD-49 | W4 API-started Do and browser runs, approvals over the API, mail-task pipeline (F-t3-064) | Run unattended work in a user's identity? | An "unattended approval contract" design | Design it under DEC-02 (no stored or renewable user bearer) and DEC-16 | Authority design on DEC-02/DEC-16. |
| OD-50 | W4 cloud-role BYO (F-t3-065 phase 3) | Accept cloud-role credentials as BYO? | Per-cloud attestation | Keep last, as the audit says | External attestation (DEC-40). |
| OD-51 | W4 mail-received triggers, Space triggers, blueprints (F-t3-047) | Add trigger sources beyond webhooks? | Ingestion change first; Control's shared-Space decision family | Wait for those | Another plane's contract; Control decision. |
| OD-52 | W4-CC-4 seal evidence | integration-corev2 seals with ZDR and waiver flags from the key-holding principal. Should it verify a Control-signed decision? | (a) verify the Auth Core retention decision (the same signer as the waiver issuer); (b) keep trusting the principal | (a), after the waiver issuer exists | Needs the decision format and key (ADR sections 2 and 7). |
| OD-53 | W4-CC-4 point O4 | An unauthenticated credential reference gets 403 and no audit row. | (a) a content-free audit row in an "unauthenticated" bucket; (b) a rate-limited log only | (b): an audit row needs an org to belong to | Audit policy. |
| OD-54 | W4-CC-6 frozen actions-surface doc | The custom_rest section added to `docs/actions-surface-operations.md` needs owner review. | Accept or revert the section | Review it; the route (OD-10) does not depend on it | Frozen contract. |
| OD-55 | W4-R2-b timeline action id (DEC-GAP-02) | Quarry timeline entries have no action id, so gateway steps cannot join their approvals. | (a) Quarry adds an optional `action_id` and the Model Plane reads it; (b) leave unjoined | (a) as a contract request to Quarry; the Model Plane reader follows | Quarry owns the contract. |
| OD-56 | EP-13-S8 Gemini prices and funded run | Which prices, and who funds the live run? | Sourced vendor prices (no invented prices); owner-funded run | Owner supplies prices and budget | Money. |
| OD-57 | EP-6-P3 unattended named-agent runs | How does a scheduled run read a Convex agent definition? | (a) snapshot the definition into the schedule at creation; (b) a service path from the scheduled lane to Convex | (a): no new cross-plane read at fire time, and the snapshot is erased with the schedule | No design on record; it stores instructions in a new place. |
| OD-58 | Goal judge principal | Moved to section 1 as OD-24 (built dark in round 2). | - | - | - |
| OD-59 | GDPR: thread-bound memory rows | Moved to section 1 as OD-25 (built dark in round 2). | - | - | - |
| OD-60 | L12/L33 embeddings and background indexing (D-ZDR-05) | Should the Data Plane apply the org's ZDR setting to indexing, entity extraction and summaries? | A Data Plane contract field and an indexing change | Yes, as a contract request to the Data Plane owner | The Data Plane owns embedding. The Model Plane already sends `zdr_mode` (the org floor OR the request) and `sovereign_required` on every retrieval. |
| OD-61 | D-ZDR-08 turning the waiver issuer on | When may `ZDR_WAIVER_ENABLED` go on? | After the four enforcing services report the waiver contract version (launcher check) | As ZDR.md 5.7 says | Rollout order. |
| OD-62 | D-ZDR-09 waiver and trust wording | Who approves the acknowledgement and trust-page wording? | Counsel | Ship copy keys marked draft | Legal sign-off (OD-LEGAL). |
| OD-63 | W2 deferred slices: WS15-S8 (lifecycle notes), WS15-S9 (connector incident ring), WS19-S10 (Do file delivery), WS20-S9 (owner-bound reads), WS21-S9 (browser login hand-off), WS22-S7/S8 (SandboxBackend, microVM), WS23-S9 (calendar writes), WS27-S11 (cost bound on the plan card), WS08-S8 (durable reference uploads) | Build now? | See the deferred inventory (units-draft/deferred/_summary.md, D-2..D-11) | Keep deferred: each needs a store decision, a frozen-contract amendment, a credential change, redaction in Quarry, an ADR, or an external agreement | One of the owner-only reasons per row. |
| OD-64 | W2 rollout and measurement decisions | AGENTIC_SPACE_RETRIEVAL_ENABLED rollout order (D2-773), Space decision TTL (D2-772), migration 0061 index build (D2-576), workspace write durability (D2-613) | Deploy order; live measurement; out-of-band CONCURRENTLY index build; overlay-only durability | Execution-core first; measure on a live Do run; out-of-band index build | Operational decisions for the flip, not code. |
| OD-65 | W5 other not-built parts | 070 code-mode bridge and WebMCP, 073 reasoning replay envelope, 074 allow rules and CEL, 075 telemetry export, 076 review gate and PromoteWorkspace caller, 083 paused_for_credit, 086 review metering, 087 per-run preview origin, 090 meeting pull and video, 091 dashboards, 093 delegated ingest credential, 094 activity-to-page, 095 prewarm and Responses continuation, 096 product guide, 097 permission-path reviewer and information-flow control, 098 calm tips | Each has its reason in the W5 inventory (forbidden by rule 12, no design, frozen contract, demand trigger, missing event source) | Keep as recorded | See `C:/dev/mp-w345-plan/inventory.json` not_gaps (copied into the coding-complete report). |
| OD-66 | W5-093-c data-source zdr_allowed values, W5-098-a accent contrast | Confirm the values used | Confirm or adjust | Confirm | Product decisions; the code reads them as data. |

## 3. Decisions from the round-2 design scopes (2026-11-11)

Each scope was designed, adversarially reviewed and revised before its units were adopted. The entries are copied
from the scope summaries as written; "assumed, owner to confirm" entries are built dark, the others are not built.

### 3.1 WS13-S7 "Always" chain and the WS13-S6 grant surface

| Id | Question | Options | Recommended and why | Class | Units |
|---|---|---|---|---|---|
| D-WS13-1 (D2-1126) | May a ZDR or waived run use a standing "Always" rule? | (a) a person's (user-scope) allow is never applied to a retention-bound or unstated run, no row; org-scope allows apply as today and write no row for such a run; (b) apply the rule without an audit row; (c) apply it with a content-free row | (a). K3-10's 'every rule use is audited' is read as every use on a persistent run: the Approvals v2 ledger entry (2026-10-21, point 6) and DEC-32 forbid an audit row for a ZDR or waived run, so a person's rule, whose use there could not be audited, never relaxes such a run; it mirrors StandingCall::grantable for grants. Org allows are the org's own WS05-S5 policy and keep applying to a ZDR or waived run with no row. An explicit retention_bound always tightens; an unstated posture counts as bound only while TOOL_POLICY_RULE_AUDIT_ENABLED is on (review findings 7, 20). | assumed, owner to confirm (incl. the reading of K3-10) | rule-use-a, rule-use-b, zdr-row (ZDR.md row) |
| D-WS13-2 (D2-1127) | Who performs the confirm write? | (a) model-gateway runs a saga from the server-held proposal: session-core claim (state 'confirming') -> capability-core PUT with the person's capability bearer -> session-core resolve, a dismiss refusing a claim younger than 60 s; (b) the SPA PUTs capability-core directly (today's confirmApprovalRule) and separately marks the proposal | (a). The capability id and digest come from the server record, not the client; the proposal is linked to the rule (policy_id); every step is idempotent so a 503 retry is safe, and the claim keeps a dismiss from landing between the PUT and the resolve (review finding 3; residual: a confirm that dies mid-saga and is dismissed after 60 s leaves a revocable allow that lapses in 90 days); capability-core stays the enforcing owner (ADR-0005 pattern: the owner enforces, the edge asks). | assumed, owner to confirm | gw-proposals, bff, spa-proposals, proposal-store-a, proposal-store-b |
| D-WS13-3 (D2-1124) | Where is tool_policy_rule_used written? | (a) capability-core registry_audit_log inside EvaluatePolicy, fail closed (demote to ask) if the write fails; (b) execution-core writes through session-core's audit outbox | (a). capability-core owns tool_policies, makes the relaxing decision and owns registry_audit_log (WS05-S5, capability-ownership matrix), so row and decision cannot disagree and the K3-2 fail-closed rule can hold. Gap noted: registry_audit_log is not forwarded to audit-core yet (WS06 chain). | assumed, owner to confirm | rule-use-b, rule-expiry |
| D-WS13-4 (D2-1123) | What happens to a grant when the MCP definition drifts? | (a) no match; the next decision rebinds the live grant to the newly reviewed digest; (b) revoke on drift via a sweep | (a). Revocation is the owner's act; a drift sweep in session-core would need registry reads across services; no-match is already fail closed. | assumed, owner to confirm | grant-digest |
| D-WS13-5 | approval:grant_use for the execution-core principal | (a) request it through the Control principal train (PRINCIPAL-TRAIN.md, golden re-pin) before APPROVAL_SCOPE_GRANTS_ENABLED; (b) edit plane-service-principals.json in a Model Plane unit | (a). Today execution-core's session-core scopes are approval:deliver, session:heartbeat, session:terminalize only, so every UseApprovalScopeGrant fails closed (pauses). Principal scopes are Control's, changed through its train with pins; give the scope to execution-core only (W2-31 verifier: any holder can release for any owner). | owner must decide; not built (Control process) | grant-digest (live only) |
| D-WS13-6 (D3-938) | Shell "Always" (EP-9 E4) | (a) keep shell ONCE (no Always); (b) a per-user shell allow-rule store keyed on the closed shell_rule action_key; (c) send shell rules to the W5-074 org permission rules (they only tighten) | (a) for now. Offering it reverses plan amendment K3-3 ("shell and process_* are ONCE") and capability-core's WS05-S5 rule that a built-in tool's approval is never relaxed (checkRelaxable), and needs a new rule store; not a small, safe addition to this chain. Keep APPROVAL_SHELL_RULE_PROPOSALS_ENABLED off. | owner must decide; not built | none |
| D-WS13-7 | User-subject erasure of standing approval data | (a) a later unit: session-core deletes the person's approval_rule_proposals and approval_scope_grants on user erasure, and capability-core gains an erasure consumer for user-scope tool_policies and their registry_audit_log rows; (b) leave to org erasure only | (a), split: the session-core half is inside Model Plane rules and is now unit W2-WS13-S7-user-erasure (review finding 21); the capability-core half stays an owner decision, because capability-core has no erasure consumer at all and needs an erasure-event subscription that does not exist yet. Org erasure is covered (proposal-store-a adds an explicit delete). Wider pre-existing gap, not ws13: session-core user erasure (gdpr.rs erase_user_personal_data_for) erases no approval rows at all, including the person's approvals. | session-core half: built by user-erasure; capability-core half and the approvals gap: owner must decide | user-erasure, proposal-store-a |
| D-WS13-8 (review finding 1) | Which pauses may offer 'Always'? A confirmed rule is a capability-core user-scope allow, and capability-core relaxes only a risk-level ask (engine.go DecideWithPolicies 353-355; riskResult: high-risk capability), shown as CapabilityHighRisk. RiskyEffectUnderAsk / ConsentToDisclose come from execution-core's permission gate, which reads no tool policy. | (a) narrow mp-contracts rule_offered to CapabilityHighRisk causes of a pinned MCP tool (with K3-3 that is read, sandbox_compute or context_write) that carry a capability id, which execution-core sets only when the permission gate did not also hold the call; SPA copy says Always skips the high-risk check only; (b) a new execution-core unit (L, authz) in which a verified allow whose signed policy_source is tool_policy_allow satisfies the RiskyEffectUnderAsk/ConsentToDisclose permission pause for one pinned MCP call, behind a new switch | (a). Small and safe: it only narrows what is offered, so no card promises a relaxation that cannot happen (the D2-1109 defect class), and the DoD 'later run proceeds with policy_source=tool_policy_allow' is reachable for every offered rule (a later run whose permission gate also asks still pauses there, which the card copy says). (b) lets a stored rule override the run's permission mode and needs owner sign-off; it can be added later without undoing (a). | assumed, owner to confirm; (b) owner must decide, not built | producers-a, producers-b, spa-decide, spa-proposals |

### 3.2 WS14-S5 input pause and apply-on-resume (EP-4-S7)

Unless an entry says otherwise, the class is "assumed, owner to confirm".

#### DEC-WS14-1: what carries the question pause (D2-1158, D2-1160)

**Options:**
- (a) the pause fold;
- (b) a new descriptor column on `run_questions`;
- (c) a gateway rebuild of the turn.

**Recommended: (a).** The encrypted pause fold carries the question pause, extended with `offered_tools`, the taint,
the selection and the thread position.

**Why:**
- It is the one resume mechanism that in-place approvals and parks already use (DEC-03).
- The input pause has its own reference namespace: `{run_id}:{step_id}:input`, or `:elicit:1` for MCP. That reference
  is both the question's idempotency key and the fold's `pause_ref`.
- Session-core (b2) stores a `resume` question only behind a paused fold whose `pause_ref` equals that key. An approval
  fold (`{run_id}:{step_id}`) can never match it, so the owning plane enforces the pairing by reference, without
  opening the fold.

**Depends on it:** a, b2, b3, c1, e2, f1, f2.

#### DEC-WS14-2: claim, spend and release order (exactly once)

**Apply path.** Claim the fold, run the pure checks, spend the answer, then continue. The answer becomes the paused
call's tool result. A question has no stage 1.

**Release path.** Whenever a continuation cannot apply the answer, it first tries to win the single-use spend with its
own claim token:
- The fold is gone or closed, the thread moved, the fold check failed, or the writer is off.
- Only a fresh win returns `question_answer_released`. Only that code lets a client send the answer as the next
  message, once.
- A replay or an already-consumed spend returns `continuation_already_consumed`.
- A competing live continuation (`claimed_elsewhere`), a busy thread or the same-token hold returns
  `continue_in_progress`. Nothing is spent.
- `question_resume_unavailable` means that nothing was applied and nothing was released. No resend follows.

**Why:** the first draft let a refused claim resend an answer that another continuation was applying (review blocker
1).

**Depends on it:** b3, f1, f2, g, h, i.

#### DEC-WS14-3: the answer window and lapsed questions (D2-1162, D2-1168)

**Recommended:**
- One `RESUME_ANSWER_WINDOW_SECS` (3600) is shared by the spend and the recovery hold. It is bound as `f64`.
- The spend locks the runs row `FOR SHARE`, refuses an ended run (`question_run_ended`), and refuses an answer past the
  window (`question_expired`).
- A winning spend renews the recovery deadline in the same transaction, as the approval spend does.
- A lapsed question, answered or not, ends the run as failed with `question_expired`.

**Depends on it:** b1.

#### DEC-WS14-4: a skip resumes the run

**Recommended:** a skip resumes the run with "the person chose not to answer". Migration 0123 widens
`run_questions_consumed_shape` to include `skipped`, with a revert.

**Depends on it:** b1, f2, i.

#### DEC-WS14-5: safe defaults for taint and selection over a pause (D2-354, D2-540)

**Taint.**
- The fold or descriptor value is used.
- An absent or malformed taint counts as **marked**, always. The taint is set by connector reads, custom REST reads,
  any tool whose screening flags its result, and the browser agent. Tool names in the history therefore cannot prove
  that a run is clean.
- The cost is one person release per held egress call.

**Selection.**
- The paused turn's selection, intersected with any selection the request states.
- Absent means empty (the strictest).

**Combining.** Stage 1 reads the descriptor (the next free `ApprovedAuthority` tags). In-place resumes combine the fold
and the descriptor strictly.

**Alternative:** rebuild the taint from transcript tags. It is rejected because content can forge tags.

**Depends on it:** a, c2, d.

#### DEC-WS14-6: no SPA flag for the question path

**Recommended:** key the path on `delivery === 'resume'`. Such a question exists only when the server switches and the
BFF flag are on.

**Depends on it:** i.

#### DEC-WS14-7: `AgentResume.answer_json` is an echo

**Recommended:**
- The answer applied is session-core's consumed row. `answer_json` is only shape-checked, with `deny_unknown_fields`.
- The gateway writes an answer only while the question is `requested`. For a question that is already answered, it
  continues with the stored answer.

**Depends on it:** a, f1, g.

#### DEC-WS14-8: what this design leaves as it is

- `ask::budget_when` still gives a continuation `max_questions = 0`, so a resumed run cannot ask again.
- **Deviation from plan row S5:** resume is chosen whenever its prerequisites hold. The row also wanted "at least one
  effectful step done".

**Depends on it:** e1, e2.

#### DEC-WS14-9 (new): how a run ends after its answer is released

**Recommended:**
- After a fresh release win, the executor closes the fold.
- It records the run's terminal outcome as `completed` through `record_managed_agent_terminal_outcome` (best effort).
  This is the same end a `next_turn` turn has, because the answer now continues the thread as the next message.
- If the record fails, recovery ends the run.

**Alternative:** a new failure code `question_answer_released`. That needs a CHECK change and a recovery-only code path.
It is rejected for v1.

**Depends on it:** f2.

#### DEC-WS14-10 (new): the thread moves while a run waits

**Recommended:**
- Session-core stamps the thread position into the input fold (b3).
- A claim after a newer message closes the fold and returns `loop_checkpoint_thread_moved`.
- The continuation then releases the answer as the next message (DEC-WS14-2).

**Alternative:** a new user message cancels the waiting question at append time. It is rejected because it needs a
hook in the message append path of another service, and the claim check covers the same race under the thread lock.

**Depends on it:** b3, f1, f2.

#### DEC-WS14-11 (new): the run row's status during the pause

**Recommended:**
- The session-core run row stays `running`. Today `attention.rs` (near line 115) reads that as Working.
- WS16 derives "waiting on you" from `run_questions`: a `requested` question with delivery `resume`.
- The plan line 909 wish for an `awaiting_input` run status is deferred, because a new run status touches every
  terminalization and recovery predicate.

**Units:** none in this scope. This is a note for WS16.

#### DEC-EP4-S7-1: elicitation scope in v1

**Recommended** (unchanged; it applies once EP-4-S7 is unblocked):
- One form per MCP call.
- "Parkable" means that the run can pause in place.
- A failed pause cancels the held call.

#### DEC-EP4-S7-2 (new; owner must decide, not built): the credential for a non-model-origin question

**Problem:**
- execution-core has no production credential that session-core accepts for `question_origin`. It needs a service
  caller `service:execution-core` with `orchestration:write`.
- `plane-service-principals.json` grants execution-core only `approval:deliver`, `session:heartbeat` and
  `session:terminalize` towards session-core.

**Options:**
- (A, recommended) a narrow new scope `orchestration:question:service-origin`;
- (B) a delegated bearer plus a verified service actor.

**Depends on it:** EP-4-S7 (see `_blocked-W3-EP-4-S7.md`).

### 3.3 ZDR waiver issuer chain and posture readers (zdr2)

- **D-ZDR-01**
  - Question: which key signs the Auth Core waiver decision?
  - Options: (a) reuse the token signing key; (b) a dedicated RS256 keypair, read from files by key id, never published in the JWKS, behind one `RetentionDecisionSigner` interface, with an optional previous key for rotation; (c) HMAC.
  - Recommended: (b). A leaked token key cannot mint a waiver. The interface lets the key move to an HSM later. It matches the design on record (auth-core.md: "60 s RS256 JWT"). When the key is absent, every waiver fails closed with `zdr_decision_unavailable`. The owner provisions the key files.
  - Class: assumed, owner to confirm.
  - Units: Z-14, Z-14b.
- **D-ZDR-03**
  - Question: should the integration-corev2 capability record, the acknowledgement API and the E2 read (Z-12, Z-13) be built now?
  - Options: build now; wait.
  - Recommended: build now, as the lead ruled. It is already mostly built (section 3). Z-12-13 closes the remaining gaps inside the design on record: integration-corev2 owns the record and the digest, the routes are bearer-only and owner-only, and the frozen actions surface is untouched.
  - Class: assumed, owner to confirm.
  - Units: Z-12-13, Z-14, Z-19a to Z-19e.
- **D-ZDR-04** (unchanged from the zdr summary)
  - Question: what thread receipt do existing threads get?
  - Recommended: NULL reads as persistent, with no backfill.
  - Class: assumed, owner to confirm.
  - Unit: ZDR-Z-17.
- **D-ZDR-05** (unchanged)
  - Question: should the Data Plane contract gain a field for embedding and rerank posture?
  - Recommended: request the field, and fail closed until it exists.
  - Class: owner must decide; not built.
  - Unit: ZDR-L12-L33.
- **D-ZDR-06** (unchanged)
  - Question: what happens to orphan request-snapshot blobs when a user is erased?
  - Recommended: delete only blobs that nothing references.
  - Class: assumed, owner to confirm.
  - Unit: ZDR-Q-ERASE.
- **D-ZDR-07**: resolved. The org-posture readers were located:
  - Auth Core `interactive-retention-policy.ts` line 219 (intent AND entitled);
  - the BFF: `zdr.rs` about line 205, `domains/inbox.rs` lines 335 and 417, `domains/orgs/model_entitlement.rs` line 432 (intent only);
  - conversation-core `apps/Application Plane/conversation-core/conversation-core-go/internal/clients/orgcore_client.go` line 121 (intent only, raw flag);
  - integration-corev2 `internal/controlplane/clients.go` line 381 (intent AND entitled);
  - quarry-edge `schedule_preflight.rs` line 247 (intent AND entitled).

  No reader sits in a plane where reading Control's posture breaks the ownership rules, and none writes. The one mechanism is that org-core computes the answer (Z-03a) and every reader reads that field behind its own switch (Z-03b to Z-03e, Z-12-13).

  Corrected on review: the first draft said each reader "reads through a route and credential it already has". That was false for integration-corev2. Its org-core credential holds only `org:read:any` (Control compose line 627). The posture route needs `org:retention:read:any`, and org-core pins exactly two holders, auth-core and quarry-edge (`TestOnlyAuthCoreAndQuarryEdgeMayReadRetentionPosture`). So integration-corev2's posture read is 403 today, and its acknowledgement API always answers 503. Z-12-13 now reads `retention_posture.effective_zdr` from the org record (`GET /internal/orgs/:id`, scope `org:read:any`, Z-03a's projection), behind `INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED`. No scope is granted. Each reader's route:
  - Auth Core and quarry-edge: the posture route;
  - the BFF, conversation-core and integration-corev2: the org record.
  - Class: resolved.
- **D-ZDR-08** (unchanged)
  - Question: when may `ZDR_WAIVER_ENABLED` be switched on?
  - Recommended: only after the four enforcers report the waiver contract version. No launcher check exists in the tree (Z-32's launcher half), so the flip is a manual owner step.
  - Class: owner must decide; not built.
  - Units: Z-14, Z-19a.
- **D-ZDR-09** (unchanged)
  - Question: what wording does the acknowledgement and the trust text use?
  - Recommended: the server-pinned draft texts (`draft_pending_od_legal`) and draft SPA copy keys, held for counsel (OD-LEGAL).
  - Class: owner must decide; not built for public trust text.
  - Units: Z-20-to-Z-21b, Z-05b.
- **D-ZDR-10** (new; OD-LAPSE)
  - Question: does an org whose plan lapsed stay ZDR?
  - Options: intent stays ZDR (the matrix's applied default, `policy.plan_lapse`); intent AND entitlement (what Auth Core, integration-corev2 and quarry-edge compute today).
  - Recommended: intent stays ZDR, computed once in org-core with a lapsed flag. It is the stricter, sovereignty-first reading and the matrix pins it. Each reader adopts it only behind its own default-off switch. Cost: a lapsed org's turns are refused `zdr_unavailable` until a provider is attested.
  - Rollout (added on review): org-core with Z-03a ships first. Then `AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED` and `INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED` flip together. With only Auth Core's switch on, a lapsed org is ZDR in the decision, while integration-corev2 refuses the acknowledgement with `zdr_org_not_on`. The BFF, conversation-core and quarry-edge switches should flip in the same window. A reader switched on against an older org-core fails closed (503 or an error), never open.
  - Class: assumed, owner to confirm.
  - Units: Z-03a to Z-03e, Z-12-13.
- **D-ZDR-11** (new; revised on review)
  - Question: which token audiences carry the waiver?
  - Options:
    - every user audience the turn mints;
    - only the four enforcers.
  - Recommended: every user audience the turn mints, except browser-broker and sandbox-manager.
    - Those two are never minted for a waived turn. Auth Core answers 412 `zdr_waiver_route_mismatch` if a decision header arrives on them.
    - Reason: model-gateway refuses any delegated user bearer whose `zdr` or `zdr_waiver` differs from the ingress token (`auth.rs` `verify_delegated_user_bearer`, lines 1325-1356, applied to every delegated header). So the first draft's "four enforcers only" would fail every waived turn with `zdr_posture_mismatch`.
    - Planes that do not read the waiver (Data Plane, Quarry, capability-core, cost-core, ingestion) see `zdr: false`. ZDR.md 5.3 and 5.4 specify exactly that: the decision goes on every mint of the turn, and a waived run retrieves as a non-ZDR run.
    - The rejected alternative: drop the non-enforcer bearers from a waived turn. That loses retrieval and the cost token, so it would need an owner decision.
  - Class: assumed, owner to confirm.
  - Units: Z-14b, Z-19b, Z-19e.
- **D-ZDR-12** (new)
  - Question: who builds the quarry-edge reader (Z-03e)?
  - Options: this programme, in mp-w345; the Quarry session on `quarry-w45`.
  - Recommended: hand the spec to the Quarry session, or build it here only after checking that `quarry-w45` has not changed the same lines of `schedule_preflight.rs`. Quarry is owned by its own build programme, and a parallel edit would conflict at merge.
  - Class: assumed, owner to confirm.
  - Unit: Z-03e.
- **D-ZDR-13** (new)
  - Question: how does the org admin record get its waived-run counts (Z-21b)?
  - Options: a new BFF service principal holding `session:retention:read`, plus an HTTP read edge, because the BFF has no session-core gRPC client; leave the counts out.
  - Recommended: a dedicated principal (`verevon-gateway-retention-read`, audience session-core, directory tenancy) and a model-gateway HTTP route over `CountWaivedRuns`.
  - Why it is not built: it needs a new service credential.
  - Class: owner must decide; not built. Z-20-to-Z-21b shows the acknowledgement ledger and its counts without waived-run counts.
  - Units: none until decided.
- **D-ZDR-14** (new)
  - Question: what text version must an acknowledgement have?
  - Options: always the current version; a `minimum` field in the wording index.
  - Recommended: a `minimum` field, defaulting to current and set to `v1` now, so that a typo fix does not force every member to acknowledge again. The matrix's `text_version_below_minimum` ack state becomes real.
  - Class: assumed, owner to confirm.
  - Unit: Z-12-13.

- **D-ZDR-15** (new on review)
  - Question: how is a waived run continued (approval, plan approval, todo update, resume, continue, cancel)?
  - Options:
    - a continuation asks for a fresh decision for the connection the SPA names, and relays it;
    - every continuation of a waived run is refused (model-gateway's `continuation_gate` already refuses a ZDR credential with 412 `zdr_durable_mutation_forbidden`).
  - Recommended: a fresh decision.
    - Session-core checks the waiver against the run's stored receipt (built), so a decision for another connection cannot advance the run.
    - A revoked acknowledgement is seen at the next step.
    - Without the connection named, today's typed refusal stands.
  - Class: assumed, owner to confirm.
  - Units: Z-19e, Z-20-to-Z-21b.
- **D-ZDR-16** (new on review; OD-LEGAL)
  - Question: how long do revoked acknowledgement rows keep `user_id`, `request_id` and the text snapshot after a user-level erase?
  - Options:
    - a retention window (`ZDR_ACK_RETENTION_DAYS`, default 365, clamp 30 to 3650), then the sweep deletes them;
    - pseudonymise them at erase.
  - Recommended: the window.
    - Pseudonymising in place is blocked by the ledger's BEFORE UPDATE guard (migration 0013), and changing the guard weakens the append-only proof.
    - The window is a config value, so counsel can shorten it without code.
  - Class: assumed, owner to confirm.
  - Unit: Z-12-13.

D-ZDR-02 is not used.

### 3.4 Round-2 items (r2)

- **R2-DEC-OD-12** ("assumed, owner to confirm"). Question: how do rewind and SetActiveLeaf get posture-matrix rows (D2-577, D2-580)? The matrix has no route input, rejects rows with equal inputs and every loader rejects unknown fields. Options: (a) two enforcement rows with distinct inputs (provider `anthropic`; connection `conn-versions`) in the existing feature-row pattern (EP-2 S2/S4, EP-5 S3), whose notes name the refusing code sites; (b) a new `mutation_rows` table (matrix format version 2, every loader changed); (c) code-only. Recommended (a). Also assumed: a waived caller on SetActiveLeaf is refused (DEC-32), feature message_versions. Supersedes DEC-W2-WS17-S9-c (partials-a), which assumed the matrix is frozen; it is authored and pinned (README Z-10). Units: R2-OD-12-a, R2-OD-12-b.
- **R2-DEC-OD-58-a** ("assumed, owner to confirm"). Question: which principal records goal verdicts? Recommended: a dedicated persistent principal `model-gateway-goal-judge` on session-core with scopes `session:goal-judge:read` and `session:goal-verdict:write` (PRINCIPAL-TRAIN rule 4, mail-task bridge precedent). Rejected: flipping the `model-gateway` row to persistent (changes the retention of every token it mints); keeping the judge off. Units: R2-OD-58-a, R2-OD-58-b, R2-OD-58-c.
- **R2-DEC-OD-58-b** ("assumed, owner to confirm"; changed after review). Question: are verdict rows content-free? Finding: migration 0093 keeps `verdict_reasons` (judge model text) and `evidence_ids`. The scope requires content-free rows, so the units build (b) by default: record_goal_verdict binds NULL for both columns and keeps verdict, judge_model and judge_tokens; no migration (both nullable). Owner's alternative: (a) keep the reasons on persistent threads only (GoalBar would show them; the SPA field lastVerdictReasons has no server producer today). Units: R2-OD-58-b.
- **R2-DEC-OD-59** ("assumed, owner to confirm"). Question: what does a user-subject erasure do to thread-bound memory? Recommended: delete rows bound to the person's own personal threads; on Space threads apply the EP-8 S7 attribution rule. Sub-choices assumed: user_anonymize also deletes own-thread rows and keeps Space rows (FU-03's rule); a legal hold on any affected thread (own threads, Space threads with the person's links, and the threads the person's Space-source rows cite) refuses the whole event before any store, including the existing Space-source erasure, so a release completes it on redelivery. Units: R2-OD-59-a1, a2, a3.
- **R2-DEC-OD-40-a** ("assumed, owner to confirm"; changed after review). Recommended: a closed failure vocabulary; per-replica in-memory benching of (provider, model) for 30 s doubling to at most 300 s, clamped retry-after, half-open recovery; Server, Timeout and Empty bench only after 3 consecutive failures from at least 2 distinct orgs (cross-tenant guard; a single-org deployment then benches only on Auth, Quota and declared rate limits); a bare Unavailable never benches; a platform request never reaches the subscription provider; all eligible benched gives `providers_benched` (UNAVAILABLE plus retry-after), ahead of the posture refusals. Same-tier fallback is a separate switch (INFERENCE_SAME_TIER_FALLBACK) because it can refuse requests servable today. Rejected: a shared Redis bench board. Units: R2-OD-40-a, b1, b2, b3, c.
- **R2-DEC-OD-40-b** ("owner must decide; not built"). Question: may a turn fall back between a platform model and the user's subscription or BYO route? Needs a reversal of ownership-matrix rule 17. Recommended: keep rule 17. Units: none.

### 3.5 Depth-register triage decisions (step F, 2026-11-11)

The open P0/P1 code rows of `docs/plans/model-plane-depth-register.md` (569 rows) were triaged: 127 closed, 54 covered by existing units, 285 real code gaps (207 F units after the merge), 47 owed a proof only, 43 needed a decision and 13 were stale. Because of the usage budget only the F units that close a hole in live authorization, ZDR or org-policy enforcement are built in this phase: F-f02-10, F-f02-18, F-f03-18, F-f05-01-executestep-space-search, F-f06-01, F-f06-19, F-f06-21, F-f08-12, F-f10-01, F-f10-04, F-f10-05, F-f10-16, F-f10-29. A class-A ("assumed") entry below is built only where its unit is in that list; otherwise its unit is written and waits for the next phase. Class O entries are not built.

Class codes: A = "assumed, owner to confirm" (unit built dark); O = "owner must decide; not built". Source = chunk triage file. Entries are in chunk order; the merge decisions DEC-M1..M6 come last.

| id | rows | question | options | recommended | class | units |
|---|---|---|---|---|---|---|
| DEC-F01-01 | D1-156,157 | File-size ratchet fails on this branch (hot files grew past ceilings); how to get the gate green | A split files later; B reviewed time-boxed exception list; C merge red | A for new growth + B for current backlog | O | none |
| DEC-F01-02 | D1-177 | Should a member hold capability:write on capability-core? | A read-only + service principal; B capability:write:self; C capability:write | B after a route-by-route owner-check audit | O | none (F-f01-04 independent) |
| DEC-F01-03 | D1-464,688-692 | May a person retract a queued mid-run message (reverses "persist before deliver")? | A approve + build withdrawn marker; B keep persist-first; C visible retracted note | B until wanted; then A | O | none (F-f01-09 independent) |
| DEC-F01-04 | D1-475 | Model entitlement also gate non-chat inference RPCs? | A chat only; B + AnalyzeImage/image gen; C every RPC | B (embeddings stay out) | O | F-f01-11 does not extend to them |
| DEC-F01-05 | D1-724 | How inference-core learns the Codex broker accepts medium/xhigh | A broker advertises; B runbook order; C retry clamped | A | O | F-f01-21 (order rule in PR text meanwhile) |
| DEC-F01-06 | D1-808 | Extend stable-prefix prompt order to Anthropic routes | A dark switch + A/B; B drop cache_control; C leave | A | A | F-f01-27 |
| DEC-F01-07 | D1-186 | Purge organization_siem_audit_outbox on org erasure? | A keep; B purge delivered; C purge all | A | A | none |
| DEC-F01-08 | D1-426 | Repair procedure for a broken audit chain head | A owner-signed re-anchor runbook; B new epoch; C auto reset | A (owner names signers) | O | none (F-f01-02 skips broken orgs) |
| DEC-F01-09 | D1-508 | Keep hosted URL-only image attachments? (allowlist ships off) | A allowlist on; B refuse all; C allow any | A | A | F-f01-12 |
| DEC-F01-10 | D1-545 | Crash-parked run time-to-live and notice | A TTL ~1h; B 24h + notice; C revert to outcome_unknown | B if a resume control is planned, else A | O | none |
| DEC-F01-11 | D1-812 | How the auth-core base schema is committed | A pg_dump live; B restore init file; C snapshot | A (operator step) | O | none |
| DEC-F01-12 | D1-176 | Scope name capability:admin | A capability:admin; B org:admin | A | A | F-f01-04 |
| DEC-F01-13 | D1-399 | Details guard: redact or reject | A redact; B reject + dead-letter | A | A | F-f01-03 |
| DEC-F02-1 | D2-038 | Rollout default of CRON_CONTRACT_GUARDS_ENABLED | a off, flip with SCHEDULE_CONTRACT_ENABLED; b default true when contracts on; c independent | a (already built) | A | none |
| DEC-F02-2 | D2-046 | Host cap counts raw entries (32) | a keep; b dedupe first | a | A | none |
| DEC-F02-3 | D2-057 | Rotate MODEL_EXECUTION_SERVICE_API_KEY exposed in a 2026-10-23 transcript | a rotate; b accept if dev-only | a if used outside dev (needs live credential store; do not grep .env) | O | none |
| DEC-F02-4 | D2-140 | Per-cap budget windows instead of the widest | a per-cap; b widest; c one stored period | a | A | F-f02-04 |
| DEC-F02-5 | D2-299 | Where the Space browser entitlement is read | a org-core resolves floor; b broker asks user-core; c mint into grant request | a (same answer as DEC-F03-1) | A | F-f03-01, F-f03-02, F-f03-03 (F-f02-19/20 merged in) |
| DEC-F03-1 | D2-309 | Where the Space browser floor bit comes from | a org-core reads user-core via scope spaces:browser-floor:read; b signed field on Space decision; c boolean from caller | a (c refused) | A | F-f03-01, F-f03-02, F-f03-03 |
| DEC-F03-2 | D2-321 | Scheduled-run web_search egress | a withhold after external content read; b refuse always; c cap + log | a | A | F-f03-07 |
| DEC-F03-3 | D2-452 | Posture auto and step-up causes | a never auto, stays pending for a person; b as today | a | A | F-f03-12 |
| DEC-F03-4 | D2-464 | ZDR follow-up carrying a Space append context | a honour (narrows only); b refuse | a | A | F-f03-17 |
| DEC-F03-5 | D2-379 | Who may read a skill bundle manifest | a follow skill visibility; b any org member | a | A | F-f03-08 |
| DEC-F03-6 | D2-424,428 | Read-class owner actions in the admission table | a stay on owner_reads pins; b add read-class validator | a (rows closed as not applicable) | A | none (F-f04-17 builds the single listing row) |
| DEC-F03-7 | D2-319 | Scheduled-run token ceiling value | 120,000 input+output tokens lane constant | as stated | A | F-f03-06 |
| DEC-F04-1 | D2-549,550 | Deterministic marker scan: quoted marker / "you are now a" under enforce | a keep; b narrow list; c flag but withhold only when classifier agrees | c | A | none |
| DEC-F04-2 | D2-700 | Unpriced metered call with a USD-only run cap | a stop (fail closed); b default price; c ignore | a | A | F-f04-07 |
| DEC-F04-3 | D2-583 | Second narrow service key execution-core-screening for Do classifier | a separate principal; b widen existing; c deterministic only | a | A | F-f04-12 |
| DEC-F04-4 | D2-586 | ZDR/waived Do run sends a tool result to gateway for deterministic scan only | a yes; b no | a | A | F-f04-13 step 4 (drop if no) |
| DEC-F04-5 | D2-604 | After a flagged connector read in the Ask loop: refuse web tools or ask approval | a refuse; b approval pause | a | A | F-f04-16 |
| DEC-F04-6 | D2-751 | One late "needs approval" notification inside the outbox latency | accept | accept | A | F-f04-20 |
| DEC-F04-7 | D2-595 | Corpus and pass mark to release BUILTIN_OFFICE_SKILLS_ENABLED | >=100 labelled NO/EN requests, zero false injections | as stated (verification work) | A | F-f04-23 |
| DEC-F04-8 | D2-612 | Space Do run pausing for approval loses its overlay on another process | a typed workspace_lost; b upload overlay at pause; c pin replica | a first, b later | O | none |
| DEC-F04-9 | D2-621,624 | Stop then regenerate on a versioned thread | a refuse and say so; b regenerate names the question | b (contract of chat.run regenerate) | O | none |
| DEC-F04-10 | D2-717 | Rotate EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY printed in a transcript | operator act | rotate before any shared deployment, purge transcript | O | none |
| D-F05-1 | D2-878 | Renewal of an expired schedule approval: new schedule or in-place PATCH | a new schedule; b in-place | a (assumed); in-place not built | A (b is O) | F-f05-20 context |
| D-F05-2 | D2-810 | Who gets approval:grant_use scope | a execution-core only via principal train + golden re-pin; b edit registry from a Model Plane unit | a | O | W2-WS13-S7-grant-digest, W2-WS13-S7-producers-b (live only) |
| D-F05-3 | D2-863 | Service worker silent notice when a window is visible | a show then close behind VITE_PUSH_SHOW_WHEN_VISIBLE; b silent; c always show | a after a live Safari/Chrome check | A | F-f05-12 |
| D-F05-4 | D2-936 | Browser bearer near expiry mid-turn | a typed refusal now; b park + re-mint | a now, b later | A | F-f05-15 |
| D-F05-5 | D2-767 | Action-surface contract meaning | a ratchet with closed client-routed list; b arm per id; c delete test | a (see D-f09-5: owner prefers an executor field) | A | F-f05-02 |
| D-F05-6 | D2-896 | Build overlay tombstones before any promote caller | a build now dark; b wait | a | A | F-f05-16, F-f05-17 |
| D-F05-7 | D2-890 | Should a fork copy artifact revisions | a no; b copy up to fork point | a | A | none |
| D-F05-8 | D2-861 | Extend org purge to every notification-core table | a yes, follow-up unit; b leave to teardown | a | A | F-f05-10 (extension, not yet a unit) |
| D-F05-9 | D2-816 | Server WORKING vs legacy needs_approval | a server wins when present; b change session-core | a | A | F-f05-07 |
| D-f06-A | - | Should sandbox-manager verify thread ownership itself too | a gateway-only (built); b sandbox-manager principal + orphan sweep | a now, b later | A | F-f06-01, F-f06-03 |
| D-f06-B | - | Should lockdown/read-only also withdraw inline code_interpreter from Ask turns | a leave; b withdraw on lockdown; c on lockdown and read-only | b | O | F-f06-21 covers standalone browser runs only |
| D-f06-C | - | How far project threads must go before VITE_/GATEWAY_CHAT_PROJECTS_ENABLED may go on | a append-only; b live following, approvals, plan hydration first | a first release, b before wide enablement | O | F-f06-15, F-f06-16 |
| D-f06-D | D2-1087 | Link answered_message_id on a run question | a leave empty; b send client id + atomic append | a | A | F-f07-01 (does not depend) |
| D-f06-E | D2-1033,1035 | Bind the Do browser session to the live panel (browser_session_opened) | a owner-only event with quarry_run_id; b gateway session id; c unbound | a (needs trace vocabulary + Quarry contract) | O | none (F-f05-28 builds the emitter dark; F-f06-10 mounts only the consent card) |
| DEC-F07-1 | D2-1179 | What a plan approval durable grant covers after approve-then-execute | a 24h thread grant; b one approval one run; c cap by runs/TTL | b | O | none yet |
| DEC-F07-2 | D3-077,078,080,153 | Gemini ids, regions and records at enablement | a source rows at S8 + funded live run; b ship dark only | a | O | none |
| DEC-F07-3 | D3-180 | Precision of local time in the date notice | a minute; b hour; c date | a, flip with the stable prompt order | A | none |
| DEC-F08-01 | D3-317 | Public Cohere rerank for tenant retrieval | a keep; b refuse non-Azure (RERANK_REQUIRE_AZURE_ENDPOINT); c b + remove default | b | A | F-f08-06 |
| DEC-F08-02 | D3-319 | Fifteen not_gated egress destinations | a declare, publish nothing until counsel; b per-destination gate; c retire | a now, then one at a time | A (no unit) | none |
| DEC-F08-03 | D3-392 | When the data-use label may show | a always; b behind VITE_MODEL_DATA_USE_LABEL; c remove | b | A | F-f08-17 |
| DEC-F08-04 | D3-517 | Org spend ceilings for API-key turns | a service principal budget check; b accept api_key in cost-core; c refuse runs for ceilinged orgs (interim) | c now, a real fix | O (a/b not built there; F-f10-23a/b now builds a dark, see D-f10-5) | F-f08-14, F-f10-23a, F-f10-23b |
| DEC-F08-05 | D3-440,449 | Credential for non-model-origin MCP elicitation questions (= DEC-EP4-S7-2) | scope orchestration:question:service-origin | as recorded in units-draft/ws14/_blocked-W3-EP-4-S7.md | O | none |
| DEC-F08-06 | D3-442 | Held MCP elicitation calls with more than one gateway replica | a single replica; b sticky routing; c shared store | a until WS04, then b | A | none |
| D-f09-1 | D3-583 | Do browser session across replicas or a restart | a single replica; b sticky by run id; c ids in pause state | a now, b when scaled | O | none (F-f09-02 does not hold across replicas) |
| D-f09-2 | D3-865 | Identity that reads skill bundle blobs for the gateway | a narrow service credential; b relax rule for theme file; c leave off | a | A | F-f09-07 |
| D-f09-3 | D3-930 | Does web.mode=off also cover browser tools | a yes behind own switch; b site policy only | a (CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER) | A | F-f09-13 |
| D-f09-4 | D3-734 | Document and skill references on a Do turn | a refuse typed now; b carry + scoped search; c silent drop | a now, b later | A | F-f09-05 |
| D-f09-5 | D3-894 | Action-surface contract test vs client-executed actions | a committed list; b executor field on registry; c arms for all | b (changes action contract type + admission golden) | O | none (F-f05-02 builds the a-style ratchet meanwhile) |
| D-f09-6 | D3-938,945 | May a shell approval become an Always rule | a never; b read-only argument-screened table; c wider table | b; F-f09-14 narrows regardless | O | F-f09-14 (narrowing only) |
| D-f09-7 | D3-872 | USD half of a goal turn budget inside a run | a tokens now, USD at settle; b cancel on streamed meter; c price table in execution-core | a now, b later | A | F-f04-08, F-f04-09 (F-f09-09 merged in) |
| D-f10-1 | D3-948 | Which roles may place/release legal holds ("compliance" role does not exist) | a add compliance role end to end; b owner-only, remove from lists; c leave | see f10 triage (no unit) | O | none |
| D-f10-2 | D3-981 | Space settings mount of the effective-policy panel | a wait for Space ownership check; b space_id route now | a | A | none |
| D-f10-3 | D3-975 | Do org web domain rules also bind BFF person search and crawl links | a model web calls only; b + BFF; c + Quarry deny list | a, turn on Quarry CRAWL_SCOPE=enforce at deploy | A | F-f10-04, F-f10-05 |
| D-f10-4 | D0-014 | Scheduled Do lane non-completed step: Complete or Fail activity | a FailRunActivity closed code; b Complete + marker | a | A | F-f10-29 |
| D-f10-5 | D3-1216 | How an Agent API key turn is budget-checked | a cost-core accepts api_key; b gateway service credential + key principal; c metering only | b | A | F-f10-23a, F-f10-23b (needs the Control registry grant) |
| D-f10-6 | D3-947 | May org-core ask session-core whether a thread retains anything | a dedicated read-only gRPC; b push receipts; c BFF check | a | A | F-f10-02, F-f10-03 |
| D-f10-7 | D3-1051 | How the retention sweeper erases a thread workspace | a new sandbox-manager RPC with own scope; b impersonate owner; c unconfirmed | a | A | F-f10-08, F-f10-09 |
| D-f10-8 | D3-1096 | Where thread export gets a run model and token totals | a join cost-core ledger at export; b stamp on run row; c omit | a | A | F-f10-13 |
| D-f10-9 | D3-940 | Should a waived Space run state neither gate tool key nor digest | a yes; b keep (digest only) | a when owner confirms | O | none |
| D-f10-10 | D3-1151 | Which Space role may author a Space skill | same as model.space.memory.manage | as stated | A | F-f10-18 |

Merge-pass decisions (new):

| id | rows | question | options | recommended | class | units |
|---|---|---|---|---|---|---|
| DEC-M1 | D3-733,736,1070 | Learning review budget store: Postgres counters (F-f01-22) or Redis (F-f10-12) | Postgres: no new dependency, rows erased with the org purge, inside capability-core own database; Redis: cheaper counters but a new client dependency and TTL-only erasure | Postgres | A | F-f01-22, F-f10-19 (also fixes /learn 503) |
| DEC-M2 | D2-316,317 | Scheduled step retention transport: two bools zdr/zdr_waived (F-f10-26) or a string retention_class (F-f03-04) | bools match the existing AuthenticatedUser flags; string needs an unknown-value rule | bools, absent/unknown context refused as scheduled_retention_unknown | A | F-f10-26, F-f10-29 |
| DEC-M3 | D1-292,310,399 | audit-core switches read the literal "1" (existing parseStrictSwitch), not "true" | a keep service convention; b change parseStrictSwitch to "true" (touches every audit-core switch) | a | A | F-f01-01, F-f01-02, F-f01-03 |
| DEC-M4 | - | Five units add or change scopes in apps/Control Plane/config/plane-service-principals.json (F-f04-12, F-f09-07, F-f10-03, F-f10-08, F-f10-23a); D-F05-2 classes a similar grant as owner-must-decide | a treat as one principal-train approval, built dark, pins re-pinned by the repo script; b hold all five | a; the owner reviews the five scopes together (each is a narrow read/erase scope on one audience) | A | the five units |
| DEC-M5 | D3-517 | Interim guard F-f08-14 and real check F-f10-23: both kept | the real check replaces the guard when AGENT_API_BUDGET_CHECK is true; the guard is the only control if the owner rejects the principal grant | keep both, ordered | A | F-f08-14, F-f10-23a, F-f10-23b |
| DEC-M6 | D2-299,307,309,311 | Switch name for the Space browser floor: BROWSER_SPACE_FLOOR_ENABLED (F-f03-03) vs BROWSER_SITE_SPACE_FLOOR (F-f02-20, dropped) | one name only, read in browser-broker and execution-core | BROWSER_SPACE_FLOOR_ENABLED | A | F-f03-03 |

Not determined (carried from the chunk triage files): see the "Not determined" and "Notes for the integrator" sections of each f0x/_triage.md. This pass did not run any code, tests or docker, so all "verified" statements in the specs are the chunk analysts, spot-checked here against C:/dev/mp-w345 for file paths (every non-[new] path exists), session-core migration numbers, the session-core crate layout and the audit-core switch parser.
