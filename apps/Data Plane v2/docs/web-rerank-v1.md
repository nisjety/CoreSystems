# Web rerank (`POST /v1/rerank`): runbook

Status: **built and switched off** (QB-T08). Nothing here calls a paid provider until the owner
switches the route on. Contract tests cover the rules below; no live run has happened. The live
proof belongs to the orchestrator (section 12).

Owners: the Data Plane owners run the route; the Control Plane owns the principal registry; Quarry
owns its own calls to the route (QB-T09). Named owners for quota and price are open (section 14).

## 1. What the route does

Quarry option B. The Data Plane reranks **candidates a service supplies** (a query and up to 100
texts) and returns them ordered by a cross-encoder score. There is one provider call per request
and no Model Plane hop, no subscription route and no sharding. The model is the one the engine
already uses for tenant rerank (`RERANKER_MODEL`), served by the Azure deployment named by
`RERANK_ENDPOINT`.

The route is a sub-router of retrieval-engine (`services/retrieval-engine-rs/src/rerank_api/`).
Its decision order is in `rerank_api/policy.rs`; its wire types are in `rerank_api/schema.rs`.

## 2. Who may call it

- Only `service:quarry-edge`, holding scope `data:rerank`. The route table
  (`authz/service_routes.rs`) grants the pair; anything else is refused before the handler runs.
- `WEB_RERANK_ALLOWED_SERVICES` is a second gate inside the handler. It defaults to
  `service:quarry-edge`.
- A user token is refused with 403 (`service_only_route`) before any membership lookup.
- No bearer token is 401.

## 3. Switches (default, boot check, effect)

Every switch is read **at boot**. Change one in the launcher environment, then recreate
retrieval-engine. A plain restart keeps the old value.

| Name | Default | Boot check | Effect |
| --- | --- | --- | --- |
| `WEB_RERANK_ENABLED` | `0` | `0`, `1`, `true` or `false`. Any other value reads as off and logs one ERROR. | Off answers `skipped` with reason `disabled`. No provider call. |
| `WEB_RERANK_PROCESSOR_APPROVED` | `false` | The same switch rule. | Off answers `skipped` with reason `processor_unapproved`. Turning it on needs counsel's Art. 28 confirmation (section 5). |
| `WEB_RERANK_ALLOWED_SERVICES` | `service:quarry-edge` | A comma-separated list of service ids. | Only the listed ids may call the route, and only when the route table also grants them `data:rerank` (section 2). |
| `WEB_RERANK_DAILY_UNIT_BUDGET` | `10000` | A whole number. `0` disables the alert. | An alert threshold, never a stop. Above it the engine logs a warning and counts `dpv2_web_rerank_budget_exceeded_total`; calls go on. |
| `USAGE_EMIT_ENABLED` | `0` | `0`, `1`, `true` or `false`. Any other value reads as off and logs one ERROR. | The usage ledger. Off answers `skipped` with reason `metering_unavailable` and calls no provider, because a paid call that cannot be recorded is not made. On needs the `DATAPLANE_COST` stream to be creatable at boot; a failure logs one ERROR and the route stays `metering_unavailable`. Turn it on only after the metering consumer is deployed and verified. |
| `DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_RPS` | `5` | A number. | Per-org admission for the route (429 `rate_limited`). Shared through Dragonfly; it fails open when Dragonfly is unreachable. |
| `DPV2_WEB_RERANK_RATE_LIMIT_PER_ORG_BURST` | `10` | A number. | The burst of the same bucket. |
| `RERANK_PROVIDER_MAX_INFLIGHT` | `0` | A whole number from 0 to 1024. | Provider calls in flight on this replica, shared by tenant retrieval and the route. The route may hold this minus 2; tenant retrieval may hold all of it. `0` leaves tenant retrieval unbounded and gives the route no capacity. |
| `RERANK_PROVIDER_RPS` | `0` | A whole number from 0 to 10000. | Provider requests per second on this replica. Tenant calls spend it and are never refused by it; the route is refused while it is spent. `0` gives the route no capacity. |
| `RERANK_PROVIDER_TENANT_WAIT_MS` | `1000` | 0 to 30000. | How long a tenant call waits for a permit once the pool is saturated. Past it the call falls back to the fused order. |
| `RERANK_BREAKER_FAILURES` | `5` | 1 to 1000. | Failed provider calls inside the window that open the breaker. |
| `RERANK_BREAKER_WINDOW_SECS` | `30` | 1 to 3600. | The breaker's failure window. |
| `RERANK_BREAKER_OPEN_SECS` | `30` | 1 to 3600. | How long the breaker stays open before one probe call. |

