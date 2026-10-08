# Control Plane config

## `plane-service-principals.json`

The **policy half** of the cross-plane service principal registry: which
audiences each principal may address, the scopes it holds per audience, its
per-audience data-retention posture, and its organization binding.

It contains **no credentials**, and must never contain any. The secret half is
one credential per principal, minted by `scripts/run-control-plane.sh` into this
machine's gitignored `.env.generated-secrets` and assembled together with this
file into `PLANE_SERVICE_PRINCIPALS_JSON`.

### Why the split exists

The registry used to live entirely inside the gitignored store. A fresh checkout
therefore produced **no principals at all**, every cross-plane lane was closed,
and nothing in the repository said what was missing or why — the failures showed
up much later as a 401 from org-core or a 403 from auth-core, several services
away from the cause. Policy is reviewable and belongs in git; credentials are
per-machine and do not.

### Adding or changing a principal

Edit this file, then re-run the plane runner. The generator:

- **never rotates an existing credential.** A credential already present in the
  registry is reused as-is, so a re-run cannot invalidate one the running fleet
  is currently presenting.
- mints a fresh credential only for a principal that has none.
- publishes the credential to the principal's `consumerEnv` when one is
  declared, so the validating end and the presenting end resolve **one value**.
  Independent minting on the two ends is the single most common cause of the
  cross-plane auth failures this repo has seen.
- **reports rather than silently repairs** a registry/consumer mismatch. If the
  two disagree, one end is presenting a credential the other will reject, and
  that is a drift bug to fix at its source.

`consumerEnv` names the variable the generator publishes the credential to. All
nineteen principals declare one, so a fresh checkout resolves every lane from
Control's store alone.

Three of them were briefly assumed unpublishable because their Data Plane
containers all read a variable called `MODEL_PLANE_INFERENCE_SERVICE_API_KEY`.
That is the *container-internal* name; the host-side variable each one
interpolates from is already distinct, and it is the host-side name that belongs
here:

