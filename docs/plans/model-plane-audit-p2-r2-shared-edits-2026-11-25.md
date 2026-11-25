# Audit phase 2 run r2: shared edits still to apply (2026-11-25)

Run r2 (44 items plus related follow-ups, 72 results): 61 PASS, 11 FAIL or no verdict. The agents did not touch shared files (flag-registry, compose, admission golden, protos, principals); their exact instructions are below, one block per item. Items that did not pass are listed at the end with their open points; their files were left uncommitted in the worktree.

## P2-cp-07

Fixer shared_edits:

A. Registry row for apps/Control Plane/flag-registry/flag-registry.json. Insert after the ORG_RETENTION_POLICY_ENABLED row, before ORG_ROLE_CAPABILITIES_ENABLED:
{ "name": "ORG_RETENTION_THREAD_RECEIPT_CHECK", "kind": "switch", "onLiteral": "true", "owner": "control-plane", "source": "compose", "default": "false", "stage": "dark", "purpose": "F-f10-03: on, a thread legal hold asks session-core whether the thread retained anything and is refused legal_hold_nothing_retained (422) when it did not. Until org-core is built with the session-core receipt client (blocked on F-f10-02), the literal true refuses to start org-core. Off, nothing changes.", "declarations": [ { "service": "org-core", "env": "ORG_RETENTION_THREAD_RECEIPT_CHECK", "default": "false" } ], "codePath": "apps/Control Plane/org-core/internal/retention/types.go" }

B. Compose, apps/Control Plane/docker-compose.yml, org-core service env. Add after the line "ORG_RETENTION_POLICY_ENABLED: ${ORG_RETENTION_POLICY_ENABLED:-false}" (about line 659), with 12 spaces of indent:
ORG_RETENTION_THREAD_RECEIPT_CHECK: ${ORG_RETENTION_THREAD_RECEIPT_CHECK:-false}
Safe to apply now: it is off by default, and with it off the gate does nothing.

C. Principal, apps/Control Plane/config/plane-service-principals.json, under the DEC-M4 train. Insert between "orchestrator-core" and "quarry-edge", in the file's multi-line style. Apply with the adapter, or in the DEC-M4 batch:
"org-core": { "tenancy": "directory", "audiences": ["session-core"], "consumerEnv": "ORG_CORE_SESSION_BEARER", "orgIds": [], "retentionByAudience": { "session-core": "persistent" }, "scopes": ["session:retention:read"], "scopesByAudience": { "session-core": ["session:retention:read"] } }
Then run the train checks: principal-registry-parity, zdr-contract-sync and control-train-repin.

D. Session-core credential, docker-compose.yml, SESSION_CORE_SERVICE_CREDENTIALS in the session-core block (about line 850). Apply with the adapter. Add:
{"principal":"org-core","audience":"session-core","token":"${ORG_CORE_SESSION_BEARER:?ORG_CORE_SESSION_BEARER is required}","scopes":["session:retention:read"]}

E. Proto, owned by F-f10-02 (other track; not edited here). On SessionRetentionService in apps/Model Plane/proto/model_plane/v1/sessions.proto, add rpc GetThreadRetention(GetThreadRetentionRequest) returns (GetThreadRetentionResponse). Request: string org_id, string thread_id. Response: bool found, bool nothing_retained. Field numbers are F-f10-02's to choose. Regenerate the Go bindings under apps/Model Plane/go/gen.

F. With the adapter (lead, after E):
- org-core/go.mod: require github.com/triodelab/model-plane/gen/go v0.0.0 and replace github.com/triodelab/model-plane/gen/go => "../../Model Plane/go/gen", as user-core's go.mod does. MVS bumps grpc to v1.80.0 and protobuf to v1.36.11. Refresh go.sum with GOWORK=off.
- org-core/Dockerfile: copy user-core's pattern. Add COPY --from=model-plane-go-gen [".", "/Model Plane/go/gen/"] before go mod download.
- docker-compose.yml, org-core build: additional_contexts: model-plane-go-gen: "../Model Plane/go/gen", as user-core does (about line 429). Without it the org-core image build fails once the replace lands.
- org-core env names the adapter reads, mirroring user-core's SESSION_CORE_RUN_AUTHORITY_* and CONTROL_ALLOW_INSECURE_* (user-core cmd/server/main.go about lines 479-481 and 566-577): ORG_CORE_SESSION_ADDR, ORG_CORE_SESSION_BEARER, ORG_CORE_SESSION_TLS_CA_FILE, ORG_CORE_SESSION_TLS_SERVER_NAME, and ORG_CORE_ALLOW_INSECURE_SESSION_LOOPBACK (literal true or false, development only).
- New file internal/clients/session_thread_retention_grpc.go. The constructor mirrors user-core's NewSessionRunActionAuthorityClient (session_run_action_authority.go lines 46-66): an empty addr or bearer is an error, never nil,nil; a bearer must not carry a scheme; dial with grpc.NewClient; return newSessionThreadRetentionClient(grpcReceiptSource{client: mpv1.NewSessionRetentionServiceClient(conn), bearer: bearer}, conn.Close). The adapter is:
func (s grpcReceiptSource) GetThreadRetention(ctx context.Context, orgID, threadID string) (ThreadReceipt, error) { resp, err := s.client.GetThreadRetention(metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+s.bearer), &mpv1.GetThreadRetentionRequest{OrgId: orgID, ThreadId: threadID}); if err != nil { return ThreadReceipt{}, err }; return ThreadReceipt{Found: resp.GetFound(), NothingRetained: resp.GetNothingRetained()}, nil }
- main.go: replace the call to retentionThreadReceiptGate with wiring. When ThreadReceiptCheckEnabled is true, build the client from env, refuse to start on error, defer Close, and pass the client as the reader to retention.NewService. Keep the reader nil when off. Update cmd/server/retention_thread_receipt_gate_test.go to match.
- No migration number is used. org-core stores nothing new.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED F wiring when the bindings land.
- The proto the adapter needs is already in the tree (apps/Model Plane/proto/model_plane/v1/sessions.proto:2072-2090, field names org_id=1, thread_id=2, found=1, nothing_retained=2), matching the fixer's assumption.
- ZDR: a refused hold writes nothing; no migration; no content or bearer in logs; nothing cached; no session-core table read.

WHAT IS WRONG (shared text, not this fix's code)
The fixer's shared_edits D is wrong and must not be applied. The in-tree P2-cp-07-rel1 compose entry (docker-compose.yml:853-854) is wrong for the same reasons. See SHARED D and RELATED 1-2.

SHARED (corrected; the lead applies these)
A. apps/Control Plane/flag-registry/flag-registry.json, inside the "flags" array, after the ORG_RETENTION_POLICY_ENABLED row, before ORG_ROLE_CAPABILITIES_ENABLED. Apply unchanged (verified: parses, same keys as the neighbour row, name unused):
{ "name": "ORG_RETENTION_THREAD_RECEIPT_CHECK", "kind": "switch", "onLiteral": "true", "owner": "control-plane", "source": "compose", "default": "false", "stage": "dark", "purpose": "F-f10-03: on, a thread legal hold asks session-core whether the thread retained anything and is refused legal_hold_nothing_retained (422) when it did not. Until org-core is built with the session-core receipt client (blocked on F-f10-02), the literal true refuses to start org-core. Off, nothing changes.", "declarations": [ { "service": "org-core", "env": "ORG_RETENTION_THREAD_RECEIPT_CHECK", "default": "false" } ], "codePath": "apps/Control Plane/org-core/internal/retention/types.go" }
Update "purpose" when the adapter lands.

B. apps/Control Plane/docker-compose.yml, org-core environment block, directly after the ORG_RETENTION_POLICY_ENABLED line (about 659), 12-space indent. Apply unchanged:
ORG_RETENTION_THREAD_RECEIPT_CHECK: ${ORG_RETENTION_THREAD_RECEIPT_CHECK:-false}

C. apps/Control Plane/config/plane-service-principals.json, insert the key between "orchestrator-core" (line 427) and "quarry-edge" (line 478), in the file's multi-line style. Apply unchanged (verified: scopes equal the union of scopesByAudience; no org-core key yet; ORG_CORE_SESSION_BEARER unused; no principal holds session:retention:read yet):
"org-core": { "tenancy": "directory", "audiences": ["session-core"], "consumerEnv": "ORG_CORE_SESSION_BEARER", "orgIds": [], "retentionByAudience": { "session-core": "persistent" }, "scopes": ["session:retention:read"], "scopesByAudience": { "session-core": ["session:retention:read"] } }
Then: add a PRINCIPAL-TRAIN.md batch row (row 11, after the in-tree row 10); run principal-registry-parity, zdr-contract-sync and control-train-repin; recreate auth-service (it reads the registry at start). Decide RELATED 3 before this row lands.

D. WITHDRAWN. Do not add an org-core entry to SESSION_CORE_SERVICE_CREDENTIALS. Reasons: (i) that variable is read only by the Control Plane's Go session-core (apps/Control Plane/session-core/internal/http/server.go, loadServiceCredentials), which keeps only sessions:read or sessions:write scopes and does not serve the receipt RPC; (ii) the RPC is served by the Model Plane Rust session-core (apps/Model Plane/deploy/docker-compose.yml session-core block, gRPC port 9091), which accepts only RS256 Auth Core tokens (auth.rs validated_keys and verify_token) and requires principal service:org-core with scope session:retention:read (auth.rs:597-622); (iii) the fixer's `${ORG_CORE_SESSION_BEARER:?...}` form aborts compose parsing before the launcher mints the bearer. The mechanism is the principal row (C) plus the launcher-minted ORG_CORE_SESSION_BEARER on org-core's side only. Restore the HEAD SESSION_CORE_SERVICE_CREDENTIALS block and drop the P2-cp-07 comment above it (docker-compose.yml lines 849-854). Update or delete the untracked internal/clients/session_core_credentials_compose_test.go, which pins the wrong text. It is not in this fix's file list; do not stage it with this fix.

E. Proto: already in the tree (uncommitted, F-f10-02's). Regenerate the Go bindings under apps/Model Plane/go/gen with F-f10-02. Nothing to add in this item.

F. After E, with the adapter (lead):
1. org-core/go.mod: require github.com/triodelab/model-plane/gen/go v0.0.0 and replace github.com/triodelab/model-plane/gen/go => "../../Model Plane/go/gen", as user-core's go.mod does. The gen module declares go 1.25.1, grpc v1.80.0, protobuf v1.36.11; org-core's go line and grpc v1.75.1 will move. Run GOWORK=off go mod tidy and check go.sum.
2. org-core/Dockerfile: after "COPY go.mod go.sum ./" and before the "go mod download" RUN, add user-core's comment and the line COPY --from=model-plane-go-gen [".", "/Model Plane/go/gen/"].
3. docker-compose.yml, org-core build block (after line 597, "args: *build-metadata"): add additional_contexts: (12 spaces) and model-plane-go-gen: "../Model Plane/go/gen" (16 spaces), as user-core lines 429-430.
4. org-core env names the adapter reads: ORG_CORE_SESSION_ADDR (session-core gRPC, port 9091; org-core is on controlplane-net and inter-plane-bus, so confirm that network path), ORG_CORE_SESSION_BEARER, ORG_CORE_SESSION_TLS_CA_FILE, ORG_CORE_SESSION_TLS_SERVER_NAME (optional), ORG_CORE_ALLOW_INSECURE_SESSION_LOOPBACK (literal true or false only; anything else refuses start, as user-core main.go:566-569 does). Set them in org-core/.env (env_file) or in environment, not both: an environment key overrides env_file (docker-compose.yml:620-621). Put ORG_CORE_SESSION_BEARER in environment only if the launcher writes it to the top-level Control Plane .env (the ORG_CORE_IDENTITY_SERVICE_TOKEN pattern).
5. Launcher check: the value minted for ORG_CORE_SESSION_BEARER must be an Auth Core RS256 token for audience session-core. Any other value is refused as unauthenticated, and every thread hold is then placed. Verify with a real token mint, not only a green config check.
6. main.go: replace the retentionThreadReceiptGate call. When ThreadReceiptCheckEnabled: build the client from env, refuse start on error, defer Close, assign to a retention.ThreadRetention variable that stays a true nil when off, and pass it to NewService. Replace TestThreadReceiptSwitchOnRefusesToStartInThisBuild with a wiring test.
7. Adapter file internal/clients/session_thread_retention_grpc.go: the fixer's constructor and adapter, plus a bounded per-call deadline (RELATED 6). Constructor: empty addr or bearer is an error, never nil,nil; reject a bearer beginning "bearer "; grpc.NewClient.

G. Not this item (F-f10-02's): apps/Model Plane/deploy/docker-compose.yml, session-core environment (about line 1245): SESSION_THREAD_RETENTION_READ: ${SESSION_THREAD_RETENTION_READ:-false}. Until it is true the RPC answers unavailable, which the client reads as unknown, so every thread hold is placed and the check is inert.

LEFTOVERS
1. BLOCKED on F-f10-02: the generated binding. The adapter, go.mod, Dockerfile, compose additional_contexts and main.go wiring cannot compile here. With the switch on, org-core refuses to start. Do not set ORG_RETENTION_THREAD_RECEIPT_CHECK=true before the adapter and SHARED G land.
2. Gate decision (accepted): refuse start while on. The lead may change retentionThreadReceiptGate only.
3. Staging: stage hunks, not files. service.go: only the PlaceHold log hunk is this fix (the AuditRefusedErasure block is F-f10-06). main.go: only the gate call and retentionThreadReceiptGate are this fix (the runOrgPurge move and SetErasureRefusalAuditor belong to F-f10-06). retention_handlers_test.go, service_test.go, types.go and the three new files are wholly this fix.
4. Not run: docker, DB harness (the new tests need none), and the train scripts (principal-registry-parity, zdr-contract-sync, control-train-repin). The lead runs those.
5. user-core's run-authority client uses the same TLS-by-default dial (RELATED 4), so that path needs the same check.
6. Disclosure: my greps were limited by file type except two that did not finish. One recursive ripgrep over apps/Control Plane timed out, and one grep over apps and scripts was stopped. Either walk may have opened .env* files internally to match text. No output named a .env file, nothing from one was printed or used, and I wrote nothing into the repo.

## P2-cp-08

Fixer shared_edits:

Migration: apps/Control Plane/org-core/migrations/037_purge_respects_legal_holds.up.sql / .down.sql (number 037; the files already exist in the tree, untracked; the lead only needs to commit them).

Proposed flag-registry row (apps/Control Plane/flag-registry/flag-registry.json, alphabetical position) for the existing but undeclared switch; drop the session-core declaration if the Model Plane compose already carries it under another file:
{
  "name": "ERASURE_HOLD_CHECK",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "control-plane",
  "source": "compose",
  "default": "false",
  "stage": "dark",
  "purpose": "EP-9 D-C1 legal-hold gate on erasure: org-core's hard and soft delete routes refuse a held org (409 erasure_blocked_legal_hold, audited in the retention outbox), the 30-day purge cron skips held orgs (migration 037, receipt held_org_ids, one refusal audit row each), and session-core refuses thread and org erasure under a hold. Off, nothing is read and erasure and purge are byte-for-byte unchanged. Only the literal true turns it on.",
  "declarations": [
    { "service": "org-core", "env": "ERASURE_HOLD_CHECK", "default": "false" },
    { "service": "session-core", "env": "ERASURE_HOLD_CHECK", "default": "false" }
  ],
  "codePath": "apps/Control Plane/org-core/internal/http/gdpr_handlers.go"
}

Compose line, apps/Control Plane/docker-compose.yml, service org-core environment, after the ORG_COMPLIANCE_ROLE_ENABLED line:
            # EP-9 D-C1 / F-f10-06: erasure routes and the 30-day purge respect legal holds; refusals are audited. Off unless the literal true.
            ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}
Plus the matching `ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}` in the session-core environment block of apps/Model Plane/deploy/docker-compose.yml, if it is not declared there yet.

No golden or proto changes.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: (the fixer's text is correct and complete. I checked it against the registry row format, the org-core compose block at line 661, and session-core gdpr.rs, which reads ERASURE_HOLD_CHECK.)
- Migration: apps/Control Plane/org-core/migrations/037_purge_respects_legal_holds.up.sql and .down.sql, number 037. The files already exist untracked, so they only need committing.
- flag-registry.json row, in alphabetical position:
{"name":"ERASURE_HOLD_CHECK","kind":"switch","onLiteral":"true","owner":"control-plane","source":"compose","default":"false","stage":"dark","purpose":"EP-9 D-C1 legal-hold gate on erasure: org-core's hard and soft delete routes refuse a held org (409 erasure_blocked_legal_hold, audited in the retention outbox), the 30-day purge cron skips held orgs (migration 037, receipt held_org_ids, one refusal audit row each), and session-core refuses thread and org erasure under a hold. Off, nothing is read and erasure and purge are byte-for-byte unchanged. Only the literal true turns it on.","declarations":[{"service":"org-core","env":"ERASURE_HOLD_CHECK","default":"false"},{"service":"session-core","env":"ERASURE_HOLD_CHECK","default":"false"}],"codePath":"apps/Control Plane/org-core/internal/http/gdpr_handlers.go"}
- Compose, apps/Control Plane/docker-compose.yml, org-core environment, after the ORG_COMPLIANCE_ROLE_ENABLED line (L661):
            # EP-9 D-C1 / F-f10-06: erasure routes and the 30-day purge respect legal holds; refusals are audited. Off unless the literal true.
            ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}
- Compose, apps/Model Plane/deploy/docker-compose.yml, session-core environment (service at ~L1218): `ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}`. It is not declared in any compose or json file in the tree today.
- No golden or proto changes.

## P2-cp-08-rel1

Fixer shared_edits:

Apply A, B and C together in one commit. The flag-registry tests check each compose declaration against its registry row and the reverse. The scratch verification used exactly these texts. No migration number, golden, proto or admission-golden change is needed.

A. apps/Control Plane/flag-registry/flag-registry.json: insert a new object into the "flags" array in NAME ORDER. Put it after the ENABLE_COMPAT_ADAPTER object and before the EVENT_TRIGGERS_ENABLED object. The array must stay sorted by name (byte order) with no duplicates, or every registry test fails with "flags must be sorted by name". Object text (4-space indent for the braces, as the neighbouring rows):
    {
      "name": "ERASURE_HOLD_CHECK",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "control-plane",
      "source": "compose",
      "default": "false",
      "stage": "dark",
      "purpose": "F-f10-06: org-core's 30-day purge skips a held org and audits each refused erasure; session-core refuses a thread or org erasure under a legal hold, or when holds are unreadable. Off, nothing changes. Only the literal true turns it on.",
      "declarations": [
        { "service": "org-core", "env": "ERASURE_HOLD_CHECK", "default": "false" },
        { "service": "session-core", "env": "ERASURE_HOLD_CHECK", "default": "false" }
      ],
      "codePath": "apps/Control Plane/org-core/internal/http/gdpr_handlers.go"
    },
(The purpose is 235 characters, under the 240 limit. The registry Load validated it in the scratch run.)

B. apps/Control Plane/docker-compose.yml, org-core environment block. Insert directly after the existing line "            ORG_RETENTION_POLICY_ENABLED: ${ORG_RETENTION_POLICY_ENABLED:-false}" (12-space indent; about line 659). Add these two lines at the same indent:
            # F-f10-06 (P2-cp-08): org-core's 30-day purge skips a held org and audits each refused erasure. Off unless the literal true.
            ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}

C. apps/Model Plane/deploy/docker-compose.yml, session-core environment block. Insert directly after the existing line "      SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE: ${SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE:-false}" (6-space indent; about line 1241). Add these two lines at the same indent:
      # F-f10-06 (P2-cp-08): a thread or org erasure asks org-core for legal holds first and refuses while one is active or they are unreadable. Needs ORG_CORE_URL and SESSION_ORG_CORE_SERVICE_TOKEN. Only the literal true.
      ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}

Each anchor line occurs exactly once in its file (asserted in the scratch run). Switch-off behaviour is unchanged: with the variable unset, compose passes "false", org-core's os.Getenv(...) == "true" is false, and session-core's ErasureHoldGate::from_lookup returns Off.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: (the lead applies A, B and C together, then commits them with the new test file)
A. apps/Control Plane/flag-registry/flag-registry.json (LF). Insert between the closing "    }," of ENABLE_COMPAT_ADAPTER and the opening "    {" of EVENT_TRIGGERS_ENABLED. Indentation: braces 4 spaces, fields 6, declaration entries 8.
    {
      \"name\": \"ERASURE_HOLD_CHECK\",
      \"kind\": \"switch\",
      \"onLiteral\": \"true\",
      \"owner\": \"control-plane\",
      \"source\": \"compose\",
      \"default\": \"false\",
      \"stage\": \"dark\",
      \"purpose\": \"F-f10-06: org-core's 30-day purge skips a held org and audits each refused erasure; session-core refuses a thread or org erasure under a legal hold, or when holds are unreadable. Off, nothing changes. Only the literal true turns it on.\",
      \"declarations\": [
        { \"service\": \"org-core\", \"env\": \"ERASURE_HOLD_CHECK\", \"default\": \"false\" },
        { \"service\": \"session-core\", \"env\": \"ERASURE_HOLD_CHECK\", \"default\": \"false\" }
      ],
      \"codePath\": \"apps/Control Plane/org-core/internal/http/gdpr_handlers.go\"
    },
B. apps/Control Plane/docker-compose.yml (LF). Insert after the line "            ORG_RETENTION_POLICY_ENABLED: ${ORG_RETENTION_POLICY_ENABLED:-false}" (12 spaces, line 659), at 12 spaces:
            # F-f10-06 (P2-cp-08): org-core's 30-day purge skips a held org and audits each refused erasure. Off unless the literal true.
            ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}
   This file also carries other agents' hunks, so stage only this insertion.
C. apps/Model Plane/deploy/docker-compose.yml (LF, clean in the tree). Insert after the line "      SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE: ${SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE:-false}" (6 spaces, line 1241), at 6 spaces:
      # F-f10-06 (P2-cp-08): a thread or org erasure asks org-core for legal holds first and refuses while one is active or they are unreadable. Needs ORG_CORE_URL and SESSION_ORG_CORE_SERVICE_TOKEN. Only the literal true.
      ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}
Optional wording, not required by any test: C's comment could also name session-core's credential grant (see R3) and say that both planes must be set together (see R4). Keep the phrase "the literal true" in any comment, because a test checks for it.

Corrections to the fixer's report:
- Deploy note 3 is not accurate for this tree. The scope org:retention-policy:read:any for session-core is in the ORG_CORE_SERVICE_CREDENTIALS default in apps/Control Plane/docker-compose.yml (line 690), on the session-core entry, which is the only entry carrying it. plane-service-principals.json does not mention the scope, although depth register D3-1002 points there. The live credentials value was not checked, because env files are excluded. Confirm it before the switch goes on.
- Depth register D3-1043 can be marked resolved once A to C land. D3-1002 needs its pointer corrected (see R3).
- The org-core purge skip depends on migration 037 (untracked, P2-cp-08 work). org-core runs migrations at start and exits if they fail (cmd/server/main.go:84-85), so the skip is not silently missing. Checked, not a finding.

Leftovers: (1) The real-tree module fails TestSwitchSpellingTablesAgreeWithTheRegistry because of SESSION_THREAD_RETENTION_READ, from an uncommitted session-core edit. That is not part of this fix (see related 2). (2) The new test file is untracked, so the lead must add it with A to C. (3) Process note: my first repo-wide git grep over apps/ did not exclude tracked .env*.example files from its search. It printed no line from any .env file, and I read none afterwards; later searches exclude them. (4) Not run: session-core cargo tests (no Rust changed; the strict gate is read at gdpr.rs 2644-2713) and DB tests (none touched). No docker was run and no .env file was read. The scratch copy was deleted.

## P2-cp-09

Fixer shared_edits:

No registry rows, compose lines, migrations, admission golden, or proto changes are needed. Do NOT add COST_CORE_API_KEY_BUDGET_CHECK: COST_CORE_ACCEPT_API_KEY already gates the api_key path (flag-registry.json lines 1057-1066; docker-compose.yml lines 564 and 3175, default false). Deploy after merge, per PRINCIPAL-TRAIN's deploy order: recreate auth-service (the registry is read at start), then cost-core and orchestrator-core. No new environment values.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none. The fixer's shared_edits (none) is right and complete for what was built: no registry row, compose line, migration, admission golden or proto change. If the lead wants the separate kill switch, it needs a registry row, a cost-core compose line, and AcceptAPIKey = acceptAPIKey && budget-switch in cost-core cmd/main.go with a test. The fixer did not make that change.

LEFTOVERS: (1) scripts/tests/principal-registry-parity.test.sh was NOT run because it reads .env files. The lead must run it before the principal change lands. (2) zdr-contract-sync.test.sh gave 205 pass and 1 fail. The failure comes from another item's uncommitted edit of apps/Model Plane/api/contracts/agent-action-admission.json and its .sha256 (pin now a6f62f0c...). The GOLDENS row (zdr-contract-sync.test.sh line 215) still holds 386f22f1..., so reconcile it before the train goes green. (3) Deploy after merge: recreate auth-service first (the registry is read at start), then cost-core, user-core and orchestrator-core. No new env values.

RECOMMENDATION (not a fail): add an auth-core case that pins model-gateway's cost:budget:check to cost-core only, and one pinned v2 step digest vector shared by the Go and Rust tests.

## P2-cp-10

Fixer shared_edits:

Unchanged from the verified previous attempt. The lead applies all of them:
A. apps/Control Plane/flag-registry/flag-registry.json: replace the seven audit-core rows (AUDIT_ALLOW_NATS_TOKEN_FALLBACK, AUDIT_CHAIN_ENABLED, AUDIT_CHAIN_PRUNE_ENABLED, AUDIT_CHAIN_PRUNE_DRYRUN, AUDIT_CHAIN_VERIFY_ENABLED, AUDIT_SIEM_EXPORT_ENABLED, AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS) with the previous attempt's rows. Each has onLiteral "true", default "false" and an updated purpose. The verifier applied them to a scratch copy and the pins pass.
B. apps/Control Plane/docker-compose.yml, audit-core and audit-siem-exporter blocks, six replacements:
   - AUDIT_CHAIN_ENABLED: ${AUDIT_CHAIN_ENABLED:-false}
   - AUDIT_CHAIN_PRUNE_ENABLED: ${AUDIT_CHAIN_PRUNE_ENABLED:-false}
   - AUDIT_CHAIN_PRUNE_DRYRUN: ${AUDIT_CHAIN_PRUNE_DRYRUN:-false}
   - the comment line now names the literal true
   - AUDIT_SIEM_EXPORT_ENABLED: ${AUDIT_SIEM_EXPORT_ENABLED:-false}
   - AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS: ${...:-false}
   AUDIT_CHAIN_VERIFY_ENABLED stays as it is.
C. apps/Control Plane/docker-compose.production.yml line ~212, the comment: 'refuse AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS=true.' (cosmetic).
D. apps/Control Plane/audit-core/.env.example (not read here):
   - AUDIT_CHAIN_ENABLED=false
   - AUDIT_CHAIN_PRUNE_ENABLED=false
   - AUDIT_CHAIN_PRUNE_DRYRUN=false
   - empty AUDIT_CHAIN_KEY_ID, AUDIT_CHAIN_PRIVATE_KEY_BASE64 and AUDIT_CHAIN_PUBLIC_KEY_BASE64 lines
   - check any comment in the file that names the literal 1 for these switches.
E. apps/Control Plane/docker-compose.yml, org-core service environment (~line 624): add `ORG_CORE_SESSION_SERVICE_API_KEY: ${ORG_CORE_SESSION_SERVICE_API_KEY:-}` after the USER_CORE_SERVICE_TOKEN line.
F. apps/Control Plane/docker-compose.yml, USER_CORE_SERVICE_CREDENTIALS (~line 478): the org-core scopes become ["users:read:any","spaces:browser-floor:read"].
G. apps/Control Plane/docker-compose.yml, session-core block (~lines 849-854): delete the four P2-cp-07 comment lines and restore the value line to the HEAD text, which equals the constant pinned in org-core/internal/clients/session_core_credentials_compose_test.go. Land it together with the session-core test update named in leftovers.
H. For the P2-cp-07 owner: a flag-registry row and an org-core compose line for ORG_RETENTION_THREAD_RECEIPT_CHECK (default false, literal true), added together.
No migrations, no golden changes, no proto changes.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED below), the commit split, and the runs that need containers.

O-24 (audit-core exact literal true): correct.
- One reader: switches.On (exact "true") is the only reader. A grep of non-test Go finds no remaining "== 1" reader. The former-spelling set (1, on, yes, enabled, true in another case or with spaces) equals mp_contracts::loop_fold::former_spelling.
- Refusals: AUDIT_ALLOW_NATS_TOKEN_FALLBACK, AUDIT_CHAIN_ENABLED, AUDIT_CHAIN_PRUNE_ENABLED, AUDIT_CHAIN_VERIFY_ENABLED, AUDIT_SIEM_EXPORT_ENABLED and AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS refuse boot in production with "<NAME> must be true or false". AUDIT_CHAIN_PRUNE_DRYRUN refuses in every environment. That is stricter than the spec, and I keep it because it is the purge guard. ORG_COMPLIANCE_ROLE_ENABLED only warns, since its reader was already exact. Check logs the name and a bool, never the value.
- Off path: with every switch unset, "false" or "0", Check is silent and every reader is off, as before. Both binaries log.Fatal on a loadConfig error.
- Verifier: blank (spaces included) or the literal true follows the chain. Any other value turns it off, and the chain being off always wins. The table covers "", true, TRUE, True, 1, on, yes, garbage, spaces.
- Tests present: reader tables for parseStrictSwitch, parseChainEnabled, parsePruneMode and the token fallback; a production typed refusal per former spelling; the dry-run refusal in every environment; compose and example pins; registry pins; siem-exporter tables. All green except the two compose-default pins, which wait on shared B and D.

O-27 (policy and text): correct as a request.
- Holders, from the policy: approval:grant_use and orchestration:question:service-origin are execution-core on session-core only. session:retention:read is org-core only. cost:budget:check is model-gateway only. session:thread-owner:resolve is sandbox-manager on session-core. sandbox:thread-workspace:erase:any is session-core on sandbox-manager. verevon-gateway's apiKeyExchange includes cost-core. capability:read is held by browser-broker, execution-core and orchestrator-core on capability-core, as the README says.
- Dormant: each consumer is behind a switch that reads the literal true and defaults off. The switches are APPROVAL_SCOPE_GRANTS_ENABLED, MCP_DO_ELICITATION, MCP_ELICITATION_ENABLED, ORG_RETENTION_THREAD_RECEIPT_CHECK, SKILL_BUNDLE_READ_RULE_ENABLED and BROWSER_SPACE_FLOOR_ENABLED.
- Row 11 claims checked against code: StandingCall::released and grantable_with_switch (zdr and waived refused). Session-core use_approval_scope_grant checks approval:grant_use, requires a service identity, and returns an empty grant_id while the switch is off. The session-core interceptor refuses a call with no bearer, and execution-core's session channel has no interceptor or layer. allow_elicitation_when refuses zdr and waived runs. The question-origin token's only caller is the MCP form path. AuthorizeGRPCWith refuses user principals for GetSkillBlob, and deck_templates.rs reads the manifest and then the theme blob under the caller's bearer. GetThreadRetention checks session:retention:read and binds service:org-core. The org-core flag reads the literal true, and the client mints from ORG_CORE_SESSION_SERVICE_API_KEY. The org-core floor gate is correct, and user-core's floor guard admits only a static service_principal credential. The README's batch-11 paragraph matches the code.
- Principal count: the policy and the launcher both have 24 principals, including org-core. The parity run passes 37 of 37.

Caveat (related item 5): the granted scope does not, on its own, open the MCP form path. session-core's create_run_question authorizes orchestration:write before it checks the origin, and question_origin checks only the principal id. The question-origin token carries only orchestration:question:service-origin, so the call is refused until the W3-EP-4-S7 session-core unit accepts that scope for an MCP origin. The refusal fails closed, and nothing was widened. This is not an O-27 change, but the row must not read as unblocking the unit. Recommended, not blocking, one sentence for PRINCIPAL-TRAIN row 11 request (1): "The grant alone does not open the form path: session-core's create_run_question still authorizes orchestration:write before it checks the origin, and the question-origin token carries only this scope, so the call is refused until the W3-EP-4-S7 session-core unit accepts this scope for an MCP origin."

Text nits (optional, low): (a) runbook "Former spellings" paragraph: add that a misread AUDIT_CHAIN_PRUNE_DRYRUN refuses boot in every environment. (b) PRINCIPAL-TRAIN row 11 request (4): "were removed from plane-service-principals.json" should read "were never added to plane-service-principals.json", since HEAD never had them. (c) audit-core/cmd/server/main.go, the comment above the verifier call (about line 124): "unless AUDIT_CHAIN_VERIFY_ENABLED is false" should say "unless AUDIT_CHAIN_VERIFY_ENABLED is anything but blank or true".

Files the fixer did not list but this fix changes: apps/Control Plane/scripts/validate-release-credentials.sh and its -test.sh. The sibling AUDIT_CHAIN reader is already the exact true in the tree. They belong to the O-24 commit. Without them the release preflight keeps the old "1" rule and disagrees with audit-core.

SHARED (the lead applies these; I edited nothing). The fixer's shared_edits text is not right for A and is incomplete for B. I validated corrected A, B (defaults), E and H in a scratch copy under C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\verify-root (rows in apply-rows2.js, H and E in apply-h-e.js). All 15 registry and compose pins in the subset pass. The scratch run did not apply the B comment lines, which do not affect the tests.

A. apps/Control Plane/flag-registry/flag-registry.json, seven audit rows. Loader rule: purpose must be one trimmed line of at most 240 characters. Pins: the purpose must END with "Only the literal true turns it on." for the first five rows, and with "Only the literal true turns it on; with ENVIRONMENT=production the exporter refuses to start." for AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS.
- AUDIT_ALLOW_NATS_TOKEN_FALLBACK: onLiteral "true", default "false", purpose "Security-relaxing migration switch: audit-core and its extra NATS buses may connect with the shared NATS token instead of a scoped NATS user and password. Off requires the scoped credential. Only the literal true turns it on."
- AUDIT_CHAIN_ENABLED: onLiteral "true", default "false", declaration default "false", purpose "Audit hash chain on audit-core ingest (cmd/server). In production it needs the chain signing keys. Only the literal true turns it on."
- AUDIT_CHAIN_PRUNE_DRYRUN: onLiteral "true", default "false", declaration default "false", purpose "Dry run of the chained audit purge (audit-core): it verifies and logs the plan, deletes no chained row, and wins over AUDIT_CHAIN_PRUNE_ENABLED. A misread value refuses boot. Only the literal true turns it on."
- AUDIT_CHAIN_PRUNE_ENABLED: onLiteral "true", default "false", declaration default "false", purpose "Pruning of audit rows, each batch signed by a chain checkpoint (audit-core cmd/server). AUDIT_CHAIN_PRUNE_DRYRUN=true wins and never deletes. Needs the chain signing keys when on. Only the literal true turns it on."
- AUDIT_CHAIN_VERIFY_ENABLED: KEEP kind "text", default "", and declaration default "". The fixer's "onLiteral true, default false" is wrong here, because compose keeps ${AUDIT_CHAIN_VERIFY_ENABLED:-}. Purpose: "Chain verifier on audit-core (cmd/server, verifier.Enabled). Blank, or the literal true, follows AUDIT_CHAIN_ENABLED; false, and any other value, turns verification off. It is always off with the chain off. Read-only."
- AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS: onLiteral "true", default "false", declaration default "false", keep devOnly true. Purpose: "Dev-only lift of the SIEM address check for private destinations (audit-core cmd/siem-exporter). Only the literal true turns it on; with ENVIRONMENT=production the exporter refuses to start."
- AUDIT_SIEM_EXPORT_ENABLED: onLiteral "true", default "false", declaration default "false", purpose "SIEM export of the audit chain (audit-core cmd/siem-exporter). Off, the process serves metrics and exports nothing. Only the literal true turns it on."

B. apps/Control Plane/docker-compose.yml, audit-core and audit-siem-exporter blocks. A grep of every docker-compose*.yml finds these as the only old defaults.
- Line 991 comment: "only the literal 1 turns chained inserts on" becomes "only the literal true turns chained inserts on".
- Line 994: AUDIT_CHAIN_ENABLED: ${AUDIT_CHAIN_ENABLED:-0} becomes ${AUDIT_CHAIN_ENABLED:-false}.
- Line 1000: AUDIT_CHAIN_PRUNE_ENABLED default 0 becomes false.
- Line 1001: AUDIT_CHAIN_PRUNE_DRYRUN default 0 becomes false.
- Line 1002, AUDIT_CHAIN_VERIFY_ENABLED: ${AUDIT_CHAIN_VERIFY_ENABLED:-} stays.
- Line 1053 comment: "AUDIT_SIEM_EXPORT_ENABLED=0 by default" becomes "=false by default".
- Line 1069: AUDIT_SIEM_EXPORT_ENABLED default 0 becomes false.
- Line 1070: AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS default 0 becomes false.

C. apps/Control Plane/docker-compose.production.yml, line 212: "refuse AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS=1." becomes "=true.".

D. apps/Control Plane/audit-core/.env.example. I did not read it. Set AUDIT_CHAIN_ENABLED=false, AUDIT_CHAIN_PRUNE_ENABLED=false and AUDIT_CHAIN_PRUNE_DRYRUN=false, matched by ^NAME=\r?$. Leave AUDIT_CHAIN_KEY_ID=, AUDIT_CHAIN_PRIVATE_KEY_BASE64= and AUDIT_CHAIN_PUBLIC_KEY_BASE64= empty, matched by ^NAME=\r?$. Check any comment that names the literal 1 for these switches.

E. apps/Control Plane/docker-compose.yml, org-core environment, directly after the USER_CORE_SERVICE_TOKEN line (about line 624), 12-space indent: ORG_CORE_SESSION_SERVICE_API_KEY: ${ORG_CORE_SESSION_SERVICE_API_KEY:-}. Validated in scratch.

F. apps/Control Plane/docker-compose.yml, USER_CORE_SERVICE_CREDENTIALS (about line 477), the org-core entry: scopes ["users:read:any"] becomes ["users:read:any","spaces:browser-floor:read"]. The token stays ${USER_CORE_ORG_TOKEN:?...}, the value org-core sends as USER_CORE_SERVICE_TOKEN. Not covered by a registry test, so it is checked by code reading only.