Boot rules, all tested or stated in the code:

- A malformed provider limit logs one ERROR, and every provider limit reads as its default.
- A malformed route switch reads as off and logs one ERROR.
- With `WEB_RERANK_ENABLED` on, an endpoint that fails the Azure rule, or a missing `COHERE_API_KEY`,
  logs one ERROR. The route then answers `skipped` with reason `misconfigured`. The engine still starts, and
  tenant retrieval is untouched. With the switch off, the same problem is silent until the switch is turned
  on, so check the endpoint before turning it on.
- With `WEB_RERANK_ENABLED` on and no provider capacity (`RERANK_PROVIDER_MAX_INFLIGHT` at 2 or
  less, or `RERANK_PROVIDER_RPS` at 0), the route answers `skipped` with reason `misconfigured`.

Enabling `RERANK_PROVIDER_MAX_INFLIGHT` also bounds tenant calls, because the permit pool is shared.
`RERANK_PROVIDER_RPS` never refuses a tenant call. That is why the
defaults stay at `0` until the owner enables the route (section 13).

## 4. Kill switches and rollback

1. **Route off (fastest):** set `WEB_RERANK_ENABLED=0` and recreate retrieval-engine. The route
   answers `skipped` with reason `disabled` and makes no provider call. It takes effect when the
   engine starts.
2. **Capacity off (web only):** set `RERANK_PROVIDER_RPS=0` and recreate. Tenant retrieval is not
   affected, because tenant calls are never refused by the request budget.
3. **Grant removed:** remove `data:rerank` from the `quarry-edge` row of the committed principal
   policy (`apps/Control Plane/config/plane-service-principals.json`), then redeploy Control Plane through the
   launcher so the new registry is loaded.
   A new token then lacks the scope and the route answers 403. Tokens minted before the change keep
   the scope until they expire. Nothing in this repo exercises that mint (section 14).
4. **Caller off:** Quarry stops calling the route (its own flag, owned by Quarry QB-T09).
5. **Breaker:** automatic. Five failures in 30 seconds open it for 30 seconds, then one probe is let
   through. Open, the route answers `skipped` with reason `circuit_open`, and tenant retrieval is not
   refused.

### Deploy order for quarry-edge (QB-T09)

Owner ruling of 2026-10-12. Quarry's rerank client must not ship before this route's gate and the
Control Plane grant are live. Until they are, every non-ZDR search from quarry-edge skips or degrades
its rerank, the outcome is never cached (skipped and degraded results are not cached, by design), and
each search pays the search providers again. That is a cost regression, not a fallback. The order is:

1. Control Plane: principal bundle P1 is live in auth-service, so quarry-edge mints `data:rerank` on
   `data-plane`.
2. Data Plane: `WEB_RERANK_ENABLED=1`; `WEB_RERANK_ALLOWED_SERVICES` names `service:quarry-edge`;
   `RERANK_PROVIDER_MAX_INFLIGHT` is above 2 and `RERANK_PROVIDER_RPS` above 0; and
   `WEB_RERANK_PROCESSOR_APPROVED=true` only after counsel's Article 28 sign-off (OD-LEGAL).
3. Check, from this directory: `bash scripts/qb-preflight.sh --live --ship-qb-t09` must print PASS.
   Record the sign-off as `DPV2_PREFLIGHT_ART28_CONFIRMED=yes`, so the check accepts the processor gate.
   The check reads the switch values and the live registry in place and prints labels only.
4. Ship quarry-edge. Its rerank is off by default (`QUARRY_EDGE_SEMANTIC_RERANK=0` in the Ingestion compose).
   Set `QUARRY_EDGE_SEMANTIC_RERANK=1` only once steps 1 to 3 pass. The first run uses `QUARRY_EDGE_RERANK_SHADOW=1`.

