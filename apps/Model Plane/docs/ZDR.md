# Zero Data Retention (ZDR) in the Model Plane

Status: **the enforcement machinery is built; ZDR is NOT usable end to end on the
developer stack, because no model provider deployment is attested.** The org-wide
switch is gated on that attestation. A way for a member to use their own
subscription in an organisation that has ZDR on (the per-connection waiver, section
5) is designed and **not built**.

Last verified: 2026-10-06 against the working tree and the running developer stack
(read-only checks of one machine on one day; not production). The boundary table in
section 2 was last checked line by line on 2026-08-01 and was not re-run for this
rewrite; the rows added since say where they were read.

History: rewritten to the current state on 2026-10-06 (task Z-30). It replaces the
2026-08-01 text, including that text's statement that the OpenAI path is attested by
the boolean `AZURE_OPENAI_ZDR_CONFIRMED` (it is evidence-bound, section 3) and the
correction of the Anthropic attestation statement made earlier the same day (task
CR-T16).

---

## 0. State in one table

| Piece | State on 2026-10-06 |
|---|---|
| Enforcement inside Verevon (threads, memory, cache, feedback, envelopes) | Built. Section 2. |
| Provider attestation for Azure OpenAI and Azure AI Foundry Claude | Evidence-bound and built (section 3). **No provider is attested on the stack read**: every `AZURE_OPENAI_ZDR_*` and `AZURE_ANTHROPIC_ZDR_*` field is empty and both `*_ZDR_CONFIRMED` are `false` (inference-core container, read 2026-10-06). |
| Typed posture refusals in inference-core (`zdr_unavailable`, `residency_unavailable`, `tier_unavailable`) | Built in the working tree on 2026-10-06. The decision ledger entry of that date flips to implemented on deploy. |
| Org-wide switch gated on an attested provider | Built in the gateway (`zdr_no_attested_provider`, `zdr_gate_unavailable`). The running gateway binary carries the gate (string check, 2026-10-06); no refusal was exercised live for this rewrite. |
| Org ZDR reaches the signed tokens (Auth Core reads org-core, fails closed) | Built. The running Auth Core and org-core images of 2026-10-04 (UTC) carry it (string check), and Auth Core's own service credential reads the new org-core route on the running stack: a hand-made request from inside the auth-service container returned 200 with the keys `zdrIntent` and `entitled` (2026-10-06; the resolver itself was not driven). Not proven on a ZDR organisation: none exists. |
| Per-connection waiver (a member's own subscription in a ZDR organisation) | **Designed, not built.** Section 5. |
| Attestation of a provider (the contract) | Operator action, not done (section 4). |
| Known gaps and defects | Section 6. |

---

## 1. What ZDR means in Verevon

A **Zero Data Retention** turn is one where the user's prompt and the model's
answer leave **nothing durable behind**: not in Verevon's own stores, and not in
the model provider's. It is the mode a user picks for a sensitive question they
do not want remembered, and it is also an org-wide posture an admin can set for a
whole tenant. It covers **interactive content**. It does not cover connector
ingestion, search engines or the Data Plane's background jobs (section 2).

Two distinct sources set it, and they combine as a **monotonic floor**: ZDR can
only be turned *on* by a caller, never off:

- **Per-turn**: the "Midlertidig samtale / Temporary chat" toggle in the composer
  sets `zdr: true` on the request.
- **Org-wide**: an org admin switches Zero Data Retention on in workspace settings.
  org-core stores the intent. Auth Core resolves it when it mints a token and signs
  it as `claims.zdr`, so every turn for that org is ZDR regardless of the toggle.

`effective_zdr = issuer_zdr || request_zdr` (`inference-core/src/auth.rs`). The BFF
ORs the header flag with the body flag, deliberately preserving a malformed value so
the typed boundary rejects rather than silently coercing it to `false`, and it forces
`zdr: true` into chat, speech and dictation calls for a ZDR organisation. It only
raises the flag and never turns a `true` into `false`.

How Auth Core resolves the org posture (built 2026-10-03 and 2026-10-04, detail in
`apps/Control Plane/docs/core-research/auth-core.md`): `zdr` is the org's stored
intent AND its plan entitlement, read from org-core with a service credential.
A success is cached 45 seconds. If org-core cannot answer, the last answer is served
(up to 24 hours if it was ZDR, 15 minutes if it was not); with nothing usable the
mint fails with 503 `retention_posture_unavailable` and no token. It never
guesses. The old resolver sent no credential, got 401, and resolved every org to
non-ZDR; that is fixed.

Readers outside the Control Plane still compute the posture their own way: the
gateway BFF from the intent alone (and to true on error), conversation-core from the
raw flag. A plan lapse or an outage can give mixed answers inside one request. For an
organisation whose plan lapsed, Auth Core signs `zdr: false` (intent AND entitlement)
while the stored intent stays on, so a reader of the token claim treats it as not ZDR,
and the BFF, which reads the intent alone, still forces `zdr: true` into chat bodies.
The applied design default is the stricter reading (the intent stays ZDR until an
admin turns it off); it is not built and is an open item (`OD-LAPSE`, decision ledger,
entry of 2026-10-03 on the tenant retention posture). Moving the readers onto one read
model is task Z-03 and is not built.

---

## 2. What ZDR must guarantee, boundary by boundary

ZDR is only real if **every** content-persisting boundary honours it. These are
the boundaries and their current state.

| Boundary | What ZDR requires | State |
|---|---|---|
| **session-core threads/messages** | No thread row, no message row for a ZDR turn | Built. `prepare_managed_run_with_bearer` returns an empty thread id and appends no user message when `zdr` is set. A ZDR turn is structurally unreachable from `messages`. |
| **Dreaming / agent_memory** | No memory extracted from a ZDR turn | Built, by construction. Dreaming reads `messages JOIN threads`, and a ZDR turn writes neither, so it can never be extracted. `dream_extractor` has a test pinning this against the gateway source. |
| **Space memory (`agent_memory` scope='space', EP-8 S6)** | No Space memory row from a ZDR or waived turn | Built dark behind `SESSION_SPACE_MEMORY_ENABLED` (off). No write on a ZDR credential (`session:write` refused) or on a waived credential or thread; never mirrored to Letta; erased by Space deletion in S7. |
| **Agent memory (`agent_memory` scope='agent', EP-8 S10)** | No agent note written or recalled on a ZDR or waived turn | Built dark behind `SESSION_AGENT_MEMORY_ENABLED` (off; also needs `SESSION_SPACE_MEMORY_ENABLED`). Written only by the woken agent's `save_memory` (topic `AGENT_SELF`, origin `model_tool`) on its own verified wake run; recalled only on runs addressed to that agent in that Space; never mirrored to Letta; erased by Space deletion; erasure on binding revocation pending its RPC. |
| **Standing profile (`user_profiles`, session-core, EP-8 S4)** | No profile line from a ZDR or waived turn | Built dark behind `SESSION_USER_PROFILE_ENABLED` (off). A ZDR read returns empty with `skip_reason=zdr`; ZDR and waived Set and Rebuild are refused with `zdr_feature_not_supported` (feature `user_profile`). Erased by ResetMemory, by a memory delete (its built lines) and by organization erasure. The Dreaming rebuild reads only accepted rows, and waived threads produce none. |
| **Request snapshots (`run_request_snapshots`, `run_request_snapshot_blobs`, EP-14 S6)** | No prompt text or tool schema stored for a ZDR or waived run | Built dark behind `INFERENCE_REQUEST_CAPTURE` and `SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED` (both off). Content: prompt text and tool schemas. Never written for ZDR or waived runs: inference-core skips them and session-core refuses with `zdr_durable_mutation_forbidden`. TTL 7 days (cap 30), cascade on run delete, explicit org purge. Read back only by the request inspector, `GET /v1/runs/:run_id/requests/:call_seq` (EP-14 S7, behind `MODEL_GATEWAY_REQUEST_INSPECTOR` and `SESSION_CORE_REQUEST_INSPECTOR_READ`, both off): a ZDR bearer is refused 403 `zdr_feature_not_supported` (feature `run_inspection`), and there is nothing stored to read for a ZDR or waived run. **Gap:** user-subject erasure is not covered (Q-ERASE). |
| **Artifact revisions (`artifact_revisions`, session-core, EP-3 S2)** | No revision row from a ZDR or waived turn | Built dark behind `MODEL_GATEWAY_ARTIFACT_REVISIONS` (`off`) and `SESSION_CORE_ARTIFACT_REVISIONS_ENABLED` (off). No row from a ZDR turn (it produces no artifacts; the RPC refuses ZDR and waived credentials with `zdr_durable_mutation_forbidden`). A waived turn is skipped by the gateway, and a thread holding a waived run is refused (`artifact_revisions_waived_thread`). Erased with the thread (cascade plus `delete_thread_rows`) and with the org (GDPR purge). |
| **Share snapshots (`thread_share_snapshots`, session-core, EP-12 S14)** | No snapshot of a ZDR or waived caller's thread, nor of any thread holding a non-persistent run | Built dark behind `SESSION_THREAD_SHARES_ENABLED` and `MODEL_GATEWAY_THREAD_SHARES` (both off; flip waits on the share-model ADR, OD-5). Taking one refuses a ZDR or waived caller with `zdr_feature_not_supported` (`conversation_share`) at the gateway and again in session-core, and refuses a Space thread or a thread with any non-persistent run (`thread_not_exportable`) inside the export transaction. Revoke deletes the body and digest; rows go with their thread (ON DELETE CASCADE and `delete_thread_rows`) and with the org purge. Owner-only reads. |
| **Response cache (CAG)** | No prompt/response cached | Built. `SemanticCache::lookup`/`store` short-circuit on `zdr` (`cache_io_allowed(zdr) == !zdr`). |
| **Implicit feedback** | No feedback envelope emitted (the feedback store is durable) | Built. `implicit_feedback::envelopes_for` returns empty for a ZDR turn. |
| **Anthropic prompt caching** | No provider-side prompt cache write | Built, gated on the same flag. |
| **NATS / audit envelopes** | ZDR-derived content not published to durable subjects | Built. `zdr: true` envelopes are suppressed by natsx. |
| **Chat stream path** | One persistence-free path: no run, thread, message, event, buffer, idempotency record, memory, cache or feedback; one user message (plus optional grounding) goes to inference-core | Built, read in source (`zdr_direct_stream` in model-gateway). No ZDR request has ever been served by the running stack (0 ZDR log lines in model-gateway and inference-core, 0 ZDR rows in session-core, read 2026-10-03), so this is proven by source and unit tests only. |
| **Features refused with a typed code** | Attachments, image generation, deep research and agentic runs: `zdr_feature_not_supported` (English text only today). Durable gateway mutations: `zdr_durable_mutation_forbidden`. Inbox draft persistence: `zdr_draft_persistence_forbidden`; inbox AI proposals: `zdr_ai_proposal_forbidden`. Support recurrence: `zdr_recurrence_forbidden`. Scheduled and unattended runs, browser runs and speech and other modalities are refused too. Run now (a manual schedule fire, EP-6-S2) is refused at three layers: the model-gateway durable-mutation guard, the capability-core router (non-read request from a ZDR principal, authz.go) and Control's fire and scheduled-run decisions (and the dispatcher's re-check). Browser help requests (EP-1-S1): a ZDR or waived run is refused with `zdr_feature_not_supported` (feature `browser_human_assist`) and no approval row is written. A wide-research run (`InvokeRequest.wide_research`) and `POST /v1/research/wide/estimate` are refused with `zdr_feature_not_supported`, feature `agentic` (EP-2-S8); the estimate stores nothing. The write-approval preview (EP-10-S1, owner-only, decrypted on read and never stored) adds no store and no feature key: a ZDR bearer is refused by the continuation gate with `zdr_durable_mutation_forbidden`, and a waived credential by session-core (waiver route mismatch). | Built, read in source. |
| **Context references on the send (EP-3 S5)** | No reference resolved on a ZDR or temporary turn | Built dark behind MODEL_GATEWAY_CONTEXT_REFS_ENABLED (off). A ZDR turn carrying any context_refs is refused before any network call with zdr_feature_not_supported, feature context_references (whatever the switch says). metadata.context_refs stores {kind,id,label} only, never on a ZDR turn (no message is written). |
| **Thread references (EP-3 S7)** | Tools never offered on a ZDR or waived turn; ZDR and waived threads unreadable | Built dark behind MODEL_GATEWAY_THREAD_REFERENCES_ENABLED (off). Session Core ReadThreadForReference refuses a ZDR credential (DURABLE_MUTATION_FORBIDDEN) and any thread holding a zdr or zdr_waived run (thread_reference_retention); search uses the RECALL purpose. A call that arrives anyway answers thread_reference: unavailable. No new store. |
| **Deck templates (EP-5 S7)** | Refused for ZDR tokens | The deck template list route and invoke `deck_template` read org config only, no conversation content. A ZDR token is refused with zdr_feature_not_supported, feature skills (a template is a skill bundle). Built dark behind SKILL_DECK_TEMPLATES_ENABLED (off). No store. |
| **Deck manifests (`deck` metadata kind, EP-5 S3)** | No manifest kept for a ZDR or waived turn | Built dark behind MODEL_GATEWAY_DECK_TOOL (off). The deck tool's manifest JSON is kept in the assistant message metadata `artifacts` (at most 120,000 chars) and erased with the message. A ZDR turn has no tool loop, and a waived turn keeps no manifest. The built pptx is never stored; a reloaded thread rebuilds it (op `rebuild`). |
| **Thread goals and Keep going (EP-7 S1, S4)** | Refused for ZDR tokens | A goal is user content in a durable store, so a ZDR turn naming one (`goal_id`) or sent by the goal pump (invoke `goal_auto`) is refused with zdr_feature_not_supported, feature thread_goal (no posture-matrix row). Built dark behind MODEL_GATEWAY_THREAD_GOALS_ENABLED and MODEL_GATEWAY_GOAL_AUTO_CONTINUE (off). |
| **Event triggers (EP-7 W4-F T1)** | Refused for ZDR and waived callers | A trigger record is the member's own configuration in a new durable store, so every `EventTriggerService` RPC refuses a ZDR or waived credential (`zdr_durable_mutation_forbidden`), and a service caller owns no trigger. The row holds ids, a closed event type, a short label, a flag and a status, and is erased with its org. Built dark behind EVENT_TRIGGERS_ENABLED (off). |
| **Event trigger fires (EP-7 W4-F T2)** | No fire for ZDR or waived callers | A fire is one row per trigger and webhook event, holding ids, a closed status and outcome, an attempt count and two times. It never holds the webhook body or any payload. A trigger exists only for a non-ZDR, non-waived member, and Control refuses a ZDR fire decision, so none is authorised. The row is erased with its trigger, which the org purge deletes. Built dark behind EVENT_TRIGGER_FIRE_ENABLED (off); it records and authorises fires and starts no run. |
| **Legal holds (EP-9 C1)** | Thread hold on a ZDR or waived thread refused | org-core answers 422 legal_hold_nothing_retained (details.feature legal_hold) once a thread retention reader is wired; until then a thread hold is placed. Org holds on a ZDR org are allowed. Records hold ids, numbers and a hold reference only. Built dark behind ORG_RETENTION_POLICY_ENABLED (off). |
| **/learn (EP-11 S7)** | Refused for ZDR and waived callers and threads | model-gateway refuses a ZDR or waived token, and a thread holding a ZDR or waived run (thread reference read), with zdr_feature_not_supported, feature learn_command, before any transcript read or model call; capability-core refuses a ZDR principal and a ZDR org (memory controls reason zdr) the same way (no posture-matrix row). Proposals are disabled, owner-scoped, origin background_review, erased like WS07 proposals. Built dark behind LEARN_COMMAND_ENABLED (off). |
| **Skill import (EP-11 S8)** | Refused for ZDR principals | capability-core POST /api/v1/skills/import refuses a ZDR or posture-less principal (authz.go:81, and the handler answers zdr_feature_not_supported with feature skill_authoring) before reading the file; model-gateway refuses a ZDR token before forwarding (no posture-matrix row). Imported text and files are org configuration (D9), stored as a switched-off draft and draft bundle, erased with the skill and the org. Built dark behind SKILL_IMPORT_ENABLED (off; needs SKILL_BUNDLES_ENABLED and SKILL_DRAFT_PUBLISH). |
| **Conversation import (EP-12 S15)** | Refused for ZDR and waived callers | session-core ImportThread refuses a zero-retention or waived (DEC-32) caller with zdr_feature_not_supported, feature conversation_import, before any read or write; model-gateway POST /v1/threads/import refuses a ZDR token before reading the body (no posture-matrix row). Store: imported turns are new messages rows of a new caller-owned thread (memory off, user role, metadata.imported), erased with the thread and the org. Built dark behind SESSION_THREAD_IMPORT_ENABLED (off) and MODEL_GATEWAY_THREAD_EXPORT (off). |
| **Sandbox egress (EP-9 E3)** | Held at off for ZDR and waived runs | execution-core's per-call egress ceiling (`egress_proxy::run_ceiling`) holds a ZDR or waived run's `code_interpreter` calls at off, whatever the lease tier or `EXECUTION_SANDBOX_EGRESS` says. The proxy keeps host counts only, in the tool result; nothing else is stored. No refusal code. |
| **session-core api_key principal (EP-12 S4)** | Refused for ZDR api_key tokens on writes | A ZDR api_key token is refused on CreateThread, AppendMessage, StartRun and CancelRun with zdr_feature_not_supported (feature agent_api), before any handler runs; reads are served (nothing is stored for a ZDR key); a key never carries a waiver. Key-owned threads and runs live in the existing threads and runs tables (user_id = apikey:<key_id>) and are erased by the org purge (gdpr.rs). No posture-matrix row (pinned by auth::tests::zdr_api_key_write_is_refused). Built dark behind SESSION_CORE_API_KEY_PRINCIPAL (off). |
| **Agent API read and control routes (EP-12 S5)** | Refused for ZDR api_key tokens | model-gateway refuses every /v1/agent/* route with 412 zdr_feature_not_supported (details.feature agent_api) before any upstream call, for any posture that is not persistent; no posture-matrix row (pinned by agent_api_routes::tests::zdr_key_is_refused_before_upstream). Built dark behind MODEL_GATEWAY_AGENT_API (off). |
| **Do mode and delegation children** | Refused under ZDR (the gateway and execution-core both refuse an agent run). A ZDR child cannot be registered as a metadata-only run: session-core forces an empty parent run id and a fresh thread for any ZDR start and refuses ZDR orchestration writes. | Built, by refusal. Decision: they stay refused with a typed reason. A waived run (section 5) is a normal run, so it is not affected. |
**Do mode and delegation children**' row) this row: '| **ask_user questions (`run_questions`)** | No question is offered or stored for a zero-retention or waived run: the tool is not offered (`max_questions` 0), and the list and answer routes refuse a ZDR caller with `zdr_durable_mutation_forbidden` (412) before they read anything. A question prompt and the owner answer are stored with their run and erased with it (`ON DELETE CASCADE`). | Built behind `AGENT_ASK_USER_ENABLED` (off). The ZDR matrix row `ask_user_zdr_not_offered` is not authored yet, so this is checked by code path, not by a golden. |
| **The model provider itself** | The provider must contractually not retain the request | **NOT satisfied. No provider is attested. See section 3.** |
| **Web search, browse and file or artifact output** | Refused with a typed reason | **Gap.** They are dropped silently: the ZDR path performs and reports none of them (read in source; task Z-05a would close it). Separately, the free search chain forwards the query to external engines even for a ZDR request (`docs/compliance/evidence/honest-limits.md`, L13). |
| **Connector ingestion, contextual retrieval, graph extraction, embeddings, rerank** | Follow the org's setting | **Gap, recorded.** The org setting covers interactive content only. Sync, imports and Quarry ingest run under persistent service principals. Contextual retrieval and graph extraction go to Azure OpenAI for any document not classified restricted. Embeddings and rerank (Cohere on Foundry) are guarded per document and per request, not per organisation (honest-limits L12, L33, L24). The designed fix gives background principals the `tenant` posture (matrix rule 16; built in Auth Core, held by no principal yet). |
| **Metering** | A paid add-on should be metered | **Split.** In-stream cost: built behind `MODEL_GATEWAY_ZDR_STREAM_COST` (default off, WS27-S5). When it is on, a ZDR answer's usage frame states its cost and cache counts, priced from the answer's counts and the model that served it. Numbers only: nothing is stored, no usage envelope is published and no ledger row is written. **Gap, recorded (DEF-Z-06b):** the ledger. A ZDR turn still returns before the budget check and is not ledgered, so budget ceilings and admin cost dashboards do not cover ZDR spend. |
| **Browser-side copy after an org turns ZDR on** | The SPA purges its local copy | **Gap.** The threads listing returns no retention field, so the purge cannot fire (finding F8, task Z-09). Turning ZDR on is forward-looking: it purges conversation-core drafts only, and saved threads, memory and documents stay. |
| **Vertex Gemini (EP-13 S6-S8)** | The provider has no retention attestation, so no ZDR claim for it | Built dark behind `VERTEX_GEMINI_ENABLED` (off). Not ZDR-capable (`supports_zdr` is false), so a ZDR turn skips it and gets `zdr_unavailable` when nothing else survives. The EU floor holds only when `VERTEX_GEMINI_LOCATION` is on the EU allowlist; any other location is `Global`. The release gate (`release-artifact.sh`) requires project, EU location, model list and a secret-partition key file when the switch is true, and refuses any `VERTEX_GEMINI_ZDR_*` key. |
| **Run control across replicas (Stop, queue, probe, plan grant, send receipts, shared turn record, shared idempotency claim)** | A ZDR turn registers no run-control listener, buffer or claim, and no queue text, client message id or grant from a ZDR turn crosses NATS or is stored. | Built behind default-off switches (`MODEL_GATEWAY_RUN_CONTROL_BUS`, `MODEL_GATEWAY_DURABLE_PLAN_GRANT`, `MODEL_GATEWAY_SHARED_TURN_RECORD`, `MODEL_GATEWAY_SEND_RECEIPTS`, `MODEL_GATEWAY_SHARED_IDEMPOTENCY`); not live-verified. Pinned by `run_control_two_replica_test.rs:475` `zdr_turn_registers_no_run_control_listener` (Stop for a ZDR request id is a 404); `queued_input.rs:913` `queue_bus_message_is_never_built_for_zdr_or_waived_claims`; `zdr_posture.rs:826` `every_durable_mutation_refuses_a_zdr_or_waived_token_before_it_runs`; `chat_turn_registry.rs:725` `shared_turn_key_is_identity_scoped`. Redis keys are identity-scoped and hold ids only. |

The first six rows were verified on 2026-08-01. The row that does not hold is the
model provider itself, and because inference-core fails **closed**, it takes the whole
feature down: a ZDR turn today reaches `inference-core`, is refused, and the user sees
an error.

---

## 3. The one missing piece: provider attestation

inference-core will only send a ZDR request to a provider that is **attested** to
have a real zero-retention contract. It does this deliberately, in
`provider/fallback.rs`:

```rust
if req.zdr && !caps.supports_zdr {
    // skip this provider
}
```

If no provider survives the skip, the request exhausts and returns
`ProviderError::ZdrUnavailable`, which reaches the caller as a typed
`FAILED_PRECONDITION` with the reason `zdr_unavailable` (and an
`x-mp-provider-error` trailer; the vocabulary is frozen, see the decision ledger
entry of 2026-10-06).

### What an attestation is

The attestation is evidence, not a boolean (`inference-core/src/provider/zdr.rs`).
Azure OpenAI and Azure AI Foundry Claude are separate resources under separate
retention approvals, so each has its own six fields, with the prefix
`AZURE_OPENAI` or `AZURE_ANTHROPIC`:

| Field | Meaning |
|---|---|
| `<prefix>_ZDR_RESOURCE_ID` | the cloud resource the claim covers |
| `<prefix>_ZDR_APPROVAL_REF` | the retention-exception approval behind it |
| `<prefix>_ZDR_EFFECTIVE_DATE` | when it took effect (`YYYY-MM-DD`) |
| `<prefix>_ZDR_REVIEW_BY` | when it must be re-verified (`YYYY-MM-DD`) |
| `<prefix>_ZDR_REVIEWER` | who signed it off |
| `<prefix>_ZDR_DIGEST` | sha256 over the five fields above, recomputed at boot |

`supports_zdr` is true exactly when a valid attestation is present
(`OpenAiProvider::capabilities`, `AnthropicProvider::capabilities`). Boot **fails**,
it does not downgrade, on a partial set, a digest that does not match, a future
effective date, a lapsed review-by date, or the legacy boolean
`<prefix>_ZDR_CONFIRMED=true` with no evidence behind it. The digest ties the fields
to each other, **not to the endpoint that receives traffic**: a block copied to an
environment pointing at another resource still validates, and inference-core only
logs a warning when the resource name is absent from the endpoint host. Whether the
vendor honours the arrangement is a contract fact that no code can show.

The ChatGPT-account route (`openai-codex-subscription`) declares `supports_zdr:
false` and refuses a ZDR request. The Data Plane's Cohere embedding and rerank
calls do not go through inference-core and have no attestation at all.

**On the stack read of 2026-10-06 nothing is attested**: all six fields are empty on
both prefixes and both booleans are `false`. So every ZDR turn, per-turn toggle OR
org-wide switch, currently fails closed in inference-core. The typed code `zdr_unavailable` is in the working tree only: the running inference-core image (built 2026-10-02) does not carry it (string check, 2026-10-06) and refuses with an untyped FAILED_PRECONDITION.

**This is correct behaviour, not a bug.** Setting an attestation without the signed
contract would be fabricating a compliance claim. **Only a human with the contract
can set the evidence.**

### What the product does about it

Because the gate is real, the product no longer offers a mode that always fails:

- **The composer toggle (shipped 2026-08-01).** The composer reads `/api/v1/models`
  (which advertises a `zdr` capability per model, derived from `supports_zdr`) and
  **disables** the temporary-chat toggle with an explanation when no model is
  attested. inference-core's refusal is surfaced as a **non-retryable** error.
- **The org-wide switch (built; task Z-04).** The gateway refuses to switch org ZDR
  **on** unless the model catalogue lists at least one chat model with the `zdr`
  feature or the `zdr_contractual` or sovereign privacy tier. The answer is 409
  `zdr_no_attested_provider`; if the catalogue cannot be read it is 503
  `zdr_gate_unavailable` (fail closed on enable only). The same gate stands in front
  of the settings page, the `org.update_zdr` agent action and organization
  creation. Turning ZDR **off** is never gated. The gate lives in the gateway
  because the gateway is the only holder of org-core's `org:settings:write:self`
  and the only service that can read the Model Plane catalogue; org-core has no
  gate. With ZDR on and the attestation later lapsing, the settings page shows a
  banner and chat refuses with the typed reason.
- **A test organisation.** With nothing attested the live proof still needs one
  organisation with the switch on. An operator-only tool seeds a dedicated
  `ZDR-TEST` organisation straight against org-core
  (`apps/Frontend Plane/verevonv3/docs/ZDR_OPERATOR_SEED_RUNBOOK.md`). It is for the
  proof only, never for a real organisation.

No existing organisation has ZDR on: the three organisations on the stack (read
again 2026-10-06) are all on the pro plan with the switch off.

---

## 4. Making ZDR production-ready: the operator guide

### Step 0: obtain the contract (human, not code)
Zero Data Retention is a contractual property of the model deployment. For Azure
OpenAI this is the "no data retention / no human review" configuration
(abuse-monitoring disabled), which Microsoft grants per subscription on approval.
For Anthropic it is a ZDR agreement on the account. **Nothing below is safe until
this exists in writing.** Record the evidence where the release gate can find it.

### Step 1: attest the Azure OpenAI path
Set the six `AZURE_OPENAI_ZDR_*` fields (section 3) for inference-core **only for
the deployment the contract covers**. Compute the digest with:

```sh
printf '%s\n%s\n%s\n%s\n%s\n' \
  "$RESOURCE_ID" "$APPROVAL_REF" "$EFFECTIVE_DATE" "$REVIEW_BY" "$REVIEWER" | sha256sum
```

Leave `AZURE_OPENAI_ZDR_CONFIRMED` at `false`: the boolean is not evidence, and set to
`true` without the fields it stops inference-core at boot. Verify: `/api/v1/models`
now lists a `zdr` feature on the covered model, and a per-turn ZDR chat succeeds.

**Known gap in the release gate.** `apps/Model Plane/scripts/release-artifact.sh` (its signed-policy
validation, `validate_runtime_public_policy_files`) keys on the legacy boolean. Only when
`AZURE_OPENAI_ZDR_CONFIRMED` is `true` does it demand passed compatibility gates,
`ZDR_RETENTION_PATH=zdr-provider-route-attested`, the signed route variables and
Azure first in the provider order. Nothing in the script reads the six evidence
fields that inference-core requires. An operator who sets only the evidence fields
therefore skips the release gate's route checks, and one who sets only the boolean
fails boot. Until the gate is aligned with the evidence, whoever attests must satisfy
the gate's conditions by hand and put that in the attestation record. This is a
follow-up, not part of this document's task.

### Step 2: attest the Anthropic path (the knob already exists)
`azure-anthropic` has the same evidence-bound attestation: the six
`AZURE_ANTHROPIC_ZDR_*` fields, wired into `AnthropicProvider::capabilities`. Nothing
needs adding in code. The step is the same operator action as step 1 (a contract and
its evidence, through the release path), for whoever holds an Anthropic ZDR agreement.
Until it is done, ZDR cannot route to Claude.

### Step 3: confirm retention posture flows to the callers
Data Plane services (retrieval, embedding, graph) that call inference-core carry
`MODEL_PLANE_INFERENCE_RETENTION_POSTURE` (`persistent` or `zdr`), which MUST equal
their `retentionByAudience` entry in the Auth Core registry. inference-core treats the
issuer's posture as a **floor**. Keep the env and the registry entry in sync; a
mismatch is a silent downgrade or a hard 503. The third registry value, `tenant`,
exists in Auth Core and no principal holds it yet; the order to introduce it (Auth
Core image, then Data Plane clients, then registry rows) is in the Auth Core research
dive.

### Step 4: the org-wide switch is gated (done)
The gateway gate of section 3 is the code half. The operator half is the attestation:
the switch opens by itself when the catalogue lists a ZDR chat model. Nothing needs
undoing.

### Step 5: verify the whole boundary set under a real ZDR turn
With an attested deployment, run one ZDR turn and confirm, live:
- it succeeds and streams an answer;
- `threads`/`messages` gain no row (`SELECT count(*)` before/after);
- the response cache stores nothing (`DBSIZE` unchanged for the `mp:gw:cache:*`
  keyspace);
- no `feedback_ratings` row and no durable audit envelope for that run;
- `/readyz` and the model catalogue report the attested capability honestly.

### Where the attestation record goes
The record of a real attestation (resource, approval reference, dates, reviewer,
digest) is added to this document by the operator when it is made (task Z-40). No such
record exists yet.

---

## 5. The per-connection waiver: designed, not built

**Status: designed, not built.** The owner decided the direction on 2026-10-03.
Nothing of it is in the tree on 2026-10-06: no `zdr_waiver` claim, no decision
endpoint, no acknowledgement ledger, no `ZDR_WAIVER_ENABLED` flag. This section is the
contract the build must meet. Plane rule: master ownership matrix, rule 17. Decision
record: the decision ledger entry of 2026-10-03 (own-connection ZDR). Privacy
treatment: `apps/GDPR_SUMMARY.md`, "Exception: a user's own AI connection".

### 5.1 The problem and the ruling
A member can connect their own ChatGPT subscription and pick a ChatGPT model. That
route cannot provide ZDR. If the member's organisation has ZDR on, the turn is refused
today and the member has no way to continue. The owner's ruling (2026-10-03): ZDR is
respected for organisations that turn it on; a member who uses their OWN subscription
or key that cannot do ZDR is **not stopped**. When they add the connection, we check
what it supports and tell them (a stronger warning when their organisation has ZDR on,
because it contradicts the organisation's setting). They can continue, and ZDR is then
off **only for runs that use that connection**: never org-wide, never for other
members, never for other connections. Own API keys do not exist yet; the capability
record is generic so they plug in later (task Z-50, deferred).

### 5.2 The rule
O is the org's effective ZDR. R is an explicit ZDR request on this turn (Temporary
chat, body `zdr: true`). C is the user-owned connection the turn names. K is the
connection's capability: supported, not supported, or cannot verify. Cannot verify
counts as not supported in the rule below (the posture matrix pins this). A is a valid
acknowledgement for (org, user, connection, capability digest, wording version).

`effective_zdr = O AND NOT (the run uses C AND K is not supported AND A is valid AND NOT R AND NOT a Space ZDR policy)`

The rule is pinned as data in `apps/Model Plane/api/contracts/zdr-posture-matrix.json`
(authored in the working tree on 2026-10-06, task Z-10, together with
`connection-zdr-capabilities.json`). Each plane has to evaluate those rows with its own
function, and the file fixes the refusal codes below.

- O false: nothing changes, no decision call, no waiver.
- O true and no connection named: ZDR (the org default).
- O true, C named, K not supported, A valid: the run is **persistent with a waiver**
  (retention class `zdr_waived`), bound to C.
- O true, C named, K not supported, no valid A: **refused** with
  `zdr_connection_not_acknowledged`. Never silently served, never silently re-routed.
- O true, C named, K not supported, but the kill switch is off or the connection's
  provider has no waiver route (today only `openai-codex-subscription` has one):
  **refused** with `zdr_connection_not_supported`. Acknowledging cannot help, so none
  is asked for.
- R with C where K is not supported: **refused** with
  `zdr_explicit_request_not_supported_by_connection`. A waiver covers the org default
  only, never an explicit per-turn request. A Space ZDR policy is never waivable (and
  is not applied in chat today, honest-limits L14).
- Any doubt (Auth Core, org-core or integration-corev2 unreachable): ZDR stays on and
  the turn is refused with `zdr_decision_unavailable` (retryable).

### 5.3 Who decides, and how it reaches the planes
- **Auth Core is the only issuer** (it already is the only posture authority). At the
  start of a turn that names a subscription connection in a ZDR organisation, the BFF
  asks Auth Core for a short signed decision under its own dedicated service principal
  (audience `control-policy`, scope `retention:zdr-decide`). The BFF can ask but cannot
  assert. Auth Core re-checks the org posture, reads the connection's posture and the
  stored acknowledgement from integration-corev2 as the session user (a Control Plane
  to Ingestion Plane read edge, recorded as E2 in `docs/CROSS_PLANE_CONTRACT_MATRIX.md`)
  and signs a 60 second decision.
- **The decision becomes a claim.** The BFF passes it in `x-retention-decision` on
  every token mint of that turn. Auth Core verifies it against the cookie session and
  stamps `zdr: false`, `org_zdr: true` and `zdr_waiver` (connection, acknowledgement,
  provider, digest, decision id) on tokens capped at 300 seconds. Without the header a
  token carries the org posture, so a user who mints a token directly can never obtain
  a waiver. A client's `zdr: false` or `x-zdr: false` never lowers the posture: the effective flag is an OR, and Auth Core's service mint routes refuse a client `zdr`.
- **The record belongs to integration-corev2.** The connection's capability and the
  append-only acknowledgement ledger sit next to the connection, in a side table (not
  columns of the connection row, which a read-modify-write could overwrite), written in
  the same audit transaction. integration-corev2 computes the capability digest; Auth
  Core compares it as an opaque string. The acknowledgement and posture routes accept a
  bearer only and require the caller to be the connection's owner, with no
  internal-key path: today the generic connection routes check the organisation only
  for the ChatGPT-account connection (`personalProviderKeys` lists only Microsoft and
  Google), and the internal-key calls skip the owner check.

### 5.4 What each plane does with a waiver (designed, not built, except the Quarry floor)

| Plane | Behaviour |
|---|---|
| model-gateway | Honours a waiver only for provider `openai-codex-subscription` with exactly the bound connection and no explicit `zdr`; anything else is refused with `zdr_waiver_route_mismatch`. Every `effective_zdr` site calls one shared resolver (a source-scan test names the sites). Delegated bearers must carry equal `zdr` and equal waiver, and a waiver must be at the contract version the plane knows, or the request is refused with `zdr_posture_mismatch`. The usage frame carries the retention class from the verified claim. |
| inference-core | Backstop. A principal with a waiver may call only provider `openai-codex-subscription` with the bound connection; every other provider and modality is denied with `zdr_waiver_route_mismatch`. The subscription provider keeps `supports_zdr: false`, so an explicit ZDR request is still refused. |
| session-core | Records an immutable receipt on the run (retention class `zdr_waived`, acknowledgement id, connection id): a record of what applied at start, not the live org posture. A waived run is a normal persisted run. Child runs inherit it. |
| execution-core | Do and plan runs register normally under a waiver; the provider and connection must equal the claim, so children cannot leave the connection. A waived Do run on the ChatGPT route still needs `AGENTIC_SUBSCRIPTION_ENABLED` (default off), read-only plan mode and the gateway setting `zdr` false for that run; this design does not claim the 2026-10-02 Do finding closed. |
| Data Plane, Quarry | Take the run's posture from its verified claim and body flag. A waived run retrieves as a non-ZDR run; retrieval filters documents classified restricted so they never reach the personal account (designed, not built). Quarry already reads the token's ZDR claim as a floor for an interactive user only (`Claims::zdr_floor` in quarry-edge, task Z-08, in the working tree): a service principal's claim is ignored, because it describes the principal and not the run, so the body flag carries the run's resolved posture, which the Model Plane would set to false for a waived run. |
| BFF and SPA | The BFF never writes `zdr: false`. The SPA shows the acknowledgement prompt (when the connection is added, at first use after the org turned ZDR on, and any time from Integrations), a composer chip "not ZDR in this chat", and per answer the retention class. The wording is drafted in the design, needs counsel and a check against the vendors' current documents, and must also say that organisation knowledge retrieved for the run and tool results are sent to the provider. |

### 5.5 What a waived run persists, and what is skipped
A waived run is a normal persisted run: thread, messages, run, plan, events, approvals,
child runs, response cache, feedback and the usage envelope (cost basis subscription).
Content-derived jobs that would send its content to **other** processors are skipped for
it, keyed to the run's retention class and never to the org: memory indexing, dreaming
extraction and learning review (owner default D-B, 2026-10-03). The acknowledgement text
names this.

[live-only] The learning review's cost controls (WS07-S10) keep one counter per owner and one
per organization for each UTC day, keyed by identifiers and holding only an integer, so they
hold no content. A zero-retention envelope is refused before its transcript is read and is
never counted. Session Core withholds zero-retention and waived runs before they are announced
(learning_events.rs), so the review never receives them.

Cited passages and the research stamp (EP-2-S4, behind MODEL_GATEWAY_CITATION_PASSAGES): a
citation may carry `quote` (the passage, at most 400 characters) and, for org knowledge,
`locator` (document_id and chunk_index); a deep-research report artifact may carry
`provenance` (question, sub-queries, read sources, model, coverage). All three stream live on
every turn and are stored in the assistant message metadata (citations, artifacts) only on a
persistent turn, erased with the message by the GDPR path. A ZDR or waived turn stores neither
(session_flow::persistable_citations / persistable_artifacts; goldens
e_zdr_turn_kb_citation_passage_streams_not_persisted, e_waived_research_metadata_not_persisted).
The viewer's "more" read is Data Plane's ACL'd chunk read and stores nothing.

The deep-research plan turn (EP-2-S5, behind DEEP_RESEARCH_PLAN_TURN): the plan turn streams a
research_plan event (sub-queries, up to three clarifying questions, the sub-query cap) and
searches nothing; the assistant message metadata keeps the same plan under research_plan only
on a persistent turn, erased with the message by the GDPR path. A ZDR turn is refused before
either phase through deep_research (no new refusal key); a waived turn is refused deep research
at the route, and session_flow::with_research_plan would store nothing for it anyway (D12). The
run turn's answers are user text: redacted like the message when the turn redacts, bounded to
1,000 characters, and sent only to the synthesis inference.

The claim ledger (EP-2-S6, second pass behind DEEP_RESEARCH_SECOND_PASS): research_ledger rides
artifacts[].provenance.research_ledger of the assistant message metadata. Deep research is
refused under ZDR; a waived turn streams it on the report frame and
session_flow::persistable_artifacts strips it with the stamp (D12). It is erased by the message
GDPR path and never indexed or fed to memory.

Knowledge and connector sources in deep research (EP-2-S7, behind DEEP_RESEARCH_KNOWLEDGE_SOURCES /
DEEP_RESEARCH_CONNECTOR_SOURCES): a Data Plane retrieval per sub-query and, gated, one
connected-app search per selected account; no new store. A ZDR turn is refused through
deep_research before the scope or selection is read (knowledge_only included), and
SourcePlan::for_turn returns web-only for any ZDR turn. Connector citations carry metadata only
and never a quote; nothing new is persisted beyond the existing citations and report artifact.

### 5.6 What the waiver can never do
Never org-wide (no write to org-core), never for another member (the decision requires
the session user to equal the acknowledging user and the connection's owner), never for
another connection, route or model (route binding at three planes), never for an
explicit Temporary chat or a Space policy, never for scheduled or unattended runs, inbox
or support AI, browser runs, speech or non-chat endpoints, never by a background job,
never by an admin or operator switch. No admin lock is built: the owner ruled that
members are not stopped. An organisation admin sees an aggregate record (member,
provider, time, wording version, status, a count of waived runs) and cannot prevent the
choice. Whether a member may lawfully override the organisation's instruction to its
processor is for counsel (`apps/GDPR_SUMMARY.md`).

### 5.7 Switches, order and rollback
The Auth Core decision endpoint sits behind the kill switch `ZDR_WAIVER_ENABLED`, default
off. While it is off no waiver is ever minted, whatever the acknowledgement: the
decision is refused with `zdr_connection_not_supported`, so a member of a ZDR
organisation gets that typed refusal. **Enforcers ship before the issuer.** A plane that
predates the claim ignores it and would treat a waiver token as an ordinary non-ZDR
token, so the flag stays off until model-gateway, inference-core, session-core and
execution-core all report the waiver contract version (launcher check). Order: the four
planes, then integration-corev2 (capability record and acknowledgement API), then
org-core and Auth Core, then the BFF, then the SPA. Roll back in reverse; the kill switch
comes first.

### 5.8 Build state on 2026-10-06

| Task | What | State |
|---|---|---|
| Z-00 | model-gateway refuses the subscription route by name when only the token carries ZDR. Today `normalize` checks only the body flag and the ZDR stream branch calls the direct path with no provider or connection, so the user's route choice is dropped. | Not built |
| Z-01 | Strict `zdr` claim, equal posture across delegated bearers. Today model-gateway and inference-core read an absent claim as false and model-gateway does not compare `zdr` across bearers. | Not built |
| Z-02, Z-03 | Auth Core resolver (stale rule, no failure caching, org-core read route) | Built by the Control Plane wave; readers outside the Control Plane not migrated |
| Z-04 | Gate on the org switch | Built; live-proven 2026-10-28 on the dedicated `ZDR-TEST` org: the switch is disabled with the reason, the live catalogue has the fields the gate reads, and the PATCH is refused 409 `zdr_no_attested_provider` writing nothing (`docs/compliance/evidence/live-proof-wave1-zdr-2026-10-28.txt` at the repository root) |
| Z-05a, Z-05b, Z-06 | Typed refusal catalogue and silent-drop closure in the Model Plane (Z-05a, which absorbed Z-07), the SPA's localized rendering of each code (Z-05b), usage frame provenance (Z-06) | Not built |
| Z-08 | Quarry reads an interactive user token's ZDR claim as a floor and ignores a service principal's | Built in the working tree |
| Z-10 | Golden posture matrix and connection capability contract | Authored in the working tree on 2026-10-06 with a strict loader, a drift check and, in inference-core, a test that holds the capability file against the real ChatGPT-account provider (`apps/Model Plane/api/contracts/README.md`). No plane evaluates the posture-matrix rows yet |
| Z-11 | Shared resolver in `mp-contracts` | Not built |
| Z-12, Z-13 | integration-corev2 capability record and acknowledgement API | Not built |
| Z-14 | Auth Core decision endpoint, waiver claim, kill switch | Not built |
| Z-15 to Z-18 | Enforcement in model-gateway, inference-core, session-core, execution-core | Not built |
| Z-19 to Z-21b | BFF, prompts, composer chip, answer line, admin record and counts (Z-21a is the session-core aggregate behind the counts) | Not built |
| Z-31, Z-32 | End-to-end helper and ZDR specs; launcher, compose and flag wiring | Not built |
| Z-40 | Provider attestation (operator action) | Not done |
| Z-60 | Live proof on `gpt-5.6-terra` with a ZDR-on test organisation | Pass 1 (fail-closed) run 2026-10-28 on the developer stack, on `claude-sonnet-5-5` (the stack's only allowed test model; a pass 1 turn reaches no model): the wave deployed in order, the preflight passed, one turn from the `ZDR-TEST` org was refused `zdr_unavailable` before any model call with no streamed content, and threads, messages, runs, feedback, the audit outbox and the `mp:gw:cache:*` keyspace were unchanged; the org was switched back off (`docs/compliance/evidence/live-proof-wave1-zdr-2026-10-28.txt` at the repository root). Pass 2 not run: it needs Z-40 |

### 5.9 Refusal codes: what exists and what is designed

| Code | Where | State |
|---|---|---|
| `zdr_unavailable` | inference-core, no attested provider | Built; in the running image, seen live on 2026-10-28 (Z-60 pass 1) |
| `zdr_no_attested_provider`, `zdr_gate_unavailable` | gateway, enabling org ZDR | Built |
| `retention_posture_unavailable` | Auth Core mint, posture unresolvable (503) | Built |
| `zdr_feature_not_supported`, `zdr_durable_mutation_forbidden`, `zdr_draft_persistence_forbidden`, `zdr_ai_proposal_forbidden`, `zdr_recurrence_forbidden` | model-gateway, gateway, conversation-core | Built |
| `zdr_connection_not_supported` | decision and turn start | Built (Z-00) on the HTTP stream, unary and `/v1/ai/chat` paths; not yet on gRPC |
| `zdr_connection_not_acknowledged`, `zdr_explicit_request_not_supported_by_connection`, `zdr_decision_unavailable` | decision and turn start | Designed, not built |
| `zdr_waiver_route_mismatch`, `zdr_posture_mismatch` | the planes that receive tokens | Designed, not built |

This document and the others that name these codes are checked by
`docs/compliance/evidence/tools/check-doc-codes.mjs` (registry `doc-codes.json` in the
same folder): a code presented as built must be in its source file, and a document that
names a designed code must say "not built".

### 5.10 Reduced answers: the retrieval notice (Z-41b)

A ZDR or waived turn that asks for organisation knowledge is answered from text search only when the Data
Plane has no ZDR-approved route to embed the query. No embedding route is attested while Z-40 is not done, so
that is the case the code is built for; the live state was not checked for this section. The Data Plane says so
with the notice `zdr_dense_unavailable`. A ZDR turn is told it under that code. A waived turn runs
persistent for its connection, so it is told only that its answer was reduced, under the generic code
`retrieval_reduced`, and never the zero-retention wording. model-gateway passes the notice on in three places:

- the `retrieval_notice` SSE event, `{code, wording_key, request_id}`. It is a control event, sent whatever
  features the client opted into, and before the answer. On a durable turn it is buffered like every other
  frame, so a resumed stream replays it. A ZDR answer has no stream buffer by design, so it cannot be resumed,
  and its notice reaches only the live connection;
- `notice` on the answer of `POST /v1/retrieval`, present only when the answer was reduced;
- `retrieval_notice` in the knowledge-search tool result that the model reads. A ZDR turn runs no tool loop (it
  offers no tools), so only a waived turn reaches this place; the turn emits the same typed event where it emits the
  tool result, so a resumed stream replays it.

The notice carries only the code and the key the client's wording is filed under, never the Data Plane's
English sentence. The wording belongs to the client (task Z-20). A persistent turn never carries the notice,
whatever mode its body asks for. Where the Data Plane refuses a restrictive retrieval instead of answering it
(the reason `zdr_dense_unavailable` leads the status message; its streaming retrieval sends it as
`failed_precondition`, and the gateway matches the reason, not the code), the turn gets the typed `zdr_unavailable`
refusal, not an empty answer. A refused knowledge search inside a waived turn's tool loop reaches the model as a tool
error that carries only the typed code (`knowledge_search refused: retrieval_reduced`), never the Data Plane's
sentence; a persistent turn gets a plain failure named by its status code.

Not covered: the source-verification retrieval that runs after a durable answer, whose verdict does not carry the
notice. Accepted limit (Z-41b finding b, owned by the Data Plane): the gateway's unary Data Plane call never receives
the typed refusal today, so the typed path is proven by a Data Plane double only. With hybrid search switched off on
the Data Plane, the unary retrieve returns the refusal as `internal` with the embedding error text and no typed
reason (retrieval-engine `retrieve` handler, `Status::internal(e.to_string())`), so the answer goes on ungrounded with
no notice; the gateway cannot match a reason the Data Plane does not send. The typed refusal has to come from the Data
Plane (task Z-41) and this item is raised to its owner. Proof of what is built: the streamed notice before the first
chunk, on resume, on a persistent turn (absent) and from a waived tool call is covered by the `stream_answer` and
`knowledge_search_says` tests in `model-gateway/tests/e2e_invoke_chain_test.rs`. The live proof needs an attested ZDR
route, so it waits for Z-40 and Z-60.

### 5.11 Durable mutations and the retention receipt (Z-15b)

Owner decision 17 (2026-10-13) splits the durable mutations by what they do.

1. **Deletes, archives and presentation controls are the owner's housekeeping under every posture.** A
   zero-retention user can delete or archive a thread, set its pin and persona controls, delete a memory of their
   own, and remove an MCP server they own. Each keeps its org and owner checks. A presentation request that carries a title or a preview is
   refused as a durable mutation under ZDR, because a ZDR credential never persists conversation text.
2. **Starting or advancing stored work, and continuing a stored run, is allowed only when the caller's posture
   equals the receipt of the object it changes.** The receipt is the run's retention receipt (Z-17). A plan or a
   todo takes the receipt of its run, and a plan with no run is persistent. An approval takes its run's receipt.
   A ZDR credential is always refused with `zdr_durable_mutation_forbidden`, because a ZDR run keeps no plan, todo
   or approval to advance. A waiver is valid only for the run started under the same waiver (same connection and
   acknowledgement). Any other difference is `zdr_posture_mismatch`. Every refusal comes before any state change.
3. **Service workloads are not held to a user's posture.** They act for the run's owner under their own scope, and
   their system writes (the terminal outcome of a ZDR run among them) keep working.

Where it is enforced. Session Core: `retention_receipt::require_stored_posture`, `run_owner_matches` (behind
`ResolveRunOwner`), `stored_plan_retention`, `stored_todo_retention` and `stored_approval_retention`, called from
`TransitionPlan`, `TransitionTodo` and `DecideApproval`. Model Gateway: `zdr_posture::continuation_gate` on approve,
reject, todo update, approval decision, resume, cancel and plan approval. A typed refusal from Session Core passes
through the run-owner gate unchanged.

Left as they were. Create memory, create document, queue input, feedback, the MCP register, share, OAuth and connect
routes, and the task and cron proxies are not continuations. They keep the blanket refusal of ZDR and waived
credentials. A waiver is minted only behind Auth Core's kill switch, which is off by default.

Open. Threads carry no receipt: Z-17 stamps runs. A waived turn that appends to an existing thread (`AppendMessage`)
is therefore not checked against that thread's posture. Closing this needs a thread receipt (a migration and a stamp
at thread creation). Until then the owner's continuation rule covers plans, todos, approvals and runs, not threads.
A ZDR credential is refused on thread appends by Session Core's ordinary write rule, which is untyped.

---

### 5.12 Run trace and the web-taint marker (WS10.S6, WS11.S7)

Two session-core tables/columns added after this section was written, both ZDR-safe by construction:

- `run_trace_events` (migration 0086) holds Quarry's browser-run trace. The consumer (`SESSION_CORE_TRACE_CONSUMER`, off by default) stores an event only when the message carries an explicit `"zdr": false` AND the run's `retention_class` is not `zdr`; a ZDR run never gets a row, and `EventLog.QueryEvents` therefore answers an empty page for it. Rows are removed with their run, with their org on the GDPR purge, and after `SESSION_CORE_TRACE_RETENTION_DAYS` (30) by the sweeper.
- `runs.external_content_at` (migration 0087) is a TIMESTAMP only. It is set from a tool NAME (`TrustClass::is_external_content`) and holds no URL, query, page text or result, so a ZDR run that used a web tool stores one time value and nothing about what it read. Its only reader is Dreaming, which skips assistant messages written inside a tainted run (`SESSION_CORE_DREAMING_SKIP_EXTERNAL_CONTENT`, default on, only an explicit `0`/`false`/`off`/`no` disables it).

## 6. Known defects and gaps

1. **Subscription route under token ZDR (Z-00).** With ZDR carried only by the token,
   the user's ChatGPT choice is dropped and the turn is tried on the default ZDR chain.
   The BFF masks it by forcing the body flag; a non-BFF client or the 45 to 60 second
   cache skew can reach it. With nothing attested the chain refuses it before any
   content leaves.
2. **Absent `zdr` claim (Z-01).** Read as non-ZDR by model-gateway and inference-core
   and rejected by session-core, execution-core and retrieval-engine.
3. **Three readers of the org posture (Z-03).** Section 1.
4. **Silent drops, no metering, no processing line under ZDR.** Section 2.
5. **The ZDR provider gap.** No attestation anywhere (section 3); the Data Plane's
   Cohere calls have none at all and follow no org setting (honest-limits L12, L33).
6. **Release gate and runtime key on different variables.** Section 4, step 1.
7. **ZDR refusals are untyped or English-only in several planes** (execution-core,
   inference-core modality refusals, the subscription text in `normalize`), while the
   product requires typed Norwegian and English reasons (task Z-05a).
8. **Public text that the waiver would make untrue.** The settings tooltip, the trust
   page and the privacy-tier tooltips say that interactive requests go only to a provider
   with a recorded ZDR attestation, and that the ChatGPT route refuses a ZDR request.
   For a waived run neither sentence holds. The trust edits (tasks Z-42 and X-TRUST) must
   land together with the waiver, and may state only what the live proof shows. Public
   text is changed by the owner through the trust edits, not by this document.

---

## 7. Summary

- ZDR **enforcement inside Verevon** is built across the durable boundaries of
  section 2, and the org-wide switch is gated on every product route that writes it, so an organisation cannot enable a mode
  its provider fleet cannot serve (org-core has no gate of its own, section 3).
- ZDR is **not usable** because no provider deployment is attested, and the gate
  correctly fails closed rather than leaking. Closing the gap is a **contract and
  attestation** task (section 4), not a code redesign.
- A member's own subscription in a ZDR organisation is **refused today**. The way
  through (an acknowledged, signed, per-run, connection-bound waiver, never org-wide)
  is designed and **not built** (section 5).
- **Do not mark ZDR "complete" until at least one deployment is attested and the live
  proof passes.** Until then it is "enforcement-ready, provider-unattested".