G. apps/Control Plane/docker-compose.yml, session-core block (about lines 847 to 854, the P2-cp-07 hunk). Delete the four "# P2-cp-07" comment lines. Set the SESSION_CORE_SERVICE_CREDENTIALS value line to the HEAD text: [{"principal":"verevon-gateway","audience":"session-core","token":"${SESSION_CORE_SERVICE_TOKEN:?SESSION_CORE_SERVICE_TOKEN is required}","scopes":["sessions:read","sessions:write"]}]. The org-core test session_core_credentials_compose_test.go pins exactly this value. Land it with the P2-cp-07 session-core test update (related items 1 and 2).

H. ORG_RETENTION_THREAD_RECEIPT_CHECK, owner P2-cp-07, added together. (1) Registry row, inserted after the ORG_RETENTION_POLICY_ENABLED row: {"name":"ORG_RETENTION_THREAD_RECEIPT_CHECK","kind":"switch","onLiteral":"true","owner":"control-plane","source":"compose","default":"false","stage":"dark","purpose":"Thread legal holds ask session-core whether the thread retained anything first (org-core, F-f10-03). Only the literal true turns it on.","declarations":[{"service":"org-core","env":"ORG_RETENTION_THREAD_RECEIPT_CHECK","default":"false"}],"codePath":"apps/Control Plane/org-core/cmd/server/main.go"}. (2) Org-core compose line after the E line: ORG_RETENTION_THREAD_RECEIPT_CHECK: ${ORG_RETENTION_THREAD_RECEIPT_CHECK:-false}.

COMMIT SPLIT.
- O-24 (fix(audit-core)): the O-24 files in "files", plus shared A (the seven audit rows only), B (audit comments and defaults), C and D. Commit the registry and compose with it, because the pins need them and the commit is red without them.
- O-27 (feat(control-plane)): config/plane-service-principals.json, taking only the execution-core hunks (approval:grant_use and orchestration:question:service-origin, flat and session-core) and the org-core block. Leave out the model-gateway cost-core hunk, which is batch 10 (P2-cp-09). config/PRINCIPAL-TRAIN.md, row 11 only, not row 10. config/README.md, whole diff (batch 11). The auth-core policy spec (one line). Shared E and F go here too.
- P2-cp-07 (not this fix): G and H, with the session-core test update.
- Row 10 and row 11 share a file, and so do the batch-10 and O-27 hunks in plane-service-principals.json. Stage by hunk with a filtered patch (git apply --cached).

NOT VERIFIED HERE, OR OPEN.
- .env.example: I did not read it, per the rule. The lead checks the pinned lines and any comment naming the literal 1, then runs TestAuditChainSwitchShipsOff and TestAuditChainPruneSwitchesShipOffAndExampleCarriesNoKey.
- apps/Control Plane/scripts/validate-release-credentials-test.sh: not run, because its line 41 greps .env.example. The lead runs it after D.
- Containers: auth-service recreate; scripts/run-control-plane.sh config, which writes ORG_CORE_SESSION_SERVICE_API_KEY; a live mint of session:retention:read (plan 4.2).
- git add of the two untracked files under apps/Control Plane/audit-core/internal/switches/.
- Other red tests that are not this item: the ERASURE_HOLD_CHECK row (P2-cp-08), the SESSION_THREAD_RETENTION_READ row (session-core), and the agent-action-admission digest.
- Owner decision, related item 5: whether session-core accepts orchestration:question:service-origin for an MCP origin (the least-privilege option) or the token widens. Keep MCP_DO_ELICITATION off until this is decided.
- Owner decision, F-f09-07: model-gateway's read of capability-core. SKILL_BUNDLE_READ_RULE_ENABLED stays off.
- F-f01-01 to F-f01-03: no audit-core switch from them is in the tree. When they land they must use switches.On and switches.Check.

## P2-cp-10-rel1

Fixer shared_edits:

- docker-compose.yml, session-core block: SESSION_CORE_SERVICE_CREDENTIALS is now the HEAD line (verevon-gateway only), with a 3-line comment above it. This also satisfies the P2-cp-07-rel1 pin in org-core/internal/clients. Nobody should re-add an org-core entry to this line. Any pending P2-cp-07 compose edit that adds one must be dropped.
- No registry row, flag-registry row, migration, golden, or proto change from this item. config/plane-service-principals.json is untouched. Its org-core row already has consumerEnv ORG_CORE_SESSION_SERVICE_API_KEY, audience session-core, and scope session:retention:read.
- Pending for the lead: add the org-core compose consumer line (leftover 2). Commit the new untracked session-core test file (leftover 4).

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: docker-compose.yml session-core block: SESSION_CORE_SERVICE_CREDENTIALS equals HEAD (verevon-gateway only), with a 3-line comment above it at lines 849-851. The P2-cp-07 org-core entry is gone. Nobody re-adds an org-core entry to this line, and any pending P2-cp-07 compose edit that adds one must be dropped. This line keeps the P2-cp-07-rel1 pins in apps/Control Plane/org-core/internal/clients/session_core_credentials_compose_test.go green. That file is untracked and owned by P2-cp-07-rel1, so commit it with this line. No registry, flag-registry, migration, golden or proto change comes from this item. config/plane-service-principals.json is untouched; its org-core row already has consumerEnv ORG_CORE_SESSION_SERVICE_API_KEY, audience session-core and scope session:retention:read.
Owed by PRINCIPAL-TRAIN batch 11 row 5, not this item: the org-core compose credential line for ORG_CORE_SESSION_SERVICE_API_KEY; SESSION_CORE_RETENTION_ADDR, which is absent from compose today; the flag-registry row for ORG_RETENTION_THREAD_RECEIPT_CHECK, which the train says does not exist yet; and a release-validator entry if batch 11 wants one (validate-release-credentials.sh does not list the key). If the flag is switched on before those land, org-core refuses to start, so the dark state is safe.
Commit ordering: HEAD has no ORG_CORE_SESSION_BEARER (git grep on HEAD is empty). The stale name exists only in the uncommitted P2-cp-07 hunk. Commit this fix together with P2-cp-07, or this commit has nothing to fix at HEAD.

## P2-cp-11

Fixer shared_edits:

1) apps/Control Plane/flag-registry/flag-registry.json, SPACE_HISTORY_WINDOW_CLAIM row, "purpose" value (line 4485). Current text starts "WS04-S11: a history floor on shared thread-read decisions for since_join rooms. Off, no read decision carries a floor." Replace the whole value with: "WS04-S11 and R-sws-07: a history floor on shared thread-read and thread-revise decisions for since_join rooms. Off, no read or revise decision carries a floor. A presentation rule, never an authorization input. Only the literal true turns it on." No new row. The name, default false, onLiteral, declarations and compose line stay unchanged. No test pins the purpose string.
2) apps/Control Plane/docker-compose.yml, line 499 (comment only): change "shared thread-read decisions" to "shared thread-read and thread-revise decisions". Line 501 (SPACE_HISTORY_WINDOW_CLAIM default false) is unchanged.
3) apps/Control Plane/user-core/cmd/server/main.go, line 425 (comment only): same wording change. Not made by me, because the file has other agents' uncommitted edits.
4) W3-EP-4-S7-a0: no registry row, compose line, migration, golden or proto change (no switch). The execution-core principal JSON hunk and the PRINCIPAL-TRAIN row are batch-11 edits already in the tree. R-sws-07: no compose line, migration, golden, pin or vector change. The floor sits outside ThreadRevisePayloadDigest, so the golden sha256:450775ec... is unchanged in user-core and session-core. No migration number taken.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead applies; corrected from the fixer's list):
1) C:/dev/mp-w345/apps/Control Plane/flag-registry/flag-registry.json, SPACE_HISTORY_WINDOW_CLAIM row, purpose (line 4485). Replace the value with: "WS04-S11 and R-sws-07: a history floor on shared thread-read and thread-revise decisions for since_join rooms. Off, no read or revise decision carries a floor. It only narrows what a member may reach (a read, or a revision target), never grants. Only the literal true turns it on." The fixer's "A presentation rule, never an authorization input" is wrong for revise, because the floor makes a revision target not found.
2) Same file, SESSION_CORE_SPACE_HISTORY_FLOOR row, purpose (line 4132). Missing from the fixer's list. Replace the value with: "WS04-S11 and R-sws-07: honours the signed history_floor_revision claim on transcript reads and message revisions of a since_join room. Off shows full history and lets a revision reach any message, so turn it on together with SPACE_HISTORY_WINDOW_CLAIM. Only the literal true turns it on; it only narrows, never grants."
3) C:/dev/mp-w345/apps/Control Plane/docker-compose.yml, comment lines 499-500 only: "shared thread-read decisions" becomes "shared thread-read and thread-revise decisions". Line 501 is unchanged.
4) C:/dev/mp-w345/apps/Control Plane/user-core/cmd/server/main.go, line 425 comment only: same wording change. The lead applies it, since the file has other agents' edits.
5) No compose line, migration, golden, proto, admission golden, vendored copy or pin change for either unit. The principal JSON and PRINCIPAL-TRAIN hunks are batch 11. Commit them with the approval:grant_use, org-core and cost-core hunks, and with apps/Control Plane/config/README.md (batch 11 doc, which describes the S7-a0 grant).
6) Operator: recreate auth-service after the principal JSON lands (the execution-core grant loads only then). Turn SESSION_CORE_SPACE_HISTORY_FLOOR and SPACE_HISTORY_WINDOW_CLAIM on together. With only the Control switch on, revise stays unfloored, which is HEAD behaviour.

NON-BLOCKING doc drift (fix in the same commit if convenient): history_window.go header lines 12-20 says the floor is on the thread-read decision and never an authorization input; personal_thread_decision.go 123-126 says it is filled only by ResolveSharedThreadReadDecisionEvidence; repository.go 46-47 says no read decision carries a floor; session-core space/audience_window.rs warn text says full history is shown, which is wrong for an unfloored revise.

LEFTOVERS (lead, needs harness, all #[ignore] or TEST_DATABASE_URL-gated): Go TestHistoryFloorOnReviseDecisionLive, TestHistoryFloorFollowsTheAudienceSnapshotsLive, TestHistoryFloorIsAbsentWhenFlagOffOrModeFullLive, TestSharedReviseResolverRefusesArchivedRoomsAndRemovedMembersLive; Rust space::revisions::tests::a_revision_of_a_message_before_the_history_floor_is_not_found_while_the_floor_is_on and orchestration_grpc::tests::plan_gate_pg::execution_core_with_the_narrow_scope_asks_a_server_question (scratch Postgres). Also: scripts/tests/principal-registry-parity.test.sh and zdr-contract-sync after batch 11; the DEC-M4 approval must be recorded before the batch-11 grant goes live; the operator recreate above.

## P2-cp-11-rel1

Fixer shared_edits:

["apps/Control Plane/flag-registry/flag-registry.json line 4485, SPACE_HISTORY_WINDOW_CLAIM purpose: it still reads 'a history floor on shared thread-read decisions'. It should now also name thread-revise and run-control decisions. No test pins this text (git grep found none), and the switch name, default and stage are unchanged.", "apps/Control Plane/docker-compose.yml line 499 (comment) and apps/Control Plane/user-core/cmd/server/main.go line 425 (comment) say the floor is on shared thread-read decisions only. main.go is dirty from another agent, so the lead should update these comments with the commit.", "apps/Control Plane/user-core/internal/spaces/decision.go lines 90-94: the HistoryFloorRevision doc comment should add 'or run-control' (dirty file, lead applies).", "No new switch, compose line, registry row, migration number, golden change or proto change is required for this item. The payload digest and run-control-v1.golden.json are unchanged, since the floor is outside the digest."]

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (for the lead to apply with the commit; none of these files is in the file list):
1. apps/Control Plane/flag-registry/flag-registry.json line 4485, purpose of SPACE_HISTORY_WINDOW_CLAIM: change to "WS04-S11 / R-sws-01 / R-sws-07: a history floor on shared thread-read, thread-revise and run-control decisions for since_join rooms. Off, no such decision carries a floor. A presentation rule that narrows what an authorised read, revise or stop reaches; never a grant. Only the literal true turns it on." No test pins the text; a repo grep finds the phrase only in the registry and the comments listed here.
2. apps/Control Plane/docker-compose.yml lines 499-500 (comment only; keep the env line): "sign a history floor on shared thread-read, thread-revise and run-control decisions for since_join rooms."
3. apps/Control Plane/user-core/cmd/server/main.go lines 425-427 (comment only): same wording as item 2.
4. apps/Control Plane/user-core/internal/spaces/decision.go lines 90-91 (comment only): "is present only on a shared thread-read, thread-revise or run-control decision for a since_join room while SPACE_HISTORY_WINDOW_CLAIM is on:".
5. apps/Control Plane/user-core/internal/spaces/history_window.go lines 12-20 and line 33 (comments): "thread-read" becomes "thread-read, thread-revise or run-control". This file is clean in git status and is optional for this commit; it is not in the file list.
6. No change to plane-service-principals.json, PRINCIPAL-TRAIN.md, run-control-v1.golden.json, migrations, protos or compose env lines.

## P2-cp-12

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED and was validated in a scratch copy.

C-11 (PRINCIPAL-TRAIN batch 12 row): accurate. Scripted policy check: session:goal-judge:read and session:goal-verdict:write on session-core are held only by model-gateway-goal-judge; model:goal_judge on model-gateway is held only by orchestrator-core; both rows are persistent, with no tokenExchange, no allowAnyOrg and no NATS ACL. The wording "verdict rows are content-free" is correct for the code: record_goal_verdict validates reasons and evidence ids and writes NULL (thread_goals.rs ~1491 and ~1530-1542), and verdict_tests.rs 254-255 asserts it (DB-gated, not run). The migration 0093 header says the opposite, which is stale (related, low). Batch 12 is right because 9-11 are taken. The flags are default off in compose and the registry, except SESSION_CORE_THREAD_GOALS_ENABLED, which is an explicit unregistered exception. Checks: control-train-repin ok (exit 0), auth-core jest 8 suites and 340 tests, user-core ok.

F-f01-04: the capability-core branch returns capability:admin only for owner or admin, only when the strict literal true is set, read per call. Off returns the same two arrays as HEAD. The role comes from resolveCanonicalTokenContext (plane-token.controller.ts 279 and 326), not from the client. Every admin-gated capability-core write handler I read takes its org from principal.OrganizationID or verifiedOrganizationID (org_agent_policy, org_hooks, permission_rules, tool_policies, skill_import, plugin_catalog, skill create and update, the bundle handler, MCP register, patch and approve), so the marker does not widen cross-org access. Tests: the scopes spec covers off, on for admin, owner and ' ADMIN ', never for member, viewer or null, the strict literal, and the call-time read; the controller spec covers on for five roles, and the pre-existing "mints least-privilege capability scopes" test pins the off case.

F-f05-11: notificationCoreOrgErasureConsumerConfig has the same shape as the conversation one. Its durable and deliver names match notification-core's binding (notification-core/internal/consumers/org_erasure_consumer.go:38 and :165-167). The map and ProvisionControlSharedRuntime both reach 17. The ACL adds exactly the INFO and ACK publish and the queue-form subscribe, with nothing on the erasure subject for notification-core. main_test drives a real bind to the durable and checks two forgery denials. Drift is refused. gofmt and go vet are clean.

F-f07-07: no code change needed. The model-gateway block (service line 194, anchor line 251) has no RESULT_HANDLES_EXTENDED yet. The Rust gate is !zdr_or_waived plus switch_from (tool_loop.rs 5806-5807; tool_result_handles.rs 87-89).

SHARED (the lead applies these by hand; the verifier did not touch the files):
A1, flag-registry.json: insert this object before the object named PLUGIN_LOADER_ENABLED. Name order holds: PERSONAL_SETUP_VISIBLE, PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE, PLUGIN_LOADER_ENABLED.
    {
      "name": "PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "control-plane",
      "source": "compose",
      "default": "false",
      "stage": "dark",
      "purpose": "F-f01-04: owners and admins also carry capability:admin on the capability-core audience, the admin marker that capability-core reads for org-scope writes. Members and viewers never get it. Only the literal true turns it on.",
      "declarations": [
        { "service": "auth-core", "env": "PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE", "default": "false" }
      ],
      "codePath": "apps/Control Plane/auth-core/src/auth/plane-token-scopes.ts"
    },
A2, flag-registry.json: replace the RESULT_HANDLES_EXTENDED object (currently source "code", no declarations) with:
    {
      "name": "RESULT_HANDLES_EXTENDED",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "purpose": "EP-2-S2: park the complete screened page or search result under a handle that result_query reads back. Never on a ZDR or waived turn, and not for content whose screening was skipped. Only the literal true turns it on.",
      "declarations": [
        { "service": "model-gateway", "env": "RESULT_HANDLES_EXTENDED", "default": "false" }
      ],
      "codePath": "apps/Model Plane/rust/services/model-gateway/src/tool_result_handles.rs"
    },
B, apps/Control Plane/docker-compose.yml, service auth-core: insert after the line "            API_KEY_AGENT_EXCHANGE_ENABLED: ${API_KEY_AGENT_EXCHANGE_ENABLED:-0}" (line 300). Corrected: the first comment line is at 12 spaces, like the rest.
            # F-f01-04: owners and admins also carry capability:admin on the capability-core audience, the
            # admin marker capability-core reads for org-scope writes. Members and viewers never get it. Off
            # keeps the two scopes. Only the literal true turns it on.
            PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE: ${PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE:-false}
C, apps/Model Plane/deploy/docker-compose.yml, service model-gateway: insert after the line "      MODEL_GATEWAY_WEB_READER: ${MODEL_GATEWAY_WEB_READER:-false}" (line 251). Corrected: the comment is at 6 spaces, like the rest of the block.
      # EP-2-S2: park the complete screened page or search result under a handle that result_query reads back. Never on a ZDR or waived turn, and not for content whose screening was skipped. Only the literal true turns it on.
      RESULT_HANDLES_EXTENDED: ${RESULT_HANDLES_EXTENDED:-false}
Validation: the scratch copy with these exact texts gives the same failing tests and messages as the base copy. The 19 compose and catalogue ratchets pass, and both compose files parse with js-yaml, with the two keys resolving to their :-false defaults. After applying, re-run GOWORK=off go test ./... in apps/Control Plane/flag-registry. The real tree fails 5 ratchets from other items' in-flight switches (AUDIT_CHAIN_PRUNE_DRYRUN, the AUDIT_ALLOW_NATS_TOKEN_FALLBACK and AUDIT_CHAIN_ENABLED pins, AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS, the ERASURE_HOLD_CHECK row, the SESSION_THREAD_RETENTION_READ row). Optional: add to A1's purpose the sentence "Turn it on only after capability-core refuses skill writes on ZDR orgs" (see related 1).

FLIP GATE (does not block this commit; the switch stays off by default): capability-core's skill write paths refuse nothing for ZDR. registry_apis.go has no ZDR check on the SkillsHandler create, update and delete paths or on the SkillBundlesHandler put. skill_import.go:139 refuses ZDR for skill_authoring, and zdr-refusals.json lists skill_authoring ("saving or editing skills"). Member-scope writes already pass the same way. The marker lets org admins reach org-scope skill writes, so flipping PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE on exposes ZDR orgs to that gap. The spec's premise ("capability-core still refuses durable mutations for ZDR in its handlers") does not hold for skills. Before the owner turns the switch on, add the refusal in capability-core (another track) or accept the gap in writing.

LEFTOVERS for the lead:
1. Stage only this item's hunks. PRINCIPAL-TRAIN.md has rows 10 and 11 from other items. plane-token-scopes.spec.ts has the orchestration:question block from another item. audit-core cmd/server/main_test.go has audit-chain switch hunks from another item. apps/Control Plane/docker-compose.yml has other items' hunks. Put only the B and A1/A2 text into the compose and registry files.
2. Not run by the verifier: scripts/tests/principal-registry-parity.test.sh and scripts/tests/zdr-contract-sync.test.sh (PRINCIPAL-TRAIN acceptance). They run the real launcher and bootstrap in sandboxes, and this item changes neither the policy JSON nor the goldens, so no change is expected. Also not run: the full lint ratchet (over 400 s), the full auth-core pnpm test (CI), and the session-core verdict_tests (DB, ignored).
3. control-train-repin.sh in check mode prints one "ok - ..." line on success. The spec's "expect no output" is wrong; exit 0 is the signal.
4. Lint: plane-token.controller.spec.ts has 14 eslint errors at HEAD (12 prettier/prettier, 2 no-unsafe-assignment). eslint-suppressions.json is empty, so pnpm run lint (a CI step) reports them at HEAD too. The new block (lines 373-428) is clean. Lead decision: format the file or leave it.
5. audit-core full cmd/server: TestAuditChainSwitchShipsOff and TestAuditChainPruneSwitchesShipOffAndExampleCarriesNoKey fail until the audit-chain item's compose defaults (AUDIT_CHAIN_ENABLED and AUDIT_CHAIN_PRUNE_ENABLED at :-false) land. Not this item. The focused broker tests pass.
6. Batch number: the row is 12, because the spec's 9 was taken and 10 and 11 are other items. It is appended, status open.
7. Process note: several broad searches (Grep and grep -r over apps/Model Plane/deploy, apps/Control Plane and apps/Application Plane) also covered .env.example files in those trees. No output came from any env file, none was used, and no env file was opened. Recorded here for the audit trail.

## P2-cp-12-rel1

Fixer shared_edits:

No registry rows, compose lines, migration numbers, golden or proto changes are needed.

Verification log: the fixer did not edit the shared log. Proposed row for docs/plans/model-plane-verification-log-2026-11-12.md, for the lead to apply after commit:
P2-cp-12-rel1 | apps/Control Plane/auth-core/src/auth/plane-token.controller.spec.ts | 14 eslint errors at HEAD (12 prettier/prettier, 2 no-unsafe-assignment) are now 0: eslint --max-warnings 0 exit 0; prettier --check exit 0; jest 53/53 including the pin test "keeps this spec free of prettier findings (lint pin, P2-cp-12-rel1)"; tsc --noEmit -p tsconfig.json exit 0 with 0 TS error lines. The pin covers the 12 prettier findings only, by running the repo prettier 3.6.2 CLI, because the in-process prettier import fails under jest's default runtime. The 2 no-unsafe-assignment findings are pinned only by the zero-baseline ratchet (scripts/lint-ratchet.mjs). No production code, assertion, switch or suppression changed; switch-off behaviour is unchanged.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: No registry rows, compose lines, migration numbers, golden or proto changes are needed. The fixer's text is right and complete. A git grep for plane-token.controller.spec finds no registry, golden or compose reference (only .plan-work notes), and the fix adds no runtime behaviour.

Nit (not blocking): the test title embeds a tracker id. Shorten it to "keeps this spec free of prettier findings" if preferred.

Disclosure: a few broad git grep searches ran over apps/ and the tracked tree, with output truncated to 10 to 40 lines. No .env* path appeared in the printed output, and no .env* file was opened or printed.

## P2-mg-07

Fixer shared_edits:

1) apps/Control Plane/flag-registry/flag-registry.json: add this row directly after the MODEL_GATEWAY_THREAD_EXPORT row (that row's name is at line 3211), in the same multi-line style as its neighbours:
{ "name": "MODEL_GATEWAY_THREAD_EXPORT_USAGE", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f10-13: the thread export's run summaries take their token totals and model from the cost-core usage ledger, read as the viewer. Needs MODEL_GATEWAY_THREAD_EXPORT. Off: the copy is as it was. Only the literal true turns it on.", "declarations": [ { "service": "model-gateway", "env": "MODEL_GATEWAY_THREAD_EXPORT_USAGE", "default": "false" } ] }

2) apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block (service header at line 194), directly after line 544 (MODEL_GATEWAY_THREAD_EXPORT), add these two lines with the same 6-space indent:
      # F-f10-13: the export's run summaries take their token totals and model from the cost-core ledger, read as the viewer. Needs MODEL_GATEWAY_THREAD_EXPORT. Only the literal true.
      MODEL_GATEWAY_THREAD_EXPORT_USAGE: ${MODEL_GATEWAY_THREAD_EXPORT_USAGE:-false}
COST_CORE_URL is already in model-gateway's environment (line 942), so no other compose change is needed.

3) No migration number, no golden or vendored admission file, no .proto or generated-code change, and no openapi change.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (for the lead; copied and checked):
(1) apps/Control Plane/flag-registry/flag-registry.json: insert into the "flags" array directly after the MODEL_GATEWAY_THREAD_EXPORT row (index 256 today; the next row is MODEL_GATEWAY_THREAD_GOALS_ENABLED). Row: { "name": "MODEL_GATEWAY_THREAD_EXPORT_USAGE", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f10-13: the thread export's run summaries take their token totals and model from the cost-core usage ledger, read as the viewer. Needs MODEL_GATEWAY_THREAD_EXPORT. Off: the copy is as it was. Only the literal true turns it on.", "declarations": [ { "service": "model-gateway", "env": "MODEL_GATEWAY_THREAD_EXPORT_USAGE", "default": "false" } ] }. Checked: parses, same keys as the neighbour, no duplicate name after insert.
(2) apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block (service at line 194; COST_CORE_URL at line 942 is in the same block), directly after line 544, same 6-space indent:
      # F-f10-13: the export's run summaries take their token totals and model from the cost-core ledger, read as the viewer. Needs MODEL_GATEWAY_THREAD_EXPORT. Only the literal true.
      MODEL_GATEWAY_THREAD_EXPORT_USAGE: ${MODEL_GATEWAY_THREAD_EXPORT_USAGE:-false}
This is the only deployment compose file that sets the export switch.
(3) No migration, admission golden, proto, generated code or openapi change is needed.

ENABLEMENT BLOCKERS (other tracks):
(a) The Frontend BFF relay sends no viewer cost bearer. owner_export_request (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/history.rs 789-808) attaches only the model bearer, the session bearer and the user identity. With the switch on and no bearer, every run comes out unknown. Needs an owner decision to add the cost audience to the export relay, then a BFF change and the waiver table.
(b) The SPA replay schema. apps/Frontend Plane/verevonv3/src/features/chat/replay/thread-export-schema.ts runSchema (lines 29-37) requires model as a string and input/output tokens as numbers. With the switch on, unknown runs omit model or send null tokens, so readThreadExport returns invalid, and ExportConversationAction.tsx openReplay (line 74) shows "The export could not be read." Fix: model optional, tokens nullable, render unknown, plus a null-case test. Not in this unit's files.

OWNER CALL
- Run bound. The spec says 50 is the export page size, but session-core's cap is MAX_EXPORT_RUNS = 2000 (thread_export.rs:53). The join covers the newest 50 exported runs, so older runs of longer threads stay null (honest, not zero). Covering all runs means up to 2000 lookups, 8 at a time, 2 s each, about 500 s worst case.

LEFTOVERS
- Not run: the 7 #[ignore] session-core DB tests (need the harness). get_thread_export wiring needs a gRPC fake, so it is compile-checked only.
- Hunk staging for budget.rs. This unit's hunks: the helper (hunk -36,0, lines 37-41), the check_budget call (hunk -120,2, line 125), list_run_entries and its types (hunk -534,0, lines 631-702), and the tests (hunk -1237,0, lines 1631-1733). The check_budget_for_key call at line 459 sits inside hunk -404,2 +452,8, shared with the key-budget change; this unit does not need it. Keep the key-budget, DO_RUN_CAP and per-cap hunks out. session-core: only the doc comment at lines 408-411. thread_export_routes.rs is all this unit's.
- rustfmt: 8 hunks in thread_export_routes.rs (lines 285, 782, 1054, 1134, 1146, 1205, 1249, 1273) are HEAD code. Do not reformat them in this commit.
- Coverage: run_id is optional on USAGE_ENVELOPE (cost-core cmd/main.go:86). sse.rs stamps it at 8285 and 11689, and the agent legs at 17645 and 17726 (behind MODEL_GATEWAY_AGENT_USAGE_ENVELOPE). The gRPC envelope (grpc.rs about 855-880) and the /v1/ai/chat envelope (http_routes.rs about 5636) carry none, so runs whose usage went only through those paths stay null. I did not verify that every thread run's usage passes through a stamped site.
- Housekeeping: I briefly wrote three rustfmt output files into apps/Model Plane/rust and removed them. The tree is as it was. No git writes, no docker, no .env read, no edits.

## P2-mg-07-rel1

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead applies; checked by reading, not compiled):
(a) agent_api_run.rs line 475: publish_usage(&state, &request_id, &org_id, &owner, &infer, latency_ms, &run.run_id).await; run is destructured at 392-402 and only borrowed before 475, and &run.run_id coerces to &str. Correct as written.
(b) agent_api_run.rs 616-637: the 7-parameter publish_usage exactly as the fixer wrote it. Correct: publish_usage_envelope_for_run (grpc.rs:875, pub(crate)) takes (state, request_id, org_id, infer, latency_ms, user_id, zdr, min_privacy_tier, cost_basis, transport, run_id), which matches the call order. USAGE_TRANSPORT is at line 108. Seven parameters is within clippy's default threshold. publish_usage_envelope_via stays in use (web_extraction.rs:500), so nothing becomes dead.
   Conditions: stage only these two hunks (agent_api_run.rs also has uncommitted check_turn_budget hunks from F-f10-23b). Apply (a) and (b) only together with an agent-API test: the Ask row carries payload.run_id equal to the run's id with the switch on, and has no run_id key with it off. Otherwise leave them for a follow-up item, since the spec does not name this site.
(c) flag-registry.json line 3277, purpose: append "Direct invoke rows (gRPC Invoke, HTTP /v1/invoke) and agent Ask-turn rows also name their run." only if (a) and (b) land; otherwise drop "and agent Ask-turn rows". Leave the ticket id out of the registry text. No new env var, so compose and declarations are unchanged.

## P2-mg-08

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: nothing is required. The registry row AGENT_API_BUDGET_CHECK (model-gateway, default "false", onLiteral "true") and the compose line in the model-gateway block, `AGENT_API_BUDGET_CHECK: ${AGENT_API_BUDGET_CHECK:-false}` (line 562), are already in from r1. The model-gateway block also already has AUTH_CORE_URL (879), MODEL_GATEWAY_SERVICE_API_KEY (881, required) and COST_CORE_URL (942). No migration, golden or proto change. The fixer's two optional wording updates are correct and the lead may apply them:
(a) docker-compose.yml, model-gateway block. Replace the comment above COST_CORE_ACCEPT_API_KEY with: "# O-18: lets the key's own cost-core bearer decide a key turn's budget check only when no MODEL_GATEWAY_SERVICE_API_KEY is configured (F-f10-23b: with it, only the gateway's own cost:budget:check credential is used). Must match cost-core's value. Only the literal true."
(b) flag-registry.json, AGENT_API_BUDGET_CHECK purpose: "O-18 / F-f10-23b: Agent API key turns are checked against the org's spend ceilings before the model call, with model-gateway's own aud=cost-core cost:budget:check credential: the key principal apikey:<key_id> and then org-wide. Over budget ends the run with a budget receipt; an unanswerable check fails closed. Off, a key turn is not budget-checked. Only the literal true turns it on."

## P2-mg-09

Fixer shared_edits:

1) Flag registry row. Add to apps/Control Plane/flag-registry/flag-registry.json, next to MODEL_GATEWAY_CONTEXT_REFS_ENABLED:
{
  "name": "MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "stage": "dark",
  "purpose": "F-f09-05 (owner D-f09-4): a Do turn carries its attached document and skill references to the run as RunAgentRequest.context_document_ids / context_skill_ids; execution-core narrows every knowledge_search of the run to the documents (together with any Space narrowing) and injects the attached skills first. Off, a Do turn's document or skill reference is refused typed (kind_not_in_do_mode). Counts only with MODEL_GATEWAY_CONTEXT_REFS_ENABLED on. Only the literal true turns it on.",
  "codeDefault": "false",
  "declarations": [
    { "service": "model-gateway", "env": "MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY", "default": "false" }
  ]
}

2) Compose line. In apps/Model Plane/deploy/docker-compose.yml, model-gateway service environment, directly after the existing `MODEL_GATEWAY_CONTEXT_REFS_ENABLED: ${MODEL_GATEWAY_CONTEXT_REFS_ENABLED:-false}` (line ~534):
      MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY: ${MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY:-false}

3) Proto. The .proto edit is already in the tree: RunAgentRequest gets `repeated string context_document_ids = 30;` and `repeated string context_skill_ids = 31;`, and field 32 is the next free number. The lead must regenerate the Go (go/gen/model_plane/v1/execution.pb.go), Python (mp-events-py _gen execution_pb2.py) and any TS/SDK bindings with the repo buf recipe. The fields are additive only, so buf breaking should pass.

4) No migration, and no change to the admission golden.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: (the fixer's text is correct and complete; copied here for the lead)
1) Flag registry: add to apps/Control Plane/flag-registry/flag-registry.json, directly after MODEL_GATEWAY_CONTEXT_REFS_ENABLED (line ~2567):
{
  "name": "MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "stage": "dark",
  "purpose": "F-f09-05 (owner D-f09-4): a Do turn carries its attached document and skill references to the run as RunAgentRequest.context_document_ids / context_skill_ids; execution-core narrows every knowledge_search of the run to the documents (together with any Space narrowing) and injects the attached skills first. Off, a Do turn's document or skill reference is refused typed (kind_not_in_do_mode). Counts only with MODEL_GATEWAY_CONTEXT_REFS_ENABLED on. Only the literal true turns it on.",
  "codeDefault": "false",
  "declarations": [
    { "service": "model-gateway", "env": "MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY", "default": "false" }
  ]
}
2) Compose: in apps/Model Plane/deploy/docker-compose.yml, model-gateway service environment, directly after line 534 `MODEL_GATEWAY_CONTEXT_REFS_ENABLED: ${MODEL_GATEWAY_CONTEXT_REFS_ENABLED:-false}`, add:
      MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY: ${MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY:-false}
3) Proto: the .proto edit (fields 30 and 31 on RunAgentRequest) is already in the tree. Regenerate the Go (go/gen/model_plane/v1/execution.pb.go), Python (mp-events-py _gen execution_pb2.py) and TS/SDK bindings with the repo buf recipe. The change is additive only, so buf breaking should pass. The Rust side builds from the proto in mp-contracts build.rs and already compiles.
4) No migration and no change to the admission golden.

## P2-mg-09-rel1

Fixer shared_edits:

No registry row, compose line, migration number, switch or proto field was added by this delta, so nothing is owed for the lead's registry or compose files. The only proto change is a comment on context_document_ids in apps/Model Plane/proto/model_plane/v1/execution.proto (line 353, no wire change). Regenerate the bindings only if the gate checks generated-doc drift, since the Go gen still has the old comment. The Go gen gap in execution.pb.go (missing RunAgentRequest context fields, flagged by the verifier) is owed by the base P2-mg-09 work and must land before any build that uses those Go gen types runs with MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY on. Do not enable MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY on any build without this restore: an older executor resumes unnarrowed.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (the fixer's text is mostly right, with corrections):
1. Registry: MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY has no row in apps/Control Plane/flag-registry/flag-registry.json (siblings MODEL_GATEWAY_CONTEXT_REFS_ENABLED ~2557 and MODEL_GATEWAY_DO_RUN_CAP ~2636 have rows), and no model-gateway compose line exists in the tree. Base P2-mg-09 owes both, default "false", literal "true". This delta adds none.
2. Generated bindings: the fixer named only the Go gen. Go apps/Model Plane/go/gen/model_plane/v1/execution.pb.go and Python apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/execution_pb2.py both lack RunAgentRequest fields 30 and 31 entirely. Go also lacks the new comment. Regeneration is owed by base P2-mg-09. The rel1 proto change is a comment only, with no wire change.
3. Rollout order: the fixer says an older executor "resumes unnarrowed". That understates it. An executor without fields 30 and 31 drops them as unknown fields, so every Do turn from its first dispatch searches the whole organisation. Deploy the executor first and keep MODEL_GATEWAY_DO_CONTEXT_REFS_CARRY off until every executor replica runs this build.
4. Migrations: none. Admission golden: not checked.

Leftovers (not caused by this delta): durable_loop_contract has 3 failures, each from one guard violation in an unmodified file. the_inline_chat_loop_has_no_resume_path and every_guard_goes_red both name model-gateway tool_loop.rs ("mentions loop_fold"), and the flip-together test names switch_spellings.rs. These tests panic at their first failing assert, so later asserts did not run: the flip test's no_attestation check and every_guard's remaining checks. This delta adds no consumed_by_gateway text (grep-checked), so the no_attestation check should be unaffected. The lead should confirm the 3 fail at HEAD and re-run after clearing. approval_gate_contract approval_cause_not_persisted_under_zdr fails on a source scan of session-core orchestration_grpc.rs, which another agent has in flight; this delta does not touch session-core.

## P2-mg-10

Fixer shared_edits:

1. Compose: apps/Model Plane/deploy/docker-compose.yml, service model-gateway. Insert after line 274 (MODEL_GATEWAY_DO_RUN_CAP) with 6-space indent:
      # F-f04-16: after a flagged or degraded connected-account read in the Ask loop, the web and network tools are withheld for the rest of that turn, and a call to one is refused as tainted_egress. Needs CONNECTOR_ASK_READS_ENABLED. Off by default; only the literal `true` enables it.
      MODEL_GATEWAY_ASK_READ_TAINT_HOLD: ${MODEL_GATEWAY_ASK_READ_TAINT_HOLD:-false}

2. Registry: apps/Control Plane/flag-registry/flag-registry.json. Add this object to the flags array, next to the MODEL_GATEWAY_DO_RUN_CAP row (about line 2636). Purpose is 216 characters (limit 240).
{
  "name": "MODEL_GATEWAY_ASK_READ_TAINT_HOLD",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "F-f04-16: after a flagged or degraded connector read in Ask, web and MCP tools are withdrawn for the rest of the turn. CONNECTOR_SUGGESTIONS_ENABLED must not go on until this is on. Only the literal true turns it on.",
  "codeDefault": "false",
  "codePath": "apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs",
  "declarations": [
    { "service": "model-gateway", "env": "MODEL_GATEWAY_ASK_READ_TAINT_HOLD", "default": "false" }
  ]
}

3. Optional, not required by any test: apps/Model Plane/rust/services/model-gateway/src/switch_spellings.rs, EXACT_SWITCHES, add (crate::tool_loop::ASK_READ_TAINT_HOLD_ENV, "true"). I did not edit that file.

4. F-f04-11 adds no switch and no registry row; it uses the existing CONTENT_SAFETY_SEMANTIC_INLINE.