Roll back in this order: first the caller (`QUARRY_EDGE_SEMANTIC_RERANK=0` on quarry-edge), then the
route (`WEB_RERANK_ENABLED=0` here).

## 5. Processor and data

- **Processor:** Cohere, served from the Azure AI Foundry deployment `core-ai-rg` (model
  `Cohere-rerank-v4.0-pro`, the same deployment as tenant rerank). The deployment's region is
  unverified.
- **Processor gate:** `apps/GDPR_SUMMARY.md` requires that external reranking is allowed only when the
  processor is approved **and** `allow_third_party_processing=true`. `WEB_RERANK_PROCESSOR_APPROVED`
  is the deployment half. The Quarry half (its `PrivacyPolicy.allow_third_party_processing`) is not yet
  carried onto the search path. That is open until Quarry QB-T09 lands.
- **Data classes:** the route does not inspect the text it is given. Quarry must send quarry-origin or
  public text only. Vector hits from the Data Plane corpus are tenant chunk text, so Quarry's vector
  leg is confined to `documents.source = 'quarry'` before any candidate reaches this route (QB-M1a).
  Until that is proven on the live stack, this is a rule the caller must keep, not a fact about the data.
- **ZDR:** the effective ZDR of a call is on when the signed token's `zdr` claim is set or the body's
  `zdr_mode` is `reject` or `ephemeral`. A call with effective ZDR is not run; its reason is `zdr` unless an earlier check in section 9 applies, and its
  response lists `zdr_actions_applied`: `no_provider_call`, `no_cost_event`, `no_result_cache`.
- **Documentation conflict, recorded and not relied on:** `sovereign-rag-blueprint-reconciliation.md`
  calls Embed v4 on Azure EU DataZone "ZDR", while `sovereign-rag-phased-plan.md` (Phase 7) still lists
  "ZDR contractual for Cohere ... Rerank" as planned work. No contractual ZDR evidence for Cohere on Azure
  exists in this repo. Neither document is used as evidence for this route.

## 6. Attestation (data, not code; owned by Z-41)

- The route asks one seam, `EgressAttestation::permits_rerank()`. Its default (`NoAttestation`) answers
  "not attested", so every call with effective ZDR is skipped. The engine wires the real answer from the
  shared module `src/egress_attestation.rs`, which **Z-41** owns. The route has no attestation file and
  no attestation switch of its own.
- The attestation is six data fields for one Azure resource, empty by default and set together through
  the release path: `AZURE_COHERE_ZDR_RESOURCE_ID`, `AZURE_COHERE_ZDR_APPROVAL_REF`,
  `AZURE_COHERE_ZDR_EFFECTIVE_DATE`, `AZURE_COHERE_ZDR_REVIEW_BY`, `AZURE_COHERE_ZDR_REVIEWER` and
  `AZURE_COHERE_ZDR_DIGEST`. The digest is `printf '%s\n%s\n%s\n%s\n%s\n' "$RESOURCE_ID" "$APPROVAL_REF"
  "$EFFECTIVE_DATE" "$REVIEW_BY" "$REVIEWER" | sha256sum`.
- The rerank route is attested only when `RERANK_ENDPOINT` is a host of that resource: the resource name
  followed by one of the Azure domains of its surface (`services.ai.azure.com` or
  `cognitiveservices.azure.com` for the Cohere resource; `openai.azure.com` or
  `cognitiveservices.azure.com` for the Azure OpenAI resource). A same-named resource under another Azure
  domain does not match, and an endpoint with a space, a control character or a backslash is refused. The
  public Cohere API is never attested. The same attestation covers the query embedding on
  `COHERE_EMBED_V4_ENDPOINT`.
- A field set that does not validate, a review-by date that has passed, or an endpoint the attestation does
  not describe is logged at boot as `ZDR egress attestation refused`. That route stays unattested, and the
  engine keeps running. Review-by is enforced at boot only, so restart the engine after a renewal.
- No provider is attested today (owner default D-A). Until one is, no ZDR organization can use this route,
  and the org ZDR switch stays gated on the attestation.

## 7. Capacity, quota and budget

- **Capacity now:** a shared Foundry deployment behind a limiter (owner default D-D). A dedicated
  deployment is required before general availability.
