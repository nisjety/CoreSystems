# Control shared-NATS scoped credential rollout

Status: implementation and focused broker tests are complete; production
execution is not claimed. This runbook describes the bounded migration from the
legacy token-only Verevon broker to `control-shared-nats` without interrupting
non-Control consumers.

Final validation evidence: the non-printing preflight accepts three valid rollout profiles and rejects 20 fail-closed cases while validating 58 pairwise-distinct credential values and 10 bounded credential/registry/key/TLS files. Auth→User TLS certificates and CA must be coordinated before the Auth client and User server rollout. No real secret-manager value was generated, read, printed, rotated, or deployed in this workspace.

## Safety invariants

- `control-shared-nats` accepts distinct user/password principals. NATS does not
  permit a single account configuration to combine a server token with a users
  array, so migration uses two brokers plus a compatibility bridge.
- Auth→User gRPC certificates and CA are a separate coordinated rollout: the
  User server requires TLS 1.3 credentials, Auth pins the CA, and plaintext
  channels are rejected. File-backed Compose secrets are copied by a root-only
  handoff into app-owned `0600` files before either service runs as `appuser`.
- Only `control-shared-legacy-bridge` receives `VEREVON_NATS_TOKEN`. Auth, User,
  Org, Billing, and Session use scoped user/password credentials and set token
  fallback to `0` in the release Compose path.
- Documents API consumes GDPR erasure children only as `documents-api-gdpr`.
  The principal can bind/ACK one fixed durable, publish one DLQ and the bounded
  ownership receipt, and cannot publish an erasure request or administer
  JetStream. `DOCUMENTS_GDPR_NATS_PASSWORD` must be injected with the same
  secret version into the Control broker and Data workload, while remaining
  distinct from every other credential.
- `audit-nats-provisioner` owns required local Control/shared topology. Runtime
  services start only after it exits successfully. The independently retrying
  `audit-extra-nats-provisioner` owns Model/Application topology but never gates
  Control startup; Audit reports an unavailable extra plane as degraded.
- Model and Application ordinary runtime principals have only their exact data,
  fixed-consumer, and private inbox subjects. Application's Convex and Insight
  Model consumers use separate scoped principals; the Model plane-wide token is
  not a release credential.
- The bridge binds the pre-provisioned `control-shared-legacy-bridge` durable,
  forwards durable domain/session subjects through legacy JetStream, preserves
  an existing `Nats-Msg-Id`, or derives
  `control-shared:<stream>:<sequence>`. It ACKs the source only after a valid
  target PubAck, so durable redelivery is duplicate-safe on the legacy stream.
  `notifications.*` remains core NATS by contract and is source-ACKed only after
  a successful legacy connection flush.
- `verevon.gdpr.*` is scoped-only and is ACKed without forwarding by the
  compatibility bridge; deletion/security evidence must never cross to the
  legacy shared-token broker.
- No step deletes, purges, or replaces an incompatible stream or consumer.
- Auth gRPC, Auth internal HTTP/NATS, User gRPC, Auth→User, and User→Auth use exact file-backed credential tuples. Registry files may contain explicitly named old/new principals during a bounded rotation; client files contain exactly one selected tuple. Wildcards, ambiguous IDs/tokens, placeholders, wrong audiences, and unregistered methods/scopes fail closed.
- The Auth RSA private/public pair is a single secret version. Auth mounts both files; User mounts that same public-key secret and an explicit `AUTH_CORE_ISSUER`. Never rotate User to a public key or issuer that Auth is not currently issuing.

## Non-printing preflight

Inject the release secrets, then run:

```bash
./scripts/validate-release-credentials.sh
```

The command rejects missing, shorter-than-32-character, placeholder-marked, or
reused NATS and HTTP service-principal credentials. It prints variable names
only, never values or fingerprints. During the compatibility window the legacy
bridge credential is required. After the bridge is formally retired, use
`REQUIRE_LEGACY_BRIDGE=0`.

The same preflight parses `AUTH_GRPC_SERVICE_CREDENTIALS_FILE`, `AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE`, `USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE`, `USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE`, `USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE`, and the Convex RSA private/public files. It verifies required current tuples, client/server token equality, RSA type/strength/pair matching, and cross-file reuse constraints without printing their contents. Production Compose separately requires the canonical `AUTH_CORE_ISSUER`.

Also verify that the exact legacy-token scan has only the bridge wiring and the
root environment template:

```bash
rg -n "VEREVON_NATS_TOKEN" \
  auth-core user-core org-core billing-core session-core audit-core \
  docker-compose*.yml .env.example
```

## Ordered rollout

1. Required broker and provisioner: start `control-shared-nats`, then run the
   local-only `audit-nats-provisioner`. Require healthy broker monitoring and
   successful creation/convergence of `AQENCIA_CONTROLPLANE`, local Control
   streams, and fixed durables. Stop if it reports an incompatible resource.
   It must not connect to Model or Application.