5. No migration, no admission golden change, no vendored golden copies, no .proto or generated-code change."

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead adds after the run; corrected from the fixer's shared_edits): (1) apps/Model Plane/deploy/docker-compose.yml, service model-gateway, insert after the MODEL_GATEWAY_DO_RUN_CAP line (currently 274), 6-space indent: "      # F-f04-16: after a flagged or degraded connected-account read in the Ask loop, the web and network tools are withheld for the rest of that turn, and a call to one is refused as tainted_egress. Needs CONNECTOR_ASK_READS_ENABLED. Off by default; only the literal `true` enables it." then "      MODEL_GATEWAY_ASK_READ_TAINT_HOLD: ${MODEL_GATEWAY_ASK_READ_TAINT_HOLD:-false}". (2) apps/Control Plane/flag-registry/flag-registry.json: PLACEMENT CORRECTED. Do not put the row next to MODEL_GATEWAY_DO_RUN_CAP. registry.go:228 requires strictly sorted names, so insert it between the MODEL_GATEWAY_ARTIFACT_REVISIONS object (ends at line 2322 with "    },") and the MODEL_GATEWAY_ASK_USER_ASK_MODE_ENABLED object (starts at line 2323). Row JSON, validated with a JSON parse, purpose 220 characters (limit 240): {"name": "MODEL_GATEWAY_ASK_READ_TAINT_HOLD", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f04-16: after a flagged or degraded connector read in Ask, web and network tools are withdrawn for the rest of the turn. CONNECTOR_SUGGESTIONS_ENABLED must not go on until this is on. Only the literal true turns it on.", "codeDefault": "false", "codePath": "apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs", "declarations": [{ "service": "model-gateway", "env": "MODEL_GATEWAY_ASK_READ_TAINT_HOLD", "default": "false" }]}. The wording "web and network" replaces the fixer's "web and MCP", because the withdrawn set also has fetch and extract. After adding, run GOWORK=off go test ./... in apps/Control Plane/flag-registry; TestLiteralSwitchReadersInSourceAreRegistered will expect this row because the reader is in tool_loop.rs. (3) Optional, not required by any test: switch_spellings.rs EXACT_SWITCHES (crate::tool_loop::ASK_READ_TAINT_HOLD_ENV, "true"). Not edited. (4) F-f04-11 adds no switch, registry row or compose line. (5) No migration, admission golden, vendored golden, proto or generated code. Judgement gaps and open decisions are in related.

## P2-mg-10-rel1

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED), so no new flag is needed.

Re-run here on the tree as of 09:04 (not the fixer's numbers): see tests. Everything passes except two pre-existing scanner tests (related 3), which also fail on HEAD.

CAVEATS FOR THE LEAD:
(a) Stage by hunk. zdr_posture.rs and tests/provider_read_contract.rs are wholly this item. The other three files carry other items' uncommitted work: connector_reads.rs taints_turn and taints_turn_tests (F-f04-16); tool_loop.rs EgressHold / ASK_READ_TAINT_HOLD_ENV and its call sites (~6739-6830); sse.rs many other hunks. Stage only: connector_reads.rs ~100-108 (offered doc) and ~1258-1278 (test); tool_loop.rs ~5208-5229 and ~9673-9699; sse.rs ~3392-3400 (gate) and ~2733-2757 (test).
(b) Residual risk, owner decision (related 1): with the switch off (the default), the Ask loop still selects and reads connected accounts on a waived turn, and parks the result for result_query (related 2). The Do loop refuses the same tool on waived runs with no switch (execution-core runtime_loop/mod.rs:1211-1217), and mcp_credential_ref refuses waived turns unconditionally. The spec's switch-off rule keeps the Ask hole open until the owner approves a switch-off behaviour change.

SHARED: None required in the registries or compose. ZDR_TYPED_REFUSALS_ENABLED already exists: apps/Control Plane/flag-registry/flag-registry.json lines 5364-5375 (kind switch, default false, declared for model-gateway) and apps/Model Plane/deploy/docker-compose.yml line 552 (default false). No new switch, compose line, migration, golden or proto change is needed. Optional, docs only, the lead's call: add connected-app reads (connector_read) on a waived turn to the flag-registry purpose text (~line 5370), to apps/Model Plane/docs/ZDR.md line 153 (feature row) and the 5.9 refusal row at line 534, and state that the Do loop refuses them unconditionally until the owner decides the Ask loop.

## P2-mg-11

Fixer shared_edits:

No change from the verifier's SHARED list, which the lead applies:
A. Five flag-registry.json rows (A1-A5) as given, including the corrected A3 (declarations for frontend VITE_QUESTION_ANSWER_MESSAGE_LINK) and the corrected A5 (codePath apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs).
B. Compose lines:
- apps/Frontend Plane/verevonv3/docker-compose.yml, gateway service, after GATEWAY_CHAT_PROJECTS_ENABLED:
  `      GATEWAY_QUESTION_ANSWER_MESSAGE_LINK: ${GATEWAY_QUESTION_ANSWER_MESSAGE_LINK:-false}`
  `      GATEWAY_AGENT_API_IDEMPOTENCY: ${GATEWAY_AGENT_API_IDEMPOTENCY:-false}`
- Same file, frontend service, after VITE_CHAT_PROJECTS_ENABLED:
  `      VITE_QUESTION_ANSWER_MESSAGE_LINK: ${VITE_QUESTION_ANSWER_MESSAGE_LINK:-false}`
- apps/Model Plane/deploy/docker-compose.yml, model-gateway service, after MODEL_GATEWAY_AGENT_API:
  `      MODEL_GATEWAY_QUESTION_ANSWER_MESSAGE_LINK: ${MODEL_GATEWAY_QUESTION_ANSWER_MESSAGE_LINK:-false}`
  `      MODEL_GATEWAY_AGENT_API_IDEMPOTENCY: ${MODEL_GATEWAY_AGENT_API_IDEMPOTENCY:-false}`
C. Refresh the agent-api-v1.json golden pin through the repo's pin step, to f235ace65c7cd4788ba18a5cd189b64f8969dc6ab9245988e24b5867ecfec8f9. This pass did not touch agent-api-v1.json, so the value is unchanged.
D. No migrations, proto or generated-code changes, or admission golden changes.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED C is applied. The YAML/JSON header parity check (inside spec_paths_and_methods_are_the_golden_routes) passes.
- Not re-run: BFF (verevon-gateway-rs) and SPA tests, since no BFF or SPA file changed this round; the F-f07-01b integration tests.

Owed or open, none blocking:
1. No route-level test drives POST /v1/agent/runs twice (one run; the replay answers without a turn). The seams are pinned: request-id determinism, the dispatch gate, the replay read and the typed read failure. The wiring in start_run is a few lines that read correctly. A route test needs gRPC mocks for model admit, managed start and GetRun, which this crate's harness lacks. Owed as a follow-up.
2. Owner decision: the BFF adds a second switch, GATEWAY_AGENT_API_IDEMPOTENCY. The spec gates the BFF forward on GATEWAY_AGENT_API_ENABLED alone. Keep it (with SHARED A and B) or drop it. Dropping it changes config.rs, domains/agent_api.rs and the PHASE_SWITCHES list in config_zdr_switch_tests.rs.
3. Nothing else owed for F-f07-01b in this round; its hunks are present and unchanged.

SHARED (for the lead to apply; nothing edited by the verifier):
A. apps/Control Plane/flag-registry/flag-registry.json, "flags" array. Insert after the MODEL_GATEWAY_AGENT_API row this object: name MODEL_GATEWAY_AGENT_API_IDEMPOTENCY, kind switch, onLiteral true, owner model-plane, source compose, default false, purpose "F-f08-13: POST /v1/agent/runs honours the caller's Idempotency-Key (hashed, never stored): the same key on the same body answers the run the first start began and starts nothing. Only the literal true turns it on.", declarations [ service model-gateway, env MODEL_GATEWAY_AGENT_API_IDEMPOTENCY, default false ]. Insert after the GATEWAY_AGENT_API_ENABLED row, only while the BFF switch exists: name GATEWAY_AGENT_API_IDEMPOTENCY, kind switch, onLiteral true, owner frontend-plane, source code, default false, purpose "F-f08-13: the public Agent API ingress reads and forwards the caller's one Idempotency-Key, only while GATEWAY_AGENT_API_ENABLED is on. Only the literal true turns it on.", declarations [ service gateway, env GATEWAY_AGENT_API_IDEMPOTENCY, default false ], codePath apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs. Both parse as JSON (checked); neither name is in the registry yet.
B. Compose, one line each, LF, six-space indent, same form as the existing MODEL_GATEWAY_AGENT_API line: the model-gateway name MODEL_GATEWAY_AGENT_API_IDEMPOTENCY with the default-false substitution of its own name, inserted in apps/Model Plane/deploy/docker-compose.yml after the MODEL_GATEWAY_AGENT_API line (currently line 560). Gateway name GATEWAY_AGENT_API_IDEMPOTENCY with the default-false substitution of its own name, inserted in apps/Frontend Plane/verevonv3/docker-compose.yml after GATEWAY_AGENT_API_ENABLED (currently line 243), only while the BFF switch exists. config_zdr_switch_tests.rs requires exactly one such line per switch.
C. Pin: apps/Model Plane/api/contracts/agent-api-v1.json.sha256 must hold f235ace65c7cd4788ba18a5cd189b64f8969dc6ab9245988e24b5867ecfec8f9 for agent-api-v1.json, written by the repo's pin step and not by hand. It holds 0bd0c4974e6be687e548dddfe37c8b9f8d93d72e4946f6e1f3411dae1146dabb now. The contract file is LF, so the test digest equals sha256sum of the file.
D. Nothing else for this unit: no migration, no proto or generated code, no admission golden. The contract JSON, api/agent-api.v1.yaml and docs/ZDR.md already carry this unit's entries.

Staging: agent_api_run.rs also holds F-f10-23b hunks: the run_ask_turn budget branch (HEAD line 427), the check_turn_budget hunks, budget_terminal and the test a_budget_refusal_ends_the_run_with_a_budget_receipt. The F-f08-13 test hunk (diff hunk at new line 1343, 325 lines) is contiguous with that budget test, so split it by hand. src/3150.head is a pre-existing untracked stray in model-gateway/src; do not stage it.

## P2-mg-12

Fixer shared_edits:

Only F-f09-10 needs shared edits; R-reg-02 has none. These are the verifier-validated edits, unchanged.

(A) apps/Control Plane/flag-registry/flag-registry.json: insert this object, with a trailing comma, between the MODEL_GATEWAY_GOAL_SEGMENT_TOPUP object and MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED. Keep LF endings.
{"name": "MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f09-10: a Continue goal turn is settled by its dispatch task as soon as the run answers, so a follower that gave up cannot lose the run's debit. Needs MODEL_GATEWAY_THREAD_GOALS_ENABLED and SESSION_CORE_THREAD_GOALS_ENABLED. Goal turns only. Only the literal true.", "codeDefault": "false", "declarations": [{"service": "model-gateway", "env": "MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH", "default": "false"}]}

(B) apps/Model Plane/deploy/docker-compose.yml, model-gateway service environment block: add these two lines directly after the MODEL_GATEWAY_GOAL_SEGMENT_TOPUP line, with the same 6-space indentation:
      # F-f09-10: a Continue goal turn is settled by its dispatch task when the run answers, not by the follower stream. Only the literal true; off, the follower settles the turn as before.
      MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH: ${MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH:-false}

(A) and (B) go together: catalogue_test.go and TestSwitchSpellingTablesAgreeWithTheRegistry check them.

(C) No migration, golden or proto change.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (corrected; the fixer's (A) text is INVALID as delivered). Its purpose is 292 characters; the registry loader rejects purposes over 240 ("purpose must be a trimmed, non-empty line of at most 240 characters"), which fails Load and breaks nearly every registry test. Corrected (A), inserted in apps/Control Plane/flag-registry/flag-registry.json between the MODEL_GATEWAY_GOAL_SEGMENT_TOPUP object and the MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED object, with a trailing comma, LF endings: {\"name\": \"MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH\", \"kind\": \"switch\", \"onLiteral\": \"true\", \"owner\": \"model-plane\", \"source\": \"compose\", \"default\": \"false\", \"purpose\": \"F-f09-10: a Continue goal turn is settled by its dispatch task when the run answers, not by a follower that gave up. Needs MODEL_GATEWAY_THREAD_GOALS_ENABLED and SESSION_CORE_THREAD_GOALS_ENABLED. Goal turns only. Only the literal true.\", \"codeDefault\": \"false\", \"declarations\": [{\"service\": \"model-gateway\", \"env\": \"MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH\", \"default\": \"false\"}]}
(B) apps/Model Plane/deploy/docker-compose.yml, model-gateway environment block, directly after the MODEL_GATEWAY_GOAL_SEGMENT_TOPUP line (currently line 585), 6-space indent, two lines:
      # F-f09-10: a Continue goal turn is settled by its dispatch task when the run answers, not by the follower stream. Only the literal true; off, the follower settles the turn as before.
      MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH: ${MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH:-false}
Validation: a scratch copy of the Control Plane flag-registry package and Model Plane compose with A (corrected) and B applied, run with GOWORK=off go test ./... against a baseline in the same scratch root with the tree's files restored. The failing-test sets are identical; the only message change is that TestSwitchSpellingTablesAgreeWithTheRegistry no longer reports MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH. Its remaining report, SESSION_THREAD_RETENTION_READ (session-core), is another track's row. Land A and B in the same commit as switch_spellings.rs, because its model-gateway table row names the switch; otherwise that Go test fails. The scratch root lacks the Control Plane service and frontend sources, so codePath tests fail identically with and without A and B. Those failures are scratch artifacts.

Leftovers: (1) The tree's model-gateway lib tests do not compile: session_flow.rs test calls near lines 4201 and 4213 pass 5 arguments to append_prompt_to_fresh_run, which takes 6 (refused_message). That function is new and uncommitted (absent from HEAD), in another track's file, last changed 16:36. Fix it before the lead's model-gateway lib run. (2) The switch_spellings.rs diff also carries the F-f09-10 table row at line 103 (model-gateway only). Stage it with A and B, or split that hunk out. (3) Other tracks' Go failures in flag-registry, not this item: audit-core switches (AUDIT_CHAIN_ENABLED, AUDIT_CHAIN_PRUNE_*, AUDIT_SIEM_*, AUDIT_ALLOW_NATS_TOKEN_FALLBACK), the missing ERASURE_HOLD_CHECK row, and SESSION_THREAD_RETENTION_READ. (4) Scratch to delete: C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/verify-p2mg12 (mp-scratch, goreg, goreg_base), and the target C:/dev/mp-target-w345-fixer, which the fixer's scratch build also used. (5) Needs harness: nothing. Only unit tests ran; no DB-backed tests, no docker. (6) Test nit: the_reader_check_tells_an_exact_read_from_a_lenient_or_wider_one asserts !read_is_exact on "let v = raw.trim();" with id X absent, so it passes for that reason, not because of .trim(). Pin it with an X-bearing statement. (7) Commit body note (F-f09-10): a terminal answer without usage is left to the follower. That is an accepted deviation, and it is necessary: a task settle would block the follower's D2-005 progress tally.

## P2-spa-08

Fixer shared_edits:

Registry rows: none. Compose lines: none. Flag-registry rows: none (no switch on either unit). Migrations: none. Proto or generated-proto changes: none.

1) Commit the new untracked file apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/action-executors.json. It is generated by action-registry.test.ts and include_str!'d by the gateway, so the gateway build fails without it.

2) Admission golden: pre-existing drift at HEAD, not caused by O-26. The generator output differs from the committed golden in exactly two rows:
 a) Insert this row before the chat.queue_invocation_input row (sorted by actionId, '.' sorts before '_'):
 {"actionId": "chat.queue.retract", "registry": true, "access": "none", "authority": "control_run_decision", "sideEffectClass": "context_write", "exemptReason": "no_owner_executor", "eligible": false, "executor": {}, "hashBasis": "catalog_entry", "schemaHash": "sha256:7e3707eb3ea6085dbd74c2843bd2c891581cf6eaeeea179caf0fcceb82dd8ff0", "zdr_allowed": false}
 b) The chat.save_thread_snapshot row's schemaHash changes from sha256:e4bd4bf23a92f33b0a634084e5b74d365bcc9e782d70afc7d18874dabf949095 to sha256:7ada85de4bdac95e9a49a91630c0c3cce58b65a6ba6b8d4fc1efc1f03f8544d8 (the memoryMode input from commit 8418364b7).
 Process for the lead: the generator writes the SPA-side copy apps/Frontend Plane/verevonv3/src/shared/api/contracts/agent-action-admission.json. After the other items of this run have settled (the generator reads the whole registry), run VEREVON_UPDATE_GOLDEN=1 npx vitest run --pool=forks src/shared/actions/admission-contract.test.ts inside apps/Frontend Plane/verevonv3. Check that the diff is only the two rows above. Then copy that file byte-identical to:
 - apps/Model Plane/api/contracts/agent-action-admission.json (canonical)
 - apps/Model Plane/go/services/capability-core/internal/api/testdata/agent-action-admission.json
 - apps/Model Plane/rust/services/execution-core/tests/fixtures/agent-action-admission.json
 - apps/Model Plane/rust/services/model-gateway/tests/fixtures/agent-action-admission.json
 Refresh the .sha256 beside the canonical file and the digest pinned at line 215 of scripts/tests/zdr-contract-sync.test.sh through the repo's sync tooling (not by hand). Then rerun scripts/tests/zdr-contract-sync.test.sh. Today the SPA copy, the canonical file and the three vendored copies are byte-identical, and the pin matches, so that script passes as it stands.

3) F-f01-19: no shared edits.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED item 2).

F-f01-19: flushAllDeltaCoalescers walks a module-level live set (added on create, removed on close and after the abort flush). flushOnPageLifecycle runs beforeFlush immediately before writer.flush on hidden and pagehide. The controller's onCleanup flushes first and leaves disposed set afterwards. The controller passes a wrapper (flushDeltaBatches) rather than the bare flushAll, and the wrapper also schedules a snapshot write when text was held. That deviation is required: snapshotWriter.flush() writes only a pending call, so text the batch holds would miss storage with the window open and idle (the test 'pagehide with the window open and nothing pending...' pins it). Switch off: no coalescer exists and every new call is a no-op. Writes still go through writeThreadSnapshot, so the temporary, foreign-origin, retention and disposed guards apply; nothing new is stored or logged.

FILES: the fixer's files field named one file. The fix touches the 28 files listed in files. O-26 (1-21) and F-f01-19 (22-28) use disjoint files, so two commits are cleaner; messages are in the feedback below.

SHARED (lead, after the other items settle, because the generator reads the whole registry):
1. Commit the new untracked apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/action-executors.json with O-26; the gateway include_str!s it. If another item changes the registry, regenerate it: from apps/Frontend Plane/verevonv3, VEREVON_UPDATE_GOLDEN=1 npx vitest run --pool=forks src/shared/actions/action-registry.test.ts. Update the hard-coded 139 in that test if the client set changes.
2. Admission golden: pre-existing at HEAD (commit 8418364b7 changed action-registry.ts without regenerating), not caused by O-26. Regenerated output differs from the committed golden in exactly two rows (confirmed from the vitest diff):
 a. Insert before the chat.queue_invocation_input row: {"actionId": "chat.queue.retract", "registry": true, "access": "none", "authority": "control_run_decision", "sideEffectClass": "context_write", "exemptReason": "no_owner_executor", "eligible": false, "executor": {}, "hashBasis": "catalog_entry", "schemaHash": "sha256:7e3707eb3ea6085dbd74c2843bd2c891581cf6eaeeea179caf0fcceb82dd8ff0", "zdr_allowed": false}
 b. chat.save_thread_snapshot schemaHash changes from sha256:e4bd4bf23a92f33b0a634084e5b74d365bcc9e782d70afc7d18874dabf949095 to sha256:7ada85de4bdac95e9a49a91630c0c3cce58b65a6ba6b8d4fc1efc1f03f8544d8 (the memoryMode input). tickets.create's owner pin is unchanged.
 Steps: (i) from apps/Frontend Plane/verevonv3 run VEREVON_UPDATE_GOLDEN=1 npx vitest run --pool=forks src/shared/actions/admission-contract.test.ts; it writes src/shared/api/contracts/agent-action-admission.json. Check that git diff shows only rows a and b. (ii) Copy that file byte-identical to apps/Model Plane/api/contracts/agent-action-admission.json, apps/Model Plane/go/services/capability-core/internal/api/testdata/agent-action-admission.json, apps/Model Plane/rust/services/execution-core/tests/fixtures/agent-action-admission.json and apps/Model Plane/rust/services/model-gateway/tests/fixtures/agent-action-admission.json. (iii) Refresh apps/Model Plane/api/contracts/agent-action-admission.json.sha256 (sha256 of the LF file) and the pin on line 215 of scripts/tests/zdr-contract-sync.test.sh through the repo's sync tooling, not by hand. The four copies have no sibling pin files; the script checks them byte-for-byte (lines 455-464). (iv) Run bash scripts/tests/zdr-contract-sync.test.sh (expect 0 not ok), then the admission test without VEREVON_UPDATE_GOLDEN.
3. The fixer's other shared-edit claims are right: no registry rows, compose lines, flag-registry rows, migrations or protos.
4. Staging: use-chat-controller.ts and chat-client.ts also carry other items' uncommitted hunks. Stage only: in use-chat-controller.ts, the import lines (about 121-130), the flushDeltaBatches block and flushOnPageLifecycle call (about 1120-1134), and the first statements of onCleanup (about 2005-2010); in chat-client.ts, lines 9-15 (the re-export). The O-26 files and the F-f01-19 test files contain only this item's hunks (I checked the diffs).

Commit messages if you split: [A] "feat(verevonv3): executor field on the action registry" for files 1-21; [B] "fix(verevonv3): flush frame batches before the chat snapshot" for files 22-28.

Leftovers: docker and DB harnesses not run (none needed; no DB tests added). Full SPA suite and use-chat-controller.paged-transcript.test.ts not run; run them on the merged tree. The controller test file re-imports the 7000-line controller per test; the fixer saw six timeouts at the default 30 s under load, and my default-timeout run passed 27 of 27. Consider vi.setConfig({ testTimeout: 180000 }) at file level. Do not run cargo fmt --all: handlers.rs rustfmt hunks are in other items' import lines and tests.

## P2-spa-08-rel1

Fixer shared_edits:

Lead, for the SHARED refresh of the admission golden: (a) scripts/tests/zdr-contract-sync.test.sh line 215: change the digest from 386f22f18aa015bb95ee2c9a68cd44825adf603f6628755e3ad9a7b7b763dc16 to a6f62f0cade8d36fa42983fa573db261870ca73aea06c4fd9edb9c929436f4c3. This is the only shared edit I know of. (b) Already applied by me in this tree and needing no further action: the SPA golden, the Model Plane canonical copy, the .sha256 pin (a6f62f0c...), and the capability-core, execution-core and model-gateway vendored copies. (c) No registry row, compose line, migration number or proto change is needed for this item. (d) If the registry changes again before commit, regenerate as described in leftover 3.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED golden refresh the spec names, so it does not need them.

SHARED: (1) REQUIRED, in the same commit: scripts/tests/zdr-contract-sync.test.sh line 215 (GOLDENS): replace 386f22f18aa015bb95ee2c9a68cd44825adf603f6628755e3ad9a7b7b763dc16 with a6f62f0cade8d36fa42983fa573db261870ca73aea06c4fd9edb9c929436f4c3 and keep the rest of the line. Verified in a scratch copy run against the tree (CONTRACT_SYNC_ROOT=C:/dev/mp-w345): 207 passed, 0 failed, exit 0. The fixer's expected 206 is off by one, because the pin-file sibling check runs only when the digest matches. Unpatched on the tree: 205 passed, 1 failed (line 47, golden differs from its pinned digest). (2) Already applied and verified, no action: the SPA golden, the Model Plane canonical golden and its .sha256 pin, and the SPA copy plus the three Model Plane vendored copies (cmp-identical to canonical, no CR bytes, 393 rows, 393 zdr_allowed false, 0 true). (3) No registry row, compose line, flag-registry row, migration number or proto change is needed. (4) If registry ids or input/output schemas change before commit, regenerate from the SPA dir with VEREVON_UPDATE_GOLDEN=1 npx vitest run --pool=forks src/shared/actions/admission-contract.test.ts, re-copy to the three vendored paths, rewrite the pin as '<sha256> *agent-action-admission.json', and redo line 215.

## P2-spa-09

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none. No registry row, compose line, migration, admission golden (or its sha256 or vendored copy), or proto change is needed. The switch stays VITE_CHAT_PAGED_TRANSCRIPT, read as the strict literal 'true'.

For the lead: the files list includes ChatPage.tsx, ChatMessages.tsx and the two new test files, which the fixer did not list. The same files carry F-f01-19 hunks: chat-client.ts (the re-export of flushAllDeltaCoalescers and hasPendingDeltaText near the top) and use-chat-controller.ts (those two imports, flushDeltaBatches, and its use in flushOnPageLifecycle and onCleanup). Stage only this item's hunks, or commit P2-spa-08 first. Re-run the seven core suites after staging, because other agents are editing this tree.

## P2-spa-11

Fixer shared_edits:

1. Flag registry (apps/Control Plane/flag-registry/flag-registry.json): insert a MODEL_GATEWAY_USAGE_MIXED_BASIS row directly after the MODEL_GATEWAY_USAGE_PROGRESS row and before MODEL_GATEWAY_USER_LOCALE_CONTEXT. Use the same key order as its siblings, with default "false", read as the exact literal "true", owner model-gateway. Purpose: "Adds the boolean cost_mixed key to the usage and usage_progress SSE frames when a turn's calls span more than one cost basis, so the UI labels the cost as covering token-priced calls only." The verifier validated a row like this in a scratch copy: it parses, gives 430 rows, and has no duplicate.
   - Then run `GOWORK=off go test ./...` in apps/Control Plane/flag-registry.
   - Five failures there come from other tracks and do not involve MODEL_GATEWAY_*: TestControlComposeScanReadsLiteralAndInterpolatedSwitches, TestControlPlaneSwitchesAreRegisteredAsPinned, TestAuditSIEMPrivateDestinationSwitchIsDevOnlyAndExact, TestErasureHoldCheckIsRegisteredAndDeclaredByBothReaders and TestSwitchSpellingTablesAgreeWithTheRegistry.
2. Compose (apps/Model Plane/deploy/docker-compose.yml): in the model-gateway service environment block, after the MODEL_GATEWAY_USAGE_PROGRESS line (around line 599), add this line with a six-space indent: `MODEL_GATEWAY_USAGE_MIXED_BASIS: ${MODEL_GATEWAY_USAGE_MIXED_BASIS:-false}`
3. Golden, in this order:
   - (a) From apps/Model Plane/rust, run `MP_UPDATE_GOLDEN=1 CARGO_TARGET_DIR=C:/dev/mp-target-w345 cargo test -p model-gateway --lib -- sse_event_payload_golden`. The only change should be `events.usage.keys.cost_mixed` and `events.usage_progress.keys.cost_mixed`, each `{"types":["boolean"],"presence":"optional"}`. I confirmed this today with a structural diff.
   - (b) Update apps/Model Plane/api/contracts/sse-event-payloads.json.sha256 to `<sha256> *sse-event-payloads.json`.
   - (c) Copy the golden and its .sha256 byte for byte to apps/Frontend Plane/verevonv3/src/shared/api/contracts/.
   - (d) Put the new digest in scripts/tests/zdr-contract-sync.test.sh (around line 209).
   - (e) Run `node scripts/gen-chat-event-types.mjs`, then `--check`. This regenerates chat-event-payloads.generated.ts with the new key (FU-11); do not hand-edit that file.
   - (f) Run `npx vitest run --pool=forks src/shared/api/chat-event-contract.test.ts`; expect 8 of 8.
4. Keep MODEL_GATEWAY_USAGE_MIXED_BASIS off until step 3 lands.
5. This item needs no migration, no proto change, no admission golden change, no ZDR.md row and no vite-env.d.ts change.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (corrected shared_edits for the lead):
1. Flag registry, apps/Control Plane/flag-registry/flag-registry.json: insert after the MODEL_GATEWAY_USAGE_PROGRESS row (closes at line 3293) and before MODEL_GATEWAY_USER_LOCALE_CONTEXT (line 3294). Row: {"name":"MODEL_GATEWAY_USAGE_MIXED_BASIS","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"Adds the boolean cost_mixed key to the usage and usage_progress SSE frames when a turn's calls span more than one cost basis, so the UI labels the cost as covering token-priced calls only. ZDR streams never state it. Off by default: only the literal true turns it on.","declarations":[{"service":"model-gateway","env":"MODEL_GATEWAY_USAGE_MIXED_BASIS","default":"false"}]}. CORRECTION: owner must be "model-plane" (all 289 existing rows; no row uses "model-gateway"), not "model-gateway" as the fixer wrote. Then run GOWORK=off go test ./... in apps/Control Plane/flag-registry; the five named failures are pre-existing (other tracks) and none is a MODEL_GATEWAY_* check.
2. Compose, apps/Model Plane/deploy/docker-compose.yml (model-gateway block; service header at line 194): after line 599 (`      MODEL_GATEWAY_USAGE_PROGRESS: ${MODEL_GATEWAY_USAGE_PROGRESS:-false}`) add `      MODEL_GATEWAY_USAGE_MIXED_BASIS: ${MODEL_GATEWAY_USAGE_MIXED_BASIS:-false}` (six spaces).
3. Golden, in order: (a) from apps/Model Plane/rust: MP_UPDATE_GOLDEN=1 CARGO_TARGET_DIR=C:/dev/mp-target-w345 cargo test -p model-gateway --lib -- sse_event_payload_golden. The only change is $.events.usage.keys.cost_mixed and $.events.usage_progress.keys.cost_mixed, each {"types":["boolean"],"presence":"optional"} (verified structurally from the panic output). (b) apps/Model Plane/api/contracts/sse-event-payloads.json.sha256 becomes "<sha256> *sse-event-payloads.json" plus LF (matches the current 90-byte format). (c) copy the golden and the .sha256 byte for byte to apps/Frontend Plane/verevonv3/src/shared/api/contracts/ (currently identical to the Model Plane copy). (d) scripts/tests/zdr-contract-sync.test.sh line 209: replace the pinned digest 027ec84c... for apps/Model Plane/api/contracts/sse-event-payloads.json. (e) CORRECTION: the generator is apps/Frontend Plane/verevonv3/scripts/gen-chat-event-types.mjs, not scripts/gen-chat-event-types.mjs at the repo root. Run `node scripts/gen-chat-event-types.mjs` then `node scripts/gen-chat-event-types.mjs --check` from apps/Frontend Plane/verevonv3 (it writes src/shared/api/chat-event-payloads.generated.ts; never hand-edit). (f) from apps/Frontend Plane/verevonv3: npx vitest run --pool=forks src/shared/api/chat-event-contract.test.ts, expect 8 of 8. Today 4 fail, each with "usage reads 'cost_mixed', which the golden does not list" (or the usage_progress twin).
4. Keep MODEL_GATEWAY_USAGE_MIXED_BASIS off until step 3 lands.
5. Nothing else is needed: no migration, no proto, no admission golden, no ZDR.md row (the diff has no mixed row), no vite-env.d.ts change.

## P2-spa-11-rel1

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: None for this fix. No new switch, so no flag-registry row, compose line, admission golden, proto or migration is needed. Pre-existing and not caused by this fix: VITE_CHAT_CONTENT_CONTAINMENT has no row in apps/Control Plane/flag-registry/flag-registry.json and no compose or Dockerfile mention, while other frontend VITE_ switches are registered with service frontend. The lead may register it under the item that introduced it.

Related bugs (same paint-containment root cause, not fixed here; each needs its own item):
1. CitedPassageViewer side sheet (.verevon-chat-cited-passage, position fixed) is rendered inside the turn content (chat-media-markdown.tsx 1296-1297). It is not in the list, so a contained non-latest turn confines it and clips it when the switch is on. Fix: add .verevon-chat-cited-passage and extend the test. It renders only under Show when opened(), so presence means open. Static reasoning from CSS containment semantics, not browser-verified.
2. Citation chip popover (.verevon-chat-citation-chip__popover, chat-media-markdown.tsx 1236) opens below its chip and clips at the turn's bottom edge. Fix: exempt the open state with .verevon-chat-citation-chip[open]. Do not use the popover class in :has(): details content stays in the DOM while closed, so that would exempt every turn with a citation.
3. Chart hover tooltip (.verevon-chat-chart__tooltip, ChatChart.tsx 634-637) can clip at the top of a short turn. It is conditional (tooltip() memo), so :has(.verevon-chat-chart__tooltip) would be exact. Geometry-dependent, low severity.
Checked and not affected: .verevon-chat-table-tools__bar (sits inside the table wrap, top 6px), .verevon-chat-attachment-source-badge, .verevon-chat-step__line, and .chat-work-period__menu (composer only, DashboardComposer).

Leftovers: (1) No browser render check with the switch on; clipping and the exemption were verified by selector, specificity, parser and mutation checks, not a screenshot. (2) :has() needs Chrome 105+, Safari 15.4+ or Firefox 121+. (3) Spec fragments F-f04-05 and F-f02-07 are listed in docs/plans/model-plane-audit-phase2-inventory-2026-11-20.md under P2-spa-11, not this bug; confirm they are tracked under the parent. (4) Only chat-prefixed absolute or fixed rules were enumerated (19 rules); non-chat floating classes (verevon-tooltip, popover and similar) were not audited. (5) apps/verevon-web product-demo bundle contains no containment rule and is not affected. (6) No docker, no DB harness, no git writes; no files edited.

## P2-spa-12

Fixer shared_edits:

1. Registry row for apps/Control Plane/flag-registry/flag-registry.json (lead applies). Insert into flags[] after the VITE_PLAN_EXECUTE_ENABLED object (its closing "}," is line 5230) and before the VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED object (opens at line 5231). Brace at 4 spaces, fields at 6, LF file. Row:
    {
      "name": "VITE_PUSH_SHOW_WHEN_VISIBLE",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "frontend-plane",
      "source": "code",
      "default": "false",
      "stage": "dark",
      "orgOverridable": false,
      "purpose": "D2-863: when a window is visible a push shows a silent notice that closes itself after 1.5 s, so Safari and strict Chromium builds do not revoke the subscription; enable only after a live Safari and Chrome check",
      "codePath": "apps/Frontend Plane/verevonv3/src/features/core/lib/push-subscription.ts"
    },
Result after insert: 430 flags, sorted, the row at index 415 (checked by a dry run in memory; nothing written).
2. Compose lines: none. No compose file or registry entry names VITE_PUSH_SHOW_WHEN_VISIBLE today (grepped apps/Frontend Plane/verevonv3 and apps/Control Plane).
3. Migrations: none. Admission golden (api/contracts/agent-action-admission.json, its .sha256 and vendored copies): none. Proto: none.
4. Hunk map for the shared files: use-chat-controller.ts lines 507-514 (resetUiEvents filter and appendUiEvent early return), F-f04-06. chat-thread-history.ts: isAttentionState import (line 11), attentionState field and doc (102, 134-135), input at 685, carry call at 312 and withAttentionCarry (789-796), validator at 1148, all F-f05-07; openForkedChatThread (595-605), F-f05-21. CoreSidebar.tsx: threadAttentionState import (100), chatRunStatusChip call (951) and definition (1312-1329), F-f05-07. ChatMessages.tsx: ChatThreadSession import (91), onForked props (280, 683), ForkAction forward (1156), F-f05-21. ChatPage.tsx: openForkedChatThread import (102) and onForked line (1587), F-f05-21. vite-env.d.ts: line 5 only, F-f05-12.
5. Whole-file changes for this item (diff checked): public/sw.js, src/features/core/lib/push-subscription.ts and its test, src/shared/attention/attention-state.ts and its test, src/features/chat/lib/chat-organization.ts and its test, ForkAction.tsx and its test, src/features/spaces/lib/space-thread-presentation.ts (comment at line 38 only), and the new files ChatMessages.fork.test.tsx, use-chat-controller.usage-progress.test.ts and ChatPage.fork-wiring.test.ts. CoreSidebar.chat-sessions.test.tsx and chat-thread-history.test.ts also hold other items' tests: stage only the attention and fork cases.
6. Migration numbers: none.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead applies; not in the tree):
1. Registry row in apps/Control Plane/flag-registry/flag-registry.json, inserted into flags[] after the VITE_PLAN_EXECUTE_ENABLED object (its closing "}," is line 5230) and before VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED (opens at 5231). Brace at 4 spaces, fields at 6, LF file:
{
  "name": "VITE_PUSH_SHOW_WHEN_VISIBLE",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "frontend-plane",
  "source": "code",
  "default": "false",
  "stage": "dark",
  "orgOverridable": false,
  "purpose": "D2-863: when a window is visible a push shows a silent notice that closes itself after 1.5 s, so Safari and strict Chromium builds do not revoke the subscription; enable only after a live Safari and Chrome check",
  "codePath": "apps/Frontend Plane/verevonv3/src/features/core/lib/push-subscription.ts"
},
Checked in memory with node: 429 becomes 430 flags, still sorted, inserted at index 415. The key order matches VITE_APPROVAL_SCOPE_GRANTS. Purpose is 211 runes (limit 240). codePath contains the name, which TestShippedCodeFlagsAreStillRead requires. The row carries no declarations, matching the code-only neighbour.
2. Compose lines: none. Migrations: none. Admission golden (and .sha256/vendored copies): none. Proto: none.
3. Hunk-map correction: vite-env.d.ts adds lines 4-5 (the doc comment and the declaration), not line 5 only.
4. Optional, not required: the purpose does not end with "Only the literal true turns it on.", which VITE_APPROVAL_SCOPE_GRANTS carries. Adding it would exceed 240 runes, so keep the spec's wording.

Staging (the lead must stage hunks only): use-chat-controller.ts 507-514; chat-thread-history.ts attention hunks (11, 102, 134-135, 312, 685, 789-796, 1148) and openForkedChatThread (596-605); CoreSidebar.tsx 100, 951, 1312-1329; ChatMessages.tsx 91, 280, 683, 1156; ChatPage.tsx 102, 1587; vite-env.d.ts 4-5. CoreSidebar.chat-sessions.test.tsx and chat-thread-history.test.ts also hold other items' cases.

Leftovers (not blocking): the post-row Go registry suite was not run; the lead should rerun it, and compare against the 5 baseline failures. The live Safari/Chrome check is owner-gated. The chip and the attention filter read the cached server state, which refreshes only on mount, unarchive or a local run-status write, while the rail is live. This is parity with the old run-status staleness, not a regression.

## P2-spa-15

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none required for this item. The admission golden is built only from actionRegistry, MODEL_ACCESS and MODEL_READ_SURFACE (admission-contract.test.ts), and the new sibling file is not imported there, so the golden and .sha256 do not move. No flag-registry, compose, migration or proto change. The lead must add the untracked src/shared/actions/action-executors.ts, and must stage only this item's hunks in the three shared files: action-registry.test.ts (the ACTION_EXECUTORS/getActionExecutor/ActionId imports and the two executor-map tests in the 'action executors (O-26)' block), action-surface-contract.test.mjs (only the block from the '// F-f05-02b' comment to end of file), and use-chat-controller.ts (the ThreadLookup/readThreadLookup helper near line 1563, the early-read block and projectRef branch at about 1644-1672, and the lookup line near 1697). use-chat-controller.project-gate.test.ts is wholly this item.