- **Measured shortfall:** the shared S0 quota returned 429 for a measured share of tenant rerank
  queries (about 19 percent in an earlier run; `docs/retrieval-fusion-evidence-2026-08-26.md`). A shared deployment cannot meet
  a degraded-rate gate of 2 percent, so a dedicated deployment or an owned quota is a precondition for
  making the route default-on.
- **Quota owner:** not named yet. Open (section 14).
- **Price sheet:** cost-core's code carries no price for `Cohere-rerank-v4.0-pro` (its Postgres price catalogue was not checked). The usage event records the billed search units when the provider reports them, and the candidate count otherwise (section 8). No USD figure is computed. Open.
- **Budget:** `WEB_RERANK_DAILY_UNIT_BUDGET=10000` is a placeholder. It alerts; it never stops a call. The
  owner's figure replaces it later (owner default D-E).

## 8. Cost events

- One usage-ledger event per successful provider call (schema version 2, `event_type` `rerank`, `feature`
  `web-rerank`), built by `usage-ledger-rs` (`CallUsage::rerank`), signed by the retrieval producer and
  published to the durable `DATAPLANE_COST` stream. The data-orchestrator consumer verifies the envelope and
  writes a `cost_events` row. It names no user. `count` is the candidates scored. `units` is the provider's
  billed search units when it reports them; otherwise the event is `estimated` and carries no `units`. The
  token counts are zero, because a reranker bills search units. `zdr` is the call's effective posture.
- A skipped or degraded outcome is never metered. While `USAGE_EMIT_ENABLED` is off the route answers
  `skipped metering_unavailable` and calls no provider.
- Idempotency: the call's deterministic key is `rerank:{org}:{request_id}:{digest}`. The digest covers the
  query, the documents, the `top_n`, the `zdr_mode` and the sovereignty flag, so a reused request id with
  other content gets a different key. The ledger's own key, which the verifier checks, is
  `usage:web-rerank:{sha256 of that call key}`, and the broker de-duplicates a publish on
  `{org}:{that key}`.
- A replay inside 120 seconds on the same replica is served from a cache and makes no second call. A
  replay on another replica, or after the window, calls the provider again. This is a known gap. A shared
  replay store is a follow-up.
- The answer waits for the ledger's acknowledgement for at most 250 ms, and never past the caller's deadline.
  A publish still in flight then finishes in the background, so a slow broker neither delays the answer nor
  loses the call.
- If publishing the event fails, the call still answers. The ledger retries an unacknowledged publish three
  times; if all three fail, the engine logs the failure and counts `dpv2_web_rerank_cost_event_failures_total`,
  and the event is not sent again. Any non-zero value of that counter means the ledger is missing a paid call.
- Forwarding to cost-core: the forwarder (`USAGE_FORWARDER_ENABLED`, off by default) posts only rows with
  `schema_version >= 2`. The web route's rows are version 2, so they are posted once the forwarder is on, and
  cost-core accepts their `feature` and `units` only after its ingest change (CR-T4) is deployed. Until then the
  forwarder stays off. Open (section 14).

## 9. Request and response contract

Route: `POST /v1/rerank`, bearer token only.

Request body (strict: an unknown field is refused):

- `request_id`: 8 to 64 characters from `[A-Za-z0-9._:-]`.
- `query`: 1 to 1024 characters, not blank. A longer query is refused with `query_invalid`.
- `profile`: `web-v1`.
- `zdr_mode`: **required**; `disabled`, `reject` or `ephemeral`. Never defaulted. The value `off` is refused
  with `zdr_mode_invalid`.
- `sovereign_required`: optional. Absent means **true** (strict). A call with `sovereign_required` true is not run;
  its reason is `sovereign` unless an earlier check in the order under `skipped` applies, because the route cannot show that the Cohere deployment is sovereign (its
  region is unverified, section 14). A signed `sovereign` claim floors it to true.
- `top_n`: optional, from 1 to the number of documents. Absent means all.
- `deadline_ms`: optional, from 100 to 8000, default 2000.
- `documents`: 1 to 100 items of `{ id, text }`. An `id` is 1 to 256 bytes, unique, with no control
  characters. A `text` is not blank and is cut at 4000 characters, never mid-character; a cut id is listed
  in `truncated_ids`.