| principal | host-side variable |
| --- | --- |
| `embedding-engine` | `MODEL_PLANE_EMBEDDING_INFERENCE_SERVICE_API_KEY` |
| `graph-index` | `MODEL_PLANE_INFERENCE_SERVICE_API_KEY` |
| `retrieval-engine` | `CONTROL_POLICY_SERVICE_API_KEY` (feeds both of its container variables — one principal, one credential, two audiences) |
| `model-execution` | `MODEL_EXECUTION_SERVICE_API_KEY` (execution-core's container reads it as `INGESTION_SERVICE_API_KEY`) |

The `model-execution` row had the mistake the other way round: it named the
container-internal `INGESTION_SERVICE_API_KEY`, which no host-side file defines, so
a fresh launcher run minted a second credential that nothing presented while the
central bootstrap provisioned the real one under `MODEL_EXECUTION_SERVICE_API_KEY`.
The row now names the host-side variable, and the parity test fails when a
principal's `consumerEnv` is not what its presenting end reads.

`corpus-seeder` is not a service. It is the identity the Data Plane retrieval
evaluation scripts present, and its credential used to be hand-pasted into a
`seed-cred.txt`. It now publishes to `DATA_PLANE_CORPUS_SEEDER_API_KEY`, which
those scripts read directly.

### The plane's own copy still wins

Data Plane v2 keeps its own copies of the three host-side variables in its
`.env`, and `build-verevon-services.sh` loads Control's store *first* — so the
plane's value shadows Control's whenever both exist. That is the established
arrangement for the nine GDPR NATS passwords too, and it is why publishing from
Control is necessary but not by itself sufficient: it makes a fresh checkout
work, and it gives drift something to be measured against, but it does not stop
a stale copy in the plane's `.env` from being the value that actually runs.
`make cross-plane-env` checks those names are non-empty; it does not check they
match this registry.

### Retention markers are a data-retention decision

`retentionByAudience` is not a toggle. A credential minted as `zdr` is refused by
session-core for durable writes, which is correct for content that must not
persist and wrong for records that must — `execution-core`'s `session-core`
entry is `persistent` precisely because approval receipts are durable by nature,
while its `quarry` entry stays `zdr`. Change these with whoever owns retention
policy, not to make an error go away.

The marker is the principal's posture, fixed per audience, not any user's or
turn's. One session-core call therefore does not read it: `model-gateway`'s
`session-core` credential is always `zdr`, and
`ManagedRunLifecycle.CompleteDirectRunWithMessage` (the gateway saving a
direct turn's answer with its receipt) takes retention from the run instead,
which recorded the user's posture when it started, and refuses a zero-retention
run. Every other durable write still refuses a `zdr` credential.

### The `tenant` retention marker

`retentionByAudience` takes exactly three values: `zdr` (the default for an
audience the row does not name), `persistent`, and `tenant`.

`tenant` is for background workers that act for whichever org the work belongs to
(the embedding and graph workers contextualising and extracting org content). The
token's `zdr` claim is then **that org's own ZDR setting**, read from org-core
when the token is minted, not a property of the principal:

- ZDR on for the org (stored intent **and** plan entitlement): `zdr: true`.
  Otherwise `zdr: false`.
- org-core cannot be read and nothing usable is remembered for the org: Auth Core
  answers **503 `retention_posture_unavailable`** and issues **no token**. It never
  guesses either way; the caller retries (the Data Plane marks the work pending).
- A tenant token lives **at most 60 seconds**, so a worker cannot keep acting on a
  `zdr: false` snapshot for long after an org turns ZDR on. With the resolver's
  45 second cache that is about 105 seconds in normal operation.
- The `zdr_source` field of the durable issuance audit event says which it was:
  `tenant-policy` (the org) or `registry` (this file).
- No request body field or header can choose a posture, for any principal.

Rules for editing the rows:

- **Deploy the Auth Core image that understands `tenant` before changing any row
  to it.** An older image rejects the value, which invalidates the whole registry:
  every principal's token request on every plane then fails with a 503.
- **The workers' own clients must accept it first.** The Data Plane inference
  clients compare the token's `zdr` claim with a configured
  `*_INFERENCE_RETENTION_POSTURE` and refuse a mismatch; a row switched to
  `tenant` before they follow the org's value stops those workers.
- The `quarry` audience stays `zdr` for every principal. Quarry reads the run's
  zero-retention mode from the request body, not from a token an org setting could
  change. The policy contract spec fails if a `quarry` row is anything else.
- Exactly the `embedding-engine` and `graph-index` rows on `inference-core` are
  `tenant` (switched by the principal bundle below); the policy contract spec names
  them and fails for any other lane, and fails if either goes back.

### Web tools: `execution-core` on the Quarry audience

`execution-core` holds `search:read` and `extract:read` on the `quarry` audience,
in addition to `browser:execute` and `scrape:write`, so the agent runtime's
`web_search` and `web_fetch` can mint the credentials quarry-edge checks on
`/v1/search` and `/v1/extract`. Without them Auth Core answers 403 and
execution-core's honest `cap.tool.http` probe keeps web search unavailable. The
grant is one principal, one audience, two named scopes, no wildcard; the policy
contract spec fails if either scope appears on another audience or on any
principal beyond `execution-core` (both scopes) and `model-gateway`
(`search:read`, which it already held). The principal is directory-bound with `global` as a
fixed entry (see "Organization binding" below), so the readiness probe (org
`global`) and a run (the run's org) both mint, and its
`quarry` posture stays `zdr`: quarry-edge reads zero-retention from the request
body, not from the token.

### The Data Plane principal bundle

One change set carries every grant the Data Plane work needs. Each scope below is
held by exactly one principal on exactly one audience. The policy contract spec
(`plane-service-principals-policy.contract.spec.ts`, the "principal bundle P1"
blocks) asks the real authorizer for each scope from **every principal on every
audience** and fails if any other pair succeeds, mints the new lanes through the
real token controllers, and pins each new row key for key, so a second audience, a
read scope, delegation or an unchecked organization form added later fails by name.

| principal | audience | scope | retention | credential published to (presented by) |
| --- | --- | --- | --- | --- |
| `quarry-edge` | `data-plane` | `data:rerank`, new, beside `data:read`, `documents:write`, `org:data:read_all` | `persistent` | `QUARRY_SERVICE_API_KEY` (unchanged) |
| `data-orchestrator` (new) | `cost-core` | `cost:write` | `persistent` | `DATA_ORCHESTRATOR_COST_SERVICE_API_KEY` (data-orchestrator) |
| `verevon-gateway-retention-decision` (new, dark) | `control-policy` | `retention:zdr-decide` | `zdr` | `GATEWAY_RETENTION_DECISION_SERVICE_API_KEY` (gateway BFF) |
| `embedding-engine`, `graph-index` | `inference-core` | `inference:invoke`, unchanged | `persistent` to `tenant` | unchanged |

Every row of the bundle is `tenancy: "directory"`: Auth Core checks at every mint
that org-core holds the organization. None carries `allowAnyOrg`, `tokenExchange` or
a wildcard.

**`data:rerank`** is Quarry's web rerank: `POST /v1/rerank` on the Data Plane
retrieval engine (paid Azure Cohere; never the Model Plane, never a subscription
route). Why it cannot be used for more than that:

- the engine's route table (`retrieval-engine-rs/src/authz/service_routes.rs`)
  admits `service:quarry-edge` to `/v1/rerank` only with this scope, marks the route
  service-only (an interactive user is refused before any membership lookup), and
  drops every scope of a service token except `data:read` and `data:rerank` at
  admission, so the org-wide scopes the registry ceiling also lists never reach a
  handler;
- retrieval stays on `data:read`; `data:rerank` alone opens no retrieval route and
  `data:read` alone opens no rerank route;
- `planeScopesForRole` can never return it, so no user token carries it;
- Quarry's manifest row `web-rerank` requests it alone, on `data-plane`.

(The route table is in the retrieval engine's working tree; the `/v1/rerank` handler
itself is a later Data Plane task, so until it ships this scope opens a door with
nothing behind it.)

quarry-edge gets **no `model-gateway` audience** for this: rerank no longer uses the
Model Plane. The spec pins the manifest's rerank row (see below), not a blanket
refusal of `models:invoke`, so the separate decision on Quarry's other Model Plane
callers (autoprompt, answer synthesis, ai-formats, page titles) is not blocked: a
grant there flips `granted` in Quarry's manifest in the same change, and the spec
holds the two together.

Quarry's `retrieve returned status 403` is **not** a missing scope or audience.
quarry-edge's token already carried `aud=data-plane`, `service:quarry-edge` and
`data:read`; the retrieval engine pushed that service subject through the *user*
membership lookup, and Auth Core's honest answer for a service subject is
`not_member`. The repair is the engine's principal-aware admission (the route table
above); the registry needed nothing for it.
`data-plane-service-principal.contract.spec.ts` records the real token and the real
`not_member` answer the engine's tests replay.

**`cost:write`** lets `data-orchestrator` forward the Data Plane's metered usage to
cost-core (`POST /api/v1/cost/record`). cost-core's authorizer (`CostAuthorizer`)
lets a service principal with `cost:write` do that one thing; every other cost route
(entries, aggregates, budget checks, Azure actuals) needs `cost:read` or an
organization admin's user token, neither of which this principal can obtain from
here. The write is pinned to the organization in the token (`pinOrganization`), the
token is minted per organization, and `tenancy: "directory"` has Auth Core confirm
the organization exists at every mint. **Residual risk, owned by cost-core:** a
leaked credential can still append rows to any real organization's ledger. cost-core
must therefore keep producer-attributed rows out of the chat budget aggregate and
restrict which `feature` values this producer may write (decision ledger,
"background provider metering").

**`retention:zdr-decide`** is what the gateway BFF will present to Auth Core's
waiver decision endpoint (ZDR task Z-14, not built). **Dark:** the principal and its
scope exist and are pinned, nothing presents the credential and nothing verifies the
scope yet. It is a principal of its own, not `verevon-gateway`: that row carries the
BFF's model credential, and a leaked model credential must not become a decision
credential or the reverse. The one endpoint that verifies `aud=control-policy`
tokens today, Auth Core's Data Plane authorization decision, checks the caller's
subject (`service:retrieval-engine`) **and** the scope `data:authorization:decide`;
the spec mints this principal's real token and shows that endpoint refusing it with
403, and shows the retrieval engine's own token getting past the same door. The Z-14
endpoint must check subject and scope the same way. `retrieval-engine` is the only
other principal on `control-policy` and holds `data:authorization:decide` alone.

**Vendored Quarry manifest.** Quarry lists every Auth Core token request it makes in
`Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json` and says
for each whether the registry grants it (`granted`). A byte-for-byte copy, pinned by
the sha256 beside it, lives in `auth-core/src/auth/fixtures/`; the policy contract
spec runs the real authorizer over every row (granted rows authorized, the others
refused) and, in a full checkout, compares the copy with the original (a missing
original fails under `REQUIRE_CONTRACT` or `CI`). Refresh both copies and the pins
in the change that edits a row, then `scripts/tests/zdr-contract-sync.test.sh`.

**Deploy order, and what breaks otherwise.** The registry is read when Auth Core
starts, so none of this is live until `auth-service` is recreated with the new
`PLANE_SERVICE_PRINCIPALS_JSON`.

1. org-core with `GET /internal/orgs/:orgId/retention-posture` and Auth Core's
   `org:retention:read:any` grant (the org-core block of `docker-compose.yml`), then
   the Auth Core image that understands `tenant` and `tenancy`. A `tenant` row (in
   either profile) or a `directory` row in the production profile, read by an older
   image, is an invalid registry: every principal's token request on every plane
   fails with a 503. With the image but without the org-core route or grant, every
   lane that asks org-core (the two tenant lanes, and every directory-bound lane in
   the production profile or under a `tenant` posture) answers 503 for every
   organization.
2. Run `scripts/run-control-plane.sh config` once and discard its output (it prints
   resolved secrets). The launcher mints the two new credentials from their
   `consumerEnv`, publishes them to Control's store and reassembles the registry,
   reusing every credential that already exists. A launcher dry run before this one
   would see no minted keys.
3. Recreate `auth-service`.
4. Then the presenters: the Data Plane inference clients follow the org's value
   (`*_INFERENCE_RETENTION_POSTURE`), data-orchestrator reads
   `DATA_ORCHESTRATOR_COST_SERVICE_API_KEY`, Quarry's rerank call site, the gateway
   reads `GATEWAY_RETENTION_DECISION_SERVICE_API_KEY`.

Switching the two `tenant` rows before the Data Plane clients follow is safe but not
free: a non-ZDR organization's token carries `zdr: false`, which equals the clients'
`persistent` assertion, so nothing changes for it; a ZDR organization's token carries
`zdr: true`, which a client still configured `persistent` refuses, so that
organization's contextualization and graph work stalls (fail closed) until the client
follows. Rolling back is the reverse: set the two rows to `persistent`, rerun the
launcher, recreate `auth-service`. Removing `data:rerank` makes Quarry's rerank mint
answer 403, whatever its call site then does; removing a new principal makes only its
presenter fail to mint. Neither touches any other principal.

**Release preflight.** `scripts/validate-release-credentials.sh` requires
`DATA_ORCHESTRATOR_COST_SERVICE_API_KEY` and `GATEWAY_RETENTION_DECISION_SERVICE_API_KEY`
(present, 32+ characters, no placeholder marker, not shared with any other
credential), and the policy contract spec fails if a new principal's `consumerEnv` is
missing from that list. The graph-events signing keypair that data-orchestrator will
verify is provisioned by the central bootstrap (`ensure_event_keypair graph`,
`scripts/bootstrap_runtime_environment.sh`), not by this launcher; production keys
come from the secret manager at an absolute path, never `.secrets/event-keys`.

### Mail-to-task principals (dark)

Two principals exist so switching mail-to-task on needs no policy change. Nothing
presents either until `MAIL_TASK_PROVIDER` is set and the pipeline is wired.

| principal | audience | scope | retention | key published to |
| --- | --- | --- | --- | --- |
| `model-gateway-mail-task-bridge` | `session-core` | `session:user-owned-thread-prepare` | `persistent` | `MAIL_TASK_BRIDGE_SERVICE_API_KEY` (model-gateway) |
| `verevon-gateway` | `model-gateway` | `mail-task:prepare-thread` | `zdr` | `MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY` (gateway BFF) |

The bridge is `persistent` because session-core refuses a zero-retention caller
for any non-`:read` scope and preparing a thread is a durable write. The gateway
principal is `zdr` because model-gateway's prepare-thread route reads neither the
token's retention nor its org beyond equality with the request body; the thread is
persisted by the bridge's own credential and a signed Control decision. Neither
holds `integration:write`, a browser grant, or any other session scope.

**`verevon-gateway` here is not user-core's `verevon-gateway`.** user-core keeps
its own inline registry, `USER_CORE_SERVICE_CREDENTIALS` in `docker-compose.yml`,
where a `verevon-gateway` entry (audience `user-core`) is a different credential
with different scopes. The integration worker's credential,
`integration-mail-task-worker` (scope `mail-task:resolve` alone), also lives in
that user-core registry, not in this file; the launcher mints its token as
`USER_CORE_MAIL_TASK_WORKER_TOKEN`. `scripts/check-mail-task-bindings.sh` reports,
without printing any value, which of these bindings are set and fails when one is
half-set.

### Organization binding (`tenancy`): the production principal model

Auth Core's production loader refuses `allowAnyOrg: true` (a leaked workload
credential could then mint a token for a body-selected organization), and until
this file every principal but `corpus-seeder` said exactly that, so the real
principal set could not load in production at all. A row now names its
organizations in exactly these ways:

| form | meaning | where it is valid |
| --- | --- | --- |
| `orgIds: [...]` | a fixed list; nothing is looked up. Only the pseudo-organization `global` (no directory holds it) and the dev-eval corpus organizations belong here | everywhere |
| `tenancy: "directory"` | any organization org-core's directory holds, verified by Auth Core at **every mint**, before anything is signed | everywhere |
| `allowAnyOrg: true` | the legacy "any organization, unchecked" form | development and tests only; the production loader refuses it, also beside `tenancy` |

A fixed entry wins over the directory, so a `directory` row with
`orgIds: ["global"]` (`execution-core`, whose capability-health and web-tool
readiness probes mint for `global`) is bound by the list for `global` and by the
directory for everything else. A row with none of the three is refused when the
registry is assembled.

What the directory check does. `assertServiceOrganizationBinding`
(`auth-core/src/auth/interactive-retention-policy.ts`) asks org-core
`GET /internal/orgs/:orgId/retention-posture` and reads its 404 `organization not
found` as "no such organization": the same narrow `org:retention:read:any` grant
and the same cache, stale rule and failure semantics as the `tenant` retention
posture, so one org-core round trip serves both for a lane that is both. An
organization org-core holds: the mint proceeds. One it does not: **403**
(`Service principal is not authorized`, which tells the caller nothing about which
organizations exist). org-core unreadable with nothing usable remembered: **503
`organization_directory_unavailable`** and no token. A positive answer is cached
like the posture (45 s fresh; 15 min, or 24 h for a ZDR organization, if org-core
is down); a negative one is never cached, because Auth projects new organizations
asynchronously. The durable issuance audit event records `org_binding`
(`fixed`, `directory` or `any`), so unchecked mints can be counted.

What it buys, and what it does not. It stops a token for an organization that does
not exist (a fabricated or mistyped id, a deleted organization). It does **not**
stop a leaked credential from minting for a real organization: that boundary is the
second proof of the delegated-identity design (token exchange from a verified
subject token), which a principal that has a user subject for its work should use
instead of `/internal-token` (`model-execution` already does for `ingestion`).
Background workers that have no user subject (`embedding-engine`, `graph-index`)
have no such proof today; a proof bound to the signed work event is a Data Plane
change, tracked as a follow-up.

Why not the alternatives. A fixed `orgIds` list per principal would need a registry
change and a secret rollout before a new organization's first document could be
embedded, and a list of every organization is "any organization" with an outage
mode. Token exchange alone cannot cover principals that act on events.

Profiles. The launcher and the central bootstrap assemble this file into
`PLANE_SERVICE_PRINCIPALS_JSON` through one shared program,
`scripts/plane-principal-registry.jq`, in one of two profiles chosen by
`PLANE_SERVICE_PRINCIPALS_PROFILE`:

- `development` (the default): a `tenancy: "directory"` row is emitted as the
  legacy `allowAnyOrg: true`. This is what the development stack has always run
  on; nothing about it changes, and an older Auth Core image reads it unchanged.
- `production`: the policy as stated, no `allowAnyOrg` anywhere. A row carrying
  `allowAnyOrg: true` makes the assembly fail rather than emit a registry the
  production loader would refuse as a whole. Use it to render the production file
  from a secret manager's credentials, or locally to run the production binding.

**Deploy order.** The Auth Core image that understands `tenancy` must be running
before a registry in the `production` profile is loaded: an older image reads a
row with `orgIds: []` and no `allowAnyOrg` as an invalid policy and refuses the
whole registry (every principal on every plane then fails with a 503). The
`development` profile has no such constraint. The directory check also needs
org-core's retention-posture route and Auth Core's `org:retention:read:any` grant
(the same prerequisite as the `tenant` posture).

`auth-core/src/auth/plane-service-principals-policy.contract.spec.ts` runs the
production loader over this file with credentials added and authorizes every lane;
`plane-principal-registry-assembly.contract.spec.ts` feeds both profiles through
the real assembler into the real loader.

### One policy, three writers (and a parity test)

The registry exists in three forms: this file, the launcher's assembly
(`scripts/run-control-plane.sh`) and the central bootstrap's
(`scripts/bootstrap_runtime_environment.sh`). The bootstrap used to write a third,
inline registry of its own (14 principals, older scopes, no `tokenExchange`); a
re-bootstrap silently reverted policy, and because the launcher read the
plane's `.env` before its own store it adopted that stale copy's credentials and
wrote them back into the live registry file. Now:

- the policy lives here and nowhere else;
- both writers assemble through `scripts/plane-principal-registry.jq`;
- the launcher reads the registry it built earlier from its own store only, never
  from another file that happens to define the same variable;
- the bootstrap resolves each credential in order (the variable the presenting end
  reads, the launcher's store read-only, the previous registry, a new mint) and
  never rotates one it can see;
- `scripts/tests/principal-registry-parity.test.sh` runs the real launcher and the
  real bootstrap in hermetic sandboxes and fails when either disagrees with this
  file, emits a null, shares a credential, or leaves a credential different from the
  variable its presenting end reads.

### Requires `jq`

The generator assembles JSON with `jq` and **fails loudly** if it is absent
rather than skipping the step — a silently skipped assembly would leave every
cross-plane lane closed while the bring-up still looked successful.