Non-blocking, for the owner or a follow-up:
(1) origin 'space' threads: the first open keeps Chat's route (spec), but a second open in the same page session goes to the room, because loadThread records scopedThreadRefs from any session spaceRef (about 1699-1700) and the hint path trusts it. This is pre-existing, and the existing 'recorded project thread' test pins it (its fixture is origin 'space'). The loadThread comment (about 1582-1585) requires Chat's route only with the switch off. The fix needs an owner decision, because origin-gating the record also changes connectResumable (about 6292) and the project chip.
(2) No test pins the switch-off route or the single read for an un-hinted origin-chat project thread, so a regression that moves the early read outside its guard would pass all 343 controller tests. Suggest one switch-off case.
(3) The spec's DO NOT ('do not trust a client-supplied ref over the session') conflicts with 'keep the hint path'. The fixer kept the hint per spec, which is safe because the room route is Control-checked.
(4) Name clash: ActionExecutor is exported from types.ts ('gateway'|'client') and from action-executors.ts ('gateway'|'client'|'owner_not_wired'). The spec names both, so this is an owner call to unify.
(5) Report accuracy: the fixer's '243 gateway' does not reconcile with 384 rows (HEAD gives 244 gateway, 139 client, plus the ignored alias), and the controller eslint shows 25 warnings, not 26 (the 16 errors match).

## P2-sc-13

Fixer shared_edits:

Flag registry: none. No new switch. The fork count reads SESSION_CORE_THREAD_FORK_ENABLED. The goal ledger reads SESSION_CORE_THREAD_GOALS_ENABLED, and the cancel and terminal settle read SESSION_CORE_SCHEDULED_RESULT_PERSIST.

Compose: none. Migrations: none; the fork lineage columns and index come from 0059.

Golden or admission files: none.

Proto: apps/Model Plane/proto/model_plane/v1/sessions.proto already has `int32 fork_count = 32;` in ThreadSummary, uncommitted in the worktree. Generated bindings are not regenerated. apps/Model Plane/go/gen/model_plane/v1/sessions.pb.go has no ForkCount yet, so F-f05-99 must regenerate it. Do not hand-edit generated code.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none required. No new switch, no migration, no golden, no compose line. The Go bindings in the worktree already contain ForkCount at tag 32, matching the proto (another item regenerated them), so the lead should confirm that when running F-f05-99. Optional, pre-existing gap: SESSION_CORE_SCHEDULED_RESULT_PERSIST is declared in neither apps/Model Plane/deploy/docker-compose.yml nor apps/Control Plane/flag-registry/flag-registry.json, though this unit's goal ledger and cancel settle depend on it.

Staging: sessions.proto, grpc.rs and http_routes.rs also carry other items' hunks (GetThreadRetention, the F-f10-07 erasure audit, the delegable-tools route). Stage this fix by hunk.

Needs harness: 15 new #[ignore] DB tests. The fixer listed 14; fork_refuses_a_thread_holding_a_zdr_run_and_still_forks_a_persistent_one is the 15th. Run them with DATABASE_URL set and --test-threads=1. The unattended tests return early without DATABASE_URL, and their FireSwitches set process-wide env vars.

## P2-sc-13-rel1

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none needed. The three files add no env var, migration, proto field, golden or registry row. Pre-existing gap for the lead to confirm: SESSION_CORE_SCHEDULED_RESULT_PERSIST is read by session-core and execution-core at HEAD, but has no flag-registry row or compose line at HEAD or in the tree.

## P2-sc-14

Fixer shared_edits:

A. apps/Control Plane/flag-registry/flag-registry.json: add two rows in name order. Add ERASURE_HOLD_CHECK only once; F-f10-06 needs the same row, and erasure_hold_check_switch_test.go pins owner control-plane, stage dark (no stage field), that codePath, both declarations and the purpose suffix.
{
  "name": "ERASURE_HOLD_CHECK",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "control-plane",
  "source": "compose",
  "default": "false",
  "purpose": "EP-9 C4: before an erasure, org-core and session-core ask whether the org or thread is under a legal hold, and a held erasure is refused (session-core audits the refusal content-free). Only the literal true turns it on.",
  "declarations": [
    { "service": "org-core", "env": "ERASURE_HOLD_CHECK", "default": "false" },
    { "service": "session-core", "env": "ERASURE_HOLD_CHECK", "default": "false" }
  ],
  "codePath": "apps/Control Plane/org-core/internal/http/gdpr_handlers.go"
}
{
  "name": "SESSION_THREAD_RETENTION_READ",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "F-f10-02: SessionRetentionService.GetThreadRetention answers org-core whether a thread keeps nothing (all runs zdr, no held content), two booleans only; off answers unavailable. Only the literal true turns it on.",
  "declarations": [
    { "service": "session-core", "env": "SESSION_THREAD_RETENTION_READ", "default": "false" }
  ],
  "codePath": "apps/Model Plane/rust/services/session-core/src/thread_retention_read.rs"
}

B. apps/Model Plane/deploy/docker-compose.yml, session-core service environment (the service starts at about line 1218). Add these after about line 1381 (SESSION_CORE_ATTENTION_TICK_SECONDS), at six spaces of indentation. The ERASURE_HOLD_CHECK line is shared with F-f10-06; add it once.
      # EP-9 C4: refuse (and audit, content-free) an erasure of a held thread or org. Off unless the literal true.
      ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}
      # F-f10-02: GetThreadRetention for org-core's legal-hold check. Off (unavailable) unless the literal true.
      SESSION_THREAD_RETENTION_READ: ${SESSION_THREAD_RETENTION_READ:-false}

C. apps/Control Plane/docker-compose.yml, org-core service environment (the service starts at about line 592). Add these after about line 659 (ORG_COMPLIANCE_ROLE_ENABLED), at twelve spaces of indentation, once (shared with F-f10-06):
            # EP-9 C4: refuse an erasure or purge of a held org or thread. Off unless the literal true.
            ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}

D. Bindings: regenerate from the final apps/Model Plane/proto/model_plane/v1/sessions.proto with the buf recipe. The proto change is rpc GetThreadRetention(GetThreadRetentionRequest) returns (GetThreadRetentionResponse) on SessionRetentionService, with GetThreadRetentionRequest { string org_id = 1; string thread_id = 2; } and GetThreadRetentionResponse { bool found = 1; bool nothing_retained = 2; }. Targets:
- Go: sessions.pb.go and sessions_grpc.pb.go. They are already modified in the tree, but the NothingRetained comment (about lines 8167-8168) is stale and still says "zero-retention or waived".
- Python: mp-events-py sessions_pb2.py.
- TypeScript: sessions_pb.ts and sessions_connect.ts.
The Go regen also carries ThreadSummary.fork_count from the fork track, which matches the current proto.

E. No migration, no admission golden change and no other proto change.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED, and the DB harness run for three tests.

Previous must-fix items, checked in the tree:
- thread_retention_read.rs seed_goal binds format!("goal_{thread}") (line 223), which meets the thread_goals id CHECK (goal\_%) in migration 0091.
- The gdpr.rs source-scan lines are rustfmt-shaped. No rustfmt diff falls on this item's lines.
- grpc.rs delete_threads drops tx before record_refused_erasure, and the source-order test passes.

Correctness, checked against the code and the schema:
- GetThreadRetention is served only to service:org-core holding session:retention:read and not ZDR. User credentials, other services, a missing scope and another org's token are refused. The org check runs before the switch check, so a refused caller learns nothing of the switch. An unknown thread answers found=false. A DB error gives a generic Internal. The answer is two booleans.
- SESSION_THREAD_RETENTION_READ is read once at boot, as the literal "true". Off answers unavailable. ERASURE_HOLD_CHECK off means ensure_not_held returns Ok before any record call. The test_gate block is cfg(test) only, so the production gate is unchanged.
- The erasure audit row is content-free: org, actor, op, scope, held count, "denied". The event id is blake3(org, op, actor, bucket), 93 characters, inside audit-core's stable-id pattern. Its subject passes the session_audit_outbox CHECK and audit-core's authority rule. It is written for ZDR and waived callers (no zdr gate) and erased with the org (the purge deletes by payload org_id). Unavailable writes nothing. An insert error is logged with a closed word and does not change the refusal.
- Every table and column the predicate names exists in the migrations. The predicate covers every family in HELD_THREAD_CONTENT.

OWNER GATE 1 (rule before merge): waived threads.
The spec says a waived run keeps nothing, so zdr and zdr_waived both count. The shipped code counts only zdr. ZDR.md section 5.5 says a waived run is a normal persisted run (thread, messages, plan, events, approvals). DEC-32 only refuses new stores for waived runs. So the spec's premise is wrong. Following it would let org-core refuse a hold on a thread that holds messages, a legal-hold gap. The shipped choice fails toward the hold, which is the safe side.
If the owner confirms, correct: ZDR.md row 132 first cell (it still says a waived thread's hold is refused), the zdr_refusal.rs lines 394-395 comment, and the NothingRetained/ThreadReceipt comment in apps/Control Plane/org-core/internal/clients/session_thread_retention.go (F-f10-03 track).
If the owner reverses, change keeps_nothing_only in thread_retention_read.rs, the matrix expectations (waived_only, zdr_and_waived), and the proto comment. That weakens holds and needs explicit sign-off.

Other owner questions, verifier view:
2. Side tables. The predicate checks what a hold keeps (HELD_THREAD_CONTENT). loop_checkpoint's tests pin that park saves are refused for zdr and zdr_waived runs. I did not verify events, todos, memory_index, dream_runs, subagent_edges or session_tool_audit_intents row by row. Owner to confirm. A test diffing delete_thread_rows tables against HELD_THREAD_CONTENT plus an explicit no-content list would pin it.
3. Per-user and Space-deletion refusals write no row. Adding one needs new op words in the closed set, a spec change (related 4).
4. Yes: a ZDR caller's refused erasure writes the row, as the spec asks.
5. The shared scope also lets org-core call CountWaivedRuns for its own org. That returns per-connection, per-user counts, no content. I would accept it, or add a scope in Auth Core.

Prerequisites outside this fix:
- Until F-f10-03 adds service:org-core with session:retention:read to plane-service-principals.json, every caller is refused (fail closed).
- Stage only this item's hunks. grpc.rs, gdpr.rs, auth.rs and sessions.proto also carry other tracks' hunks (ListThreads/fork_count, Space-deletion tests, skill-bundle erasure, unattended goal).

SHARED: the fixer's report carried no shared_edits. These are complete and corrected.
A. apps/Control Plane/flag-registry/flag-registry.json, rows in name order. Insert ERASURE_HOLD_CHECK between ENABLE_COMPAT_ADAPTER and EVENT_TRIGGERS_ENABLED (add once, shared with F-f10-06). Insert SESSION_THREAD_RETENTION_READ between SESSION_THREAD_RECEIPT_ENFORCE and SKILL_BUNDLE_MOUNT_ENABLED. Rows:
{ "name": "ERASURE_HOLD_CHECK", "kind": "switch", "onLiteral": "true", "owner": "control-plane", "source": "compose", "default": "false", "purpose": "EP-9 C4: before an erasure, org-core and session-core ask whether the org or thread is under a legal hold, and a held erasure is refused (session-core audits the refusal content-free). Only the literal true turns it on.", "declarations": [ { "service": "org-core", "env": "ERASURE_HOLD_CHECK", "default": "false" }, { "service": "session-core", "env": "ERASURE_HOLD_CHECK", "default": "false" } ], "codePath": "apps/Control Plane/org-core/internal/http/gdpr_handlers.go" }
{ "name": "SESSION_THREAD_RETENTION_READ", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f10-02: SessionRetentionService.GetThreadRetention answers org-core whether a thread keeps nothing (every run zdr and no held content), two booleans only; off answers unavailable. Only the literal true turns it on.", "declarations": [ { "service": "session-core", "env": "SESSION_THREAD_RETENTION_READ", "default": "false" } ], "codePath": "apps/Model Plane/rust/services/session-core/src/thread_retention_read.rs" }
These satisfy erasure_hold_check_switch_test.go, switch_spellings_table_test.go and TestShippedCodeFlagsAreStillRead (both codePaths contain their names).

B. apps/Model Plane/deploy/docker-compose.yml, session-core environment, after line 1385 (SESSION_CORE_ATTENTION_TICK_SECONDS; the spec's 1381 has moved), six spaces:
      # EP-9 C4: refuse (and audit, content-free) an erasure of a held thread or org. Off unless the literal true.
      ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}
      # F-f10-02: GetThreadRetention for org-core's legal-hold check. Off (unavailable) unless the literal true.
      SESSION_THREAD_RETENTION_READ: ${SESSION_THREAD_RETENTION_READ:-false}

C. apps/Control Plane/docker-compose.yml, org-core environment, after line 661 (ORG_COMPLIANCE_ROLE_ENABLED), twelve spaces, once (shared with F-f10-06):
            # EP-9 C4: refuse an erasure or purge of a held org or thread. Off unless the literal true.
            ERASURE_HOLD_CHECK: ${ERASURE_HOLD_CHECK:-false}

D. Bindings, regenerated from the final sessions.proto with the buf recipe (buf.gen.go-only.yaml for Go; buf.gen.yaml for the rest):
- Go: apps/Model Plane/go/gen/model_plane/v1/sessions.pb.go and sessions_grpc.pb.go. The NothingRetained comment at lines 8167-8168 still says "zero-retention or waived". The Go package builds today (go build ./... in apps/Model Plane/go/gen is clean).
- Python: apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/sessions_pb2.py. It has no GetThreadRetention and no ThreadSummary.fork_count.
- TypeScript: apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/sessions_pb.ts and sessions_connect.ts. Same gap. Use the pins in buf.gen.yaml, then run apps/Model Plane/scripts/tests/gateway-gencode-pin-test.mjs.
E. No migration, admission golden or other proto change for this unit.

Tests run and related findings are in the tests and related fields.

## P2-sc-14-rel1

Fixer shared_edits:

Registry and compose (the lead to check; not edited by me): neither switch is in apps/Control Plane/flag-registry/flag-registry.json (grep found no match), and neither appears in apps/Control Plane/docker-compose.yml or apps/Model Plane/deploy/docker-compose.production.yml. The switches are SESSION_THREAD_RETENTION_READ (session-core, read at boot via mp_contracts::loop_fold::switch_from, literal true only, default off) and ORG_RETENTION_THREAD_RECEIPT_CHECK (org-core, retention.ThreadReceiptCheckEnabled, literal true only, default off). Add rows and compose lines if the ratchets require them. No migration, golden, or proto change from this item. The zdr-refusals.json fixture is unchanged. The catalogue labels and messages are unchanged, and only the comment in zdr_refusal.rs moved.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead-owned; corrected from the fixer's shared_edits):
(1) apps/Control Plane/flag-registry/flag-registry.json: add a row for SESSION_THREAD_RETENTION_READ. Do NOT add it to the "unregistered" baseline (it may only shrink). Today TestSwitchSpellingTablesAgreeWithTheRegistry FAILS: "session-core lists 1 switches with neither a registry row nor an unregistered baseline entry: [SESSION_THREAD_RETENTION_READ]". Row: name SESSION_THREAD_RETENTION_READ, kind switch, onLiteral "true", owner model-plane, source compose, default "false", stage dark, declarations [{service session-core, env SESSION_THREAD_RETENTION_READ, default false}], codePath apps/Model Plane/rust/services/session-core/src/thread_retention_read.rs.
(2) Same file: add a row for ORG_RETENTION_THREAD_RECEIPT_CHECK. kind switch, onLiteral "true", owner control-plane, source compose, default "false", stage dark, declarations [{service org-core, env ORG_RETENTION_THREAD_RECEIPT_CHECK, default false}], codePath apps/Control Plane/org-core/internal/retention/types.go.
(3) apps/Model Plane/deploy/docker-compose.yml, session-core service block (service declared at line 1218), next to SESSION_THREAD_IMPORT_ENABLED (line 1464): SESSION_THREAD_RETENTION_READ: ${SESSION_THREAD_RETENTION_READ:-false}, with a comment that only the literal true turns it on. CORRECTION: not apps/Model Plane/deploy/docker-compose.production.yml; the session-core switches are declared only in the base deploy compose.
(4) apps/Control Plane/docker-compose.yml, org-core environment next to ORG_RETENTION_POLICY_ENABLED (line 659): ORG_RETENTION_THREAD_RECEIPT_CHECK: ${ORG_RETENTION_THREAD_RECEIPT_CHECK:-false}. The switch-on path also needs ORG_CORE_SESSION_SERVICE_API_KEY: ${ORG_CORE_SESSION_SERVICE_API_KEY:-} (secret passthrough, same pattern as ORG_CORE_IDENTITY_SERVICE_TOKEN; its consumerEnv is in apps/Control Plane/config/plane-service-principals.json, org-core entry) and SESSION_CORE_RETENTION_ADDR (session-core's private gRPC host:port; the Model Plane compose uses session-core:9091 for SESSION_CORE_ADDR; the lead must confirm org-core can reach it on the shared network). AUTH_CORE_URL is already set (line 667). With the switch on and any of the address or key unset, org-core refuses to start (cmd/server/main.go threadReceiptReader). The off path needs none of these.
(5) Owner ruling: record it in apps/Model Plane/docs/decisions/ledger.md (none recorded today) before merge.
(6) Not this item: ERASURE_HOLD_CHECK has no registry row, so TestErasureHoldCheckIsRegisteredAndDeclaredByBothReaders fails; the same Legal holds row names this switch.

## P2-sc-15

Fixer shared_edits:

Migration number: session-core 0137 (`apps/Model Plane/rust/services/session-core/migrations/0137_retention_sweep_deletions.sql`, revert in `migrations/revert/0137_retention_sweep_deletions.down.sql`). If another item also claimed 0137, renumber this one to the next free number and update the include_str! paths in the `retention_sweeper.rs` test `retention_sweep_error_vocabulary_matches_the_migration`. No flag-registry rows (no new switch; it reuses SESSION_RETENTION_SWEEP), no compose lines, no admission golden or proto changes.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: Migration number: session-core 0137 (apps/Model Plane/rust/services/session-core/migrations/0137_retention_sweep_deletions.sql; revert in migrations/revert/0137_retention_sweep_deletions.down.sql). If another item of this run also claimed 0137 at merge time, renumber to the next free number and update both include_str! paths in the retention_sweeper.rs test `retention_sweep_error_vocabulary_matches_the_migration`. No flag-registry row, since it reuses SESSION_RETENTION_SWEEP. No compose line, admission golden or proto change. The metric mp_session_retention_sweep_runs_total gains the label value outcome="interrupted" under an existing label key; update any dashboard or alert that lists the outcome values.

## P2-sc-16

Fixer shared_edits:

["1) apps/Control Plane/flag-registry/flag-registry.json: add after the SESSION_RETENTION_SWEEP row: {\"name\": \"SESSION_RETENTION_WORKSPACE_ERASE\", \"kind\": \"switch\", \"onLiteral\": \"true\", \"owner\": \"model-plane\", \"source\": \"compose\", \"default\": \"false\", \"purpose\": \"F-f10-09 (D-f10-7): the sweeper's enforce pass erases each deleted thread's sandbox workspace through sandbox-manager's EraseThreadWorkspaceForRetention. Only the literal true enables it, and only while SESSION_RETENTION_SWEEP is enforce; unconfigured or off, the workspace stays unconfirmed.\", \"declarations\": [{\"service\": \"session-core\", \"env\": \"SESSION_RETENTION_WORKSPACE_ERASE\", \"default\": \"false\"}], \"codePath\": \"apps/Model Plane/rust/services/session-core/src/sandbox_workspace_eraser.rs\"}. 2) apps/Model Plane/deploy/docker-compose.yml, service session-core (block starts line 1218), after the SESSION_RETENTION_SWEEP_INTERVAL_SECS line (about line 1507): SESSION_RETENTION_WORKSPACE_ERASE: ${SESSION_RETENTION_WORKSPACE_ERASE:-false} and SANDBOX_MANAGER_ADDR: ${SANDBOX_MANAGER_ADDR:-http://sandbox-manager:9094} (service sandbox-manager is at line 2848, gRPC port 9094; session-core has no SANDBOX_MANAGER_ADDR today). AUTH_CORE_URL, SESSION_CORE_SERVICE_ID and SESSION_CORE_SERVICE_API_KEY are already in the session-core block. No migration number is needed; no proto, golden or admission-file change; F-f10-22 adds no switch (SESSION_CORE_API_KEY_PRINCIPAL, compose line about 1435, already gates the key path)."]

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (the fixer's text is correct and complete; the lead adds these after the run):
1) apps/Control Plane/flag-registry/flag-registry.json: insert after the SESSION_RETENTION_SWEEP row. Row: {name SESSION_RETENTION_WORKSPACE_ERASE, kind switch, onLiteral true, owner model-plane, source compose, default false, purpose (as the fixer wrote it), declarations [{service session-core, env SESSION_RETENTION_WORKSPACE_ERASE, default false}], codePath apps/Model Plane/rust/services/session-core/src/sandbox_workspace_eraser.rs}. Verified: the row parses as JSON and the registry parses (429 rows).
2) apps/Model Plane/deploy/docker-compose.yml, service session-core (block starts line 1218, inference-core starts 1563): after the SESSION_RETENTION_SWEEP_INTERVAL_SECS line (1507) add SESSION_RETENTION_WORKSPACE_ERASE: ${SESSION_RETENTION_WORKSPACE_ERASE:-false} and SANDBOX_MANAGER_ADDR: ${SANDBOX_MANAGER_ADDR:-http://sandbox-manager:9094}. Verified: session-core and sandbox-manager are both on the default network, so the service name resolves. AUTH_CORE_URL (1252), SESSION_CORE_SERVICE_ID (1268) and SESSION_CORE_SERVICE_API_KEY (1269) already exist in the block.
3) No change needed for: the principal grant (committed); the admission golden, the proto and generated code (none changed by this unit); a migration (none for this unit; 0137 is F-f10-10's); SESSION_CORE_API_KEY_PRINCIPAL (compose line 1435, default false, already gates F-f10-22); SANDBOX_MANAGER_SERVICE_THREAD_ERASE (compose line 2887, default false, must be true for any erase to succeed).

LEFTOVERS (not run by me: no docker, no DB harness):
- DB tests for the lead's harness: 17 retention_sweeper DB tests, one new (retention_sweep_sandbox_refusal_is_counted_and_the_pass_completes), the others now asserting (thread, owner, deleted) tuples; and 3 grpc_api_key_run_tests DB tests (an_api_key_starts_a_managed_run_on_its_own_thread_only, an_api_key_may_not_start_a_run_in_a_space, an_api_key_streams_only_its_own_runs_events). If the Space test returns internal rather than permission_denied, a run-table constraint is rejecting the copied Space columns before the refusal.
- Flag-registry Go tests, after the row is added (GOWORK=off go test . in apps/Control Plane/flag-registry). TestLiteralSwitchReadersInSourceAreRegistered passes now. These fail and are not this unit: TestSwitchSpellingTablesAgreeWithTheRegistry (SESSION_THREAD_RETENTION_READ has no row, F-f10-02), TestErasureHoldCheckIsRegisteredAndDeclaredByBothReaders (ERASURE_HOLD_CHECK has no row), TestControlComposeScanReadsLiteralAndInterpolatedSwitches (AUDIT_CHAIN_PRUNE_DRYRUN).
- Other items' hunks sit in the same files and must not be staged with this unit: F-f10-10 (ledger, run_id) in retention_sweeper.rs; thread_retention_read (the mod line in main.rs, switch_spellings.rs); F-f10-02 (SessionRetentionGrpc in grpc.rs, THREAD_RETENTION_READ in auth.rs); the thread-fork work (fork_count in grpc.rs); settle_scheduled_fire and the pub(crate) changes in terminalization.rs.

## P2-sc-16-rel1

Fixer shared_edits:

A. apps/Control Plane/flag-registry/flag-registry.json: insert this row between the closing "}," of SESSION_CORE_TRACE_CONSUMER (line 4186) and the opening "{" of SESSION_ORG_ROUTING_OVERLAY (line 4187). Keep the file's indentation (4 spaces before "{", 6 before keys); the verifier's copy was indented 8/10. Row (purpose is about 190 characters, under the 240 limit):
{
      "name": "SESSION_DELETE_WORKSPACE_ERASE",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "stage": "dark",
      "purpose": "P2-sc-16-rel1: user delete, bulk delete and org purge erase each deleted thread's sandbox workspace after commit. Needs SANDBOX_MANAGER_SERVICE_THREAD_ERASE=true. Only literal true enables it.",
      "declarations": [
        { "service": "session-core", "env": "SESSION_DELETE_WORKSPACE_ERASE", "default": "false" }
      ],
      "codePath": "apps/Model Plane/rust/services/session-core/src/sandbox_workspace_eraser.rs"
    },
Shape matches the neighbouring compose-sourced rows (e.g. SANDBOX_MANAGER_SERVICE_THREAD_ERASE at line 3909).
B. apps/Model Plane/deploy/docker-compose.yml, session-core environment block (service at line 1218), after line 1269 (SESSION_CORE_SERVICE_API_KEY), at 6-space indent:
      # P2-sc-16-rel1: where session-core reaches sandbox-manager for a workspace erase (same address as model-gateway).
      SANDBOX_MANAGER_ADDR: ${SANDBOX_MANAGER_ADDR:-http://model-plane-sandbox-manager-1:9094}
      # P2-sc-16-rel1: erase a deleted thread's sandbox workspace after its delete commits. Off by default; only the
      # exact literal true enables it. sandbox-manager must also run with SANDBOX_MANAGER_SERVICE_THREAD_ERASE=true.
      SESSION_DELETE_WORKSPACE_ERASE: ${SESSION_DELETE_WORKSPACE_ERASE:-false}
SANDBOX_MANAGER_ADDR is not set in the session-core block today; model-gateway sets it at line 951 with a literal value. AUTH_CORE_URL (1252), SESSION_CORE_SERVICE_ID (1268) and SESSION_CORE_SERVICE_API_KEY (1269) are present.
C. SANDBOX_MANAGER_SERVICE_THREAD_ERASE stays at its compose default false (sandbox-manager block, line 2887; registered at flag-registry.json line 3909). No change.
D. apps/Control Plane/config/plane-service-principals.json already gives session-core the sandbox-manager audience and sandbox:thread-workspace:erase:any (lines 615 and 626; confirmed by grep). No change.
E. Migrations, golden and proto files: none changed by this item.
F. For P2-sc-16 (not this item): SESSION_RETENTION_WORKSPACE_ERASE (read as SWITCH_ENV in sandbox_workspace_eraser.rs, used at line 108) has no flag-registry row or compose line in the tree. Add both for that item.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: (the lead applies these with the commit; both checked, see below)
A. apps/Control Plane/flag-registry/flag-registry.json. Insert after the "    }," that closes SESSION_CORE_TRACE_CONSUMER (line 4186), before the "    {" of SESSION_ORG_ROUTING_OVERLAY (line 4187). Indentation must be 4 / 6 / 8 spaces; the fixer's 8 / 10 is wrong for this file. The file is LF.
    {
      "name": "SESSION_DELETE_WORKSPACE_ERASE",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "stage": "dark",
      "purpose": "P2-sc-16-rel1: user delete, bulk delete and org purge erase each deleted thread's sandbox workspace after commit. Needs SANDBOX_MANAGER_SERVICE_THREAD_ERASE=true. Only literal true enables it.",
      "declarations": [
        { "service": "session-core", "env": "SESSION_DELETE_WORKSPACE_ERASE", "default": "false" }
      ],
      "codePath": "apps/Model Plane/rust/services/session-core/src/sandbox_workspace_eraser.rs"
    },
Checked: purpose is 192 runes and names "literal true"; codePath exists and names the switch; declaration default matches; the name sorts between its neighbours; no unregistered-list entry conflicts; the file parses with the row spliced in (430 flags).

B. apps/Model Plane/deploy/docker-compose.yml. Insert after line 1269 (SESSION_CORE_SERVICE_API_KEY), at 6 spaces, LF. The fixer's text is correct.
      # P2-sc-16-rel1: where session-core reaches sandbox-manager for a workspace erase (same address as model-gateway).
      SANDBOX_MANAGER_ADDR: ${SANDBOX_MANAGER_ADDR:-http://model-plane-sandbox-manager-1:9094}
      # P2-sc-16-rel1: erase a deleted thread's sandbox workspace after its delete commits. Off by default; only the
      # exact literal true enables it. sandbox-manager must also run with SANDBOX_MANAGER_SERVICE_THREAD_ERASE=true.
      SESSION_DELETE_WORKSPACE_ERASE: ${SESSION_DELETE_WORKSPACE_ERASE:-false}
Both lines match the flag-registry compose scanner's composeEnv pattern. SANDBOX_MANAGER_ADDR has no other reader in session-core (only the eraser reads it), so it changes nothing while the switch is off.

C. Optional, recommended, not required by any test: switch_spellings.rs EXACT_SWITCHES gets (crate::sandbox_workspace_eraser::DELETE_SWITCH_ENV, "true"). That follows the crate's convention for new exact switches and gives an operator a startup warning for on or 1. If C is added, A must land in the same commit, or the Go spelling-table test reports an unregistered name.

COMMIT NOTES
- sandbox_workspace_eraser.rs is untracked and also holds P2-sc-16's GrpcWorkspaceEraser. It cannot be split.
- This fix needs P2-sc-16's uncommitted retention_sweeper.rs. Its WorkspaceEraser trait takes three arguments there; HEAD's takes two. Commit with P2-sc-16 or after it.
- main.rs line 61 (mod sandbox_workspace_eraser;) is needed here and is shared with P2-sc-16.
- In grpc.rs and gdpr.rs, stage only the regions named in this item. The hold-refusal audit blocks (grpc.rs ~12789-12802 and ~12918-12934) and the test_gate changes in gdpr.rs (~505-530) belong to other items.

RELATED
Related findings are in the related list. The most serious is #1: the BFF erases a workspace before the hold-checked durable delete. The hold check then refuses the delete after the workspace is gone, and the workspace route does not check holds.

TESTS (CARGO_TARGET_DIR=C:/dev/mp-target-w345, RUST_MIN_STACK=16777216, from apps/Model Plane/rust)
- sandbox_workspace_eraser: 16 passed, 0 failed, 2130 filtered, exit 0. Includes each_delete_path_erases_its_workspaces_after_its_commit.
- caller_contract 5 passed; memory_erasure 10 passed (includes the caller_contract_tests pins); gdpr 37 passed, 12 ignored (DB); write_policy 25 passed; delete 32 passed, 38 ignored (DB); owed 26 passed, 16 ignored; grpc_owed 0 matched; retention_sweeper 19 passed, 18 ignored.
- Full session-core --bins, non-DB: 1394 passed, 751 ignored, 1 failed. The failure is message_tree::tests::every_messages_reader_filters_the_active_path, which flags the thread-list fork_count SQL in grpc.rs (~222-241). That is another item's uncommitted change, not this item's hunks; the lead should resolve it before the session-core commit.
- Only compiler warning: pre-existing dead code erase_user_personal_data (gdpr.rs:1780).
- rustfmt --check --edition 2021: sandbox_workspace_eraser.rs is clean. grpc.rs has 414 hunks and gdpr.rs has 48, none in this item's regions (grpc.rs 12840-12990; gdpr.rs 200-240, 940-975, 1150-1175).
- GOWORK=off go test ./... in apps/Control Plane/flag-registry: fails only the five pre-existing tests (TestAuditSIEMPrivateDestinationSwitchIsDevOnlyAndExact, TestControlComposeScanReadsLiteralAndInterpolatedSwitches, TestControlPlaneSwitchesAreRegisteredAsPinned, TestErasureHoldCheckIsRegisteredAndDeclaredByBothReaders, TestSwitchSpellingTablesAgreeWithTheRegistry).
- Read-only Node check of shared A and B, run from a scratch script: passes.
- Not run: the DB-gated tests (lead), docker, and any git write.

LEFTOVERS / PRE-ENABLE GATES
- Bound and detach the erases (related #4) and build the eraser once behind the switch (related #5) before SESSION_DELETE_WORKSPACE_ERASE is enabled anywhere.
- The lead's harness must run the switched-on purge (RETURNING feeding deleted_threads) and the DB-gated gdpr and delete tests.

## P2-sc-17

Fixer shared_edits:

A. Go bindings (required before any Go build of capability-core in the tree): regenerate apps/Model Plane/go/gen/model_plane/v1/runs.pb.go AND runs_grpc.pb.go (the RPC comment changed) from the edited runs.proto. Verified scratch regen: run from apps/Model Plane/proto with buf 1.72 and protoc-gen-go v1.36.12, using a template {version: v2, managed: {enabled: true}, plugins: [{local: protoc-gen-go, out: ., opt: [paths=source_relative]}]} and the command buf generate --template <tpl> --path model_plane/v1/runs.proto -o <dir>. Scratch output: C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\p2sc17-gen\out\model_plane\v1\runs.pb.go. Its diff against the checked-in runs.pb.go is only ResolveThreadOwnerResponse (three fields, three getters, descriptor bytes). Overlay for tests: C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\p2sc17-gen\overlay.json.
B. Other checked-in bindings that carry ResolveThreadOwnerResponse, to regenerate with the repo's full buf recipe (pinned plugins): apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/runs_pb.ts and apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/runs_pb2.py. I did not touch them.
C. Rust bindings: no checked-in copy; mp-contracts build.rs regenerates them, and rerun-if-changed covers the proto.
D. buf breaking: additive changes only, so apps/Model Plane/proto/baseline.binpb is unchanged. buf lint: the new fields carry comments.
E. No flag-registry row, no compose line, no migration, no admission-golden or vendored-copy change. The switch is the existing LEARN_COMMAND_ENABLED.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED F). (b) capability-core does not compile in the tree until the Go bindings are regenerated, since learn_command.go calls the new getters. (c) Deploy session-core before capability-core: an older session-core answers no origin, so every /learn is then refused as not eligible (fail closed, no leak).

SHARED (for the lead, after the run):
A. Go bindings (required): regenerate apps/Model Plane/go/gen/model_plane/v1/runs.pb.go and runs_grpc.pb.go from the edited runs.proto (runs_grpc.pb.go changes only the ResolveThreadOwner RPC comment). Verified: a fresh buf 1.72 generate of model_plane/v1/runs.proto from apps/Model Plane/proto with the local template (version v2, managed enabled, plugin local protoc-gen-go, out ., opt paths=source_relative; protoc-gen-go v1.36.12) is byte-identical to the fixer's overlay output, and differs from the checked-in runs.pb.go only in ResolveThreadOwnerResponse (three fields, three getters, descriptor bytes). Preferred recipe: apps/Model Plane/proto/buf.gen.go-only.yaml for both files (remote plugins, needs network; not run by me). Test overlay: scratchpad p2sc17-gen/overlay.json (replaces runs.pb.go only).
B. Other checked-in bindings with the message (paths exist, not regenerated, not verified by me): apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/runs_pb.ts and apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/runs_pb2.py. Regenerate with the repo's pinned full recipe.
C. Rust: no checked-in copy; mp-contracts build.rs regenerates it. Verified: the test build compiles against the new fields.
D. buf: additive fields 2-4 with comments; leaving proto/baseline.binpb unchanged is consistent (buf breaking/lint not run by me).
E. No flag-registry row, compose line, migration or admission-golden change is needed (LEARN_COMMAND_ENABLED already exists; F-f10-26 adds no switch). Agreed.
F. Commit staging. learn_command.go and learn_command_test.go contain only this item's hunks. thread_reference.rs: lines 43-44 (CHAT_ORIGIN made pub(crate)). http_routes.rs: one line in the cfg(test) fake (~21130). run_service_grpc.rs: ~112-210 (trait, ThreadPosture, owner_posture), ~1319-1350 (resolve_thread_owner), ~3079-3270 (fake and tests), ~3712-3819 (DB test). grpc.rs: ~16135-16200 (helper refactor), ~16358-16362 (replay assertion), ~16679-16741 (zdr/waived flags test).

## P2-sc-18

Fixer shared_edits:

Unchanged from the verified previous attempt; the lead adds these after the run:
A. apps/Control Plane/flag-registry/flag-registry.json: insert a new switch object after the SESSION_CORE_CRASH_PARK_NOTICE_SECONDS object (about line 4083), minding the comma:
{"name":"SESSION_CORE_APPROVAL_EVENTS_ENABLED","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"F-f10-34 (WS13-D10): write one content-free RUN_APPROVAL_DECIDED run event in the decide transaction for each decided approval on a persistent run. Ids and closed words only; zero-retention and waived runs write none, and the skip is counted. Only the literal true; with it off the decide transaction writes what it wrote before.","declarations":[{"service":"session-core","env":"SESSION_CORE_APPROVAL_EVENTS_ENABLED","default":"false"}],"codePath":"apps/Model Plane/rust/services/session-core/src/approval_events.rs"}
(Use the field shapes of the neighbouring rows for default and onLiteral.)
B. Same file, SESSION_CORE_CRASH_PARK_NOTICE_SECONDS: replace the whole purpose string with: "O-02: the wait before the crash-park notice, in seconds (four hours by default). A positive whole number is used as it is, capped one second under SESSION_CORE_PARK_TTL_SECONDS; any other value reads as the default. Read only while the notice switch is on."
C. apps/Model Plane/deploy/docker-compose.yml, session-core block (starts about line 1218):
- About line 1414: change the comment text "clamped to 600 up to the park TTL" to "a positive whole number, capped one second under the park TTL".
- After about line 1415, add these two lines:
      # F-f10-34 (WS13-D10): one content-free RUN_APPROVAL_DECIDED run event per decided approval on a persistent run. Off by default; only the literal true.
      SESSION_CORE_APPROVAL_EVENTS_ENABLED: ${SESSION_CORE_APPROVAL_EVENTS_ENABLED:-false}
D. No migration, proto, admission-golden or vendored-copy change. The run-approval-decided-envelope.json golden and its .sha256 are already in the tree.
E. Optional: in apps/Model Plane/api/contracts/README.md, add a row after the run-attention row (about line 19) for run-approval-decided-envelope.json, pinned by its .sha256. It describes the content-free decision event (opaque ids and closed words; no reason, tool input or digest) read by session-core approval_events.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (for the lead; text checked against the tree):
A. Insert after the closing brace of the SESSION_CORE_CRASH_PARK_NOTICE_SECONDS object in apps/Control Plane/flag-registry/flag-registry.json (that object is at lines 4076-4084 now; mind the comma). The keys match the neighbour switch rows (name, kind, onLiteral, owner, source, default, purpose, declarations, codePath):
{"name":"SESSION_CORE_APPROVAL_EVENTS_ENABLED","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"F-f10-34 (WS13-D10): write one content-free RUN_APPROVAL_DECIDED run event in the decide transaction for each decided approval on a persistent run. Ids and closed words only; zero-retention and waived runs write none, and the skip is counted. Only the literal true; with it off the decide transaction writes what it wrote before.","declarations":[{"service":"session-core","env":"SESSION_CORE_APPROVAL_EVENTS_ENABLED","default":"false"}],"codePath":"apps/Model Plane/rust/services/session-core/src/approval_events.rs"}
B. Same file, SESSION_CORE_CRASH_PARK_NOTICE_SECONDS: replace the purpose string with the text in the fixer's shared_edits. It is correct: loop_checkpoint.rs crash_park_notice_seconds_from uses a positive whole number as it is and caps it at TTL minus one second. The current "clamped to 600" text is stale.
C. apps/Model Plane/deploy/docker-compose.yml, session-core block: at line 1414 replace "clamped to 600 up to the park TTL" with "a positive whole number, capped one second under the park TTL". After line 1415 add:
        # F-f10-34 (WS13-D10): one content-free RUN_APPROVAL_DECIDED run event per decided approval on a persistent run. Off by default; only the literal true.
        SESSION_CORE_APPROVAL_EVENTS_ENABLED: ${SESSION_CORE_APPROVAL_EVENTS_ENABLED:-false}
D. No migration, proto, admission golden or vendored copy change for this fix.
E. Optional README row for run-approval-decided-envelope.json, not checked.

Carried from the previous attempt and not re-judged here: crash-park notes (a) to (d).

## P2-go-08

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none required. Verified at HEAD: flag-registry.json lines 3949-3960 already declare SCHEDULE_CONTRACT_ENABLED for capability-core (default false). docker-compose.yml line 2587 sits in the capability-core block (starts at 2556), and CRON_CONTRACT_GUARDS_ENABLED is at 2589. No migration (session-core 0056 exists), no golden, proto or registry change.

Commit split (stage hunks, not files):
- internal/api/workplane_apis.go: keep the log/slog import, the CronHandler fields and methods for the gate and audit, auditContractRefusal and its helpers, the createIntentFor hunks, the cronSchedule* SQL constants, and the listOrCreate, listFires and runNowFire hunks. Leave out the F-f02-14 delegable-tools code (sort import, delegableTools*, delegableToolsSQL, the Register hunk).
- cmd/main.go: keep fireSchemaGate, contractAuditor, the sweeper and buildTaskDispatcher hunks, and contractAuditorFromEnv. Leave out the F-f03-08 skill-bundle scope-rule hunks.
- docs/ZDR.md: keep only line 162. Lines 111, 132 and 136 belong to other items.
- internal/erasure/ and cmd/org_erasure*.go are another item's untracked files. The ZDR "erased with the org" claim depends on them, so they must be committed with this work.

Leftovers for the lead's harness (not run here): FireSchemaReady on a migrated database with and without 0056 (expect 5 and fewer); the create SQL (row written with the schedule, and a failing audit rolls the schedule back); the renewal SQL (entity kind schedule); the expiry row inside the pause transaction; ON CONFLICT (id) DO NOTHING on a real database.

## P2-go-08-rel1

Fixer shared_edits:

- apps/Model Plane/docs/ZDR.md, schedule contract audit row (about line 162): replace 'the sweeper's expired rows and the dispatcher's refused rows are logged and retried on the next tick or fire' with: 'the sweeper's expired rows are retried on the next tick, and a row lost with the process is written by the sweep's durable recovery (on the first sweep after a start, then every 10 minutes, for each disabled schedule whose approval has lapsed, including one disabled by hand, while CRON_CONTRACT_GUARDS_ENABLED and SCHEDULE_CONTRACT_ENABLED are both the literal true); the dispatcher's refused rows are retried on the next executor tick, and a refused row lost with the process is not recovered.'
- No registry row, compose line, golden, proto or migration is needed for this fix.
- Pre-existing, optional: CRON_CONTRACT_GUARDS_ENABLED is still in the unregistered list in apps/Control Plane/flag-registry/flag-registry.json (about line 5415), although this recovery depends on it.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED text below.

SHARED: apps/Model Plane/docs/ZDR.md, schedule contract audit row, line 162 (the sentence occurs once). Replace "the sweeper's expired rows and the dispatcher's refused rows are logged and retried on the next tick or fire" with: "the sweeper's expired rows are retried on the next tick, and a row lost with the process is written by the sweep's durable recovery (on the first sweep after a start that the fire-schema gate admits, then every 10 minutes, for each disabled schedule whose approval has lapsed, including one disabled by hand that the sweeper never paused, while CRON_CONTRACT_GUARDS_ENABLED and SCHEDULE_CONTRACT_ENABLED are both the literal true); the dispatcher's refused rows are retried on the next executor tick, and a refused row lost with the process is not recovered". Keep the full stop that follows. Add one sentence after it only while related item 1 stays open: "A schedule_expr change to a disabled, lapsed schedule moves its slot, so the recovery can then write a second row for the same lapse."

Related bugs (details in related[]). A schedule_expr PATCH on a disabled, lapsed schedule moves its slot and duplicates the expiry row (low). The recovery writes rows for schedules the sweeper never paused (low, owner). Refusal rows still die with the process (medium, owner; K3-4's DoD names them). The recovery scans every disabled schedule (low). A queued row can survive an org erasure (low). A registry outage floods the log (low).

Residual, not blocking. If the first recovery write fails, the row waits for the next recovery, 10 minutes later. A renewal landing in that gap would leave the row missing for good. A renewal writes its own audit row in the same statement, so it cannot succeed while the registry is down, which narrows the gap. Retrying a failed recovery write on the next minute tick would close it. This is the 10-minute versus next-tick owner decision the spec names.

Open decisions. I could not find the verifier's (a)/(b)/(c) text in the repo docs, so I list the decisions I found: recovery scope (related 2), slot basis (related 1), refusal rows on restart (related 3).

Sibling check. internal/api/tool_policy_expiry.go (WS13 S7) stamps expiry_audited_at in the same statement as its audit insert, so it has no in-memory queue and cannot lose a row. The K3-4 queue is the only in-memory audit path in the service. A stamp on the pause's cron_fires row would close the expiry gap for good, but it needs a session-core column and therefore a migration, so it is not for this item.

Nits. audit.go line 23 says the dispatcher's rows wait for the next tick; they wait for the next executor tick. The recovery's "first sweep after a start" is really the first sweep the fire-schema gate admits.

Registry. CRON_CONTRACT_GUARDS_ENABLED is still in the unregistered list (apps/Control Plane/flag-registry/flag-registry.json line 5415, pre-existing). SCHEDULE_CONTRACT_ENABLED is registered (line 3949, capability-core default false). This fix adds no switch, so it needs no registry row, compose line or golden.

## P2-go-09

Fixer shared_edits:

1) apps/Control Plane/flag-registry/flag-registry.json: insert into the "flags" array after the SKILL_BUNDLE_MOUNT_ENABLED object (ends at line 4285) and before SKILL_DECK_TEMPLATES_ENABLED (line 4287). The shape matches the neighbouring rows:
{
  "name": "SKILL_BUNDLE_SCOPE_RULES",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "F-f03-08: capability-core's SKILL_BUNDLE_SCOPE_RULES. A person's bundle manifest and listing reads follow the skill visibility rule (a skill the person may not see answers 404 as a missing bundle), a bundle body is served only to a service, an upload needs its skill (404 skill_not_found), and a skill delete removes the skill's bundles whenever SKILL_BUNDLES_ENABLED is on. Off, each of these is as it was. Only the literal true turns it on.",
  "declarations": [
    { "service": "capability-core", "env": "SKILL_BUNDLE_SCOPE_RULES", "default": "false" }
  ]
}
Do not add the name to the "unregistered" or "unregisteredSwitches" arrays. SKILL_BUNDLE_RETENTION sits in unregisteredSwitches (line 5495) and that list should not change.
2) apps/Model Plane/deploy/docker-compose.yml, capability-core service: insert directly after line 2684 (SKILL_BUNDLE_RETENTION: ${SKILL_BUNDLE_RETENTION:-false}), with six spaces of indentation:
      # F-f03-08: a person's skill bundle reads follow the skill visibility rule, a bundle body is a service's read, an upload needs its skill, and a skill delete removes its bundles. Only the literal true turns it on; off, each is as it was.
      SKILL_BUNDLE_SCOPE_RULES: ${SKILL_BUNDLE_SCOPE_RULES:-false}
3) No migration, proto, generated code, admission golden or space-surface change is needed. F-f02-14 adds no switch; it rides SCHEDULE_CONTRACT_ENABLED.
4) Optional, not required: an api/openapi.yaml entry for GET /v1/cron/delegable-tools.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead adds these; both texts were checked, JSON parses against the live registry):
1) apps/Control Plane/flag-registry/flag-registry.json: insert into "flags" after the SKILL_BUNDLE_MOUNT_ENABLED object (its closing brace is at about line 4285) and before SKILL_DECK_TEMPLATES_ENABLED. Row:
{"name":"SKILL_BUNDLE_SCOPE_RULES","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"F-f03-08: capability-core's SKILL_BUNDLE_SCOPE_RULES. A person's bundle manifest and listing reads follow the skill visibility rule (a skill the person may not see answers 404 as a missing bundle), a bundle body is served only to a service, an upload needs its skill (404 skill_not_found), and a skill delete removes the skill's bundles whenever SKILL_BUNDLES_ENABLED is on. Off, each of these is as it was. Only the literal true turns it on.","declarations":[{"service":"capability-core","env":"SKILL_BUNDLE_SCOPE_RULES","default":"false"}]}
Do not touch unregisteredSwitches (SKILL_BUNDLE_RETENTION stays there).
2) apps/Model Plane/deploy/docker-compose.yml, capability-core service (header at line 2556): insert after line 2684 (SKILL_BUNDLE_RETENTION), six spaces of indentation:
      # F-f03-08: a person's skill bundle reads follow the skill visibility rule, a bundle body is a service's read, an upload needs its skill, and a skill delete removes its bundles. Only the literal true turns it on; off, each is as it was.
      SKILL_BUNDLE_SCOPE_RULES: ${SKILL_BUNDLE_SCOPE_RULES:-false}
3) After both land, run GOWORK=off go test -count=1 . in apps/Control Plane/flag-registry. Expected pre-existing failures only: TestErasureHoldCheckIsRegisteredAndDeclaredByBothReaders and TestSwitchSpellingTablesAgreeWithTheRegistry (SESSION_THREAD_RETENTION_READ). I ran it before the rows landed and saw exactly these two.
4) No migration, proto, generated code, admission golden or space-surface change. The optional openapi entry for GET /v1/cron/delegable-tools is not required.