- The body is limited to 1 MiB (413).

Response: HTTP 200 for every outcome, with `{ request_id, status, reason, profile, zdr_actions_applied,
truncated_ids, results }`.

- `status: ok`: scored and ranked best first (ties go to the lower input index), cut to `top_n`. `reason` is
  null.
- `status: skipped`: no provider call. Results are in input order and unscored. `reason` is one of
  `disabled`, `misconfigured`, `processor_unapproved`, `metering_unavailable`, `zdr`, `sovereign`,
  `circuit_open`. The checks run in that order, and the first failing one decides.
- `status: degraded`: the provider failed. Results are in input order and unscored. `reason` is `timeout`,
  `provider_rate_limited` or `provider_error`.

Errors (never carrying query or document text): 400 with one fixed code (`invalid_request`,
`zdr_mode_required`, `zdr_mode_invalid`, `profile_unsupported`, `request_id_invalid`, `query_invalid`,
`documents_invalid`, `document_id_invalid`, `document_id_duplicate`, `document_text_invalid`,
`top_n_invalid`, `deadline_invalid`); 401 without a token; 403 for a user token (`service_only_route`) or a
service refusal from the route table; 429 `rate_limited`.

## 10. Metrics and what to watch

- `dpv2_web_rerank_total{outcome, reason}`: the labels are bounded (`outcome` is ok, skipped or degraded;
  `reason` is a closed list).
- `dpv2_web_rerank_duration_seconds`, `dpv2_web_rerank_documents`, `dpv2_web_rerank_search_units_total`.
- `dpv2_web_rerank_budget_exceeded_total{org_id}`: the alert. The org label is the same practice as the
  rate-limit counter.
- `dpv2_web_rerank_cost_event_failures_total`: must stay at zero (section 8).
- `dpv2_rerank_breaker_state`, `dpv2_rerank_breaker_opened_total`, `dpv2_rerank_provider_refused_total`,
  `dpv2_rerank_provider_calls_in_flight`: the provider gate.
- `dpv2_authz_denials_total`, `dpv2_rate_limit_denied_total`: refusals of callers and of the rate.

## 11. Verification that runs without a live call

- `bash tests/scripts/web-rerank-compose-contract-test.sh` and `--self-test`: the compose defaults equal the
  code defaults, no public Cohere fallback, no attestation switch, the OpenAPI, this runbook, the status
  record, the line endings, and the pre-flight's offline checks.
- `bash tests/scripts/auth-regression-matrix-test.sh`: the matrix encodes the service rows and their
  expected statuses (dry run; no HTTP); the isolated MVP runner hands the matrix its service bearer; and the
  fixture's service token verifies under the fixture key, with the quarry-edge claims and exactly the scope asked for.
- `bash tests/scripts/isolated-registry-parity-test.sh`: the isolated stack's quarry-edge row equals the
  committed row.
- `bash scripts/qb-preflight.sh --offline` and `--self-test`: the registry, the compose and the route agree.
- Rust, in CI's workspace test step: `rerank_contract`, `rerank_provider_gate`, `service_scope`,
  `retrieval_start_log_redaction`.

## 12. Live proof (orchestrator; not run here)

1. **Pre-flight (read-only):** `bash scripts/qb-preflight.sh --live`. Pass: the image answers 401 on the
   route without a token; the switch states are printed; the quarry-edge credential variable is set (the
   value is never printed); every migration file under `infra/postgres/migrations` is recorded in
   `schema_migrations`; the baseline counters are recorded.
