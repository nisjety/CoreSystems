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
| **Response cache (CAG)** | No prompt/response cached | Built. `SemanticCache::lookup`/`store` short-circuit on `zdr` (`cache_io_allowed(zdr) == !zdr`). |
| **Implicit feedback** | No feedback envelope emitted (the feedback store is durable) | Built. `implicit_feedback::envelopes_for` returns empty for a ZDR turn. |
| **Anthropic prompt caching** | No provider-side prompt cache write | Built, gated on the same flag. |
| **NATS / audit envelopes** | ZDR-derived content not published to durable subjects | Built. `zdr: true` envelopes are suppressed by natsx. |
| **Chat stream path** | One persistence-free path: no run, thread, message, event, buffer, idempotency record, memory, cache or feedback; one user message (plus optional grounding) goes to inference-core | Built, read in source (`zdr_direct_stream` in model-gateway). No ZDR request has ever been served by the running stack (0 ZDR log lines in model-gateway and inference-core, 0 ZDR rows in session-core, read 2026-10-03), so this is proven by source and unit tests only. |
| **Features refused with a typed code** | Attachments, image generation, deep research and agentic runs: `zdr_feature_not_supported` (English text only today). Durable gateway mutations: `zdr_durable_mutation_forbidden`. Inbox draft persistence: `zdr_draft_persistence_forbidden`; inbox AI proposals: `zdr_ai_proposal_forbidden`. Support recurrence: `zdr_recurrence_forbidden`. Scheduled and unattended runs, browser runs and speech and other modalities are refused too. | Built, read in source. |
| **Do mode and delegation children** | Refused under ZDR (the gateway and execution-core both refuse an agent run). A ZDR child cannot be registered as a metadata-only run: session-core forces an empty parent run id and a fresh thread for any ZDR start and refuses ZDR orchestration writes. | Built, by refusal. Decision: they stay refused with a typed reason. A waived run (section 5) is a normal run, so it is not affected. |
| **The model provider itself** | The provider must contractually not retain the request | **NOT satisfied. No provider is attested. See section 3.** |
| **Web search, browse and file or artifact output** | Refused with a typed reason | **Gap.** They are dropped silently: the ZDR path performs and reports none of them (read in source; task Z-05a would close it). Separately, the free search chain forwards the query to external engines even for a ZDR request (`docs/compliance/evidence/honest-limits.md`, L13). |
| **Connector ingestion, contextual retrieval, graph extraction, embeddings, rerank** | Follow the org's setting | **Gap, recorded.** The org setting covers interactive content only. Sync, imports and Quarry ingest run under persistent service principals. Contextual retrieval and graph extraction go to Azure OpenAI for any document not classified restricted. Embeddings and rerank (Cohere on Foundry) are guarded per document and per request, not per organisation (honest-limits L12, L33, L24). The designed fix gives background principals the `tenant` posture (matrix rule 16; built in Auth Core, held by no principal yet). |
| **Metering** | A paid add-on should be metered | **Gap.** The ZDR path returns before the budget check and emits no usage frame: ZDR turns are unmetered and show no processing line (the metering half of task Z-06 is deferred). |
| **Browser-side copy after an org turns ZDR on** | The SPA purges its local copy | **Gap.** The threads listing returns no retention field, so the purge cannot fire (finding F8, task Z-09). Turning ZDR on is forward-looking: it purges conversation-core drafts only, and saved threads, memory and documents stay. |

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
| Z-04 | Gate on the org switch | Built |
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
| Z-60 | Live proof on `gpt-5.6-terra` with a ZDR-on test organisation | Not run |

### 5.9 Refusal codes: what exists and what is designed

| Code | Where | State |
|---|---|---|
| `zdr_unavailable` | inference-core, no attested provider | Built in the working tree; not in the running image |
| `zdr_no_attested_provider`, `zdr_gate_unavailable` | gateway, enabling org ZDR | Built |
| `retention_posture_unavailable` | Auth Core mint, posture unresolvable (503) | Built |
| `zdr_feature_not_supported`, `zdr_durable_mutation_forbidden`, `zdr_draft_persistence_forbidden`, `zdr_ai_proposal_forbidden`, `zdr_recurrence_forbidden` | model-gateway, gateway, conversation-core | Built |
| `zdr_connection_not_acknowledged`, `zdr_connection_not_supported`, `zdr_explicit_request_not_supported_by_connection`, `zdr_decision_unavailable` | decision and turn start | Designed, not built |
| `zdr_waiver_route_mismatch`, `zdr_posture_mismatch` | the planes that receive tokens | Designed, not built |

This document and the others that name these codes are checked by
`docs/compliance/evidence/tools/check-doc-codes.mjs` (registry `doc-codes.json` in the
same folder): a code presented as built must be in its source file, and a document that
names a designed code must say "not built".

---

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