Do not stage the shared hunks of files that other units also touch: capability-core cmd/main.go, workplane_apis.go, registry_apis.go, server.go, plugin_children.go, http_routes.rs and ZDR.md each have other units' hunks. Stage only this item's hunks (listed in files).

## P2-go-09-rel1

Fixer shared_edits:

1. Compose (lead): apps/Model Plane/deploy/docker-compose.yml, session-core environment block, next to SKILL_VERSIONS_API (about line 1473). Add:
      # P2-go-09-rel1: a person's erasure removes the bundles of the skills it deletes. Only the literal `true` turns it on.
      SKILL_BUNDLE_SCOPE_RULES: ${SKILL_BUNDLE_SCOPE_RULES:-false}
Without this line the switch cannot be set for session-core through compose. The capability-core block also lacks the line today, so P2-go-09's own declaration is still missing. Add it there as well if the switch should be operable for capability-core.
2. Registry (lead): apps/Control Plane/flag-registry/flag-registry.json has no SKILL_BUNDLE_SCOPE_RULES row, and the name is not in unregisteredSwitches. The name does not match the naming convention, but its boolean-looking default in compose is switch-shaped, so TestModelComposeSwitchesOutsideTheConventionAreRegisteredOrListed requires either a registry row or an unregisteredSwitches entry once the compose line exists. A registry row should be kind switch, onLiteral "true", owner model-plane, source compose, default "false", with declarations that match compose exactly. The catalogue test compares service, env and default as a sorted multiset. Session-core is one declaration; capability-core is a second only if its compose block sets it. Run the Control Plane flag-registry Go tests after the edit (I did not run them).
3. Migrations, goldens, protos: none.
4. ZDR.md: already edited in this tree by me (two sentences in the Skill bundles row, in apps/Model Plane/docs/ZDR.md). Commit it with the other agents' uncommitted ZDR.md hunks in mind.
5. DB harness: see leftovers item 1.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (for the lead; none of this is applied yet, and both files are clean in git status):
1. apps/Model Plane/deploy/docker-compose.yml, session-core environment, after line 1473 (SKILL_VERSIONS_API). The fixer's two lines are correct and match the neighbours' style:
      # P2-go-09-rel1: a person's erasure removes the bundles of the skills it deletes. Only the literal `true` turns it on.
      SKILL_BUNDLE_SCOPE_RULES: ${SKILL_BUNDLE_SCOPE_RULES:-false}
   Recommended: add the same line to the capability-core block, after SKILL_BUNDLE_READ_RULE_ENABLED (about line 2654). capability-core reads this switch too (cmd/main.go skillBundleScopeRules, registry_apis.go deletesBundles), and P2-go-09's own declaration is missing. If you add it, the registry row needs a second declaration.
2. apps/Control Plane/flag-registry/flag-registry.json: the flags array is sorted by name (registry.go:228), so insert the row between SKILL_BUNDLE_MOUNT_ENABLED (line 4275) and SKILL_DECK_TEMPLATES_ENABLED (line 4287):
    {
      "name": "SKILL_BUNDLE_SCOPE_RULES",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "purpose": "F-f03-08 and P2-go-09-rel1: capability-core and session-core read it. capability-core: a skill delete removes the skill's bundles while SKILL_BUNDLES_ENABLED is on, a person's bundle reads follow the skill visibility rule, an upload needs its skill, and a bundle body goes to a service only. session-core: a person's erasure removes the bundles of the skills it deletes. Only the literal true turns it on.",
      "declarations": [
        { "service": "session-core", "env": "SKILL_BUNDLE_SCOPE_RULES", "default": "false" }
      ]
    },
   Add { "service": "capability-core", "env": "SKILL_BUNDLE_SCOPE_RULES", "default": "false" } to declarations only if item 1's capability-core line is added. Declarations must equal the compose lines exactly (TestShippedRegistryMatchesComposeFiles). Once the compose line exists, TestModelComposeSwitchesOutsideTheConventionAreRegisteredOrListed requires this row. Today the flag-registry suite fails only on other agents' rows (ERASURE_HOLD_CHECK and SESSION_THREAD_RETENTION_READ), not on this switch.
3. Migrations, goldens and protos: none needed. There is no schema change, and UserErasureSummary is not serialized anywhere.
4. Optional, not required: session-core switch_spellings.rs EXACT_SWITCHES does not list the new exact reader. The table test only checks names that are listed, and this switch never had a lenient spelling.
5. Commit ordering: ZDR.md line 136 (the Skill bundles row) is not in HEAD. It is P2-go-09's uncommitted row, and the two rel1 sentences sit inside it. Commit P2-go-09 first, or commit the whole row and say so in the message. ZDR.md's other uncommitted rows (lines 111, 132 and 162) belong to other agents.
6. gdpr.rs staging: stage only the rel1 hunks, by new-side lines 1588-1596, 1753-1909, 2001-2009 and 2143-2149. Leave the other agents' hunks alone (test_gate and the refused-erasure code around 504-700, the test-module finders at 2431, 2954-3081, 3112 and 3268, and the new tests). gdpr_owed_tests.rs: both hunks (158-167 and 180-461) are rel1.
7. DB harness (lead): with DATABASE_URL pointing at a disposable database whose name contains "test", run cargo test -p session-core --bins a_person_erasure_removes_the_bundles_of_their_skills_only_under_the_switch -- --ignored. Also run a_person_erasure_removes_their_own_skills_their_versions_and_their_revisions -- --ignored as the switch-off regression. The helper creates the three capability-core bundle tables with IF NOT EXISTS when they are absent.

Also: rustfmt --check shows no diff inside the item's line ranges. The remaining diffs are pre-existing (gdpr.rs 1682, 1689, 1750 and 2052; gdpr_owed_tests.rs 153 and 859) or belong to other agents' edits (gdpr.rs 2431).

## P2-go-10

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED:
- Registry rows: none. SCHEDULE_CONTRACT_ENABLED is already registered (apps/Control Plane/flag-registry/flag-registry.json, capability-core default "false"). SKILL_LINT_MODE is already listed in that file (around line 5457, pre-existing); this item adds no row.
- Compose lines: none. Migrations: none. Proto or generated code: none. Admission golden and its pin: untouched.
- Commit with this fix: apps/Model Plane/api/contracts/authored-behaviour-lint.json (mode.unknown off to report; mode.note rewritten) and apps/Model Plane/api/contracts/authored-behaviour-lint.json.sha256 = "5d590f60e9bc5f1c824a80b04ff2c4b621986c1d68672a1829043d5121f68675  authored-behaviour-lint.json". No vendored copies exist (git ls-files). No repo pin script exists for this file; the README procedure was used and matches HEAD's form.

PROCESS NOTE: one of my greps carried an --include=*.env.example filter, which breaks the rule against searching .env files. The permission layer denied it before it ran, so nothing was searched or read from those files. The follow-up search touched no .env file.

## P2-go-11

Fixer shared_edits:

A. apps/Control Plane/flag-registry/flag-registry.json (the lead applies this; I did not edit it). Insert the two flag objects below in the flags array between CAPABILITY_CORE_ALLOW_UNSIGNED_DECISIONS and CAPABILITY_PERMISSION_RULES_ENABLED. Remove CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED and CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED from the "unregistered" array (currently lines 5411-5412).
A complete copy with exactly these edits is at C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/registry/flag-registry.proposed.json. I re-checked it against the tree. Its diff is the 34-line insertion after line 684 and the removal of 5411-5412 only. It parses as JSON, flags go 429 to 431 and stay sorted with no duplicates, unregistered goes 52 to 50, and the purposes are 235 and 222 runes. Run go test ./... in apps/Control Plane/flag-registry after applying.
The two objects:
{"name":"CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"WS16-S4: RUN_ATTENTION notices, sent once session-core confirms the item waits. Needs SESSION_CORE_ATTENTION_ENABLED, SESSION_CORE_ATTENTION_EVENTS_ENABLED and NOTIFICATION_NATIVE_TYPES_ENABLED first. Only the literal true turns it on.","codeDefault":"false","declarations":[{"service":"capability-core","env":"CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED","default":"false"}]}
{"name":"CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"WS20-S7: the scheduled-run outcome notice. Needs SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED (F-f05-05), the schedule-owner check (F-f05-06) and NOTIFICATION_NATIVE_TYPES_ENABLED first. Only the literal true turns it on.","codeDefault":"false","declarations":[{"service":"capability-core","env":"CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED","default":"false"}]}
B. Compose: nothing to add. apps/Model Plane/deploy/docker-compose.yml already declares both switches under capability-core (lines 2607 and 2610, default false).
C. Migrations: none. The admission golden, its vendored copies and all .proto sources are unchanged by this run. Generated bindings for the five F-f05 protos are already in sync with the tree; no regeneration is needed in this run. Browser and orchestration bindings still need regeneration after P2-ec-29 and P2-ec-26 land (see leftovers).
D. Commit set for this unit: the four runwatch files listed in files. Every hunk in them belongs to F-f05-04. Do not stage cmd/main.go hunks from this unit; they were already in the tree from the earlier attempt.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (the fixer's shared edits are right and complete; copied here):\nA. apps/Control Plane/flag-registry/flag-registry.json: insert the two objects below after the CAPABILITY_CORE_ALLOW_UNSIGNED_DECISIONS object (it starts at line 674; insert after line 684, before CAPABILITY_PERMISSION_RULES_ENABLED), and delete both names from the unregistered array (lines 5411-5412). A complete edited copy is at C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/registry/flag-registry.proposed.json. Result: 431 flags, 50 unregistered.\n{\"name\":\"CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED\",\"kind\":\"switch\",\"onLiteral\":\"true\",\"owner\":\"model-plane\",\"source\":\"compose\",\"default\":\"false\",\"purpose\":\"WS16-S4: RUN_ATTENTION notices, sent once session-core confirms the item waits. Needs SESSION_CORE_ATTENTION_ENABLED, SESSION_CORE_ATTENTION_EVENTS_ENABLED and NOTIFICATION_NATIVE_TYPES_ENABLED first. Only the literal true turns it on.\",\"codeDefault\":\"false\",\"declarations\":[{\"service\":\"capability-core\",\"env\":\"CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED\",\"default\":\"false\"}]}\n{\"name\":\"CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED\",\"kind\":\"switch\",\"onLiteral\":\"true\",\"owner\":\"model-plane\",\"source\":\"compose\",\"default\":\"false\",\"purpose\":\"WS20-S7: the scheduled-run outcome notice. Needs SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED (F-f05-05), the schedule-owner check (F-f05-06) and NOTIFICATION_NATIVE_TYPES_ENABLED first. Only the literal true turns it on.\",\"codeDefault\":\"false\",\"declarations\":[{\"service\":\"capability-core\",\"env\":\"CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED\",\"default\":\"false\"}]}\nB. Compose: nothing to add; capability-core already declares both at apps/Model Plane/deploy/docker-compose.yml lines 2607 and 2610, default false. C. Migrations: none. Admission golden and .proto sources: none for this unit.\n\nLEFTOVERS: (1) F-f05-99 stays partial until P2-ec-29 and P2-ec-26 land; then regenerate browser.pb.go, browser_grpc.pb.go, orchestration.pb.go, orchestration_grpc.pb.go, browser_pb2.py and orchestration_pb2.py. (2) buf lint and breaking were not re-run by me; the fixer's results stand (breaking exit 0; four pre-existing lint findings in routing_policy.proto and runs.proto). (3) The uncommitted F-f05-27 files (browser-broker server.go, site_policy_test.go, the new site_policy_ask_test.go, telemetry/metrics.go; execution-core browser_session.rs and tool_bridge/mod.rs) belong to that unit's own commit. (4) The cmd nil-client guard and the NakWithDelay call in handle() have no unit test; review only. (5) The flag-registry suite fails on the current tree for other units (AUDIT_* pins, AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS, ERASURE_HOLD_CHECK registration, SESSION_THREAD_RETENTION_READ); the proposed registry adds no new failure. (6) NOTIFICATION_NATIVE_TYPES_ENABLED is declared in apps/Application Plane/docker-compose.yml line 838 (default false), not in the Model Plane compose; the registry purposes are accurate. (7) The 502, four-retry and drop sentence was cut from the registry purposes to fit the 240-rune cap; acceptable.\n\nRepo writes: none. Git was read-only; go, cargo and buf wrote only to caches or the scratch folder. The scratch registry check used junction links into the repo, all removed afterwards.

## P2-go-11-rel1

Fixer shared_edits:

Commit set for P2-go-11-rel1, paths relative to apps/Model Plane/go/services/capability-core/:
Whole files: internal/notifyclient/client.go (M); internal/notifyclient/status_error_test.go (new); internal/runwatch/consumer.go (M); internal/runwatch/consumer_backoff_test.go (new); internal/runwatch/attention.go (M); internal/runwatch/attention_test.go (M); internal/runwatch/attention_confirm_test.go (new); internal/runwatch/scheduled_outcome.go (M); internal/runwatch/scheduled_outcome_test.go (M); cmd/attention_confirmer_test.go (new, created by this fix).
Partial: cmd/main.go (M). Stage only its 8 F-f05-04 hunks, from the patch at C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\main_go_f05_04_exact.patch. Old-side hunk starts: 216, 266, 741, 751, 781 (attentionConfirmer block only), 1028, 1059, 1071. The patch applies to the exact HEAD blob without --recount.
Registry rows: none. CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED and CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED are already in apps/Control Plane/flag-registry/flag-registry.json at HEAD (lines 5411-5412).
Compose lines: none. apps/Model Plane/deploy/docker-compose.yml lines 2607-2610 are already at HEAD.
Migration numbers: none.
Golden or proto changes: none. ConfirmAttentionWaiting is already at HEAD in Go gen (attention.pb.go, attention_grpc.pb.go), Python _gen (attention_pb2.py) and Rust session-core (attention_grpc.rs). The F-f05-99 regen is not needed for this closure.
Precondition: the org-erasure files (cmd/org_erasure.go and internal/erasure/) must be committed first or in this same commit (see leftovers 1).

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED).

Evidence. I built a scratch export of HEAD (git archive of apps/Model Plane/go and apps/Model Plane/api, no repo writes), overlaid the worktree's 10 closure files and cmd/attention_confirmer_test.go, and made cmd/main.go = HEAD plus only the 8 F-f05-04 hunks (filtered from the full diff, applied with GNU patch at fuzz 0). The org-erasure non-test files were copied in so cmd can build. In that closure: vet and test pass for runwatch and notifyclient (64 top-level tests, no FAIL or SKIP), go vet ./cmd/ passes, the focused cmd tests pass, full go test ./cmd/ passes, go test ./internal/cron/ passes, and go vet ./... passes for the whole capability-core module including tests. The real worktree also passes the runwatch and notifyclient tests, go vet ./cmd/ and the focused cmd tests. The bug reproduces on clean HEAD: attention.go with its two test files fails vet with n.attentionConfirm undefined.

Logic review. Switch-off is unchanged: with the RUN_ATTENTION and scheduled switches off, process() still acks before any new code runs, and handle() still maps the terminal-run outcomes exactly as before (outcomeRetry gives a plain Nak, outcomeAck gives an Ack). The only new settlement is for outcomeRetryLater. ZDR and waived runs are refused before the confirm, so no RPC is made and no notice is sent. A confirm error or timeout is retried and never announced. A permanent refusal drops the notice and a transient one retries with backoff. The new logs carry org, run and item ids, a status code or a gRPC code, and no content. StatusError drops the response body. Authz holds: the capability-core session-core token includes session:read (cmd/main.go sessionCoreScopes), the HEAD principal grants it, and session-core admits ConfirmAttentionWaiting only for a service caller with session:read. A nil attention client keeps the attention path off, which fails closed. The registry rows (flag-registry.json lines 5411-5412), the compose lines (2607 and 2610, default false), and the proto and generated Go, Python and Rust bindings already exist at HEAD, so no registry, compose, golden or regen change is needed. gitleaks 8.30.1 with the repo config reports no leaks.

Notes, not blocking. (1) Ordering precondition: HEAD's cmd does not build without the untracked org-erasure files. Clean HEAD go vet ./cmd/ fails with undefined startOrgErasureConsumer at cmd/main.go:276. (2) The nil-attention-client gate in startRunWatchConsumer has no test; only the adapter does. Add a test if that gate is extracted into a helper. (3) The notifyclient error text change affects the always-on terminal-run log line, because the body is gone; state that in the commit body. (4) processAttention checks the nil confirmer before the ZDR check, so a ZDR-true event with no confirmer retries rather than acks. No notice is sent either way, and main.go never sets the confirm without the notify switch, so this is unreachable.

SHARED:
- Commit set, paths relative to apps/Model Plane/go/services/capability-core/. Whole files: internal/notifyclient/client.go (M); internal/notifyclient/status_error_test.go (new); internal/runwatch/consumer.go (M); internal/runwatch/consumer_backoff_test.go (new); internal/runwatch/attention.go (M); internal/runwatch/attention_test.go (M); internal/runwatch/attention_confirm_test.go (new); internal/runwatch/scheduled_outcome.go (M); internal/runwatch/scheduled_outcome_test.go (M); cmd/attention_confirmer_test.go (new).
- Partial: cmd/main.go, the 8 F-f05-04 hunks only (old-side starts 216, 266, 741, 751, 781 attentionConfirmer block only, 1028, 1059, 1071). The 781 hunk must exclude the contractAuditorFromEnv lines (+172 to +182 of that hunk). Use C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\verifier_main_f05_04.patch. It and the fixer's main_go_f05_04_exact.patch both pass git apply --check against clean HEAD with identical +/- lines. Mine has new-side start numbers that match the filtered set; the fixer's carry full-diff numbers (+222 on the first hunk). Both apply without --recount.
- Precondition (corrects the fixer's wording to exact files): cmd/org_erasure.go, internal/erasure/org_erasure.go and internal/erasure/purge.go (non-test) must land before this commit or in the same series. Their tests are not needed for this commit.
- Registry rows, compose lines, migration numbers, golden and proto changes: none. Verified at HEAD.
- After staging, confirm git ls-files --eol shows i/lf for cmd/main.go and the other tracked files.

## P2-go-12

Fixer shared_edits:

(a) GOLDEN + PIN. apps/Model Plane/api/contracts/org-agent-policy-schema.json: insert this object between the web.paid_providers object (ends at line 110 with "    },") and the object that opens mcp_apps.network_origins (line 111):
    {
      "key": "web.covers_browser",
      "kind": "enum",
      "strictness_order": ["on", "off"],
      "default": "on",
      "fail_closed_value": "on",
      "status": "enforced"
    },
A ready result with exactly this insertion (valid JSON, LF) is at C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\org-agent-policy-schema.with-covers-browser.json. Its sha256 is 459376fadd4842ef4ed794110ff7d48c5987aa488dea5b1cb578c648cdbcf35b. Re-pin apps/Model Plane/api/contracts/org-agent-policy-schema.json.sha256 to "459376fadd4842ef4ed794110ff7d48c5987aa488dea5b1cb578c648cdbcf35b *org-agent-policy-schema.json" (keep the existing " *" form), then run "go test -count=1 -run TestOrgAgentPolicySchemaMatchesTheGolden ./internal/api/" from the capability-core module.

(b) RUST MIRROR (mp-contracts, needed for the Rust golden test and for the SETTINGS source of truth). apps/Model Plane/rust/crates/mp-contracts/src/org_agent_policy.rs: insert before line 252 (the MCP_APPS_NETWORK_ORIGINS comment), after the web.paid_providers SettingDef closed at line 251:
    // F-f09-13 (owner decision D-f09-3): whether the org's browser tools follow web.mode too. Fail-closed is `on`.
    SettingDef {
        key: "web.covers_browser",
        kind: Kind::Enum,
        strictness_order: &["on", "off"],
        default: "on",
        fail_closed_value: "on",
        status: Status::Enforced,
        reserved_values: &[],
    },
Then run mp-contracts org_agent_policy tests and the golden test ("cargo test -p mp-contracts org_agent_policy"), and the model-gateway org-policy tests (org_policy_enforcement_test, standalone_run_org_policy_test), which iterate SETTINGS.

(c) COMPOSE. apps/Model Plane/deploy/docker-compose.yml, service capability-core (block starts at line 2556). Insert after line 2610 (CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED: ${CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED:-false}):
      # F-f09-13: org web.mode gate, off by default. Only the literal true turns each on.
      CAPABILITY_ORG_WEB_MODE_GATE_ENABLED: ${CAPABILITY_ORG_WEB_MODE_GATE_ENABLED:-false}
      CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER: ${CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER:-false}

(d) FLAG REGISTRY. apps/Control Plane/flag-registry/flag-registry.json: insert these two switch rows in the switches array immediately before the "{" of the CAPABILITY_PERMISSION_RULES_ENABLED row (line 684), same shape as that row:
    {
      "name": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "purpose": "F-f09-13 org web.mode gate: capability-core withholds cap.tool.http, cap.web.crawl, cap.web.map and cap.web.batch from every caller while the org's web.mode is not live. Only the literal true turns it on; off, no org web key is read and no decision changes.",
      "codeDefault": "false",
      "declarations": [
        { "service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED", "default": "false" }
      ]
    },
    {
      "name": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "purpose": "F-f09-13 org web.mode gate, browser half: with CAPABILITY_ORG_WEB_MODE_GATE_ENABLED on, withholds cap.browser.open, cap.browser.observe and cap.browser.act while the org's web.mode is not live, unless the org sets web.covers_browser to off. Only the literal true turns it on; without the gate it has no effect.",
      "codeDefault": "false",
      "declarations": [
        { "service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", "default": "false" }
      ]
    },
Also check: CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED is still in the "unregistered" array (about line 5412). If F-f05-04's registry row was not applied, add its switch row (onLiteral "true", default "false", declaration on capability-core, env in compose at line 2610).

(e) MIGRATIONS: none (neither unit needs a table or column). PROTO: none. execution-core: unchanged, as specified. Nothing in nats.conf, session-core or the notification-core allowlist was touched.

## P2-go-12-rel1

Fixer shared_edits:

a. apps/Model Plane/deploy/docker-compose.yml, capability-core service block (next to "ORG_AGENT_POLICY_API: ${ORG_AGENT_POLICY_API:-false}", about line 2579). Add CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER: ${CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER:-false}. Also add CAPABILITY_ORG_WEB_MODE_GATE_ENABLED: ${CAPABILITY_ORG_WEB_MODE_GATE_ENABLED:-false}, which P2-go-12 reads. Neither is in compose today, so neither reaches the container.
b. apps/Control Plane/flag-registry/flag-registry.json: add switch rows modelled on ORG_AGENT_POLICY_API (about line 3491). Rows: CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER and CAPABILITY_ORG_WEB_MODE_GATE_ENABLED, each with kind "switch", onLiteral "true", owner "model-plane", source "compose", default "false", and declarations for service capability-core. Required because catalogue_test.go lines 199-210 require a registry entry for a boolean-default compose variable outside the naming convention, and GATE_ENABLED also follows the convention (catalogue_test.go line 108).
c. Optional: apps/Control Plane/flag-registry/model_compose_pins_test.go modelComposePins (line 27 pins ORG_AGENT_POLICY_API). Add matching pins for the two new names if you want them pinned.
d. No golden, .sha256, zdr-contract-sync, mp-contracts or proto change was made and none is needed for this fix (see leftovers 1).
e. Optional: cmd/main.go (P2-go-12's line about 599-600) can use api.OrgWebCoversBrowserFromLookup(os.Getenv).

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED.

STAGING: org_agent_policy.go and cmd/main.go also hold other items' hunks. This fix builds on P2-go-12's uncommitted row (org_agent_policy.go line 106 and the browserSwitch field), so commit P2-go-12 first or stage that row with this fix. In main.go stage only lines 334-337 and 942-944; the F-f09-13 block at 600-603 belongs to another item. TestOrgAgentPolicyAgainstPostgres needs the DB harness (lead); vet -tags integration compiles.

SHARED (lead; land compose and registry together, because TestShippedRegistryMatchesComposeFiles fails if only one side is present):
a. apps/Model Plane/deploy/docker-compose.yml (LF). In the capability-core environment, directly after the line "      ORG_AGENT_POLICY_API: ${ORG_AGENT_POLICY_API:-false}" (line 2579), add these two lines with 6-space indent:
      CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER: ${CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER:-false}
      CAPABILITY_ORG_WEB_MODE_GATE_ENABLED: ${CAPABILITY_ORG_WEB_MODE_GATE_ENABLED:-false}
(GATE_ENABLED belongs to the F-f09-13 item; add it there if preferred.)
b. apps/Control Plane/flag-registry/flag-registry.json (LF). Insert after the ORG_AGENT_POLICY_API object (its closing "}," is line 3501):
{"name": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f09-13 (D-f09-3): web.covers_browser is served and accepted only while this is the literal true. Off, it is neither shown nor accepted.", "declarations": [{"service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", "default": "false"}]},
{"name": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f09-13: the org's web.mode withholds the public-web capabilities, and the browser ones while the covers switch is on. Only the literal true enables it.", "declarations": [{"service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED", "default": "false"}]},
Both purposes were checked in scratch: trimmed, 137 and 153 runes, each names the literal true, and no names are duplicated.
c. Optional: apps/Control Plane/flag-registry/model_compose_pins_test.go, modelComposePins after line 27: add {name: "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", kind: KindSwitch, def: "false", services: []string{"capability-core"}} and the GATE equivalent.
d. No golden, .sha256, mp-contracts or proto change is needed for this fix.

RELATED (none blocks this PASS; owner or lead decisions, detailed in the related list): R-A, whether COVERS alone should gate visibility when GATE is off, where the key is then listed "enforced" but changes no decision; R-B, the Go-only key against the Rust table and golden; R-C, a switch-off write drops a stored covers value.

## P2-go-13

Fixer shared_edits:

1. Migration number: the new migration is 0046_skill_bundle_deck_validated.up.sql and .down.sql. The spec said 0045, but 0045_learning_review_counters.* already exists. The next free number is 0047.
2. apps/Control Plane/flag-registry/flag-registry.json: add two switch rows in the same shape as SKILL_DECK_TEMPLATES_ENABLED (around line 4287). Rows:
(a) {"name": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f09-13 / O-19: the org's web.covers_browser decides whether its browser tools (cap.browser.open, observe, act) follow web.mode. On, the key is served and enforced; off, it is neither shown nor accepted and the browser tools are not gated by web.mode. Only the literal true turns it on.", "declarations": [{"service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", "default": "false"}]}
(b) {"name": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f09-13: the org's web.mode withholds the public-web capabilities (cap.tool.http, cap.web.crawl, cap.web.map, cap.web.batch) for every caller. Only the literal true turns it on.", "declarations": [{"service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED", "default": "false"}]}
Row (b) is read by cmd/main.go since P2-go-12 and has no registry row yet.
3. apps/Model Plane/deploy/docker-compose.yml, service capability-core (block starts at line 2556; the SKILL_DECK_TEMPLATES_ENABLED env line is at 2653). Add these two env lines with six-space indent, next to it:
      CAPABILITY_ORG_WEB_MODE_GATE_ENABLED: ${CAPABILITY_ORG_WEB_MODE_GATE_ENABLED:-false}
      CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER: ${CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER:-false}
4. Pins already regenerated by me: apps/Model Plane/api/contracts/org-agent-policy-schema.json.sha256 (sha256sum, per the contracts README) and scripts/tests/zdr-contract-sync.test.sh. The pinned digest is now d1dd71e1c7ace2c13ba6050440c196212449f0ad7fa4fd6d1f8fd03f801d64be. No proto, admission golden, vendored copy or docker-compose file was changed by me.
5. The flag-registry declaration tests under apps/Control Plane/flag-registry may need rows 2 and 3 before the commit.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (for the lead; not applied by me):
1. Migration number: the unit uses 0046_skill_bundle_deck_validated.up.sql and .down.sql (the spec's 0045 is taken). The next free number is 0047.
2. apps/Control Plane/flag-registry/flag-registry.json: insert these two objects after the SKILL_DECK_TEMPLATES_ENABLED row (about line 4297). Both match that row's shape.
{"name": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f09-13 / O-19: the org's web.covers_browser decides whether its browser tools (cap.browser.open, observe, act) follow web.mode. On, the key is served and enforced; off, it is neither shown nor accepted and the browser tools are not gated by web.mode. Only the literal true turns it on.", "declarations": [{"service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER", "default": "false"}]}
{"name": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f09-13: the org's web.mode withholds the public-web capabilities (cap.tool.http, cap.web.crawl, cap.web.map, cap.web.batch) for every caller. Only the literal true turns it on.", "declarations": [{"service": "capability-core", "env": "CAPABILITY_ORG_WEB_MODE_GATE_ENABLED", "default": "false"}]}
3. apps/Model Plane/deploy/docker-compose.yml, capability-core block (starts line 2556), after the SKILL_DECK_TEMPLATES_ENABLED line (currently line 2653), six-space indent:
      CAPABILITY_ORG_WEB_MODE_GATE_ENABLED: ${CAPABILITY_ORG_WEB_MODE_GATE_ENABLED:-false}
      CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER: ${CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER:-false}
4. scripts/tests/zdr-contract-sync.test.sh line 215 (GOLDENS pin for agent-action-admission.json) is stale against the lead's run-r1 golden. The file hashes to a6f62f0cade8d36fa42983fa573db261870ca73aea06c4fd9edb9c929436f4c3 and the script pins 386f22f18aa015bb95ee2c9a68cd44825adf603f6628755e3ad9a7b7b763dc16. Set the pin to the file's hash (README step 4). This is the one failure in the sync test (205 passed, 1 failed) and it is not caused by this unit.
5. After 2 and 3, re-run the flag-registry declaration tests under apps/Control Plane/flag-registry.

## P2-go-13-rel1

Fixer shared_edits:

No registry rows, flag-registry entries, compose lines, migration numbers, admission goldens, or proto changes are needed for this item. The only shared constraint: the SKILL_BUNDLE_SCOPE_RULES track owner must keep the line AND b.deck_validated_at IS NOT NULL in ListVisible's DeckTemplate column when rewriting visibility.go. The new test visibility_deck_test.go (TestDeckTemplateColumnNeedsTheStampInEveryListing) fails if they drop it. Commit the three files together with the P2-go-13 files they pair with.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: This item adds no switch, route, RPC, migration, admission golden, flag-registry or compose row, so no shared edits are needed beyond commit composition. The fixer's "commit with the P2-go-13 files" instruction is right in spirit, but the real dependency set is wider:
(a) visibility.go is untracked as a whole, so committing it ships the entire SKILL_BUNDLE_SCOPE_RULES listing code. It belongs with the scope-rules track's commit.
(b) visibility.go and the new integration test query skill_bundles.deck_validated_at. That column exists only in migration 0046 (untracked) and in store.go's uncommitted stamp changes (HEAD store.go has no deck_validated reference).
(c) visibility_deck_test.go needs sweepDB and sweepRows from deck_sweep_test.go (untracked, P2-go-13).
(d) visibility_deck_integration_test.go needs applyDeckMigrations (deck_validated_integration_test.go, untracked) and visibilityStubSchema (visibility_integration_test.go, untracked).
Land these together with P2-go-13's store.go, validate.go, migration 0046 and deck_sweep_test.go, and the scope-rules track's visibility.go and visibility_integration_test.go, in one commit or an order where each commit compiles and migrates. Committing the three files alone would put a query against a missing column at HEAD and leave the test files without their helpers.

LEFTOVERS:
(1) Integration tests need the DB harness (lead): go test -tags integration -count=1 -run 'TestScopedListingReportsADeckTemplateOnlyOnceStamped|TestDeckTemplateIsListedOnlyOnceStamped|TestVisibleBundlesFollowTheSkillVisibilityRule' ./internal/skillbundle/
(2) Server-package check owed: once internal/api compiles, run go test ./internal/server/ -count=1 -run 'TestListReportsDeckTemplate|Bundle|Scope' in apps/Model Plane/go/services/capability-core.
(3) Nothing committed or staged, no git writes, no docker, no .env read, no repo file edited. The mutant lives only in the scratch folder.

## P2-go-14

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none needed. This unit adds no switch, so it needs no flag-registry row, compose line, migration number, admission golden or proto change. Optional docs-only follow-up, not a shared edit: revert/0109 ORDER step 1 should say capability-core must restart, or be rolled back to a build without this probe, before step 2. A process on this build caches present=true, and after the column drop its list returns 500 and its get returns 404 until restart.

## P2-go-14-rel1

Fixer shared_edits:

No registry row, compose line, migration number or golden change is needed for this item. It uses AUTHORED_KINDS_ENABLED and SKILL_VERSIONS_API, which already exist, and it adds no new switch. The one shared step owed is the proto bindings regeneration: apps/Model Plane/go/gen/model_plane/v1/runs.pb.go has to be regenerated from the modified apps/Model Plane/proto/model_plane/v1/runs.proto (ResolveThreadOwnerResponse: thread_origin, thread_in_space, thread_retains_nothing). Until that is done, a plain go test in capability-core fails to compile. The overlay used for my runs is a verification aid only, not a change to commit.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: No registry row, compose line, migration number, golden or new switch is needed for this item. SKILL_VERSIONS_API and AUTHORED_KINDS_ENABLED already exist (deploy/docker-compose.yml). The fixer's shared step is right but incomplete. The proto fields are owed by whichever item changed runs.proto, not this one: regenerate apps/Model Plane/go/gen/model_plane/v1/runs.pb.go from apps/Model Plane/proto/model_plane/v1/runs.proto (ResolveThreadOwnerResponse: thread_origin=2, thread_in_space=3, thread_retains_nothing=4) with proto/buf.gen.go-only.yaml, Go only, no churn. Also owed for consistency: apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/runs_pb2.py lacks these fields (the full buf.gen.yaml would rewrite Python and TS outputs too). No TS generated file carries ResolveThreadOwnerResponse. Until the Go regen lands, no capability-core package compiles without the overlay.

## P2-go-15

Fixer shared_edits:

1) apps/Model Plane/deploy/docker-compose.yml, service capability-core: in the environment block, right after the existing `SKILL_SPACE_SCOPE: ${SKILL_SPACE_SCOPE:-false}` line (around line 2695), add:
      # F-f10-19: a Space skill is written only under a verified Control model.space.skill.write decision. Needs SKILL_SPACE_SCOPE;
      # turn on only together with user-core SPACE_SKILL_WRITE_DECISIONS. Only the literal `true` turns it on.
      SKILL_SPACE_WRITE_VERIFY: ${SKILL_SPACE_WRITE_VERIFY:-false}
(capability-core already receives CONTROL_SPACE_DECISION_KEY_ID and CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 for cron. No new key variables are needed.)

2) apps/Control Plane/flag-registry/flag-registry.json: new row, placed alphabetically near SKILL_SPACE_SCOPE:
    {
      "name": "SKILL_SPACE_WRITE_VERIFY",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "stage": "dark",
      "purpose": "F-f10-19: capability-core verifies the Control-signed model.space.skill.write decision (X-Space-Skill-Decision: skill:write, bound to org, person, Space, operation, skill id and body digest, single-use nonce) before a Space skill is created, changed or deleted; off, every Space skill write answers 403 space_skill_decision_required. Needs SKILL_SPACE_SCOPE; enable together with user-core SPACE_SKILL_WRITE_DECISIONS. Only the literal true turns it on.",
      "declarations": [
        { "service": "capability-core", "env": "SKILL_SPACE_WRITE_VERIFY", "default": "false" }
      ],
      "codePath": "apps/Model Plane/go/services/capability-core/internal/api/space_skill_decision_verifier.go"
    }
Drop the "stage" key if Model Plane rows do not carry it; the existing SKILL_SPACE_SCOPE row has none.

No migration, proto or admission-golden change.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: (the fixer's text is right and complete; confirmed against the tree)
1) `apps/Model Plane/deploy/docker-compose.yml`, service capability-core, environment block: right after `SKILL_SPACE_SCOPE: ${SKILL_SPACE_SCOPE:-false}` (line 2695), add:
      # F-f10-19: a Space skill is written only under a verified Control model.space.skill.write decision. Needs SKILL_SPACE_SCOPE;
      # turn on only together with user-core SPACE_SKILL_WRITE_DECISIONS. Only the literal `true` turns it on.
      SKILL_SPACE_WRITE_VERIFY: ${SKILL_SPACE_WRITE_VERIFY:-false}
   capability-core already has `CONTROL_SPACE_DECISION_KEY_ID` and `CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64` (lines 2708-2709), so no key lines are needed.
2) `apps/Control Plane/flag-registry/flag-registry.json`: a new row directly after the `SKILL_SPACE_SCOPE` row. Keep "stage" and "codePath": the sibling `SPACE_SKILL_WRITE_DECISIONS` row uses both, and so do 85 and 225 other rows.
    {
      "name": "SKILL_SPACE_WRITE_VERIFY",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "stage": "dark",
      "purpose": "F-f10-19: capability-core verifies the Control-signed model.space.skill.write decision (X-Space-Skill-Decision: skill:write, bound to org, person, Space, operation, skill id and body digest, single-use nonce) before a Space skill is created, changed or deleted; off, every Space skill write answers 403 space_skill_decision_required. Needs SKILL_SPACE_SCOPE; enable together with user-core SPACE_SKILL_WRITE_DECISIONS. Only the literal true turns it on.",
      "declarations": [
        { "service": "capability-core", "env": "SKILL_SPACE_WRITE_VERIFY", "default": "false" }
      ],
      "codePath": "apps/Model Plane/go/services/capability-core/internal/api/space_skill_decision_verifier.go"
    }
No migration, proto or admission-golden change.

Leftovers carried forward:
- The BFF half is not built (the request to user-core and forwarding `X-Space-Skill-Decision`), and the canonical-body definition must be pinned for it.
- The publish, discard-draft and restore routes still refuse Space skills. Extending them is an owner decision.
- The live insert into `scope_id` and the read of it need the DB harness.

## P2-go-15-rel1

Fixer shared_edits:

1) apps/Model Plane/go/services/capability-core/cmd/main.go (lead to apply; I did not edit this dirty file). Hoist the verifier so the bundle handler gets the same instance as the skill handler, which keeps a decision single-use across both routes. Replace lines 331-339 with:
		var spaceSkillVerifier *api.SpaceSkillDecisionVerifier
		if api.SkillSpaceWriteVerifyFromLookup(os.Getenv) {
			if verifier, spaceSkillVerifierErr := spaceSkillDecisionVerifierFromEnv(os.Getenv); spaceSkillVerifierErr != nil {
				slog.Warn("Space skill writes stay refused; Control decision verifier is required", "error", spaceSkillVerifierErr)
			} else {
				spaceSkillVerifier = verifier
				skillsHandler.WithSpaceSkillDecisionVerifier(verifier)
			}
		}
		skillsHandler.Register(protectedMux)
		api.NewSkillBundlesHandler(pool, api.SkillBundlesAPIFromLookup(os.Getenv)).WithScopeRules(skillBundleScopeRules).WithSpaceSkillDecisionVerifier(spaceSkillVerifier).Register(protectedMux)
Until this is applied, the bundle routes refuse every bundle write to a Space skill while both switches are on. That is fail-closed but not usable.
2) No registry rows, flag-registry entries, compose lines, migration numbers, admission goldens or proto changes. This fix adds no new switch; it reuses SKILL_SPACE_SCOPE and SKILL_SPACE_WRITE_VERIFY.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED:
1) cmd/main.go (lead applies; only lines 331-339 belong to this fix; keep the comment at 329-330). Replace the block with the text below, one tab per level. I compiled this exact text in a scratch overlay: go build -gcflags=-e -overlay on ./cmd/ exits 0 and gofmt is clean. The fixer's report shows two tabs only because of indentation; use one.
	var spaceSkillVerifier *api.SpaceSkillDecisionVerifier
	if api.SkillSpaceWriteVerifyFromLookup(os.Getenv) {
		if verifier, spaceSkillVerifierErr := spaceSkillDecisionVerifierFromEnv(os.Getenv); spaceSkillVerifierErr != nil {
			slog.Warn("Space skill writes stay refused; Control decision verifier is required", "error", spaceSkillVerifierErr)
		} else {
			spaceSkillVerifier = verifier
			skillsHandler.WithSpaceSkillDecisionVerifier(verifier)
		}
	}
	skillsHandler.Register(protectedMux)
	api.NewSkillBundlesHandler(pool, api.SkillBundlesAPIFromLookup(os.Getenv)).WithScopeRules(skillBundleScopeRules).WithSpaceSkillDecisionVerifier(spaceSkillVerifier).Register(protectedMux)
Without it, both switches on refuse every bundle write to a Space skill with 403 space_skill_decision_required. That is fail-closed but unusable. With it, the skill routes and the bundle routes share one verifier instance, so the nonce cache is shared.
2) Flag registry and compose (lead; not in the tree, not a reason to fail this item): SKILL_SPACE_WRITE_VERIFY has no row in apps/Control Plane/flag-registry/flag-registry.json (it appears only inside a purpose string near line 4781). Add a switch row next to SKILL_SPACE_SCOPE (line 4337), same shape, onLiteral "true", source "compose", default "false", declaring capability-core. In apps/Model Plane/deploy/docker-compose.yml the capability-core block (service starts at line 2556; SKILL_SPACE_SCOPE at line 2695) lacks SKILL_SPACE_WRITE_VERIFY: ${SKILL_SPACE_WRITE_VERIFY:-false}. Until these land the switch cannot be enabled through compose. CONTROL_SPACE_DECISION_KEY_ID and CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 are already wired for capability-core (lines 2708-2709).
3) No new registry rows, proto changes or migration numbers are needed for this fix, as the fixer said.

Owner decision (related #1): with a switch off, a legacy scope='space' row is still writable by an org admin. The spec mandates switch-off unchanged, so I did not fail on it. Recommendation: refuse scope='space' rows always, as the skill routes do. That changes switch-off behaviour only for such rows, so it needs owner sign-off, and the fixer's one-line option costs one read per bundle write with a switch off.

Scratch artifacts (not in the tree): C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\verifier-p2-go-15-rel1\overlay.json (learn-file overlay), ...\verifier-p2-go-15-rel1\overlay-m1.json, overlay-m2.json, overlay-m3.json (mutation overlays), and ...\verifier-p2-go-15-rel1\cmdmain\main.go (the shared-edit scratch copy). No git write of any kind and no docker run.

## P2-go-16

Fixer shared_edits:

No registry, compose, golden or proto change from this attempt. Still owed from the earlier attempt, unchanged: (a) apps/Model Plane/deploy/docker-compose.yml: add BROWSER_SPACE_FLOOR_ENABLED: ${BROWSER_SPACE_FLOOR_ENABLED:-false} to the browser-broker environment after BROWSER_BROKER_ASSISTANT_TASK_GRANTS_ENABLED (about line 2973), and to the execution-core environment near EXECUTION_DO_BROWSER_ENABLED (about line 2267). (b) apps/Control Plane/flag-registry/flag-registry.json: in the existing BROWSER_SPACE_FLOOR_ENABLED row (about line 635, owner control-plane), add after the org-core declaration {"service": "browser-broker", "env": "BROWSER_SPACE_FLOOR_ENABLED", "default": "false"} and {"service": "execution-core", "env": "BROWSER_SPACE_FLOOR_ENABLED", "default": "false"}. The registry-matches-compose test needs these and the compose lines in the same change. (c) apps/Model Plane/go/gen/model_plane/v1/browser.pb.go, about lines 100-102: regenerate the space_id comment from browser.proto (comment only). (d) Operational: do not set BROWSER_SPACE_FLOOR_ENABLED=true for browser-broker or execution-core unless org-core has its Space floor on in the same deployment; org-core ignores space_id today. (e) No migration; no admission golden change.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead). The fixer's text is right and complete; corrections are marked.
(a) apps/Model Plane/deploy/docker-compose.yml: add "      BROWSER_SPACE_FLOOR_ENABLED: ${BROWSER_SPACE_FLOOR_ENABLED:-false}" under service execution-core: (block at line 1846) after EXECUTION_DO_BROWSER_ENABLED (line 2267), and under service browser-broker: (block at line 2910) after BROWSER_BROKER_ASSISTANT_TASK_GRANTS_ENABLED (line 2973). Both line numbers and service blocks were checked. :-false is the file's dominant form (296 uses of :-false against 36 of :-0).
(b) apps/Control Plane/flag-registry/flag-registry.json: in the existing BROWSER_SPACE_FLOOR_ENABLED row (lines 634-647), append to declarations {"service": "browser-broker", "env": "BROWSER_SPACE_FLOOR_ENABLED", "default": "false"} and {"service": "execution-core", "env": "BROWSER_SPACE_FLOOR_ENABLED", "default": "false"}. CORRECTION to the spec: keep owner "control-plane". Flag names are unique and sorted in this registry (registry_test "duplicate flag name"), so a model-plane owner cannot be a second row, and the row already exists for org-core's declaration. Compose and registry must land together, because catalogue_test compares declarations with compose keys and defaults. Optional: the purpose text names only org-core; it can also name the broker refusal and execution-core.
(c) apps/Model Plane/go/gen/model_plane/v1/browser.pb.go lines ~100-103: the SpaceId comment still reads "Reserved: the broker refuses a non-empty value until the Space floor is wired". Regenerate it from apps/Model Plane/proto/model_plane/v1/browser.proto lines 121-123 (comment only; tag 11 unchanged).
(d) Operational, unchanged: do not set BROWSER_SPACE_FLOOR_ENABLED=true for browser-broker or execution-core unless org-core has its Space floor on in the same deployment (org-core ignores space_id otherwise).
(e) No migration, no admission golden change, and no proto change beyond the comment in (c).

LEFTOVERS (verifier):
- Hunk staging: the four execution-core files each carry other items' uncommitted work. This fix's hunks are: site_policy_client.rs ~40-70 (switch), ~145-310 (checker, cache key, resolve), ~370-660 (Space parameter on the free functions and SitePolicy methods), and tests from ~1190. web_tools.rs ~235-404 (Space parameters on search, fetch, map, plan, crawl, batch) and the test helpers and six Space tests ~2063-2290. runtime_loop/mod.rs: dispatch arms ~1880-1891, request_space_id doc ~1176-1186, the three web executors ~2898-3060 and ~3435-3462, execute_step_with_result_handles signature and tail ~985-1031, wrapper tail ~926-930, compat entry ~753. runtime_loop/agent.rs: only the &req.space_id argument with its comment, ~7095-7097. Do not stage the blank-final-URL, bearer-expiry or other items' hunks in these files.
- The cargo build waited about 25 minutes on the shared target lock (other agents' model-gateway and execution-core builds). Everything reported below ran on the current tree.
- capability-core ./cmd and ./internal/server do not build in this tree. internal/api/learn_command.go (another agent's in-flight edit) calls GetThreadOrigin, GetThreadInSpace and GetThreadRetainsNothing on mpv1.ResolveThreadOwnerResponse, and no generated file defines them (SDK regen owed). Not fixed here. The F-f10-15 warning test (cmd/skill_bundle_retention_switch_test.go) and the internal/server tests were not run; re-run after the regen.
- Pre-existing gofmt diffs at HEAD, not this unit: internal/skillbundle/store.go line ~90 (a '' in the stampSkillBundleSQL doc comment, rewritten by the Go doc formatter) and store_retention_test.go (QueryRow alignment). This unit's lines and the new test files are gofmt-clean.
- HEAD test off_a_same_host_final_url_and_an_empty_one_ask_org_core_nothing is now off_and_a_same_host_final_url_ask_org_core_nothing. Its blank-URL cases moved to a_blank_final_url_* in the other final-URL item (documented at site_policy_client.rs ~410). This fix does not change that behaviour, and the same-host and off cases are still asserted.
- Fixer leftovers 1 and 2 accepted: no run-level executor test (QUARRY_EDGE_URL, the service principal and the process-wide OnceLock make it impractical; the plumbing is covered by compile and the call-site reading above). The success paths of map_site, plan_crawl, start_crawl and start_batch with a Space are not pinned; their refusal path is.
- The round-1 accepted deviation still stands: the broker takes the Space from the mint request's space_id, because AcquireGrantRequest.space_id is not stored on the grant (D2-299). Still open.
- Not run: the full execution-core suite, clippy, and -race (no cgo).

## P2-go-16-rel1

Fixer shared_edits:

1. apps/Control Plane/flag-registry/flag-registry.json: in the BROWSER_SPACE_FLOOR_ENABLED row (declarations ~line 644), add { "service": "model-gateway", "env": "BROWSER_SPACE_FLOOR_ENABLED", "default": "false" } after the org-core entry. Apply together with item 2, because TestShippedRegistryMatchesComposeFiles checks both. 2. apps/Model Plane/deploy/docker-compose.yml, model-gateway block (starts ~line 194): next to WEB_DOMAIN_POLICY_MODE (~line 508), add the line '      BROWSER_SPACE_FLOOR_ENABLED: ${BROWSER_SPACE_FLOOR_ENABLED:-false}'. 3. docs/plans/model-plane-open-decisions.md, DEC-M6 (~lines 500 and 589): owner decision to name model-gateway as a reader of the switch, or keep the gateway's switch off. This is not a fail reason. 4. apps/Model Plane/rust/services/model-gateway/src/session_flow.rs (another agent's file, not touched by me): the two test calls to append_prompt_to_fresh_run at lines 4201 and 4213 need the refused_message argument that the helper now takes (6 parameters). Without this fix the shared lib test target does not compile. 5. Commit note: stage only this item's hunks in tool_loop.rs and sse.rs, as described in leftover 7. web_extraction.rs and domain_rules_tests.rs carry only this item's hunks (the extract test seam, the ExtractSpec.space_id plumbing from the earlier attempt, and the new tests). 6. Optional, not done: switch_spellings.rs EXACT_SWITCHES entry for crate::site_policy_client::SPACE_FLOOR_ENV.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED 3). (b) This fix calls P2-go-16 base code, so commit the P2-go-16 hunks first or in the same commit: site_policy_client.rs (all), the web_search/fetch_url/extract call sites in dispatch_tool_with_cost (tool_loop.rs ~4394-4771), and ExtractSpec.space_id with its use in web_extraction.rs.

COMMIT HUNKS. tool_loop.rs, this item: the dispatch_web_tool_audited comment; the space_id parameter and forwarding lines in run_forced_web_search, run_forced_web_search_for_model and the inner function (not inference_bearer or classifier_budget, which are F-f04-11); the forced dispatch's space argument and comment; the gate call's space argument and comment; gate_and_ground_web_search_outcome's parameter, closure clone and final_url_refusal_in_space call; the non-forced gate call's space argument; the test helpers and tests (ground_hit_in_space, OtherDeniedInSpaceB, the C-09 grounding test, the wiring test). Not this item: screening_bearer, forced_search_screening_bearer and classifier_budget (F-f04-11); the egress-hold and zdr_rule_applies hunks. sse.rs, this item: the forced_space_id block (~6312-6330) and its two argument lines (~6339-6345, ~7678-7681). Not this item: the classifier_budget creation and the other argument lines (F-f04-11), and the rest of the file. web_extraction.rs, this item: the extract_tool_enabled seam and test_seam module, and the new test a_page_that_ended_on_a_host_the_space_floor_denies_is_refused_before_inference. P2-go-16 base: the ExtractSpec.space_id field and doc, the from_request assignment, the extract_page final-URL call, and the json_spec literal. domain_rules_tests.rs: all 254 added lines are this item's tests and helpers, per the fixer (I confirmed the diff is additions only).

SHARED (lead applies; text corrected where noted):
1. apps/Control Plane/flag-registry/flag-registry.json, BROWSER_SPACE_FLOOR_ENABLED row (declarations, ~line 644): add a comma after the org-core declaration (the fixer's text omitted it), then add { "service": "model-gateway", "env": "BROWSER_SPACE_FLOOR_ENABLED", "default": "false" }. Verified in a scratch copy: TestShippedRegistryMatchesComposeFiles and TestLiteralSwitchReadersInSourceAreRegistered pass with items 1 and 2 together, and the test compares declaration sets exactly, so each needs the other.
2. apps/Model Plane/deploy/docker-compose.yml, model-gateway environment block (lines 211-1201), directly after line 508 WEB_DOMAIN_POLICY_MODE: ${WEB_DOMAIN_POLICY_MODE:-off}: add "      BROWSER_SPACE_FLOOR_ENABLED: ${BROWSER_SPACE_FLOOR_ENABLED:-false}". Recommended: a comment above it saying the floor binds only with WEB_DOMAIN_POLICY_MODE shadow or enforce, and only where org-core reads the same switch. The WEB_DOMAIN_POLICY_MODE line at ~2413 is execution-core's, not the gateway's.
3. apps/Model Plane/rust/services/model-gateway/src/session_flow.rs (another agent's file): add "                \"undispatched agent run could not be cancelled\"," after the "agent_run_start_failed" line of each of the two test calls (~4201-4206 and ~4213-4218), as the fifth argument. This is the production refused_message (session_flow.rs ~1241-1247). The fixer's "the run could not be started" also compiles. With this two-line change the lib test target built and the focused filters passed in the copy.
4. docs/plans/model-plane-open-decisions.md, DEC-M6 (~lines 500 and 589): the decision names browser-broker and execution-core as the only readers. Record model-gateway as a third reader and the WEB_DOMAIN_POLICY_MODE coupling, or get the owner's decision. Not a fail reason.
5. Commit: stage only the hunks listed above in tool_loop.rs and sse.rs; commit the P2-go-16 base first or together.
6. Optional, not needed by any test: a switch_spellings.rs EXACT_SWITCHES row for crate::site_policy_client::SPACE_FLOOR_ENV. If added, item 1 must be applied too.

LEFTOVERS: the switch_spellings drift test currently fails on browser_run.rs:2383 and :2388, budget.rs:168, selection_guard.rs:85 and nats_connection.rs:59, none in this item (in-flight or pre-existing). The full flag-registry suite on the shared tree fails 5 tests unrelated to this item (audit-core switch pins x4, ERASURE_HOLD_CHECK, SESSION_THREAD_RETENTION_READ). No docker, no DB harness, no edits outside the copy, no git writes. One incremental build left a stale test binary that did not reflect a mutation; every number reported comes from the clean CARGO_INCREMENTAL=0 build.

## P2-ec-07

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead adds these; not in the tree):
A. apps/Model Plane/deploy/docker-compose.yml, execution-core environment block. Insert after line 2259 (CONTENT_SAFETY_SCREEN_RPC line), six-space indent. Block spans lines 1846-2443, so placement is right. Use the fixer's two lines but correct the comment: replace "a flagged result is withheld under enforce as for a persistent run" with "a flagged result is withheld under enforce only while the gateway's CONTENT_SAFETY_SEMANTIC_INLINE is on, as for a persistent run". Line: EXECUTION_SCREEN_ORG_POLICY: ${EXECUTION_SCREEN_ORG_POLICY:-false}
B. apps/Control Plane/flag-registry/flag-registry.json: add the fixer's row to "flags" immediately after the EXECUTION_SCREEN_INFERENCE_CREDENTIAL entry (before EXECUTION_SHADOW_REVIEWER; name order holds). Shape matches neighbours. Do not add to unregisteredSwitches. Amend purpose to add: "Withhold under enforce needs the gateway CONTENT_SAFETY_SEMANTIC_INLINE on." File is eol=lf.
C. Regenerate checked-in gateway bindings for fail_mode = 9: apps/Model Plane/go/gen/model_plane/v1/gateway.pb.go, apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/gateway_pb2.py, apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/gateway_pb.ts. Rust picks it up via build.rs.
D. EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED remains in neither compose nor registry; that is F-f02-09's row.

## P2-ec-07-rel1

Fixer shared_edits:

"1. apps/Model Plane/deploy/docker-compose.yml, execution-core block (starts line 1846; EXECUTION_SCHEDULED_AGENT_ENABLED is at line 2183; orchestrator-core starts at 2443). Add beside that line, with six-space indent, in this LF file: '      # WS20-S5: with the literal true, the step decision must name exactly the contract its run holds. With it off, a run that holds no stored contract is not refused for a decision that names one; a partly stored contract is always re-verified. Default off.' then '      EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED: ${EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED:-false}'. 2. apps/Control Plane/flag-registry/flag-registry.json: add to the \"flags\" array (key at line 3, LF file) this row: {\"name\": \"EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED\", \"kind\": \"switch\", \"onLiteral\": \"true\", \"owner\": \"model-plane\", \"source\": \"compose\", \"default\": \"false\", \"purpose\": \"WS20-S5: with the literal true, the step decision must name exactly the contract its run holds. A decision that names one for a run holding none is refused as scheduled_contract_mismatch, and a run holding one needs a decision that names it. With the switch off, a run holding no stored contract is not refused for a decision that names one (P2-ec-07-rel1), and a partly stored contract is always re-verified. Only the literal true turns it on.\", \"declarations\": [{\"service\": \"execution-core\", \"env\": \"EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED\", \"default\": \"false\"}]}. The name is absent from the registry today (checked), so this row is the only registry entry needed. Land items 1 and 2 together, since catalogue_test.go checks compose coverage. 3. Doc wording (both files are LF). The F-f10-28 rows do not contain the phrase 'a run carrying a contract is always verified' that the spec assumed. Current text: docs/plans/model-plane-audit-phase2-inventory-2026-11-20.md line 328, description 'execution-core: the scheduled step verifier recomputes the stored contract's digest and expiry on every step' in the row with P2-ec-07 and r2; and docs/plans/model-plane-coding-complete-2026-11-11.md line 666, the same description in the row with authz. Append to that description: 'a run carrying a contract has its stored text, digest and expiry re-verified whatever the switch says; a run carrying none is governed by EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED (P2-ec-07-rel1); the decision-to-run binding stays under that switch (F-f02-17).' Keep the table columns intact. 4. No change needed: apps/Model Plane/docs/DEV_AUTHORITY_HANDOFF_MATRIX.md, and the source-contract check scripts/tests/scheduled-step-config-contract-test.sh (not run; see leftovers). 5. Test names for the verification log: scheduled_step_contract::a_contract_bearing_step_is_not_refused_for_a_run_that_carries_no_contract_with_the_switch_off; scheduled_step_contract::a_partly_served_contract_is_refused_whatever_the_switch_says; grpc scheduled_agent_lane_tests::a_contract_bearing_step_passes_a_run_serving_no_contract_with_the_switch_off. Drop a_contract_less_decision_never_takes_a_contract_run_into_the_agent_lane."

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (lead applies; corrections to the fixer's text are marked):
1. apps/Model Plane/deploy/docker-compose.yml, LF file. Insert after line 2183 (EXECUTION_SCHEDULED_AGENT_ENABLED in the execution-core block, 1846-2442). Do NOT add it to orchestrator-core (line 2459). Six-space indent:
      # WS20-S5: with the literal true, the step decision must name exactly the contract its run holds. With it off, a run that holds no stored contract is not refused for a decision that names one, and a stored contract is re-verified whenever the decision names one. Default off.
      EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED: ${EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED:-false}
   (Correction: the fixer's text said 'a partly stored contract is always re-verified'. That holds only when the decision names one.)
2. apps/Control Plane/flag-registry/flag-registry.json. The flags array must be sorted by name, or the registry fails to load (registry.go:228). Do NOT insert at line 3 as the fixer's text said. Insert the object after line 1421 (the closing '    },' of EXECUTION_CORE_RECIPE_RUNNER) and before line 1422 (the '    {' of EXECUTION_CORE_SKILL_PINS). The name is absent from flags and from unregistered and unregisteredSwitches today (checked). Object, validated as JSON, four-space indent:
    {
      "name": "EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "purpose": "WS20-S5: with the literal true, the step decision must name exactly the contract its run holds. A decision that names one for a run holding none is refused as scheduled_contract_mismatch, and a run holding one needs a decision that names it. With the switch off, a run holding no stored contract is not refused for a decision that names one (P2-ec-07-rel1), and a stored contract is re-verified whenever the decision names one. Only the literal true turns it on.",
      "declarations": [
        { "service": "execution-core", "env": "EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED", "default": "false" }
      ]
    },
   Items 1 and 2 land together (catalogue_test.go checks compose coverage). The default 'false' follows the convention of AGENTIC_SPACE_RETRIEVAL_ENABLED (compose line 464).
3. Docs. Replace the description cell rather than appending: 'on every step' is itself wrong, since the verifier compares only when the decision names a contract. Use this text in docs/plans/model-plane-audit-phase2-inventory-2026-11-20.md line 328 (row P2-ec-07, r2) and docs/plans/model-plane-coding-complete-2026-11-11.md line 666 (row authz). Both files are LF:
   execution-core: the scheduled step verifier recomputes the stored contract's digest and expiry on every step whose decision names one, whatever the switch says; a run carrying no stored contract is governed by EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED (P2-ec-07-rel1); the decision-to-run binding stays under that switch (F-f02-17)
4. No change needed: apps/Model Plane/docs/DEV_AUTHORITY_HANDOFF_MATRIX.md. No admission golden change, because no action schema hash changes.
5. Test names for the log: scheduled_step_contract::a_contract_bearing_step_is_not_refused_for_a_run_that_carries_no_contract_with_the_switch_off; scheduled_step_contract::a_partly_served_contract_is_refused_whatever_the_switch_says; grpc scheduled_agent_lane_tests::a_contract_bearing_step_passes_a_run_serving_no_contract_with_the_switch_off. The dropped test a_contract_less_decision_never_takes_a_contract_run_into_the_agent_lane was never in HEAD.

COMMIT GROUPING: grpc.rs and scheduled_step_decision.rs also hold P2-ec-07 hunks (VerifiedStep and its expiry, the verify_stored_contract body, the constants, check_contract, the execute_scheduled_step_runtime signature and its caller near 2336-2347, the unbound/named helpers) and other items' hunks (F-f03-05 at grpc.rs 511-519, 795-811 and in the test module; F-f05-13 at about 1697-1725, 2211-2250 and 3835). The rel1-only hunks are: scheduled_step_decision.rs 322-337 (the required parameter and the carries_no_contract gate), the contract_required argument of the verify call (grpc.rs about 433-439), the grpc test at 4794-4810, and the rel1 tests in tests/scheduled_step_contract.rs (245-306). The untracked test file holds both P2-ec-07 and rel1 tests. Combining P2-ec-07 and rel1 into one commit is simpler; a P2-ec-07 commit without the gate fails the rel1 tests by design. The F-f03-05 test the_legacy_lane_answers_no_content... uses P2-ec-07's unbound() helper, so it must land after P2-ec-07.

LEFTOVERS:
- Not run: scripts/tests/scheduled-step-config-contract-test.sh, because it reads deploy/.env.example. Its source checks that matter here pass when run by hand: the switch literal and Some("true") in scheduled_step_decision.rs, scheduled_contract_expired in session-core grpc.rs, the SWITCH_ENV constant in scheduled_agent.rs, and sessions.proto tags 14-16.
- Not run: flag-registry and compose Go tests (lead; they need the shared rows).
- Rollout: keep the switch off until session-core serves contract_json, as the fixer said.

RELATED (both low, same area; see the related list).
Scratch logs: C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\p2ec07rel1-verify\ (t1_contract.log, t2_lane.log, t3_decision.log, check_bins.log, fmt_grpc.txt, fmt_decision.txt, fmt_test_file.txt).

## P2-ec-08

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none. This item adds no switch, so it needs no registry row, compose line, migration, admission golden or proto change; the switch it uses already exists. Full lib (2641 tests, 309 s): 2636 passed, 4 ignored, 1 failed, which is the known sandbox::tests::an_egress_call_covers_the_socket_root_and_runs_under_the_bridge. Not blocking: see related (BASH.exe name matching, literal dash trap actions, and a slow Windows-only executor test). The lead commits the file; nothing was edited or committed here.

## P2-ec-09

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none needed. MCP_DO_ELICITATION has its registry row (apps/Control Plane/flag-registry/flag-registry.json line 2098, default false) and its execution-core compose line (apps/Model Plane/deploy/docker-compose.yml line 2294). No golden, proto or migration change. Lead note, predates this unit: EXECUTION_CORE_EXTERNAL_CONTENT_GUARD has no registry row and no compose line.

Staging: agent.rs, resume.rs, grpc.rs, auth.rs and runtime_loop/mod.rs also hold other items' hunks, so stage only this unit's hunks. mod.rs: screen_tool_result and its call in execute_step_inner. resume.rs: question_resume_in_place_allowed, restored_reads_external_content, Restored::seed_held_call, and their tests. agent.rs: resume_questioned_run, begin_question_continuation, consume_question, HeldFormAnswer, HeldCall, answer_held_form, answer_form_call, held_form_test, release_question_answer, cancel_held_form_for, and the S7-b test modules. grpc.rs: the legacy arm (about lines 511-523), RunSandboxDemand, run_sandbox_demand, step_sandbox_demand, thread_workspace_switch, the run_agent and execute_step bearer blocks, thread_bearer_tests and the legacy-lane test. auth.rs: authenticate_run_sandbox_optional and its test. mcp_gateway.rs: elicitation_on, allow_elicitation_when, SECOND_FORM, NO_LONGER_HELD.

## P2-ec-10

Fixer shared_edits:

(a) apps/Control Plane/flag-registry/flag-registry.json: insert this object into flags[] before the object named EXECUTION_CORE_BROWSER_FILL_OTP (that object starts at line 1351, directly after the EXECUTION_COORDINATOR_MODE_ENABLED object that closes at line 1349). Name order holds: COORDINATOR < CORE_BEARER < CORE_BROWSER.
{
  "name": "EXECUTION_CORE_BEARER_EXPIRY_GUARD",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "F-f05-15: refuses or degrades instead of failing mid-call. A browser bearer that expires within 20 s is not used to open a page, act on it or continue a released call: an open is refused as an expired sign-in before the held page is ended or a grant is minted, and an act or a released call is refused before its grant is read back. Cleanup (revoking a grant) still sends it. A thread-scope code_interpreter whose sandbox bearer expires within 20 s keeps no thread workspace: the program runs ephemerally and the result reports the workspace refused as unavailable. Space-run sandbox calls are not covered by this switch. The park-and-re-mint half is F-f05-15b. Only the literal true turns it on.",
  "declarations": [
    { "service": "execution-core", "env": "EXECUTION_CORE_BEARER_EXPIRY_GUARD", "default": "false" }
  ]
}

(b) apps/Model Plane/deploy/docker-compose.yml: insert after line 1949 (the EXECUTION_CORE_THREAD_WORKSPACE line) inside the execution-core service, with a six-space indent, these two lines:
      # F-f05-15: a browser bearer about to expire is refused, and a thread workspace whose sandbox bearer is about to expire is skipped. Off by default; only the literal true turns it on.
      EXECUTION_CORE_BEARER_EXPIRY_GUARD: ${EXECUTION_CORE_BEARER_EXPIRY_GUARD:-false}

(c) None. No migration, no admission golden or .sha256 change, no proto or generated-code change, and no switch_spellings.rs change (spec item c).

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (the lead copies these; the fixer's purpose text must NOT be copied as sent):
(a) apps/Control Plane/flag-registry/flag-registry.json: insert this object into flags[] before the EXECUTION_CORE_BROWSER_FILL_OTP object (after the closing brace of EXECUTION_COORDINATOR_MODE_ENABLED, around line 1350):
{ "name": "EXECUTION_CORE_BEARER_EXPIRY_GUARD", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f05-15: a browser open, act or released call on a bearer expiring within 20 s is refused; a thread-scope code_interpreter on one runs ephemerally, workspace refused. Cleanup and Space runs: not covered. Only the literal true turns it on.", "declarations": [ { "service": "execution-core", "env": "EXECUTION_CORE_BEARER_EXPIRY_GUARD", "default": "false" } ] }
Why the correction: registry.go sets maxPurposeRunes = 240 and validateFlag rejects longer purposes, so the fixer's 696-rune purpose would break the registry load test. The corrected purpose is 239 runes, and it satisfies the literal rule (the purpose must contain "literal true" at a word boundary). Verified in memory with node: 429 to 430 flags, no duplicate names, name order holds (COORDINATOR_MODE 104, this row 105, BROWSER_FILL_OTP 106), JSON round-trips.
(b) apps/Model Plane/deploy/docker-compose.yml: insert after line 1949 (the EXECUTION_CORE_THREAD_WORKSPACE line), inside the execution-core service (its header is line 1846; no other service header or column-0 key intervenes), with six-space indent:
      # F-f05-15: a browser bearer about to expire is refused, and a thread workspace whose sandbox bearer is about to expire is skipped. Off by default; only the literal true turns it on.
      EXECUTION_CORE_BEARER_EXPIRY_GUARD: ${EXECUTION_CORE_BEARER_EXPIRY_GUARD:-false}
(c) None. No migration, no admission golden or .sha256, no proto or generated code. switch_spellings.rs needs no row: its EXACT_SWITCHES lists only the former lenient QUARRY switch.

Nit (no code change): the fixer's summary wording "exp at or past now+margin is true" is inverted. The code is exp - now <= margin, and the tests pin exp = now+20 as true and now+21 as false.

## P2-ec-10-rel1

Fixer shared_edits:

Only the file workspace_sync.rs is touched by this item. No registry rows, compose lines, migration numbers, golden or proto changes are needed here. It depends on P2-ec-10's uncommitted auth.rs hunks (BEARER_EXPIRY_GUARD_ENV, BEARER_EXPIRY_MARGIN_SECS, bearer_expiry_guard_enabled, expires_within, DelegatedSandboxBearer::expires_within and DelegatedBrowserBearer::expires_within), so stage those with P2-ec-10 or before this commit. For the rel1 commit, stage only the workspace_sync.rs hunks, including the thread_bearer_expiring doc (lines around 69-76), bearer_expiring_refusal, commit/commit_guarded, the apply-time check, commit_unless_expiring, SlowPuts, rig/rig_with_cas, and the new test. The switch EXECUTION_CORE_BEARER_EXPIRY_GUARD still needs its flag-registry row and compose line from the P2-ec-10 commit, which this fix does not add.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED (for the lead):
1. workspace_sync.rs is the only file this fixer listed, but runtime_loop/mod.rs carries hunks the file's own comments label P2-ec-10-rel1 and that the fixer did not list: the post-lock bearer re-check (about lines 2547-2555), the save call site commit_unless_expiring(bearer_guard, now) (about 2597-2599), and the test a_bearer_that_expires_while_the_thread_lock_is_awaited_sends_nothing (about 8869-8926). The call site is what makes the save re-check take effect in production. commit_unless_expiring is not in HEAD, so commit the runtime_loop/mod.rs guard hunks with P2-ec-10 or with this item, not after.
2. Stacking: this fix depends on P2-ec-10's uncommitted hunks in workspace_sync.rs (thread_bearer_expiring, the entry check in commit_unless_expiring, and the tests a_save_inside_the_bearer_margin_sends_no_bearer and a_thread_bearer_is_expiring_only_inside_the_margin_and_with_the_guard_on). It also depends on auth.rs (BEARER_EXPIRY_GUARD_ENV, BEARER_EXPIRY_MARGIN_SECS, bearer_expiry_guard_enabled, expires_within, DelegatedSandboxBearer::expires_within, DelegatedBrowserBearer::expires_within). The two items interleave in workspace_sync.rs and runtime_loop/mod.rs, so the lowest-risk path is one commit for P2-ec-10 plus this fix, or a hunk split. This item's own hunks are the bearer_expiring_refusal helper, the commit to commit_guarded split with the apply-time check, the commit_unless_expiring call into commit_guarded, SlowPuts, rig_with_cas, and the new test.
3. Switch EXECUTION_CORE_BEARER_EXPIRY_GUARD: no row exists in apps/Control Plane/flag-registry/flag-registry.json and no line in apps/Model Plane/docker-compose*.yml (both checked). P2-ec-10 needs the flag-registry row (default off, strict "true") and the execution-core compose line. The fixer's note is correct that this fix adds neither.
4. This fix needs no registry, compose, migration, golden or proto change.

Leftovers: no Docker, DB or harness was run; no .env file was read; no file was edited and no git write was done. The call-site test gap is recommended only (see related).

## P2-ec-11

Fixer shared_edits:

Unchanged from the accepted attempt.

1) **Compose.** File apps/Model Plane/deploy/docker-compose.yml, execution-core environment block, next to `EXECUTION_CORE_THREAD_WORKSPACE`:
       EXECUTION_CORE_BEARER_EXPIRY_PARK: ${EXECUTION_CORE_BEARER_EXPIRY_PARK:-false}
   If P2-ec-10's guard line is not in yet, it is also needed. That line and its registry row are P2-ec-10's shared edit, not this item's:
       EXECUTION_CORE_BEARER_EXPIRY_GUARD: ${EXECUTION_CORE_BEARER_EXPIRY_GUARD:-false}

2) **Flag registry.** File apps/Control Plane/flag-registry/flag-registry.json, add to flags[]:
{"name":"EXECUTION_CORE_BEARER_EXPIRY_PARK","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"Parks a Do run whose browser or sandbox bearer is about to expire (F-f05-15b) at the next round boundary with park reason credential_expiring, so a resume can carry freshly minted bearers. Needs EXECUTION_CORE_BEARER_EXPIRY_GUARD, EXECUTION_LOOP_PARKING_ENABLED and EXECUTION_LOOP_RESUME_ENABLED. Off: the bearer guard's typed refusal stays. Only the literal true turns it on.","declarations":[{"service":"execution-core","env":"EXECUTION_CORE_BEARER_EXPIRY_PARK","default":"false"}]}

3) **Proto comments only.** Add `credential_expiring` to the closed token lists in apps/Model Plane/proto/model_plane/v1/execution.proto (RunAgentResponse.park_reason, about lines 614-615) and runs.proto (RunDetail.park_reason, about lines 474-475). These comments are copied into go/gen/model_plane/v1/{execution,runs}.pb.go, so regenerate the bindings if a generated-code drift check runs.

4) No migration and no admission golden change.

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED:
1) Compose. File apps/Model Plane/deploy/docker-compose.yml, execution-core environment block, next to `EXECUTION_CORE_THREAD_WORKSPACE` (line ~1949):
    EXECUTION_CORE_BEARER_EXPIRY_PARK: ${EXECUTION_CORE_BEARER_EXPIRY_PARK:-false}
Neither line is in compose yet. P2-ec-10's guard line is needed too, but it is P2-ec-10's shared edit:
    EXECUTION_CORE_BEARER_EXPIRY_GUARD: ${EXECUTION_CORE_BEARER_EXPIRY_GUARD:-false}

2) Flag registry. File apps/Control Plane/flag-registry/flag-registry.json, add to flags[]:
{"name":"EXECUTION_CORE_BEARER_EXPIRY_PARK","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"Parks a Do run whose browser or sandbox bearer is about to expire (F-f05-15b) at the next round boundary with park reason credential_expiring, so a resume can carry freshly minted bearers. Needs EXECUTION_CORE_BEARER_EXPIRY_GUARD, EXECUTION_LOOP_PARKING_ENABLED and EXECUTION_LOOP_RESUME_ENABLED. Off: the bearer guard's typed refusal stays. Only the literal true turns it on.","declarations":[{"service":"execution-core","env":"EXECUTION_CORE_BEARER_EXPIRY_PARK","default":"false"}]}

3) Proto comments only. Add `credential_expiring` to the closed token lists in:
- apps/Model Plane/proto/model_plane/v1/execution.proto, RunAgentResponse.park_reason comment (line ~615, after "reauth_required");
- apps/Model Plane/proto/model_plane/v1/runs.proto, RunDetail.park_reason comment (line ~475).
Then regenerate go/gen/model_plane/v1/{execution,runs}.pb.go, plus the Python/TS bindings if the drift check covers comments.

4) No migration, no admission golden change.

## P2-ec-12

Fixer shared_edits:

"None needed: there is no new switch (the permission_rules_enabled() gate covers it), so no registry row, compose line, admission golden (apps/Model Plane/api/contracts/agent-action-admission.json, untouched), proto, or migration. Commit for the lead: capability_policy.rs (the four R-ec-01 switch-off tests plus the R-ec-07 hunks) and the verification log, with a message like 'fix(execution-core): withhold the command an expanding option value or operand may split into (P2-ec-12)' and the trailer 'Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>'. Both units touch the same file, so for two commits use git add -p: R-ec-01 is the four tests with_the_permission_rules_off_a_long_run_of_assignments_after_time_names_no_command, ..._a_trap_action_shaped_like_an_option_names_no_command, ..._an_alias_operand_whose_name_expands_names_no_command, and ..._the_shells_the_lists_name_no_command (the only uncommitted hunk at the start, around lines 12760-12870); R-ec-07 is everything else. Then replace the COMMIT PENDING cells in the log with the SHA."

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none. There is no new switch, so no registry row, compose line, admission golden, proto or migration is needed. The existing CAPABILITY_PERMISSION_RULES_ENABLED gate covers both units. The fixer's shared_edits text is correct and complete.

LEAD NOTES (not blocking): (a) The V3e-ec-1/2/3 and rel1/rel2 "COMMIT PENDING" cells misstate where the code is. That code is in HEAD already: ec-1/ec-2 in f0046e523 and b08addeb1, ec-3 shell lists in 22f882224. Cite those SHAs for the code and the new commit for the pins and the R-ec-07 readings. The V3e-ec-1 re-verdict is PASS for the P2-ec-12 scope. (b) The other modified execution-core files in the tree (auth.rs, grpc.rs, runtime_loop/*, tests/*, untracked scheduled_step_contract.rs) belong to other items and are not part of this fix. (c) One commit is fine. The R-ec-07 hunks do not depend on the switch-off pins, so a git add -p split is also possible. (d) RELATED HIGH: the POSIX shell reader reads expanding option words and option values literally. A -c string the shell really runs can then be a command or a workspace script the policy never names. This is the same root cause in a reader this spec does not name, so it does not fail this item. Open a follow-up unit: apply may_split_into_words to option-shaped words and to skipped option values in shell_script_arguments and command_string_present, returning None so the command is withheld. Pin sh -$X -c 'echo hi', sh -o $X -c 'echo hi', bash --rcfile $X -c 'echo hi' and sh -c -o $X 'echo hi' so each contains an empty name, and keep sh -o pipefail -c 'rm x' as ["sh","rm"].

## P2-ec-12-rel1

Verifier's corrected text (supersedes the fixer's where they differ):

SHARED: none needed. The fix adds no switch, flag-registry row, compose line, migration, proto, or golden. The lead should copy nothing into shared files for this item.

Scope note: the file also holds other items' hunks (wrapped_from, operand_names, may_split_into_words and their tests). I did not judge those; this verdict covers only the hunks listed above.

Action for lead: rerun `cargo test -p execution-core --lib capability_policy::` (CARGO_TARGET_DIR=C:/dev/mp-target-w345, RUST_MIN_STACK=16777216) after the clean, expect 240 passed, 0 failed (the earlier 239-pass run used a binary built before the shells-the-lists test landed in the file, so it did not run that test). The earlier run passed everything it ran.

# Items that did not pass (files left uncommitted)

## P2-cp-07-rel1 (FAIL)

Title: related to P2-cp-07: Adds an org-core credential with sessions:read to the Control Plane Go session-c

Verifier feedback:

FAIL. The change meets the spec and is correct in nearly every part, but the guard it rewrites still accepts one public literal form. One fix is required in the fixer's own code.

REQUIRED (apps/Control Plane/org-core/internal/clients/session_thread_retention.go, isPrivateNetworkTarget). A zone is valid only on an IPv6 literal. The function passes a zoned IPv4 host to netip.ParseAddr, the parse fails, and a failed parse is read as a name, so the host is accepted. A verbatim copy of the function returns true for "8.8.8.8%eth0:9091" and "[8.8.8.8%eth0]:9091", while the zoned public IPv6 literal and the zoned IPv4-mapped public literal return false. Go's net package and grpc v1.80.0 do not dial a zoned IPv4 literal (they resolve the string as a DNS name), so I found no path that sends the token to 8.8.8.8. Severity is low. But the doc comment says a zoned public literal is refused, and no test pins this form. Change: when the host contains a percent sign, the part before it must parse as an IPv6 address (netip Is6), otherwise return false; keep the zoned IPv6 path as it is. Add "8.8.8.8%eth0:9091" and "10.0.0.5%eth0:9091" to the refused list of TestPlaintextIsPrivateNetworkOnly, keep "[fe80::1%eth0]:9091" accepted, and make the doc comment say a zone is accepted only on an IPv6 literal. Then re-run go test ./internal/clients/ ./cmd/server/ ./internal/retention/ -count=1 and gofmt -l on the touched files.

FILES: the list must also include apps/Control Plane/org-core/internal/clients/session_core_credentials_compose_test.go (the compose-pin test the spec asks for) and apps/Control Plane/org-core/internal/clients/session_thread_retention_grpc_test.go (the binding tests for the same client). Both are untracked and belong to this commit.

CHECKED AND CORRECT: the new cmd/server test meets the spec (nil reader, error names ORG_RETENTION_THREAD_RECEIPT_CHECK, no key in the error, zero client builds). Switch-off passes a nil interface, as at HEAD, and builds no token source and no client. Refusals name the switch and never the key or an address value. No token, id or receipt content reaches a log or an audit row; the service logs only the closed word. Each token is minted per organisation, and session-core's check is an exact org match. It admits only principal service:org-core with session:retention:read and refuses a zdr credential, so the persistent posture in the registry is required. The two compose-pin tests stay red until SHARED item 1 is applied, as expected.

SHARED (for the lead; the fixer's list, corrected):
1. apps/Control Plane/docker-compose.yml, session-core block (lines 849-854): delete the three "# P2-cp-07:" comment lines and set the value line to HEAD line 850 exactly: [{"principal":"verevon-gateway","audience":"session-core","token":"${SESSION_CORE_SERVICE_TOKEN:?SESSION_CORE_SERVICE_TOKEN is required}","scopes":["sessions:read","sessions:write"]}]. The test constant equals HEAD line 850 byte for byte (checked).
2. Same file, org-core build block (line 592): at 12 spaces add additional_contexts:, and at 16 spaces model-plane-go-gen: "../Model Plane/go/gen" and model-plane-servicetoken: "../Model Plane/go/pkg/servicetoken". This is the user-core shape (line 429). The names match the Dockerfile COPY --from lines.
3. Same file, org-core environment, after ORG_RETENTION_POLICY_ENABLED (line 659), add:
   ORG_RETENTION_THREAD_RECEIPT_CHECK: ${ORG_RETENTION_THREAD_RECEIPT_CHECK:-false}
   SESSION_CORE_RETENTION_ADDR: ${SESSION_CORE_RETENTION_ADDR:-model-session-core:9091}
   ORG_CORE_SESSION_SERVICE_API_KEY: ${ORG_CORE_SESSION_SERVICE_API_KEY:-}
   The default address is verified: the Model Plane deploy compose gives the session-core service the alias model-session-core on inter-plane-bus (about line 1552), and org-core joins inter-plane-bus (about line 697). AUTH_CORE_URL is already in org-core's environment (http://auth-core:3011); do not add it again. The key is the consumerEnv of the new principal, and this line hands it to org-core.
4. apps/Model Plane/deploy/docker-compose.yml, session-core environment (near line 1241): add SESSION_THREAD_RETENTION_READ: ${SESSION_THREAD_RETENTION_READ:-false}. Not the literal "true" the fixer wrote: the switch stays dark by default and the owner turns it on per deploy, like SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE on the line below.
5a. apps/Control Plane/flag-registry/flag-registry.json: ORG_RETENTION_THREAD_RECEIPT_CHECK as the fixer wrote it, shaped like the ORG_RETENTION_POLICY_ENABLED row: kind switch, onLiteral "true", owner control-plane, source compose, default "false", stage dark, purpose naming the literal "true", declarations [{ "service": "org-core", "env": "ORG_RETENTION_THREAD_RECEIPT_CHECK", "default": "false" }], codePath apps/Control Plane/org-core/internal/retention/types.go.
5b. CORRECTED: SESSION_THREAD_RETENTION_READ must have source "compose", not "code", because item 4 sets it in compose. Add declarations [{ "service": "session-core", "env": "SESSION_THREAD_RETENTION_READ", "default": "false" }]; the registry names Model Plane session-core "session-core". Keep the rest as the fixer wrote: owner model-plane, kind switch, onLiteral "true", default "false", stage dark, codePath apps/Model Plane/rust/services/session-core/src/thread_retention_read.rs, purpose naming the literal.
5c. No rows for the address or the key.
6. apps/Control Plane/config/plane-service-principals.json: add the org-core principal in alphabetical order, after orchestrator-core and before quarry-edge, with the fixer's shape: tenancy directory, audiences ["session-core"], consumerEnv ORG_CORE_SESSION_SERVICE_API_KEY, orgIds [], retentionByAudience {"session-core": "persistent"}, scopes ["session:retention:read"], scopesByAudience {"session-core": ["session:retention:read"]}. "persistent" is required, because session-core refuses a zdr credential on this RPC. This file already carries another item's hunk (model-gateway gains cost-core, F-f10-23a), so stage

Fixer leftovers:

- Transport: the bearer token travels in plaintext on the inter-plane bus, because session-core has no TLS listener. Whether to add TLS is the owner's decision.
- Revocation: a rotated or revoked org-core token can still be served from cache for up to an hour, because Invalidate cannot be reached through TokenSource.
- Needs harness (lead): docker build with the named contexts; an Auth Core mint that session-core accepts; a receipt read over model-session-core:9091 that returns found or nothing-retained; with the switch on and session-core down, a thread hold is still placed.
- No .env file was read. No git writes, no docker.

## P2-cp-09-rel1 (FAIL)

Title: related to P2-cp-09: Nothing implements ResolveScheduledRunAuthority and main.go never wires a resolv

Verifier feedback:

VERDICT FAIL (round 2). The change is the right fail-closed interim and keeps switch-off unchanged: the boot check is a no-op with the switch off, and the switch reader at cmd/server/main.go:435 is the strict literal "true". The same refuse-to-start pattern already exists for this switch in capability-core (apps/Model Plane/go/services/capability-core/cmd/main.go:410-412). Three things block PASS.

REQUIRED 1 (fixer, test; only cmd/server/schedule_contract_wiring_test.go changes). The boot pins do not pin what they claim. Mutants run through go test -overlay (repo untouched): M1 flips the condition to err == nil, so every start with the switch off would die, and both pins still PASS. M7 wraps the check in if false and M8 in an uncalled closure; both PASS, so the check can be dead and the original 503 bug returns silently. Change TestScheduleContractBootCheckRefusesTheStartWhenItFails so that: (a) it finds the check among the direct statements of func main's body (range over body.List), not via ast.Inspect; (b) it requires ifStmt.Cond to be a *ast.BinaryExpr with Op token.NEQ, whose X is an identifier equal to the Init's LHS identifier and whose Y is the identifier nil; (c) it keeps the existing log.Fatal, log.Fatalf, log.Fatalln or os.Exit detection inside the if body. A prototype is at C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/verifier-rel1/hardened/M0_identity_test.go (the new function is at the end of the file; restore its parse path to "main.go"). With it, the real main.go PASSES, and M1, M2, M3, M4, M6, M7, M8 and M9 FAIL. Optional: M5 (check moved after the server start goroutines) still passes; also requiring the check to precede httpServer.Start (main.go:528-533) would close it.

REQUIRED 2 (staging, lead; no code change). The staged set does not compile on its own. internal/spaces/scheduled_run_decision.go:122 uses ErrScheduledContractExpired, which is declared only in the step-side hunk of internal/spaces/scheduled_step_decision.go (the "errors" import and the declaration near line 208). Verified on a git-archive HEAD copy plus this item's files: internal/spaces fails to build. Fix one of two ways: land P2-cp-09's scheduled_step_decision.go first, or add to this set just the "errors" import and this declaration with its two-line comment: var ErrScheduledContractExpired = errors.New("scheduled_contract_expired"). With those added, go vet is clean and the item's tests pass.

ORDER (lead). Commit this boot check in the same commit as, or before, spaces.go's switch-on run hunk, never after it, or the live 503 bug is back for one commit.

RUN HUNK (not in this set; reviewed read-only). With the switch on, boundScheduledRunExecution is fail-closed: 503 when unresolved, 400 on a malformed intent, 403 on any binding mismatch or an expired prepared contract, and it never binds a body-claimed contract. No change needed here.

OPTIONAL (lead). docker-compose.yml lines 511-513 can add: "On, a start is refused at boot until the Session Core prepared-run resolver is wired (F-f10-27)." The value stays false.

SHARED:
1. flag-registry.json, SCHEDULE_CONTRACT_ENABLED purpose (line 3956): leave unchanged. The current 240-rune text is valid and pinned; the repo tests TestControlPlaneSwitchesAreRegisteredAsPinned and TestScheduleContractSwitchNamesItsBlocker PASS. If the owner wants the On behaviour recorded, this 238-rune text also loads and passes both pins (checked on a scratch copy; its failure set matched the current text's): "WS20-S4: stored-consent schedule contracts. Off, a create with a contract is refused. On, boot is refused until a prepared-run resolver is wired (F-f10-27). Blocked by depth-register D0-010: keep it off. Only the literal true turns it on." Do not use the 427-character round-1 text.
2. Verification log entry, corrected: "P2-cp-09-rel1 (F-f10-27 rel1): user-core's boot check, ValidateScheduleContractWiring in cmd/server/main.go (line 499), refuses a start with SCHEDULE_CONTRACT_ENABLED=true until a Session Core prepared-run resolver is installed. Interim fix, fail-closed, default off; a switch-off start is unchanged. Open for the owner: a switch-on start takes all of user-core down, including schedule creation; not recorded as accepted. The boot-check pins (func main body only) require the check to be a direct statement of main with condition err != nil and a log.Fatal or os.Exit body, the switch and every resolver wired before it, and the check to run once. Residual, unchanged here: with the switch off, the run-execution decision still binds a contract the request body claims (spaces.go, else-if branch)."
3. Staging: as REQUIRED 2.
4. Compose comment: optional, as above; value unchanged.

LEFTOVERS:
- Owner decision, not a verifier FAIL reason: with the switch on, user-core will not start. That takes down schedule creation and the rest of user-core, not only scheduled runs (which is what the 503s did). The spec's two named options (wire the resolver, or keep the run side out) are not met literally; the boot refusal is the fail-closed interim. Must be accepted or changed before the switch is ever set to true. No production implementer of ResolveScheduledRunAuthority exists (only test fakes), and main.go never calls SetScheduledRunAuthorityResolver.
- The repo flag-registry package has 2 failing tests from other tracks: TestErasureHoldCheckIsRegisteredAndDeclaredByBothReaders (ERASURE_HOLD_CHECK has no registry entry) and TestSwitchSpellingTablesAgreeWithTheRegistry (SESSION_THREAD_RETENTION_READ has no row). Neither involves this switch.
- No .env file was read, no git write or docker command was run, and no repo file was edited. Mutants and the staged-set copy live in the verifier scratch directory.
- ZDR: this change touches no ZDR path; the create-side ZDR refusal tests still pass.

VERIFIED OK: gofmt clean; go vet clean; go build ./... clean; switch-off is a no-op; the switch reader is the strict liter

Fixer leftovers:

- No DB tests are involved, so no harness is needed.
- Still open for the owner, unchanged from round 1: a start with SCHEDULE_CONTRACT_ENABLED=true takes all of user-core down, including schedule creation, until a Session Core prepared-run resolver exists. This is not recorded as accepted.
- The real resolver (something that implements ResolveScheduledRunAuthority, wired in main.go) is still not built. The boot check is the fail-closed interim.

## P2-mg-12-rel1 (FAIL)

Title: related to P2-mg-12: A presence check on a helper's value parameter inside that helper's own body pas

Verifier feedback:

FAIL. What passes: the spec pins pass (inference-core switch_spellings 28/28, and the presence test passes in a standalone harness that uses the verbatim helper section and presence test). The helper section (crate_helpers through param_names) and the presence test are byte-identical in the two copies. The switch-off path is unchanged (all of this item's hunks are inside the cfg(test) module). But the presence rule is still narrower than the rule it claims to enforce ("a place followed by a presence check is never vouched for", and "a reader takes the place's value, not a value chosen on it"). 16 wrapped-presence reads are still accepted as exact (probes below). Required changes, to be applied identically to both copies: (1) Presence inside a wrapper, through a crate helper or a reader alias. vouches_for's last branch lets any enclosing crate helper vouch for a place even when the place is presence-checked through a group, a path call or matches!. Accepted now: exact_param((raw).is_some().then_some("true")); exact_param(Option::is_some(&raw).then_some("true")); exact_param(matches!(raw, Some(_)).then_some("true")); and beside an exact read, switch_from(raw) || exact_param(matches!(raw, Some(_)).then_some("true")). presence_on_place only follows a dot directly after the place, so the closing paren or comma after the place hides the check. Fix: walk the place's whole value chain out to the vouching call. Walk out of a group that wraps the place, out of a call whose whole argument the place is (var(X), get(X), lookup(X)), out of path calls such as Option::is_some(&raw), and through matches!(place, pattern) unless the pattern is an exact value. Any presence link (is_some, is_none, is_ok, is_err) or any link that is not in ADAPTERS means the place is not vouched, in every branch (helper, reader, alias, comparison, match, matches!). Keep allowed: the exact is_some_and/is_ok_and closure (chain_reads_by_closure) and the reader-by-value map (chain_passes_to_reader), plus the links after the vouching call. (2) Presence applied to the call that takes the place, then a value chosen on it. The place sits inside var(X), so presence_on_place sees the closing paren and returns false. The reader branch then vouches, because in_value_position scans only the text before the place. Accepted now: switch_from(std::env::var(X).is_ok().then_some("true")); switch_from(std::env::var(X).ok().is_some().then_some("true")); switch_from(matches!(std::env::var(X).ok().as_deref(), Some(_)).then_some("true")); exact_reader(std::env::var(X).is_ok().then_some("true")) with exact_reader in the crate; the bound form let raw = std::env::var(X).ok(); switch_from(matches!(raw.as_deref(), Some(_)).then_some("true")); the comparison std::env::var(X).is_ok().then_some("true") == Some("true") (the comparison branch vouches the other whole side and never checks the place's own side); and the match std::env::var(X).is_ok().then_some("true") { Some("true") => true, _ => false }. Fix: the change-(1) chain walk must run before the reader, comparison and match branches as well, and the place's own side must be adapter-only. (3) The alias branch skips the value-position rule. Accepted now: let on = switch_value_is_on; on(if std::env::var(X).is_ok() { Some("true") } else { None }) and let on = switch_value_is_on; on(std::env::var(X).is_ok().then_some("true")). The enclosing-alias check in vouches_for has no in_value_position test. Fix: apply the same in_value_position test to an alias call that switch_from( gets, and the change-(1) walk. (4) Pins, in a_presence_check_on_a_helpers_value_parameter_fails_beside_its_exact_read, identical in both copies, keeping every existing assert. MUST FAIL (helper): exact_param((raw).is_some().then_some("true")); exact_param(Option::is_some(&raw).then_some("true")); exact_param(matches!(raw, Some(_)).then_some("true")); switch_from(raw) || exact_param(matches!(raw, Some(_)).then_some("true")). MUST FAIL (statement): switch_from(std::env::var(X).is_ok().then_some("true")); switch_from(matches!(std::env::var(X).ok().as_deref(), Some(_)).then_some("true")); std::env::var(X).is_ok().then_some("true") == Some("true"); match std::env::var(X).is_ok().then_some("true") { Some("true") => true, _ => false }; let on = switch_value_is_on; on(if std::env::var(X).is_ok() { Some("true") } else { None }); let on = switch_value_is_on; on(std::env::var(X).is_ok().then_some("true")). MUST PASS (add): let on = switch_value_is_on; on(std::env::var(X).ok().as_deref()); and std::env::var(X).ok().as_deref() == Some("true"). Keep all current must-pass and must-fail items. (5) Docs: vouches_for's doc ("whatever it sits beside or is passed to") and the presence test's doc ("fails beside its exact read too") must describe the rule as the fixed code implements it. Optional, spec-acknowledged open (the change-(1) walk also closes them): exact_param(raw.map(|_| "true")); switch_from(std::env::var(X).ok().map(|_| "true").as_deref()); match raw { Some("true") => true, Some(_) => true, _ => false } (only the exact arm is checked). SHARED: none. This item's hunks are all inside the cfg(test) module of both files. The one production hunk in model-gateway (the EXACT_SWITCHES row at line 103, thread_goal::SETTLE_IN_DISPATCH_ENV) belongs to the P2-mg-12 base. No flag-registry row, compose line, proto, golden or migration is needed for this item. NOT VERIFIED: the model-gateway lib test and the production drift scan do not compile (see related, session_flow.rs E0061). The production shapes the probes accepted are the sse.rs and tools.rs closures, budget lookup(X).as_deref() == Some, and nats var(X).as_deref() == Ok. The other listed readers were not run. No file was edited, no git write was made, and no .env file was read.

Fixer leftovers:

1. The lead should run the model-gateway lib test (`cargo test -p model-gateway --lib switch_spellings`) once the session_flow.rs E0061 break (lines 4201 and 4213, a missing 6th argument) is fixed by its owner. Its helper section and presence test are identical to inference-core's, which passes 28/28, but the model-gateway production scan has not been run.
2. Clean-up for the lead: delete the scratch workspace copy under the scratchpad's mp folder; its api/proto/skills and other sibling folders are directory junctions, so remove the junctions themselves, not what they point to. Also delete the target dir C:/dev/mp-target-w345-scratchcopy-p2mg12, which may have been partly created.
3. Still open, listed by the spec as related rather than required:
- A transform not named in LENIENT_READS still turns presence into the literal: `switch_from(raw.map(|_| "true"))`, `raw.map_or(false, |_| true).then_some("true") == Some("true")`, `switch_from(raw.or(Some("true")))`.
- The shadowed alias.
- A match that holds the exact arm next to a lenient one (`match raw { Some("true") => true, Some(_) => true, _ => false }`) is still accepted. The spec only asks for an exact arm pattern.
4. The 8 test-only helpers I added (side_start, side_end, keyword_at, chain_links, etc.) sit inside the shared section and are in both copies.

## P2-mg-13 (NONE)

Title: model-gateway: count idempotency claims and capacity refusals, and evi; model-gateway: a duplicate send waits briefly for the original to beco; model-gateway: send the stable-prefix tier plan on Anthropic routes so

Verifier feedback:

(none: the verifier did not return)

Fixer leftovers:

1. The in-tree model-gateway lib test target does not compile: session_flow.rs lines 4201 and 4213 call append_prompt_to_fresh_run with 5 arguments instead of 6 (refused_message is missing). This belongs to another item and the file has not changed since 2026-11-24. Once its owner fixes it, re-run `cargo test -p model-gateway --lib prompt_context` and `idempotency_registry` in the tree. I ran them in the scratch copy at C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\mpfix, which can be deleted.
2. After the lead applies the shared rows, re-run the flag-registry Go tests (TestShippedRegistryLoads, TestShippedRegistryMatchesComposeFiles) in the real tree with GOWORK=off.
3. Not this item: switch_spellings every_reader_of_a_listed_switch_reads_its_exact_literal fails on MODEL_GATEWAY_ARTIFACT_EDITING. TestSwitchSpellingTablesAgreeWithTheRegistry also flags MODEL_GATEWAY_GOAL_SETTLE_IN_DISPATCH and SESSION_THREAD_RETENTION_READ. There is a rustfmt diff at sse.rs line 5615 (QF-10).
4. Not a blocker: I worded ZDR.md row 159 on the basis that, under DEC-32's 'for new stores' scope and section 5.5, a waived run keeps this in-memory response cache. If the owner wants waived runs kept out of this cache as well, that is a separate decision and a code change.

## P2-spa-10 (FAIL)

Title: SPA sidebar: server-paged active thread list behind VITE_CHAT_SIDEBAR_; SPA sidebar: pass thread state and the search-hit opener to the organi

Verifier feedback:

VERDICT FAIL (round 4 for P2-spa-10: F-f02-08 + F-f03-11). Everything the spec names is built and pinned by tests, the switch-off path is unchanged, the round-3 fixes hold (palette star announce gated; restore keeps the server copy in the paged path), and the checks are green. Three changes are needed. Nothing else in the fix needs to move.

REQUIRED 1 (owner decision, not spec): remove the transcript prune from the paged path. In CoreSidebar.tsx, delete the pruneChatThreadTranscripts() call in refreshFirstPage (line ~461) and its import (line ~41). In chat-thread-history.ts, delete pruneChatThreadTranscripts (line ~618). Delete the tests that pin it: CoreSidebar.chat-sessions.test.tsx 'drops the stored transcript of a chat the first page no longer holds...'; chat-thread-history.test.ts 'drops the transcripts the cache no longer holds...' and 'writes nothing when every stored transcript is still wanted'. Why: the spec does not ask for it, and the fixer's own report says it needs an owner decision, which the lead's rule says is not implemented in the fix. use-chat-controller.ts line 1684 says the server transcript is envelope-only, so task steps, tool calls and telemetry exist only in the local transcript. The prune also deletes the only copy of a chat whose snapshot save never reached the server: flushServerThreadSnapshot swallows the failure (line ~799), and after the marker that chat is not re-migrated. Record both as open owner decisions. The cost to record: erased or off-page transcripts then stay until the 40-entry transcript cap (maxThreadTranscriptItems) evicts them.

REQUIRED 2 (a stale page re-adds an archived or deleted chat): loadMoreSessions (line ~470) and loadStarredSessions (line ~493) merge a page fetched before the request into the cache after the await. archiveThread (line ~742) and deleteThread (line ~847) remove the row with no refresh. So a Load more or starred page still in flight lists that chat again, and it stays in the active list until the next mount. refreshFirstPage has the same exposure if an archive lands during the first request. Fix: record the threadIds that archiveThread or deleteThread remove while a request is in flight (for example a per-thread removal counter compared with the counter taken when the request started), and drop those ids from the page before it is merged. Apply it to loadMoreSessions, loadStarredSessions and refreshFirstPage. Add a test: start Load more with a deferred page, archive a chat from the loaded window, resolve the page that still lists it, and assert the archived chat is not in the active list.

REQUIRED 3 (missing test for the marker invariant): spec (1) sets the marker only after migrateLocalSessions finishes with a complete listing, and until then keeps the whole walk 'so no legacy local thread is lost'. No test pins the failure case. Add one: saveChatThreadSnapshot rejects; then CHAT_SIDEBAR_MIGRATED_KEY is still unset, listChatThreadsPage is not called on the next mount, and listChatThreads is called again.

VERIFIED AND ACCEPTED (no change needed):
- Switch off: refreshServerSessions, widenOrLoadMore, shownSessions and hasMoreSessions, the organizer's matches, hasMore and Starred button, handleAttentionChanged (refreshOpenThreadState is gated), historyRowLimit = 40, threadStateOf returns {} when off, and the palette star announce are all unchanged. The marker is written only with the switch on, and every marker read and write is in try/catch.
- First page is listChatThreadsPage({}); Load more uses { cursor } and appends with the fresh copy winning (replaceChatThreadHistory keeps the first occurrence); the starred view is the first page only (listChatThreadsPage({ starredOnly: true }), not a walk), which matches the spec's 'first page'.
- The server flags follow the carry rule: a local write keeps them, a server list replaces them, and the normaliser keeps only true or a non-blank trimmed id. The ChatThreadSession mapping and the starredOnly/cursor params are HEAD code in chat-client.ts.
- The palette star and unread actions act only on the open chat (openChatThreadId), so refreshing the open chat's flags on an attention change is the correct target. markChatThreadRead's announce is HEAD code.
- ZDR: listChatThreadsPage still applies the retention verdict on every page. No new store, no thread titles written, no console calls in the added lines.
- Organizer: a failed starred load keeps the loaded subset and shows an error line; the Starred view hides Load more; a withdrawn starred set turns the filter off.
- CSS: tokens only, 36px rows, focus-visible outline, one-line title, two-line snippet clamp, no animation. ChatSearchResults renders the classes it styles, including the aria-live status and the mark runs.
- Nits, accepted: the starred failure line is the organizer's own element with the sidebar's generic text (the spec wanted the existing one); chatSidebarThreadStateEnabled lives in chat-thread-history.ts, not next to threadForkEnabled.

SHARED (for the lead; both checked read-only, and the fixer's text is right):
A. apps/Control Plane/flag-registry/flag-registry.json (LF): insert after line 5155 (the '    },' that closes VITE_CHAT_READER_POLISH) and before line 5156 (the '    {' that opens VITE_CHAT_SPEED_TIER). Two objects, 4-space indent, each followed by a comma:
    {"name": "VITE_CHAT_SIDEBAR_SERVER_PAGING", "kind": "switch", "onLiteral": "true", "owner": "frontend-plane", "source": "code", "default": "false", "stage": "dark", "purpose": "F-f02-08: the chat sidebar lists its active chats one server page at a time, with Load more, once this browser's local chats have moved to the server (one-time marker verevon.chat.sidebar-migrated.v1). Off, the whole listing is read as before. Only the literal true turns it on.", "declarations": [{"service": "frontend", "env": "VITE_CHAT_SIDEBAR_SERVER_PAGING", "default": "false"}], "codePath": "apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-thread-history.ts"},
  

Fixer leftovers:

- apps/Frontend Plane/verevonv3/.env.example still needs commented, default-off entries for VITE_CHAT_SIDEBAR_SERVER_PAGING and VITE_CHAT_SIDEBAR_THREAD_STATE, written in that file's own style. The hard rules forbid opening .env* files, so the lead (or the owner) has to add them.
- Owner decision on pruneChatThreadTranscripts: record or narrow the loss of the local presentation projection for chats past page 1 in paged mode (details in the summary).
- No DB or harness work is involved; nothing needs the harness.
- The diff in src/shared/api/chat-client.ts belongs to F-f01-24 (P2-spa-09), not this item. Other dirty files in features/chat/lib (pending-chat-launch.ts, snapshot-throttle*, chat-evidence.ts, chat-thread-memory-mode.test.ts) and CoreSidebar.memory-mode.test.tsx belong to other agents too.

## P2-spa-13 (FAIL)

Title: SPA: answering or skipping an ask_user question stays on its own threa; BFF and SPA: the delete-chat confirmation says how many forks keep the

Verifier feedback:

FAIL on F-f07-01 (the answer half). F-f05-23 (fork notice, BFF and SPA) PASSES as it stands. Tree: C:/dev/mp-w345, uncommitted over HEAD 65219705e. No edits, no git writes, no docker, no .env file read.

WHAT IS RIGHT in F-f07-01 (read in the source and run):
- answerPendingQuestion and skipPendingQuestion capture threadId, questionId and stillHere after the early returns. After a successful answer, stillHere() gates setPendingQuestion(null) and sendContent. The catch sets the error and clears the card only when stillHere(). setQuestionPending(false) stays in finally. The resume branch (continueWaitingQuestion) is untouched.
- sendContent calls beginSend(options.draftText) and clears the composer only when draftText is undefined.
- chat-thread-draft.ts: an external send leaves the composer and stored draft alone on accept; a failed external send goes into an empty or a non-empty composer on the same thread. The spec's test cases are present and pass.

REQUIRED FIX 1 (ZDR fail-open): chat-thread-draft.ts failed(), cross-thread branch (about lines 235-242). The condition relies on sent.keepable, which is read only when beginSend runs. setRetention(true) (about lines 160-168) forgets the bound thread's stored draft but does not mark the send in flight. Temporary chat survives a switch between threads (use-chat-controller.ts resets it only by a new chat or the removal of the current thread). Trace: an answer is in flight on thread A; Temporary chat is switched on; the person opens thread B; the answer fails. failed() takes the cross-thread branch (keepable true, same epoch, workspace allows) and writes the answer into A's sessionStorage draft while zero retention is on. That breaks the spec's rule that the draft mechanism writes nothing while the next turn is zero retention. The header comment (about lines 64-68) says "under the same gate as any other draft write", which the code does not check.
Fix: add `!zeroRetention()` to the cross-thread condition, so it reads: sent.external && sent.key !== null && sent.key !== bound && sent.keepable && !zeroRetention() && sent.epoch === chatDraftClearEpoch() && isLocalRetentionAllowed(). Keep keepable. Reword the header comment to name the gate exactly. The extra block of a Temporary thread that is on for another thread is the intended fail-closed trade-off.

REQUIRED TEST 2 (chat-thread-draft.test.ts): add 'does not store a failed send of other text when Temporary chat was switched on after the send began': const page = mount('thread-a'); const settle = page.draft.beginSend('Svar på spørsmålet'); page.temporary(true); page.go('thread-b'); settle('failed'); flush(); expect(page.draft.message()).toBe(''); expect(sessionStorage.length).toBe(0). By tracing setRetention, rebind and failed (not run), this writes keyA on the current code, so it pins the gap. The existing 'never stores ... from a Temporary chat the person has left' test does not cover it, because it turns Temporary chat on at mount.

SHARED: none needed for code. Checked the fixer's claims: SESSION_CORE_THREAD_FORK_ENABLED is in apps/Control Plane/flag-registry/flag-registry.json (line 5447). Correction: VITE_CHAT_THREAD_FORK is NOT declared in src/vite-env.d.ts (only VITE_CHAT_SIDEBAR_THREAD_STATE, line 168); both are read through import.meta.env (ForkAction.tsx line 6; chat-thread-history.ts line 402). No compose file or Dockerfile in the repo names either SPA flag (the search excluded .env files, which I did not read). .plan-work/execution/results/W2-34.json says the fork switches stay unset in every environment until the P1 findings close; the lead should keep VITE_CHAT_THREAD_FORK off in stacks.

F-f05-23 PASS (no change):
- history.rs: fork_count is Option<u32> on output (omitted when none); upstream i32 with alias forkCount; relayed only above zero (zero and negative dropped). Test a_thread_relays_its_fork_count_only_when_it_has_forks.
- chat-client.ts: normalizeForkCount keeps integers above zero on ChatThreadSession.
- chat-thread-history.ts: the count is carried on local writes (withThreadStateCarry), cleared on a server replace and by applyChatThreadStateLocally, and validated in isChatThreadHistoryItem.
- CoreSidebar.tsx: forkedCopiesNotice (i18n.tr, singular and plural, only when threadForkEnabled() and the count is above zero) is appended to the Delete consequence. The delete call is unchanged. The count is the reader's own forks (session-core owner-read SQL). ChatSidebarOrganizer has no delete confirmation, so nothing to add there.

STAGING: use-chat-controller.ts holds F-f07-01b hunks (answerMessageLinks*, answerMessageIdForSummary, settleAnswerMessageLink, answeredMessageId, clientMessageId) and chat-types.ts holds another item's costMixed hunks. chat-thread-history.ts, chat-client.ts and CoreSidebar.tsx hold other items' hunks too. Stage only the F-f07-01 and F-f05-23 lines. F-f07-01b was not reviewed.

FIXER-REPORT CORRECTIONS: "a ZDR thread, a waived thread or a cleared session never stores it" does not hold for Temporary chat switched on during the send (fix 1). The vite-env.d.ts claim is wrong for VITE_CHAT_THREAD_FORK (no compile impact; tsc clean).

Fixer leftovers:

- None for F-f07-01 or F-f05-23.
- Staging note for the lead: in use-chat-controller.ts, the answerPendingQuestion and skipPendingQuestion region interleaves this item's hunks with F-f07-01b's. F-f07-01b's lines are answerMessageLinksOn, answerMessageLinkFor, answeredMessageId/clientMessageId and answerMessageLinks.delete. Stage those with F-f07-01b.
- The cross-thread restore of a failed answer adds to the sending thread's own sessionStorage draft. It is gated by keepable (not zero retention when the send began), the clear epoch, and isLocalRetentionAllowed(), so a ZDR thread, a waived thread or a cleared session never stores it.

## P2-spa-14 (FAIL)

Title: composer-capabilities advertises agent_modes and the SPA shows the mod; Action contract chat.reject_plan (registry entry, model-access row, ga

Verifier feedback:

FAIL on one point. Everything else in the item is correct and passes.

CORRECT AS BUILT (verified in the tree and by tests):
- ChatPage.tsx: the org mode cap is read only when composerCapabilities().agentModes === true, reusing the page's existing composer-capabilities read (no added network read). getComposerCapabilities() catches every failure and returns NO_COMPOSER_CAPABILITIES (agentModes false), so a failed read gives no picker and no cap read. The switch-off change (picker hidden, no org-cap read) is the intended fix.
- artifact_revision_routes.rs: agent_modes = enabled && !zdr through with_agent_modes_switch, chained after the other switches. zdr comes from zdr_or_refuse(...).unwrap_or(true), so it fails closed. The BFF relays the body and only inserts its own keys (with_bff_capabilities), so agent_modes reaches the SPA.
- chat-client.ts: strict read (=== true); NO_COMPOSER_CAPABILITIES has agentModes false.
- F-f05-29: registry entry (clientRoute /api/v1/agents/runs/{run_id}/plan-approval, the route the SPA's rejectPlan calls at chat-client.ts ~3639), inputSchema, model-access exempt row, count 384 (spec's 382 plus O-01 plus this item), generated executor row, admission golden. Adding no gateway arm is correct: spec step (3) is stale, and O-26's test "every client executor is refused by the generic arm, with no arm of its own" forbids an arm.

BLOCKER: DashboardComposer.tsx and DashboardComposer.agent-mode.test.tsx add a temporary-chat policy the spec does not ask for. agentModeOffered() = cap !== undefined && !(props.temporaryChat || props.temporaryChatLocked) hides the picker, clears a chosen mode through the effect, and drops submittedAgentMode on temporary chats. The spec makes the picker follow the server answer (present when agentModes is true and the org cap exists). The server does not require this: zdr_features.rs marks agent_mode Honored under ZDR and refuses only plan_mode. The fixer's own report lists it as open owner question 1 (open-decisions 3.7), and the lead's rule is to change nothing for an owner decision. Both DashboardComposer files are also outside the unit's file list, and the ChatPage gate alone meets the spec.
Required change:
1. Restore the three DashboardComposer.tsx hunks to their HEAD text by hand (no git checkout; keep LF): delete agentModeOffered; restore agentModeNotAdmitted to `const mode = agentMode(); const cap = props.agentModeCap; return mode !== undefined && cap !== undefined && !agentModesWithinCap(cap).includes(mode);`; restore `const submittedAgentMode = conversationOnly() ? undefined : agentMode();`; restore `<Show when={props.agentModeCap}>`.
2. Restore apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.agent-mode.test.tsx to HEAD: remove the temporary harness props, the setter hook, and the three temporary tests. The five HEAD tests cover no cap, cap, send, no send, and a dropped choice.
3. Re-run that test file (expect 5 passing), ChatPage.agent-mode-picker.test.ts, chat-client.composer-capabilities.test.ts, and tsc (33 baseline). After this the two DashboardComposer files carry no diff from this item and are not part of the fix.
Do not add the temporary-chat rule; the question stays with the owner.

ACCEPTED, NOT BLOCKING:
- agentModes is optional in ComposerCapabilities (the spec says boolean). Four ChatArtifactPanel*.test.tsx fixtures build the type literally, and tsconfig.app.json type-checks src, so a required field would add errors outside this unit. The gate is fail-closed (=== true). Keep it optional, or make it required and add agentModes: false to those four in their own track.
- ChatPage.agent-mode-picker.test.ts pins the gate by source regex. Acceptable without a render harness; a pure helper would allow a behavioural test.
- Spec step (3) (copy a chat.execute_approved_plan arm) and the spec's route /api/v1/orchestration/plans/{plan_id}/reject are both stale. Do not add the arm; the route is the plan-approval route.

SHARED:
A. F-f06-17 needs no registry row, compose line, golden, or migration. AGENT_MODES is already in apps/Control Plane/flag-registry/flag-registry.json (kind switch, onLiteral "true", default "false", declared for model-gateway) and in apps/Model Plane/deploy/docker-compose.yml line 488 (model-gateway block).
B. F-f05-29 admission golden. The Model Plane canonical and its three vendored copies still hash a6f62f0cade8d36fa42983fa573db261870ca73aea06c4fd9edb9c929436f4c3 (the O-01 state). Copy apps/Frontend Plane/verevonv3/src/shared/api/contracts/agent-action-admission.json (sha256 8c5f08be0151edaf2c1426783aa90de49d5d7221d29a306eacef18ca3604cd73) byte for byte to: apps/Model Plane/api/contracts/agent-action-admission.json; apps/Model Plane/go/services/capability-core/internal/api/testdata/agent-action-admission.json; apps/Model Plane/rust/services/execution-core/tests/fixtures/agent-action-admission.json; apps/Model Plane/rust/services/model-gateway/tests/fixtures/agent-action-admission.json. Set apps/Model Plane/api/contracts/agent-action-admission.json.sha256 to "8c5f08be0151edaf2c1426783aa90de49d5d7221d29a306eacef18ca3604cd73 *agent-action-admission.json". In scripts/tests/zdr-contract-sync.test.sh line 215, replace 386f22f18aa015bb95ee2c9a68cd44825adf603f6628755e3ad9a7b7b763dc16 with 8c5f08be0151edaf2c1426783aa90de49d5d7221d29a306eacef18ca3604cd73, changing only that line (the file carries another item's edit). The golden diff is O-01's chat.queue.retract row and its chat.save_thread_snapshot hash change (e4bd4bf... to 7ada85de...) plus this item's chat.reject_plan row, so the fixer's "only the reject row" wording is wrong. Commit all of it together.
C. apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/action-executors.json is untracked and generated (384 rows, 140 client). action-registry.test.ts checks it byte for byte, and O-26's 140-client count already includes chat.reject_plan. Commit it with O-26 (dispatchers_client_routed.rs) or with this item, not split.
D.

Fixer leftovers:

- Not run (lead or harness only): docker and DB harnesses, the admission proof script (apps/Model Plane/scripts/tests/agent-action-admission-proof.sh, its Docker step), and the e2e spec tests/e2e/plan-approve-execute.spec.ts (proof owed).
- Two owner questions for docs/plans/model-plane-open-decisions.md section 3.7 (not changed here):
  1. Should a ZDR or temporary chat ever show the agent-mode picker? The chat path honours lockdown, ask_first and auto under ZDR and refuses only read_only. The SPA now hides the picker on temporary chats, and the server answers agent_modes false for ZDR.
  2. An org mode_cap other than auto applies to every turn with no chosen mode (org_agent_policy::resolve_turn, about lines 674-686). With a read_only cap, every ZDR turn is refused.
- ChatArtifactPanel and DashboardComposer still make their own composer-capabilities reads (pre-existing, accepted deviation); this item adds none.

## P2-spa-15-rel1 (FAIL)

Title: related to P2-spa-15: A Space's own thread (origin space) gets a project scope recorded from its sessi

Verifier feedback:

FAIL: not fixed. The fixer is right to stop. No code for P2-spa-15-rel1 exists in the tree, and the spec makes the A/B/C choice an owner decision. Do not re-dispatch the fixer until the owner answers Q1, then implement the chosen option as below.

Verified on the tree (read-only). This item has no hunks of its own. The uncommitted use-chat-controller.ts and project-gate test hunks belong to P2-spa-15, and the history.rs hunks at about 100, 257 and 1404 belong to F-f05-23 (fork_count).
1. The bug is real but latent. The hint route (use-chat-controller.ts 1586-1589) reads scopedThreadRefs. The ungated record at 1699-1700 fills it from any session that names a Space. The early lookup (1650-1656) admits only origin 'chat', so a first open of an origin-space thread takes Chat's route (transcript read at 1706) and a later open takes the room route (loadProjectThread, 1541).
2. Production cannot return an origin-space session. read_durable_thread (history.rs 1326-1351) sends origin=chat and keeps only origin 'chat'. get_thread answers 404 (332), and getChatThread maps 404 to null (shared/api/chat-client.ts 4464). Every route also needs VITE_CHAT_PROJECTS_ENABLED (chat-projects.ts 28), whose wide enablement is still an open decision (D-f06-C, status O).
3. Tests on the tree: the project-gate file passes 3/3. space-first-send, space-launch, space-plain-send-ref, space-replay and project-gate together pass 44/44. The pin (same route on both opens) was not run. A scratch harness outside the worktree could not load a test file, and nothing may be written into the tree. By reading, the pin fails on the tree: the first open takes Chat's route (the switch-off test pins that route) and the second takes the room's (the HEAD 'recorded' test asserts the room read).
4. The HEAD 'recorded project thread' test (project-gate test, lines 112-119) uses an origin-space fixture and asserts only the later room read. It never checks the first open, which is why the mismatch passes.
5. The F-f06-14 test passes on the tree. By reading, removing the early lookup would fail it. With the switch off the gate never runs. In production the origin check is always true for a session that comes back, so the gate only matters once a non-chat session can come back. I could not confirm what the P2-spa-15 verifier meant by "inert".

Why the spec's option C, written as the gate at line 1700 only, is not safe: the record is more than the route hint. scopedThreadRefs is the Space-scope marker for persistence suppression (1012, switch on), the composer Space flag and Space route gates (openChatSpaceRef, 595, DEC-V-R8), new-chat-on-draft (2631), goal eligibility (3031), send refs (4194, 4489), server versions (6223), connect (6292) and the chip. Gating 1700 alone drops all of these for origin-space threads on the switch-on path, so Chat's composer could send into or persist to a room thread without the room's scope. The Space tests above use origin-space fixtures with the switch on and rely on these protections, so they would likely fail (not run).

Recommendation (the verifier's; the owner decides):
- Q1-C, built as E'' (no backend change; switch-off and production unchanged). Keep the record and every Space protection, and stop non-chat sessions from taking the project route. In use-chat-controller.ts: add `const nonChatSpaceThreadIds = new Set<string>()` beside scopedThreadRefs (line 588); at 1586 use `chatProjectsEnabled() && !nonChatSpaceThreadIds.has(threadId) ? (scopedThreadRefs.get(threadId) ?? projectThreadRef(threadId)) : undefined`; after 1700 add `if (scoped && session?.origin !== 'chat') nonChatSpaceThreadIds.add(threadId)`. Tests: give the 'recorded project thread' test a chat-origin fixture and keep its assertion; keep the switch-off test and the F-f06-14 test; add the pin. With the switch on, open THREAD, OTHER, THREAD (origin 'space') and assert getChatThreadTranscript was called with THREAD on both opens and getSpaceThreadTranscript never with THREAD. The pin fails on the current tree. The edits sit beside P2-spa-15's own hunk at 1650-1656, so stage by hunk.
- Q1-A (a Space's own thread is a project thread) is not safe as written:
  (a) read_durable_thread is the by-id read for nine call sites in five handlers (get_thread 326, get_thread_transcript 381/452/508, fork_thread 689/725, relay_thread_goal 1145, change_archive_state 1206/1230). Changing it would open Chat's transcript, fork, goal and archive routes to room threads without the room's Control check. Add a session-only read for get_thread alone.
  (b) It is not switch-neutral. With the SPA switch off, a returned origin-space session would be recorded at 1700 and the Space gates would engage, and persistence at 1012 is not suppressed. The SPA must treat non-chat sessions as null while VITE_CHAT_PROJECTS_ENABLED is off, and the gateway change needs its own default-off switch.
  (c) The bootstrap (2725-2732) loads any non-null session as a Chat-owned thread with no foreign-origin mark. It must mark non-chat origins foreign so a deep-linked room thread stays read-only.
  (d) Change the 1654 gate to test spaceRef, keep the HEAD 'recorded' test, and add the pin.
- Q1-B is not recommended for this item. The listing marker would duplicate origin plus spaceRef and does not decide what origin-space threads are, so the same question remains. It adds migration, proto and listing work with no change in behaviour.

SHARED (corrected from the fixer's list):
- Q1-C (E''): none. The fixer's "none" is right only for E''. Built as the spec's line-1700 gate, C needs the protections handled first (above).
- Q1-A: (1) history.rs: add a session-only by-id read for get_thread (313-335) that admits origin-space sessions for the caller only. Do not change read_durable_thread or chat_history_sessions. The fixer's text names them, but they serve every per-thread route. (2) A new default-off gateway switch read as the strict literal "true": the compose row and the flag-registry row (lead),

Fixer leftovers:

rel1 stays open as latent, with no production effect today, until the owner answers Q1 (A, B or C).

After the decision:
- **A:** change the gateway history.rs by-id read so it returns origin-space sessions to their owner, owner-bound and never as an authorisation for room reads. Change the F-f06-14 gate to test spaceRef. Keep the HEAD 'recorded project thread' test. Add the pin that opens the thread twice.
- **B:** add migration 0138 (check live branches first), session-core proto and grpc changes plus proto regeneration, the history.rs listing marker (stage only the marker lines; other agents have hunks at about 100, 257 and 1404 and in tests), and a compose row and flag-registry row for the default-off switch. Then update the 'recorded' test and add the pin that opens the thread twice.
- **C:** gate use-chat-controller.ts line 1700 with `!chatProjectsEnabled() || session?.origin === 'chat'`, update the HEAD 'recorded' test, and add the pin that opens the thread twice. This also changes the project-ref accessor (about line 595) and connectResumable (about line 6292), but only for origin-space sessions, which production does not return today.

No DB harness is needed for C. A and B would need gateway tests and, for B, a DB harness run by the lead.

## P2-spa-16 (FAIL)

Title: SPA: turn actions that call origin=chat endpoints are withdrawn on pro

Verifier feedback:

FAIL (round 3). What holds: the predicate (use-chat-controller.ts ~L3040), the entry guards on submitTurnFeedback, regenerateLatest, regenerateExchange, continueGeneration, branchAt, rewindTo, the connectResumable addition, the accessors turnActionsAllowed and projectThreadOpen (~L7085-7096), and the answer-row gates in ChatMessages (thumbs, reasons, rewind, regenerate, continue, branch, edit, fork, error retry, rerun). The switch-off path is unchanged: the predicate returns false, the accessors read nothing new, and ChatMessages props that are undefined or true leave every control as it was. The three new test files pass (32/32). tsc6 shows only the 33 pre-existing errors. Three gaps remain against the item's rule that a project thread calls no origin=chat endpoint and shows none of the seven controls.

REQUIRED 1 (header regenerate). ChatPanels.tsx ChatHeader still offers "Generer siste svar på nytt" / "Regenerate latest response" (menu item L226-229, prop L110) on project threads. ChatPage.tsx L1698 wires it to regenerateLatest, which is now a silent no-op there, under a hint that says regenerate is not available. Fix: add turnActionsAllowed?: boolean to ChatHeader (absent means allowed), render that menu item only when it is not false, and pass turnActionsAllowed={turnActionsAllowed()} on the <ChatHeader> element (ChatPage L1681). Test: a ChatPanels.test.tsx case (open More actions; item absent when false, present when absent or true), plus a pin in ChatPage.project-turn-actions.test.ts that the ChatHeader element carries the prop. ChatPanels.tsx and its test are clean in the worktree, so editing them is safe.

REQUIRED 2 (resends after an await). editAndResubmit (guard L6621, await Promise.all L6663, truncation L6687-6690, sendContent L6692) and rerunAsNewTurn (guard L6561, await L6581, sendContent L6590) check the guard only on entry. sendContent reads state.threadId when it is called. If the user opens another thread while the attachment read or the subscription replay lookup is pending, the edit or rerun goes to that thread. For a project room that is streamChat with spaceRef, and editAndResubmit has already cut that thread's on-screen turns to the old index. Fix: capture const threadId = state.threadId at entry, and after the await and before the abort, the truncation and the send, return if (state.threadId !== threadId || projectThreadBlocksTurnActions(threadId)). Test in use-chat-controller.project-turn-actions.test.ts: hold the attachment read (mock toStreamAttachments) or the replay lookup (resolveReplaySubscriptionConnectionId) for an edit on an ordinary thread, load a project thread, release the read, and assert no streamChat call and an unchanged project transcript.

REQUIRED 3 (version switch in the lookup window). The answer-row version switcher (ChatMessages L1245, Show when props.serverVersion) is not gated by turnActionsAllowed. switchServerVersion (L6898) checks only usesServerVersions, which is true for an unsettled thread once serverVersionsOn has been learnt. While a project thread's lookup is pending or hangs, the previous thread's answers with version badges stay on screen. An arrow then calls switchChatVersion(projectThreadId), which is POST /api/v1/chat/threads/{id}/branch, an origin=chat move. Settled project threads are safe (no badge, and usesServerVersions is false). Fix: add if (projectThreadBlocksTurnActions(threadId)) return to switchServerVersion, add the same guard to undoRewind (L6872; unreachable today, added for uniformity), and render the switcher only when props.turnActionsAllowed !== false. Tests: AssistantMessage with a serverVersion badge and turnActionsAllowed false shows no arrows; a controller test with versioned turns on an ordinary thread, then an unsettled project thread, asserts that switchServerVersion makes no switchChatVersion call.

Related gaps that share the lookup window are listed under related. The export, share and side-chat controls gate on spaceScoped only.

SHARED: none. The switch is the existing VITE_CHAT_PROJECTS_ENABLED, read as the strict 'true' in chat-projects.ts L28. No registry row, compose line, admission golden, proto or migration is needed.

Commit scope: the lead stages only this item's hunks in use-chat-controller.ts, ChatMessages.tsx, ChatPage.tsx, ForkAction.tsx and shared/chat-nodes/types.ts. The F-f01-25 and F-f05-21 hunks in the same files are not this item's.

Notes (no change needed): a failed Space-ref lookup withholds an ordinary thread's actions until it is reopened. That is fail-closed and matches R-spa-01, which already refuses its sends. Rerun is withheld beyond the seven; that is acceptable and documented. The optional connectResumable change is correct and tested. The page-level tests pin source text, as ChatPage.fork-wiring.test.ts already does. The accessor's own spaceRefLoads() read is redundant and harmless. The mutation runs quoted in the test comment were not re-run, because the tree cannot be edited here; the reasoning holds on reading the code.

Fixer leftovers:

- Nothing owed for this item. No DB harness is needed.
- Hunk staging is needed at commit time. use-chat-controller.ts, ChatMessages.tsx, ChatPage.tsx and ForkAction.tsx also hold F-f01-25 and F-f05-21 hunks; the line map is in the summary.
- The accessor's own spaceRefLoads() read in turnActionsAllowed does nothing the predicate's read does not already do. Mutation testing shows the predicate's read alone keeps the accessor reactive. I kept it as written to avoid churn; it can be dropped later with no change in behaviour.

## P2-sc-19 (FAIL)

Title: Run question answered_message_id linked atomically with the appended a; session-core archive_threads: THREAD_ARCHIVED idempotency key per arch; Memory write policy: the credential rule recognises credential-shaped 

Verifier feedback:

VERDICT: FAIL. One required fix for the fixer (O-15, conversation-only threads). F-f02-06 and F-f03-19 are correct; the remaining O-15 points are lead decisions, listed below with a recommendation to accept and record them.

WHAT I VERIFIED (re-ran; did not trust the report)
- F-f02-06 (grpc.rs archive_threads ~12811-12818): key is {thread_id}:archived:{now}, the same now bound as archived_at; payload, type URL and response unchanged; archive_thread untouched; source-scan test both_archive_paths_key_the_event_per_archive_instance and DB test bulk_archive_after_restore_writes_a_second_archived_event present. Correct.
- F-f03-19 (write_policy.rs): SECRET_SHAPES and mp-scrub untouched; only the basic and keyword entries use the refined rule (PROSE_PRONE_SHAPES, pinned to SECRET_SHAPES by a test); other shapes keep is_match; counter label closed to basic/keyword and no value is logged; check_skill calls the same secret_rule per field. Correct, subject to lead decisions (b), (c) below.
- O-15 session-core: owned_thread_kind is the only caller of the renamed fn and locks the thread row FOR UPDATE; ownership and held-elsewhere checks run before the answer compare-and-set; answer and append share one transaction; the ZDR/waived refusal runs first in the handler; a skip with a named id is refused; switch-off path is the HEAD path (empty id and summary reach answer_run_question with ""). The dedupe fill (grpc.rs fill_linked_answer_message) is fill-only and gated by an EXISTS on a run_questions row that names the message.
- O-15 gateway/BFF/SPA: literal-"true" switches; id shape (alnum start, [A-Za-z0-9_-], <=128) checked at gateway and BFF; summary trimmed and bounded; forged org/user fields refused by deny_unknown_fields on the BFF link body; skip drops names; switch-off bodies unchanged; SPA mints one id per question and reuses it on retry; both pinning tests (model-gateway orchestration_http_test, BFF orchestration.rs) are updated.

REQUIRED CHANGES (fixer)
1. session-core orchestration_grpc.rs answer_with_message (~line 3334): `let linked = !thread.is_space && !thread.conversation_only;` records a conversation-only answer with no message. The spec requires the message and the link in one transaction whenever the request names both. The exception is avoidable: the conversation-only rule in grpc.rs append_message_in_tx_as (~7603-7607, stored_isolation && !requested_isolation returns failed_precondition) is satisfied when the answer's own message carries metadata source_scope "conversation". Change: `let linked = !thread.is_space;`, and have append_answer_message (~3413) send metadata {"source_scope": "conversation"} (a prost Struct, built the way the send path builds metadata) when thread.conversation_only. Keep the Space exception: a Space append with prior messages needs a signed decision the route does not carry (permission_denied otherwise). Update the doc comments on answer_with_message and append_answer_message, which currently say a conversation-only answer is unlinked.
2. Rewrite DB test a_conversation_only_linked_answer_is_answered_and_the_send_stores_it (orchestration_grpc.rs ~11771) for the new behaviour: answered_message_id == "msg-conv-1"; exactly one message under that id with content "Svar: a" and metadata source_scope "conversation"; the summary send with the same id returns deduplicated == true and keeps the scope. Keep #[ignore].

LEAD DECISIONS (verifier recommendation in brackets)
(a) SPA still calls sendContent after a linked answer, against spec step 3 [accept; amend spec]. Needed: answerQuestion never starts a turn for next-turn questions, and the chat send's append (session_flow.rs append_user_message call in invoke) discards the receipt's deduplicated flag, so the send after the link is what starts the turn. Do not remove the send.
(b) Title-case letters-only values count as prose, so "password: Hemmelighet" passes [accept; record the reading]. The literal clause "not a plain single-case all-letters word" is vacuous: a letters-only value with two classes always has both cases, so it is never single-case. The fixer's reading (a lowercase or Capitalised word is plain) is the only reading under which the clause does anything. Residual: a letters-only Capitalised secret passes.
(c) Trailing . , ; : ! ? trimmed before the class and plain-word tests [accept]. Needed so "I have basic knowledge." is prose; the literal rule counts "." as a symbol class and refuses it. Residual: a lowercase-only 8+ value ending in "." passes, as it already does without the dot.
(d) Switch name: code uses MODEL_GATEWAY_QUESTION_ANSWER_MESSAGE_LINK (the F-f07-01b switch), not the spec's MODEL_GATEWAY_QUESTION_ANSWER_LINK [accept; one switch, record the rename].
(e) Space threads keep the unlinked answer [accept; record, see required 1].
(f) The dedupe fill is an exception to "a repeat is a no-op": fill-only, and only for a user row that a run question of the same thread names [accept; record].
(g) Summary bound is 32,000 characters, this route's own bound, not the 100 KiB UTF-8 normal bound [accept; record].
(h) Spec residual: keyword or basic values under 8 characters still pass (the spec's >= 8 rule).

RECOMMENDED, NOT REQUIRED FOR PASS
- orchestration_grpc.rs answer_with_message, Replay branch: a second tab with a stale card that answers with the same answers and a new message id gets 200 with the first id; the new message is not stored; the SPA then sends the summary under its own id, which stores a second copy and starts a second turn. Refuse the replay (names_a_different_message, or 409) when the stored answered_message_id is non-empty and differs. Keep accepting when the stored id is empty (lost-reply retry on Space or conversation-only threads).

SHARED (lead adds these; the earlier attempt's A1-A3 and B1-B4 are not in the tree, so the exact text is below)
A. apps/Control Plane/flag-registry/flag-registry.json: insert in name order.
{ "name": "MODEL_GATEWAY_QUESTION_ANSWER_MESSAGE_LINK", "ki

Fixer leftovers:

- These DB tests need the harness (`DATABASE_URL` to a scratch DB named *test*):
  - new: `a_conversation_only_linked_answer_is_answered_and_the_send_stores_it`, `a_byo_send_repeating_a_linked_answer_keeps_its_connection_for_a_regenerate`, `a_repeat_of_an_ordinary_send_fills_nothing`
  - existing O-15 tests: `a_linked_answer_is_one_commit_and_its_repeat_stores_nothing_again`, `an_id_naming_another_threads_message_is_refused_and_the_question_stays_open`, `a_first_linked_answer_whose_message_clashes_rolls_its_answer_back`, `a_skip_names_no_message_and_a_named_skip_is_refused`
  - Run: `cargo test -p session-core --bins plan_gate_pg -- --ignored`.
- message_tree active-path source scan fails because of another item's fork_count hunk in grpc.rs (~line 227). It is not this item's.
- The model-gateway half of "a regenerate replays it" (normalize.rs replay_stored_connections) was not changed. It runs only with MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED on.
- Lead decisions still open:
  - (a) Amend the O-15 spec: the SPA still calls sendContent after a link, and that send dedupes and starts the turn.
  - (b) Title-case values stay prose in F-f03-19, so 'password: Hemmelighet' is allowed.
  - (c) The 32,000-char summary bound is unchanged; only the comments were corrected. Aligning it to 100 KiB bytes would touch the model-gateway, BFF and session-core constants and their tests.
  - Record the dedupe fill exception to "repeat is a no-op".
- Stage only this item's hunks. grpc.rs, orchestration_grpc.rs, orchestration_store.rs and http_routes.rs also carry other items' hunks.

## P2-ec-09-rel1 (FAIL)

Title: related to P2-ec-09: a server's held call is not cancelled when the continuation releases before it h

Verifier feedback:

FAIL, and blocked on the owner, not a fixer defect. Nothing was changed for this item: the fixer edited no file, and the three release sites still pass held=None (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs lines 1739 stage release, 1829 claim-refused release, 1857 unparseable fold). The check and restore release sites (1893, 1943) pass held.as_deref(). The bug is confirmed by reading the tree and by the test run below. Section 3.7 of docs/plans/model-plane-open-decisions.md still holds only DEC-V-R8 and DEC-V-R14, so the lead must record the owner's answer there before the fixer runs again. Do not re-run the fixer before that.

Facts that bear on the owner's choice (all verified in the tree):
- HEAD 65219705e has no held parameter at all. The held-call cancel on release (W3-EP-4-S7-b) exists only in uncommitted hunks of agent.rs, resume.rs, mcp_gateway.rs and runtime_loop/mod.rs. So on HEAD no release cancels a held call. The lead must commit that parent work first or with this fix.
- The question row carries no elicitation id. RunQuestion uses fields 1-16 (origin 15, origin_label 16) and CreateRunQuestionRequest uses 1-9, so RunQuestion field 17 and CreateRunQuestionRequest field 10 are free. The idempotency key is {run}:{step}:elicit:<ordinal> (mcp_gateway.rs elicitation_question).
- Option 1 is the only route to the stage and claim-refused paths. The fold holds pending_elicitation_id, but neither path reads the fold, and sessions.proto has no read-only loop-checkpoint RPC (only Save, Claim, Close).
- Re-sending a cancel is idempotent. The gateway's answer removes the held entry on success (model-gateway mcp_elicitation.rs answer), so a second cancel returns mcp_elicitation_expired.
- A cancel cannot race a sent answer. Only the winner of the release-token spend, or its replay, cancels. An answer needs the client-token spend, and the store spends once.
- ZDR and waived runs fail closed before any release: question_stage_decision refuses them, pause_for_elicitation refuses them before saving or asking, and consume refuses zdr/waived callers. Option 1 adds no ZDR path.
- The stall numbers match the gateway constants: MAX_HELD_PER_ORG 8, DEFAULT_HOLD_SECONDS 600, MAX_HOLD_SECONDS 1800, REAP_INTERVAL 15 s.

OPTION 1 (recommended by the verifier). The fixer must:
1. Proto: add RunQuestion `string elicitation_id = 17;` and CreateRunQuestionRequest `string elicitation_id = 10;`, each with a comment. Leave the ApprovedAuthority hunks (fields 14 and 15) alone. The lead regenerates Go, Python and TS once (go/gen is already dirty).
2. Migration: apps/Model Plane/rust/services/session-core/migrations/0138_run_question_elicitation_id.sql, additive. ADD COLUMN IF NOT EXISTS elicitation_id TEXT NOT NULL DEFAULT '', with a length bound and CHECK ((origin = 'mcp_elicitation') = (elicitation_id <> '')). Plus migrations/revert/0138_run_question_elicitation_id.down.sql with IF EXISTS drops, run by hand as 0105 does. 0138 is unreserved; 0137 is another agent's untracked file.
3. session-core orchestration_store.rs (clean in the tree): add the column to RUN_QUESTION_COLUMNS, to RunQuestionRow and NewRunQuestion, and to the insert bind. Update the test row literals near lines 6564, 6668 and 6896.
4. session-core orchestration_grpc.rs (dirty with other agents' hunks; edit only this item's hunks): in question_origin, refuse a non-empty id unless origin is mcp_elicitation and the caller is service:execution-core, and refuse any id on origin model. Map the id in the RunQuestion builder near line 3056, which consume responses also use.
5. execution-core mcp_gateway.rs elicitation_question: set elicitation_id = pending.elicitation_id.
6. execution-core agent.rs release_question_answer: take the id from the spend's returned question (Won and Replayed), falling back to held. Cancel only when crate::mcp_gateway::elicitation_on() is true, so with the switch off the spend, the refusal and the absence of a cancel stay exactly as now. On Replayed for a server question, cancel again. Treat an mcp_elicitation_expired reply as done, not as a warning. Correct the comments at about 2281-2284 and 2368-2371, which claim a cancel that held=None never sends.
7. Update the existing test a_released_server_question_cancels_its_held_call_once_only. Its last assertion, "the retry cancels nothing again", must change to re-cancel. Say so in the commit body. Do not weaken it silently.
8. Pins required: the stage release with an id cancels once; a claim-refused release (unavailable or not_found) with an id cancels; an unparseable fold with an id cancels; a model-origin row never cancels; with the switch off no cancel is sent and the spend is unchanged; model-gateway's JSON mapping (http_routes.rs near line 2300, which is explicit-field) and sse.rs never emit elicitation_id; session-core refuses an id from other callers and on model origin (unit test; the DB CHECK test is env-gated and needs the harness).
9. Compile impact: every RunQuestion or CreateRunQuestionRequest struct literal without ..Default::default() needs the field. There are 39 literal lines across execution-core (ask.rs is clean), model-gateway (http_routes.rs and sse.rs are dirty with other agents' hunks: report rather than edit) and session-core. The fixer must not touch another agent's hunks.

OPTION 2 (accept the reaper). The fixer makes docs and one pin test only: no proto, migration, store, grpc or gateway change. Document the stall in the model-plane docs with the numbers above. Correct the two misleading comments. Add a pin test that a release with held=None sends no cancel and that the spend is still single.

BOTH OPTIONS:
- No flag, compose or golden change. MCP_DO_ELICITATION stays default false (the switch reads the literal "true" only).
- Do not fix the stage-release ordering in this item. The pre-existing gap (related item 1) is a separate owner call and is listed below.

SHARED (the lead's edits, corrected from the fixer's text):
-

Fixer leftovers:

The lead needs to put this question to the owner and record the answer in docs/plans/model-plane-open-decisions.md section 3.7 before re-running this fixer:

"P2-ec-09-rel1: when a server's form is released before its fold is held (thread moved, claim refused, fold unreadable, stage release), its MCP call stays held until the gateway reaper ends it: 8 held slots per org, default hold 600 s, max 1800 s, reaper tick 15 s. Option 1: carry the gateway's elicitation_id on the run question (migration 0138), so the release cancels the call, including on a same-claim retry. Option 2: accept the reaper and document the stall. Which?"

The verifier recommends option 1. Both options' requirement lists are in the verifier's feedback and still apply as written.

Separate pre-existing gap (related item 1, also an owner call): begin_question_continuation spends an MCP answer in the stage release before the switch check.

The adjacent-gap list (items 1 to 6) was never in the task text either.