2. **Matrix:** run `bash tests/e2e/run-isolated-mvp.sh`. It starts an isolated Compose project, mints its bearers
   with the fixture (`tests/e2e/isolated/fixture.mjs`), runs `scripts/auth-regression-matrix.sh` against that
   project's retrieval-engine, and tears the project down. To run the matrix by hand instead, set
   `DPV2_MATRIX_TRANSPORT=compose`, `DPV2_COMPOSE_PROJECT=<isolated project>`, `DPV2_TEST_ORG_ID`,
   `DPV2_SPOOF_ORG_ID`, `DPV2_USER_BEARER`, `DPV2_SERVICE_BEARER`, `DPV2_QUALITY_BEARER` and
   `DPV2_QUICKWIT_ADMIN_BEARER`. `scripts/smoke-test.sh --with-auth-matrix` needs the same bearers.

   The service bearer is a `service:quarry-edge` token with `data:read` only. The fixture signs it with the key
   the isolated data plane trusts (`mint-service`), so the scope check and the route table under test are the
   data plane's own. **This is not an Auth Core mint.** The isolated real-authority stack has Auth Core but no
   retrieval-engine and no org-core, so a directory-bound mint cannot be exercised there (section 14). The paid call in step 3 needs a bearer whose
   `zdr` claim is false: a signed `zdr` claim skips every rerank call with reason `zdr`. The fixture reads
   `JWT_ZDR` for `mint-service` (default `true`, as for the user bearers), so mint the step 3 bearer with `JWT_ZDR=false`.

   Pass: the matrix section prints `Results: 33 passed, 0 failed`; the five service rows print PASS with statuses
   401 (no token on the route), 403 (user token on the route), 403 (service without the scope), 403 (service on
   the admin route) and 200 (service on `/v1/retrieve`); and the runner ends with its own PASS line. No token or
   org id is printed. The runner was not run for this record; the orchestrator runs it.
3. **One paid call (owner's go-ahead):** a single `/v1/rerank` request with the `zdr`-false bearer of step 2, `zdr_mode` `disabled`,
   `sovereign_required` set to `false` explicitly (the owner must accept a non-sovereign deployment before the
   route is switched on, section 14), and a fresh `request_id`. Pass: `status` `ok` with Cohere scores. A retry
   of the same request makes no second provider call. A `zdr_mode` `ephemeral` request answers `skipped` with
   reason `zdr`, in input order, with zero provider calls. Count the provider calls made: at most two.

## 13. Post-proof step (owner-gated, orchestrator)

After the live proof passes and the owner has signed the processor gate and named the quota owner, the
defaults change: `WEB_RERANK_ENABLED=1` in the launcher environment, with `RERANK_PROVIDER_MAX_INFLIGHT`
and `RERANK_PROVIDER_RPS` above the route's minimums (section 3), and `USAGE_EMIT_ENABLED=1` once the
metering consumer is verified (section 8). Then recreate retrieval-engine. The
numbers are the owner's to choose; the in-flight limit also bounds tenant calls.

## 14. Open items

- **Price sheet** for `Cohere-rerank-v4.0-pro` in cost-core. Unowned.
- **Quota owner** for the shared Foundry deployment and its S0 limits. Unowned. A dedicated deployment or an
  owned quota is a precondition for default-on.
- **Processor gate:** counsel's Art. 28 confirmation before `WEB_RERANK_PROCESSOR_APPROVED=true`.
- **Region** of the Cohere deployment: unverified. Until it is verified, a strict call (`sovereign_required` true) is
  always skipped, and the owner must accept the non-sovereign deployment before the route is switched on.
- **Quarry (QB-T09):** carry `allow_third_party_processing` onto the search path; confine the vector leg to
  `source = 'quarry'`; emit scores only after model-gateway calibration (QB-T12).
- **Attestation:** Z-41 (shared module) and the provider attestation record (Z-40, operator step).
- **Replay:** a shared replay store for idempotency across replicas. Follow-up.
- **Cost events:** a retry path beyond the ledger's three publish attempts. Follow-up; today a failure that
  survives them is counted and logged.
- **Cost reach:** the web route's event is a version 2 usage row (section 8). It reaches cost-core once the
  forwarder is on, cost-core's ingest of `feature` and `units` (CR-T4) is deployed, and the
  `Cohere-rerank-v4.0-pro` price row exists. Open.
- **Real Auth Core mint of the quarry-edge token:** not exercised by any run in this repo. The isolated
  real-authority stack (`run-isolated-real-authority.sh`) has Auth Core and the generated registry row, but no
  retrieval-engine and no org-core. Its directory-bound mint has no directory to read, so the code's rule answers
  503 `organization_directory_unavailable` there (inferred from the code; not run). The isolated MVP stack signs
  its service bearer with its own fixture (section 12). A follow-up gives the real-authority stack an org-core and
  a retrieval-engine, mints `data:read` for quarry-edge, and checks that the token carries exactly that scope.