2. Extra-plane topology and credentials: add the Model/Application runtime,
   Audit, provisioner, `application-convex-model`, and
   `application-insight-model` principals to their brokers. Start
   `audit-extra-nats-provisioner`; a failed Model attempt must not prevent the
   Application attempt (and conversely). Keep the prior Model broker config and
   token secret version available for rollback, but do not wire the token into
   either migrated Application client.
3. Compatibility consumer: start `control-shared-legacy-bridge`. Publish an
   isolated non-destructive fixture through the scoped source and prove legacy
   consumers receive the same payload and stable message ID. Confirm the bridge
   durable has no growing pending/redelivery count.
4. Consumers: verify Audit Core's pre-provisioned Control, Model, and Application
   consumers are ready. Prove Convex receives `mp.v1.run.*.event` with only the
   `application-convex-model` principal and Insight binds only its two fixed
   consumers with `application-insight-model`. Confirm the old Model token is
   rejected by the scoped broker before revoking its stored secret version.
   Then roll Documents API with `documents-api-gdpr`, require its fixed durable
   to bind, and verify `/internal/gdpr/health` before enabling the User producer.
5. Producers: roll Auth, User, Org, Billing, and Session together onto
   `control-shared-nats`. Each must use its named principal and exact inbox
   prefix. A missing credential, failed connection, denied publish, or invalid
   PubAck is a failed rollout; do not mark an outbox row published.
6. Reciprocal Auth/User authority: deploy Auth and User server registry files containing both old and new explicitly named tuples before changing either client file. Prove Auth gRPC missing/wrong/ambiguous/wrong-principal/insufficient-scope denial and User method-level denial with the old clients. Roll Auth→User and User→Auth clients to the new tuples, verify the authenticated lifecycle and audit evidence, and remove old entries only after the observation window is clean. Leave the Auth signing pair unchanged during ordinary credential rotation. A separately authorized signing-key rotation must coordinate Auth's key pair, User's public key, and the exact issuer; the one-key JWKS readiness contract does not permit an uncoordinated swap.
7. Prove convergence: exercise isolated registration/provider-link,
   organization projection, plan revision, retry, reordering, and session
   fixtures. Add a multi-organization erasure fixture; require one stable child
   per snapshotted organization, valid User PubAcks, Documents durable ACKs, and
   zero unaccounted GDPR pending/DLQ rows. Verify source and target stream counts,
   consumer lag/redeliveries, outbox pending/dead-letter counts, and logical
   Audit `event_id` de-duplication.
8. Remove legacy access from Control producers. The release Compose already
   enforces this: only the bridge has the legacy token. Keep the bridge running
   until every non-Control consumer has an independently verified scoped-broker
   migration.
9. Legacy revoke: after consumer owners confirm zero dependency and the bridge
   remains drained for the agreed observation window, revoke the legacy token
   in the broker/secret manager and stop the bridge. Do not delete legacy streams
   or consumers as part of credential revocation.

## Rollback

- Before producer cutover: stop the bridge if necessary and leave all producers
  and consumers on their current paths. Fix the broker/provisioner and rerun the
  idempotent convergence step.
- After producer cutover but before legacy revocation: keep the bridge running.
  Restore the previous scoped broker/principal/config as one coordinated
  producer rollout. If that bounded configuration cannot be restored, pause
  producers and retain their durable outboxes until a new scoped principal is
  provisioned. Runtime producer token fallback is prohibited in release; only
  the compatibility bridge may use the legacy token.
- If a scoped Application-to-Model consumer fails before the old Model token is
  revoked, restore the prior Model broker configuration and its prior token
  secret version as one bounded rollback, then restore both consumers together.
  Never give either consumer `model-runtime`, Audit, or provisioner credentials.
- After legacy revocation: rollback is fail-closed. Do not introduce a literal or
  placeholder credential. Restore the previous secret version through the
  secret manager, rerun the non-printing preflight, restore broker/provisioner,
  then bridge, then consumers, then producers. If the prior credential cannot be
  restored safely, keep publishing disabled and drain durable outboxes after a
  new scoped credential is provisioned.
- For an Auth/User tuple failure, restore both previous registry files first, then restore the previous client files. Keep old entries active throughout rollback. If matching cannot be proven, stop the affected caller and retain durable outboxes rather than widening scope or restoring a shared fleet key.
- For an Auth signing-key or issuer mismatch, restore the previous Auth key pair and the same User public-key/issuer secret version as one coordinated rollback. Do not mark either service ready until Auth JWKS and User delegated-proof checks agree.

## Credential rotation after migration

Provision the next uniquely named principal first, update broker configuration,
and prove its bounded ACL before changing a workload. Roll consumers before
producers, maintain an explicit dual-principal overlap, and revoke the old
principal only after health, PubAck, lag, and outbox checks pass. On any failure,
roll the workload back to the still-valid old principal; never widen the new
principal or reintroduce a shared token to make the rollout pass.
