# Audit phase 2 run r1: shared edits still to apply (2026-11-21)

Run r1 built and verified 99 items (all PASS). The agents did not touch shared files (flag-registry, compose, admission golden, protos, principals); their exact instructions are below, one block per item. Until they are applied the flag-registry/compose ratchets stay red for the new switches, so main was NOT advanced.

## P2-cp-01-rel1

Both edits below are needed together, because the compose ratchet fails if a compose variable is registered without its matching row, or the reverse. The lead makes them:
1. apps/Control Plane/docker-compose.yml, org-core service environment block, beside ORG_FLAGS_ENABLED (about line 651): add
   BROWSER_SPACE_FLOOR_ENABLED: ${BROWSER_SPACE_FLOOR_ENABLED:-false}
   The default text must be exactly false to match the registry default.
2. apps/Control Plane/flag-registry/flag-registry.json: add this switch row after the ORG_FLAGS_ENABLED row:
   {"name": "BROWSER_SPACE_FLOOR_ENABLED", "kind": "switch", "onLiteral": "true", "owner": "control-plane", "source": "compose", "default": "false", "stage": "dark", "purpose": "Space floor for the agent browser in the org-core browser-sites resolve (P2-cp-01): on, a Space that is not entitled, or whose floor is unreadable, is refused every host. Off, space_id is ignored. Only the literal true turns it on.", "declarations": [{"service": "org-core", "env": "BROWSER_SPACE_FLOOR_ENABLED", "default": "false"}], "codePath": "apps/Control Plane/org-core/internal/http/browser_sites_handlers.go"}
   The purpose is 231 characters (limit 240). codePath already contains the variable name. After both edits, rerun GOWORK=off go test ./... -count=1 in apps/Control Plane/flag-registry.
3. If browser-broker and execution-core also read BROWSER_SPACE_FLOOR_ENABLED under F-f03-03, their compose declarations join this same row. That is not part of this item.
4. No migration number, golden or proto change is needed. user-core 032_space_browser_entitlement already exists, and org-core's browser-site-policy-matrix.json is unchanged because the floor is not part of that golden.

## P2-cp-02

1. apps/Control Plane/docker-compose.yml, user-core service environment, next to the CONTROL_SPACE_MEMORY_DECISIONS line (about line 524). Add this line:
   SPACE_SKILL_WRITE_DECISIONS: ${SPACE_SKILL_WRITE_DECISIONS:-false}

2. apps/Control Plane/docker-compose.yml, user-core service principal registry literal (about line 477, the JSON starting with principal "verevon-gateway"). Append the scope to that principal's scopes array:
   "spaces:skill-write:issue"
   so it reads [..., "directory:read:self", "spaces:membership:act", "spaces:skill-write:issue"]. Only verevon-gateway holds it. No other compose file named user-core's principal registry in a grep of Control Plane config, so config/plane-service-principals.json needs no row for this scope. If docker-compose.production.yml carries the same literal, apply the same change there. Then run scripts/control-train-repin.sh to re-pin the train.

3. apps/Control Plane/flag-registry/flag-registry.json: add this object to the flags array, after the CONTROL_SPACE_MEMORY_DECISIONS row (about line 951):
   {
     "name": "SPACE_SKILL_WRITE_DECISIONS",
     "kind": "switch",
     "onLiteral": "true",
     "owner": "control-plane",
     "source": "compose",
     "default": "false",
     "stage": "dark",
     "purpose": "F-f10-18: user-core signs model.space.skill.write decisions (skill:write, 60 s) for a shared Space's manager or owner to create, update or delete a Space skill; off answers 404. Enable only together with capability-core SKILL_SPACE_WRITE_VERIFY (F-f10-19). Only the literal true turns it on.",
     "declarations": [
       { "service": "user-core", "env": "SPACE_SKILL_WRITE_DECISIONS", "default": "false" }
     ],
     "codePath": "apps/Control Plane/user-core/cmd/server/main.go"
   }

4. No migration (none needed), no golden or proto change, and no change to the admission golden."

## P2-cp-03

Migration: 033_org_compliance_role.{up,down}.sql already exists in apps/Control Plane/org-core/migrations (033 was the next free number; 034 is now taken by another item). No new migration is needed.

Nothing is needed for the admission golden or any .proto.

LEAD MUST ADD (1): flag-registry.json (apps/Control Plane/flag-registry/flag-registry.json). Insert the following two rows. Put the first next to ORG_RETENTION_POLICY_ENABLED (~line 3290) and the second next to VITE_PLAN_EXECUTE_ENABLED (~line 4705). Both tests check the stage via `EffectiveStage()`: "dark" is the explicit stage used by the neighbouring ORG_RETENTION_POLICY_ENABLED row, and the VITE sibling row omits `stage`, so check the effective value if the Vite test complains.
{
  "name": "ORG_COMPLIANCE_ROLE_ENABLED",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "control-plane",
  "source": "compose",
  "default": "false",
  "stage": "dark",
  "purpose": "O-21 compliance organisation role: org-core, user-core, auth-core, audit-core and the verevonv3 gateway accept compliance as a member role, and owner, admin and compliance may place and release legal holds. Off, compliance is refused as a role value everywhere and the hold role lists stay as they are. Only the literal true turns it on.",
  "declarations": [
    { "service": "org-core", "env": "ORG_COMPLIANCE_ROLE_ENABLED", "default": "false" },
    { "service": "user-core", "env": "ORG_COMPLIANCE_ROLE_ENABLED", "default": "false" },
    { "service": "auth-core", "env": "ORG_COMPLIANCE_ROLE_ENABLED", "default": "false" },
    { "service": "audit-core", "env": "ORG_COMPLIANCE_ROLE_ENABLED", "default": "false" },
    { "service": "gateway", "env": "ORG_COMPLIANCE_ROLE_ENABLED", "default": "false" }
  ],
  "codePath": "apps/Control Plane/org-core/internal/org/compliance_role.go"
}
{
  "name": "VITE_ORG_COMPLIANCE_ROLE",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "frontend-plane",
  "source": "code",
  "default": "false",
  "stage": "dark",
  "purpose": "O-21 SPA twin of ORG_COMPLIANCE_ROLE_ENABLED: offers the compliance role in the member list and invitation role pickers. Flip it together with the server switch. Only the literal true turns it on.",
  "declarations": [
    { "service": "frontend", "env": "VITE_ORG_COMPLIANCE_ROLE", "default": "false" }
  ],
  "codePath": "apps/Frontend Plane/verevonv3/src/features/settings/lib/org-roles.ts"
}

LEAD MUST ADD (2): compose lines.
- apps/Control Plane/docker-compose.yml, environment block of each of org-core, user-core, auth-core and audit-core: `ORG_COMPLIANCE_ROLE_ENABLED: ${ORG_COMPLIANCE_ROLE_ENABLED:-false}`. For org-core, put it after `ORG_RETENTION_POLICY_ENABLED` (~line 653), with the comment "# O-21: compliance org role; owner, admin and compliance place and release holds. Off unless the literal true." The service names are those used by the compose scanner (`org-core`, `user-core`, `auth-core`, `audit-core`); the test requires exactly these four in that file.
- apps/Frontend Plane/verevonv3/docker-compose.yml, `gateway` service environment: `ORG_COMPLIANCE_ROLE_ENABLED: ${ORG_COMPLIANCE_ROLE_ENABLED:-false}`.
- apps/Frontend Plane/verevonv3/docker-compose.yml, `frontend` service environment, after `VITE_PLAN_EXECUTE_ENABLED` (~line 583): `VITE_ORG_COMPLIANCE_ROLE: ${VITE_ORG_COMPLIANCE_ROLE:-false}`.

## P2-cp-04-rel1

No registry rows, compose lines, flag-registry entries, admission golden, proto or generated code are needed: there is no switch. New org-core migration 035_browser_site_choices_backfill (up and down) applies after 034 by file order; include it in the org-core commit with the P2-cp-04 migrations and code it sits on. Commit with the P2-cp-04 work these files depend on: the untracked user-core files internal/users/gdpr_browser_site_erasure.go and gdpr_browser_site_erasure_test.go (my saga test uses SetBrowserSiteEraser and acceptingBrowserSiteEraser from them), and the P2-cp-04 hunks already in gdpr_erasure_saga_postgres_test.go. Commit gdpr_erasure_store.go together with my new gdpr_erasure_snapshot_test.go.

## P2-cp-05-rel1

1. No registry or compose change is needed. RUN_ACTION_TARGETS is already declared in apps/Control Plane/flag-registry/flag-registry.json (choice, default tickets_only, declared for user-core) and in apps/Control Plane/docker-compose.yml around lines 452-453.
2. Migration needed for table mode, number to be assigned by the lead. The highest user-core migration in the tree is 044. The space-hybrid plan doc names 053 as next free, which may be stale, so check the live ledger. Suggested up SQL for a file such as apps/Control Plane/user-core/migrations/NNN_owner_effect_reservation_actions.up.sql:
ALTER TABLE space_owner_effect_reservations
    DROP CONSTRAINT IF EXISTS space_owner_effect_reservations_action_id_check,
    ADD CONSTRAINT space_owner_effect_reservations_action_id_check
        CHECK (char_length(action_id) <= 128 AND action_id ~ '^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$');
Suggested down SQL. It fails loudly if non-ticket rows exist, which is intended:
ALTER TABLE space_owner_effect_reservations
    DROP CONSTRAINT IF EXISTS space_owner_effect_reservations_action_id_check,
    ADD CONSTRAINT space_owner_effect_reservations_action_id_check CHECK (action_id = 'tickets.create');
The Go table stays the admission authority, because decision.Validate(mode) runs before any insert. The database only bounds the shape.
3. Optional: a contract test for the new migration in apps/Control Plane/user-core/internal/database/, modelled on owner_effect_reservation_migration_contract_test.go.
4. Application Plane (other track): generalise the tickets.create gates and the audience constant listed in leftovers item 2.
5. Verification log: record P2-cp-05-rel1 as fixed in code, with the test name above, and as not end-to-end until items 2 and 4 land.

## P2-cp-06

A. Migration number: 036, not 033 as the spec said. Tree already has 033 (org compliance role), 034 (browser site history erasure) and 035 (browser site choices backfill). New files: apps/Control Plane/org-core/migrations/036_browser_site_audit_outbox.up.sql and .down.sql (both created).

B. apps/Control Plane/flag-registry/flag-registry.json: insert after the ORG_FLAGS_ENABLED entry:
{
  "name": "ORG_BROWSER_SITE_AUDIT_ENABLED",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "control-plane",
  "source": "compose",
  "default": "false",
  "stage": "dark",
  "purpose": "F-f06-11 (WS21-S6, audit K3-11): org-core queues a content-free audit intent, in the same transaction as the write, for each agent-browser site policy write and each member choice write or delete (internal/browsersites/audit.go). Off, a write queues nothing and is unchanged; intents already queued still publish. Only the literal true turns it on.",
  "declarations": [
    { "service": "org-core", "env": "ORG_BROWSER_SITE_AUDIT_ENABLED", "default": "false" }
  ],
  "codePath": "apps/Control Plane/org-core/internal/browsersites/audit.go"
}

C. apps/Control Plane/docker-compose.yml, org-core service environment (service block starts at line 589). Add after the line "ORG_FLAGS_ENABLED: ${ORG_FLAGS_ENABLED:-false}" (about line 651):
            # F-f06-11: browser site policy and choice audit intents. Off (nothing queued) unless the literal true.
            ORG_BROWSER_SITE_AUDIT_ENABLED: ${ORG_BROWSER_SITE_AUDIT_ENABLED:-false}

D. apps/Control Plane/flag-registry/flag-registry.json: add the F-f08-10 switch (neighbour shape, owner and source copied from the integration-corev2 entries such as CUSTOM_REST_CONNECTORS_ENABLED; there is no compose wiring to add because no compose file passes the lease variables):
{
  "name": "INTEGRATION_TOKEN_LEASE_PER_CONSUMER_AUTH",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "ingestion-plane",
  "source": "code",
  "default": "false",
  "stage": "dark",
  "purpose": "F-f08-10: POST /internal/connectors/token takes the consumer from the key the request presents (INTEGRATION_TOKEN_LEASE_CONSUMER_KEYS, consumer=key pairs), and the shared internal API key no longer opens it. Off, the route is unchanged. On with an empty allow-list, every consumer is refused; with no keys, the route answers 401. Only the literal true turns it on.",
  "codePath": "apps/Ingestion Plane/integration-corev2/internal/config/config.go"
}

E. Optional, the lead's call: apps/Ingestion Plane/integration-corev2/.env.example line 148 (drop model-plane from INTEGRATION_TOKEN_LEASE_CONSUMERS; add the names INTEGRATION_TOKEN_LEASE_PER_CONSUMER_AUTH and INTEGRATION_TOKEN_LEASE_CONSUMER_KEYS). I did not open or edit it.

F. No golden, admission, proto, generated-code, or docs/actions-surface-operations.md change.

## P2-cp-06-rel1

No registry, compose, migration, golden or proto change is needed. ORG_FLAGS_ENABLED is already registered in apps/Control Plane/flag-registry/flag-registry.json (around line 3277) and declared for org-core in apps/Control Plane/docker-compose.yml (line 651). Migration 032_org_flag_values already creates org_flag_audit_outbox.
Commit staging: in cmd/server/main.go my hunks are (a) the single line `go runOrgFlagAuditOutbox(ctx, orgService, 5*time.Second)` inside the `if natsClient != nil` block after `go runIdentityAuditOutbox(...)`, and (b) the orgFlagAuditFlusher interface and runOrgFlagAuditOutbox function, placed just before the runBrowserSiteAuditOutbox comment. Everything else in that file belongs to other work. In internal/org/org_flags.go the change is the FlushOrgFlagAuditOutbox doc comment only. cmd/server/org_flag_audit_outbox_test.go is a new untracked file and needs `git add`.

## P2-mp-03

1. apps/Model Plane/deploy/docker-compose.yml, service session-core (block starts at line 1200). Replace the two comment lines at 1468-1469 (six-space indent) with these lines:
      # EP-9 C2 retention sweeper: off|dry_run|enforce, default off. A blank or padded off reads off, and so does
      # off in any case. enforce is read exactly; every other value is dry_run, never enforce. enforce deletes
      # (EP-9 C3) only when the letta configuration parses, and runs dry when it does not. Needs
      # SESSION_ORG_CORE_SERVICE_TOKEN with org:retention-policy:read:any.
   Keep the SESSION_RETENTION_SWEEP line (1470) and its default unchanged.
2. Go generated copy: copy C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/mg06/out-new/model_plane/v1/gateway.pb.go to apps/Model Plane/go/gen/model_plane/v1/gateway.pb.go (LF). The only changes are the two comment blocks near lines 1015 and 1319 of the current file. This file was produced by protoc-gen-go v1.36.12, which matches the checked-in header. If you regenerate with the repo's remote Go plugin instead, check it is the same version first.
3. TypeScript generated copy: apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/gateway_pb.ts.
   Option A (recommended): copy C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/mg06/ts216-new/model_plane/v1/gateway_pb.ts. It is generated with buf.build/bufbuild/es:v2.16.0, the version in the current header. The unpinned remote plugin is v2.17.0 and rewrites the descriptor constant, so pin the version. Option A also brings the EP-11 S3 and W4-PL-1 fields that the tree copy lacks.
   Option B (comment-only): replace the JSDoc at about line 637 with the two lines below (three-space indent), and the one at about line 846 with the two lines after them.
      * URL after the redirect chain (Quarry follows up to 5 hops); empty means the final URL is
      * unknown (the caller must treat it as the requested URL and never as a verified host).
      * URL after the redirect chain; empty means the final URL is unknown (the caller must treat it
      * as the requested URL and never as a verified host).
   Option B leaves the existing drift in place.
4. No change needed: apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/gateway_pb2.py, apps/Model Plane/go/gen/model_plane/v1/gateway_grpc.pb.go and apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/gateway_connect.ts. None of them contain field-comment text.
5. The proto source is already in the tree (apps/Model Plane/proto/model_plane/v1/gateway.proto, FetchResponse.final_url and ExtractStructuredResponse.final_url). Breaking passes, so baseline.binpb is not regenerated.
6. No registry row, compose service line other than item 1, golden, vendored copy or migration number is needed. No new switch.
7. Git modes for the new files: see leftovers item 7.

## P2-mp-03-rel1

1. Commit apps/Model Plane/proto/model_plane/v1/gateway.proto together with gateway_pb.ts. The proto's uncommitted R-mg-06 comment edit (FetchResponse.final_url and ExtractStructuredResponse.final_url, about lines 416 and 507) is already reflected in the regenerated gateway_pb.ts. If the proto is committed without this regeneration, or the wording changes, the pin test fails. 2. Stage the new untracked file apps/Model Plane/scripts/tests/gateway-gencode-pin-test.mjs. 3. CI wiring: .github/workflows/buf.yml gained two path filters (apps/Frontend Plane/verevon/src/lib/model-plane/gen/** and the test file) in push and pull_request, plus one step that runs node --test on the test. Keep or revert as you prefer. 4. No registry rows, compose lines, migration numbers, admission golden, flags, Go/Python/Rust changes or proto field changes are needed. The Go gencode (go/gen) is not touched. Suggested subject: "fix(model-plane): regenerate the gateway TypeScript gencode and pin protoc-gen-es (P2-mp-03-rel1)".

## P2-mg-01

Neither switch is in the registry or any compose file yet (I grepped docker-compose*, deploy/ and flag-registry.json), so the lead adds both. A new switch is read as the exact literal "true" and defaults off.
1. flag-registry.json (apps/Control Plane/flag-registry/flag-registry.json), two rows in the file's existing row shape, both default off, strict "true":
   - MODEL_GATEWAY_PROMPT_SECTION_NAMES: service model-gateway, default "false". Description: tag the authored instructions by source, as one named system message per section on an Ask turn and RunContext.authored_sections on a run; needs EXECUTION_PROMPT_SECTION_NAMES on to render on a run.
   - EXECUTION_PROMPT_SECTION_NAMES: service execution-core, default "false". Description: render RunContext.authored_sections as one named system message each; the gateway switch must also be on for sections to arrive.
2. Compose (apps/Model Plane/deploy/docker-compose.yml): in the model-gateway environment block add `MODEL_GATEWAY_PROMPT_SECTION_NAMES: ${MODEL_GATEWAY_PROMPT_SECTION_NAMES:-false}`. In the execution-core environment block add `EXECUTION_PROMPT_SECTION_NAMES: ${EXECUTION_PROMPT_SECTION_NAMES:-false}`. Use the same literal style as the neighbouring switches.
3. Proto and golden: execution.proto already has the new field and message (RunContext field 10, message AuthoredSection), and it is additive. Bindings come from the buf recipe or FU-11. Run the buf regen, lint and breaking check, and refresh any vendored copy of execution.proto or generated SDK. No admission-golden change.
4. Migrations: none.

## P2-mg-02

1) apps/Control Plane/flag-registry/flag-registry.json: APPROVAL_STEP_UP_ENFORCED appears only in the unregisteredSwitches list (about line 4926). Add a row to "flags" and remove the name from unregisteredSwitches. Proposed row, same shape as CLIENT_VERSION_POLICY: {"name": "APPROVAL_STEP_UP_ENFORCED", "kind": "choice", "choiceRead": "anyCase", "owner": "model-plane", "source": "compose", "default": "off", "values": ["off", "shadow", "enforce"], "purpose": "WS13-S5 / F-f03-14: off changes nothing; shadow counts a grant that lacks a fresh re-authentication for its cause and lets it through; enforce refuses it with step_up_required. Unknown values are off.", "declarations": [{"service": "model-gateway", "env": "APPROVAL_STEP_UP_ENFORCED", "default": "off"}], "codePath": "apps/Model Plane/rust/services/model-gateway/src/http_routes.rs"}.
2) No compose change needed. APPROVAL_STEP_UP_ENFORCED is already in apps/Model Plane/deploy/docker-compose.yml line 1167 under model-gateway (default off). CODE_INTERPRETER_VERIFY_ENFORCE is already in the registry (line about 810, model-gateway default false) and in compose (line 944 under model-gateway).
3) For F-f03-13 (P2-sc-11), which is not this item: add `bool step_up_verified = 14;` to DecideApprovalRequest in apps/Model Plane/proto/model_plane/v1/orchestration.proto (14 is the next free field; 1-13 are used), then regenerate the Go, Rust and Python bindings. Not done here.
4) No new switch, migration number, admission golden, ZDR.md row or proto change from this item.

## P2-mg-03

A. apps/Control Plane/flag-registry/flag-registry.json: add two rows to the flags array, for example after the MODEL_GATEWAY_SPACE_APPEND_RETRIEVAL row.
 Row 1: name MODEL_GATEWAY_MODEL_ACCESS_FAIL_CLOSED; kind switch; onLiteral "true"; owner model-plane; source compose; default "false"; purpose "F-f01-11: only the literal true makes an admission question that inference-core cannot answer refuse the turn at the gateway with model_entitlement_unavailable (503), before any thread, message or run row. Unimplemented and NotFound keep the default, so an older inference-core still serves. Off, the turn goes on to inference-core's own pre-check. Embeddings are not gated."; declarations: [{ service model-gateway, env MODEL_GATEWAY_MODEL_ACCESS_FAIL_CLOSED, default "false" }].
 Row 2: name CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS; kind text; owner model-plane; source compose; default ""; purpose "F-f01-12: comma-separated lower-case hosts a hosted image URL may name (*. and a domain for any subdomain). Set, the address must be https, carry no userinfo, not be an IP literal, and name a listed host. Any other address is refused as attachment_encoding_invalid, and the address is never echoed. Unset or empty keeps every hosted address as before. A list with no valid entry allows no host."; declarations: [{ service model-gateway, env CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS, default "" }].
B. apps/Model Plane/deploy/docker-compose.yml, service model-gateway (block from line 194 to 1199; line numbers as of this read):
 - After the CHAT_ATTACHMENT_MAX_TOTAL_BYTES line (line 441), add at six-space indent: CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS: ${CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS:-}
 - After the MODEL_GATEWAY_SPACE_APPEND_RETRIEVAL line (line 452), add at six-space indent: MODEL_GATEWAY_MODEL_ACCESS_FAIL_CLOSED: ${MODEL_GATEWAY_MODEL_ACCESS_FAIL_CLOSED:-false}
 Then run GOWORK=off go test -count=1 . in apps/Control Plane/flag-registry. Only the two ORG_COMPLIANCE failures should remain.
C. Migrations: none. Golden, proto and vendored-copy changes: none. The admission golden, zdr-posture-matrix.json and AttachmentCode are untouched.
D. Optional, see leftovers item 8: api/contracts/space-surface.v1.json (with its tests/fixtures copy and .sha256 pins) if the POST /v1/browser/runs row should name browser_run_start_route.

## P2-mg-03-rel1

1. apps/Model Plane/deploy/docker-compose.yml, model-gateway block, next to CHAT_ATTACHMENT_MAX_TOTAL_BYTES (about line 441): no line declares CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS, so a value in an operator's env file never reaches the container. Add `CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS: ${CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS:-}`; an empty value keeps the list unset, which is the unchanged behaviour. This belongs to P2-mg-03, since the chat half needs it too, and I did not add it.
2. No flag-registry row is needed, because the list is not a switch. No migration, proto, golden or admission-golden change is needed for this item.
3. Commit attachment_policy.rs together with P2-mg-03's uncommitted hosted-list hunks, which were already in the file before my edits.

## P2-mg-04-rel1

No registry rows, compose lines, migration numbers, proto or golden changes are needed for this item. HOOKS_DURABLE is already declared. Lead actions after verification: (a) mark depth-register rows D2-1013 and D2-1016 resolved by P2-mg-04-rel1 once the hook run and hook_proxy_zdr_test pass; D2-1015 can be closed for the grpc dispatch part. (b) Decide the gRPC user-caller bearer question (leftover 2) before HOOKS_DURABLE is switched on anywhere. (c) Optionally update the docker-compose.yml HOOKS_DURABLE comment once the owner decides leftover 2.

## P2-mg-05

Compose (lead to apply): apps/Model Plane/deploy/docker-compose.production.yml, execution-core environment block. Insert after the line `      INFORMATION_CORE_INTERNAL_KEY: ${INTERNAL_API_KEY:?INTERNAL_API_KEY is required}` (currently line 132) and before `  orchestrator-core:` (line 133), at 6-space indent:
      # Dev-only keys (O-05, DEC-F02-3, DEC-F04-10): production sets its own values. The public half of the provider-write attestation key is registered under issuer "model-execution" in integration-corev2's INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON.
      EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY: ${EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY:?EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY is required}
      EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID: ${EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID:?EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID is required}
Registry rows: none (no new switch; all switch names are existing ones). Migrations: none. Golden or proto changes: none (apps/Model Plane/api/contracts/agent-action-admission.json and .proto files untouched).

## P2-mg-06

(1) Flag registry, C:/dev/mp-w345/apps/Control Plane/flag-registry/flag-registry.json. Insert the first row between the MODEL_GATEWAY_BUDGET_WINDOW object (its closing "}," is at line 2290) and MODEL_GATEWAY_BYO_GATEWAY. Insert the second row between the MODEL_GATEWAY_DEDUP_CONTEXT_HISTORY object (its closing "}," is at line 2442) and MODEL_GATEWAY_EFFORT_LADDER.
ROW A: { "name": "MODEL_GATEWAY_BUDGET_WINDOW_PER_CAP", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f02-04: the cost cap and the token cap are each measured over their own reset window, not the widest. Needs MODEL_GATEWAY_BUDGET_WINDOW on; deploy cost-core first. Off keeps the shared window. Only the literal true turns it on.", "codeDefault": "false", "declarations": [ { "service": "model-gateway", "env": "MODEL_GATEWAY_BUDGET_WINDOW_PER_CAP", "default": "false" } ] }
ROW B: { "name": "MODEL_GATEWAY_DO_RUN_CAP", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f04-08: a Do run states the smaller of the request and org token ceilings, and of a goal turn's remaining allowance. Execution-core holds it only with EXECUTION_RUN_TOKEN_CAP on. Off sends none. Only the literal true turns it on.", "codeDefault": "false", "declarations": [ { "service": "model-gateway", "env": "MODEL_GATEWAY_DO_RUN_CAP", "default": "false" } ] }
Checked in a scratch copy: with both rows and the compose lines below applied, the registry tests named under tests pass.
(2) Compose, C:/dev/mp-w345/apps/Model Plane/deploy/docker-compose.yml, service model-gateway. Insert after line 270 (the line "      MODEL_GATEWAY_CAP_MIN: ${MODEL_GATEWAY_CAP_MIN:-false}"), 6-space indent, in this order:
      # F-f02-04: each org cap is measured over its own reset window (the cost cap over its row's period, the token cap over its own), not the widest of the two. Needs MODEL_GATEWAY_BUDGET_WINDOW on. Deploy cost-core FIRST: an older cost-core refuses the since_cost and since_tokens fields and the gateway then fails closed with 503. Off by default; only the literal `true`.
      MODEL_GATEWAY_BUDGET_WINDOW_PER_CAP: ${MODEL_GATEWAY_BUDGET_WINDOW_PER_CAP:-false}
      # F-f04-08: a Do run's RunAgentRequest states the smaller of the request's and the org's metered-token ceilings, and a goal turn's remaining allowance when that is smaller. Execution-core enforces it only with EXECUTION_RUN_TOKEN_CAP on, so the rollout order is free. Off by default; only the literal `true` enables it.
      MODEL_GATEWAY_DO_RUN_CAP: ${MODEL_GATEWAY_DO_RUN_CAP:-false}
MODEL_GATEWAY_BUDGET_WINDOW itself is declared by F-f04-10, not here.
(3) Generated bindings for execution.proto field 29 (max_run_tokens), not run by me. Run buf generate for: C:/dev/mp-w345/apps/Model Plane/go/gen/model_plane/v1/execution.pb.go (it has no MaxRunTokens yet, and another track also has it modified), C:/dev/mp-w345/apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/execution_pb.ts, and C:/dev/mp-w345/apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/execution_pb2.py. Rust regenerates from build.rs. The buf breaking baseline refresh is the lead's call. Tag 29 is the next free number; tag 28 is ext_skills.
(4) Migrations: none. The admission golden is unchanged.
(5) Deploy order: deploy cost-core before MODEL_GATEWAY_BUDGET_WINDOW_PER_CAP is turned on. MODEL_GATEWAY_DO_RUN_CAP has no ordering against EXECUTION_RUN_TOKEN_CAP.

## P2-spa-01

NO COMPOSE LINES NEEDED. The backend reuses existing switches and I added no new Go or Rust switch: `SCHEDULE_CONTRACT_ENABLED` is already declared for user-core, capability-core and gateway, and `CRON_CONTRACT_GUARDS_ENABLED` is already read in capability-core `cmd/main.go` (O-03). I did not edit the flag registry, compose files, the admission golden or any .proto.

FLAG REGISTRY (only if no other unit adds it first, since P2-spa-07 / F-f05-20 owns this build flag): add a row for the SPA switch, modelled on `VITE_AGENT_RESULT_FEEDBACK`:
{
  "name": "VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "frontend-plane",
  "source": "code",
  "default": "false",
  "stage": "dark",
  "purpose": "Schedule renewal in place (O-10): Renew approval opens the dialog in renewal mode and renews the existing schedule with one PATCH of its id, instead of creating a copy. Needs SCHEDULE_CONTRACT_ENABLED and CRON_CONTRACT_GUARDS_ENABLED on the server. Only the literal true turns it on.",
  "codePath": "apps/Frontend Plane/verevonv3/src/shared/api/schedule-from-chat.ts"
}
If the frontend build passes it as a compose build arg, also add `VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED` (default "false") to the `frontend` service, like the other VITE_* build args.

WIRE CONTRACT: a new action schema string, `sha256:space-cron-renew-v1`, is signed by user-core and verified by capability-core. The BFF also checks for it. Action id `model.cron.create` and permission `cron:create` are unchanged. Deploy user-core and capability-core together, because capability-core refuses a renewal decision it cannot verify.

MIGRATIONS: none. The renewal audit row reuses `registry_audit_log` (primary key `id`), and `template_digest` already exists on `cron_schedules`.

GOLDEN / PROTO: none.

## P2-spa-02

A) Proto bindings. Rust needs nothing: mp-contracts builds with max_level through build.rs. Go: apps/Model Plane/go/gen/model_plane/v1/orchestration.pb.go is stale. Regenerate Go only, as the ledger precedent (docs/decisions/ledger.md ~5667) did: from apps/Model Plane/proto run buf generate --template buf.gen.go-only.yaml, scoped to model_plane/v1/orchestration.proto. Do not run it repo-wide, because go/gen is drifted and a broad run rewrites unrelated dataplane files (docs/capability-ownership-matrix.md). Python: apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/orchestration_pb2.py is also stale; regenerate only if you choose to. B) OpenAPI, optional but recommended. apps/Model Plane/api/openapi.yaml, schema ApprovalCause (~line 2118). Add "browser_site_consent" to the code enum before "unclassified_gate". Add a property at the same level as rule_offered: max_level: {type: string, nullable: true, enum: [read, act], description: "The highest level the org's site policy allows for host on a browser_site_consent pause. Null for any other cause."}. The schema already lacks the EP-1 assist_kind, assist_origin and host fields (pre-existing drift), so decide whether to fix that at the same time. The SDK models are generated from it: apps/Model Plane/api/sdk/typescript/src/models/ApprovalCause.ts and apps/Model Plane/api/sdk/python/model_plane_sdk/models/approval_cause.py. C) execution-core/tests/approval_gate_contract.rs is already edited in the tree and sits outside my Files list. It adds BrowserSiteConsent to the variant map and sets RESERVED = ["BrowserSiteConsent"]; the golden now lists the code, so keep it. D) No registry rows, compose lines, migration numbers, admission golden, switches or sha256 changes are needed for this unit.

## P2-spa-02-rel1

1) Commit as one change: ChatMessages.tsx imports browserSiteConsentOf from approval-copy.ts, and approval-copy.ts, approval-copy.test.ts and orchestration-client.ts (plus its test) carry P2-spa-02's uncommitted hunks (the browser_site_consent words, ApprovalCause.maxLevel, and normalizeApprovalCause reading max_level). Commit those with ChatMessages.tsx, ChatMessages.site-consent.test.tsx and browser-sites-client.ts.
2) No registry rows, compose lines, migration numbers, golden or proto changes come from this unit. apps/Model Plane/api/contracts/approval-causes.json already lists browser_site_consent with scope_options ['once'].
3) Optional: declare VITE_BROWSER_SITES in apps/Frontend Plane/verevonv3/src/vite-env.d.ts. It is read untyped today by BrowserSitesSection.tsx, KnowledgeComposer.tsx, GlobalAssistant.tsx and CoreSidebarSettingsPanels.tsx. I did not touch vite-env.d.ts because another agent has hunks in it. The flag registry has no row for VITE_* switches (VITE_BROWSER_APPROVAL_CARD has none either), so I propose none.
4) Environment pairing: set VITE_BROWSER_SITES together with BROWSER_DO_ENABLED or BROWSER_SITES_ENABLED.

## P2-spa-03

A) Flag registry (lead adds; I did not edit apps/Control Plane/flag-registry/flag-registry.json). Insert this row between the VITE_EVENT_TRIGGERS and VITE_PLAN_EXECUTE_ENABLED rows, in the SPA-switch block (the other SPA switches have rows; VITE_BYO_GATEWAY has none):
{ "name": "VITE_MODEL_DATA_USE_LABEL", "kind": "switch", "onLiteral": "true", "owner": "frontend-plane", "source": "code", "default": "false", "stage": "dark", "purpose": "F-f08-17 (DEC-F08-03): the model picker's data-use line (zero data retention and training statements from the catalogue). Off until counsel approves the English and Norwegian copy; the owner turns it on after that. Only the literal true turns it on.", "declarations": [ { "service": "frontend", "env": "VITE_MODEL_DATA_USE_LABEL", "default": "false" } ], "codePath": "apps/Frontend Plane/verevonv3/src/shared/components/AiModelPicker.tsx" }

B) SPA example env (lead): add VITE_MODEL_DATA_USE_LABEL=false, in the same form as the neighbouring entries, next to VITE_BYO_GATEWAY in apps/Frontend Plane/verevonv3/.env.example (tracked) and in any other tracked example list that names VITE_BYO_GATEWAY. I did not open any .env* file, so the lead confirms which lists name it. Default off, example files only.

C) Compose: none. git grep over the tracked tree finds no compose or Dockerfile that names VITE_BYO_GATEWAY, so no compose line is needed.

D) Migrations, admission golden (and its .sha256 and vendored copies), and .proto generated code: none.

## P2-spa-03-rel1

VITE_BYO_GATEWAY has no row in apps/Control Plane/flag-registry/flag-registry.json. The only BYO rows there are the unrelated INFERENCE_BYO_GATEWAY_ENABLED and MODEL_GATEWAY_BYO_GATEWAY. When the lead adds the row, its purpose text should say the switch also gates plain sends and launches into a Space thread whose Space ref is still loading or failed (P2-spa-03 rel1), not only the composer's Space route gate. Default off; read as the strict literal 'true' in apps/Frontend Plane/verevonv3/src/features/chat/lib/space-side-features.ts (byoGatewaySwitchOn). No compose lines, migration numbers, golden or proto changes are needed for this item.

## P2-spa-04

A) apps/Control Plane/flag-registry/flag-registry.json: insert this object between the closing "}," of MODEL_GATEWAY_SEND_RECEIPTS (line 2854) and the opening "{" of MODEL_GATEWAY_SHARED_IDEMPOTENCY (line 2855). Key order and indent match the SEND_RECEIPTS object:
    {
      "name": "MODEL_GATEWAY_QUEUED_UNDELIVERED_IDS",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "purpose": "F-f01-09: the chat stream's done frame names the accepted mid-run messages the loop never took (undelivered_client_message_ids), so the SPA resends exactly those. Needs MODEL_GATEWAY_SEND_RECEIPTS on. Off, the done frame is unchanged. Only the literal true turns it on.",
      "declarations": [
        { "service": "model-gateway", "env": "MODEL_GATEWAY_QUEUED_UNDELIVERED_IDS", "default": "false" }
      ]
    },
Then check that the file still parses as JSON.
B) apps/Model Plane/deploy/docker-compose.yml: add these two lines directly after line 406 (MODEL_GATEWAY_SEND_RECEIPTS: ${MODEL_GATEWAY_SEND_RECEIPTS:-true}), in the model-gateway environment, with a 6-space indent:
      # F-f01-09: the done frame names the accepted mid-run messages the loop never took. Needs send receipts on; only the literal true turns it on.
      MODEL_GATEWAY_QUEUED_UNDELIVERED_IDS: ${MODEL_GATEWAY_QUEUED_UNDELIVERED_IDS:-false}
C) Already done in this pass, not a shared edit: the EXACT_SWITCHES row (crate::sse::QUEUED_UNDELIVERED_IDS_VAR, "true") at switch_spellings.rs line 137.
D) No migration number (this unit adds no migration). No admission golden, no .sha256, no vendored copy, and no .proto change.

## P2-spa-04-rel1

No registry, compose, migration, golden or proto change was made by me.
Flag: the gateway switch MODEL_GATEWAY_QUEUED_UNDELIVERED_IDS (strict literal "true", default off, read in apps/Model Plane/rust/services/model-gateway/src/sse.rs) is not declared in any tracked file under apps/Control Plane or apps/Model Plane (git grep over the tracked tree; the literal appears only in sse.rs and in the SPA's chat-client.ts doc comment). The SPA adds no switch of its own: this behaviour is gated by whether the gateway names the list. The lead should add the flag-registry.json row and the model-gateway compose line if the default-off switch rule applies, or confirm they exist elsewhere (P2-spa-04 may already owe them).

## P2-spa-07

Registry (lead adds): append to the "flags" array in apps/Control Plane/flag-registry/flag-registry.json, next to VITE_APPROVAL_SCOPE_GRANTS:
{"name": "VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED", "kind": "switch", "onLiteral": "true", "owner": "frontend-plane", "source": "code", "default": "false", "stage": "dark", "orgOverridable": false, "purpose": "F-f05-20 / F-f05-20b: the schedule consent card offers the org's delegable read tools (asked once per tab) and 'Renew approval' renews a lapsed schedule approval in place. Off, the card offers no tool, no request is made, and renewal creates a new schedule as before. Only the literal true turns it on.", "codePath": "apps/Frontend Plane/verevonv3/src/shared/api/schedule-from-chat.ts"}
I used schedule-from-chat.ts as codePath, not use-chat-controller.ts as the spec said, because that is where the switch is read.
Do not add a row for VITE_SCHEDULE_CONTRACT_TOOLS; that name is retired.
Compose: none. No new Go switch, and VITE_ flags are not declared in compose in this repo.
Migrations: none. Admission golden: none. Proto: none.
Optional, not required by this item: CRON_CONTRACT_GUARDS_ENABLED sits in the registry's "unregistered" list; the renewal path depends on it together with SCHEDULE_CONTRACT_ENABLED, which is registered.

## P2-sc-02

No flag-registry row, compose line, golden or proto edit is needed for this unit. Both switches already exist: EXECUTION_SCHEDULED_AGENT_ENABLED and SESSION_CORE_ATTENTION_EVENTS_ENABLED are in apps/Control Plane/flag-registry/flag-registry.json and apps/Model Plane/deploy/docker-compose.yml (SESSION_CORE_ATTENTION_EVENTS_ENABLED defaults to false there). The ScheduledStepContext contract fields and zdr/zdr_waived already exist in apps/Model Plane/proto/model_plane/v1/runs.proto (F-f10-26).

Migration number taken: session-core 0132. Files: apps/Model Plane/rust/services/session-core/migrations/0132_attention_sweep_marks.sql and apps/Model Plane/rust/services/session-core/migrations/revert/0132_attention_sweep_marks.down.sql. Both are untracked; commit them together. The next free session-core number is 0133. F-f03-04 adds no migration.

## P2-sc-03

1. Flag registry, apps/Control Plane/flag-registry/flag-registry.json: add this to flags[], in the shape of EXECUTE_STEP_SPACE_SEARCH_WITHHOLD:
{"name":"SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"F-f05-05: a finished scheduled run writes one content-free SCHEDULED_RUN_OUTCOME outbox row (ids and one closed outcome word), persistent runs only, in the transaction that settles it. Only the literal true turns it on.","codeDefault":"false","declarations":[{"service":"session-core","env":"SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED","default":"false"}]}
Also: the name array near line 4887-4911 of the same file contains SESSION_CORE_ATTENTION_EVENTS_ENABLED and the other session-core switch names. I did not read its semantics. If it is a session-core switch index, add SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED in sorted position, after SESSION_CORE_REQUEST_SNAPSHOTS_SWEEP_ENABLED and before SESSION_CORE_THREAD_FORK_ENABLED.

2. Compose, apps/Model Plane/deploy/docker-compose.yml: in the session-core service environment, directly after the SESSION_CORE_ATTENTION_EVENTS_ENABLED line (currently line 1360), add this line with 6-space indent:
      SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED: ${SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED:-false}

3. Migrations: none needed. Neither unit adds a migration; no migration number is taken.

4. Golden and proto: the admission golden (apps/Model Plane/api/contracts/agent-action-admission.json) is unchanged. The proto source apps/Model Plane/proto/model_plane/v1/attention.proto changed (the rpc and two messages). The generated Go, TypeScript and Python stubs listed under leftovers item 4 need F-f05-99 regeneration.

5. Already done by me, not a shared edit: the ZDR.md boundary row for SCHEDULED_RUN_OUTCOME (apps/Model Plane/docs/ZDR.md).

## P2-sc-04

Migrations: 0133_crash_park_notice.sql and revert/0133_crash_park_notice.down.sql already exist in the tree; no new migration this round.

Flag registry (flags array, name order; lead decides placement since SESSION_CORE_CRASH_PARK_ENABLED and SESSION_CORE_ATTENTION_EVENTS_ENABLED sit under "unregistered"):
{"name":"SESSION_CORE_CRASH_PARK_NOTICE_ENABLED","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"O-02: tell the person once a crash-parked run has waited SESSION_CORE_CRASH_PARK_NOTICE_SECONDS. One content-free RUN_ATTENTION per park, persistent runs only, on the chat and agent-run origins the attention list covers, and only for the run that is its thread's latest; the attention list then reads the run as NEEDS_INPUT with reason PARKED. Needs SESSION_CORE_CRASH_PARK_ENABLED and SESSION_CORE_ATTENTION_EVENTS_ENABLED on, or it writes nothing. Only the literal true.","declarations":[{"service":"session-core","env":"SESSION_CORE_CRASH_PARK_NOTICE_ENABLED","default":"false"}],"codePath":"apps/Model Plane/rust/services/session-core/src/loop_checkpoint.rs"}
{"name":"SESSION_CORE_CRASH_PARK_NOTICE_SECONDS","kind":"text","owner":"model-plane","source":"compose","default":"14400","purpose":"O-02: the wait before the crash-park notice, in seconds (four hours by default), clamped to 600 up to SESSION_CORE_PARK_TTL_SECONDS. Read only while the notice switch is on; a value that is not a whole number reads as the default.","declarations":[{"service":"session-core","env":"SESSION_CORE_CRASH_PARK_NOTICE_SECONDS","default":"14400"}],"codePath":"apps/Model Plane/rust/services/session-core/src/loop_checkpoint.rs"}

Compose (apps/Model Plane/deploy/docker-compose.yml, session-core block, after SESSION_CORE_PARK_TTL_SECONDS line):
        # O-02: tell the person once a crash-parked run has waited the notice time. Off by default; only the literal true.
        # Needs SESSION_CORE_CRASH_PARK_ENABLED and SESSION_CORE_ATTENTION_EVENTS_ENABLED on, or nothing is written.
        SESSION_CORE_CRASH_PARK_NOTICE_ENABLED: ${SESSION_CORE_CRASH_PARK_NOTICE_ENABLED:-false}
        # O-02: the wait before that notice, in seconds. Default four hours, clamped to 600 up to the park TTL.
        SESSION_CORE_CRASH_PARK_NOTICE_SECONDS: ${SESSION_CORE_CRASH_PARK_NOTICE_SECONDS:-14400}

Proto (attention.proto already carries ATTENTION_REASON_PARKED = 6 and ConfirmAttentionWaiting). Regenerate once for both, from apps/Model Plane/proto:
- Go: buf generate --template buf.gen.go-only.yaml --path model_plane/v1/attention.proto
- Python: buf generate --template buf.gen.yaml --path model_plane/v1/attention.proto, keeping only the mp-events-py _gen output.
Rust is generated by tonic_build in mp-contracts build.rs, so no checked-in Rust change.

Admission golden: no change.

## P2-sc-06-rel1

No registry row, compose line, migration number, or golden/proto change is needed. SESSION_THREAD_RECEIPT_ENFORCE is already registered at apps/Control Plane/flag-registry/flag-registry.json line 3822 (default false, session-core declaration), and this item adds no new switch. Optional doc edit for the lead: the purpose text of that registry row (line 3828) describes CreateThread and AppendMessage only. It could also say that ForkThread refuses a source thread whose receipt is zdr or zdr_waived. I did not edit the registry. Commit note: thread_fork.rs includes the already-uncommitted C-24a copy_thread and insert_fork_thread receipt changes, so the lead should commit them together with this fix.

## P2-sc-07

Apply these after the run; I edited none of these files (they are the lead's, or outside this unit's file list).

A. apps/Control Plane/flag-registry/flag-registry.json: add two switch rows (for example after the SESSION_CORE_AUTHZ_DECISION_AUDIT row).

Row 1, SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT:
{
  "name": "SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "EP-8 S7: writes a content-free space_memory_deleted audit row (ids and closed codes only) in the same transaction as a manager's delete of one Space memory row, and refuses the delete when that row cannot be written. Only the exact literal true turns it on.",
  "declarations": [
    { "service": "session-core", "env": "SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT", "default": "false" }
  ],
  "codePath": "apps/Model Plane/rust/services/session-core/src/audit_publisher.rs"
}

Row 2, SESSION_CORE_TODO_RECITATION_ENABLED (missing today):
{
  "name": "SESSION_CORE_TODO_RECITATION_ENABLED",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "WS02-S12: recites a plan's todos into the Volatile prompt tier, for the plan's owner only, bounded to 40 todos and 6000 characters. Off by default: no model-facing todo producer exists yet. Only the literal true turns it on.",
  "declarations": [
    { "service": "session-core", "env": "SESSION_CORE_TODO_RECITATION_ENABLED", "default": "false" }
  ],
  "codePath": "apps/Model Plane/rust/services/session-core/src/orchestration_store.rs"
}

B. apps/Model Plane/deploy/docker-compose.yml, session-core environment block (the service starts at line 1200; the block ends before inference-core at line 1527). Insert after line 1389 (SESSION_SPACE_MEMORY_ENABLED: ${SESSION_SPACE_MEMORY_ENABLED:-false}):
      # EP-8 S7: a manager's Space memory delete writes a content-free audit row in the same transaction, and is refused when that row cannot be written. Only the exact literal `true` turns it on.
      SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT: ${SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT:-false}
      # WS02-S12: recite a plan's todos into the Volatile prompt tier (owner only, bounded). Off until a todo producer exists. Only the exact literal `true` turns it on.
      SESSION_CORE_TODO_RECITATION_ENABLED: ${SESSION_CORE_TODO_RECITATION_ENABLED:-false}

C. Recommended, apps/Control Plane/flag-registry/exact_literal_switch_pins_test.go, the exactSwitchPins table (keep alphabetical order, after the SESSION_CORE_SPACE_HISTORY_FLOOR line):
	{"SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT", PlaneModel, SourceCompose, "false"},
	{"SESSION_CORE_TODO_RECITATION_ENABLED", PlaneModel, SourceCompose, "false"},

D. Recommended, apps/Model Plane/rust/services/session-core/src/switch_spellings.rs, EXACT_SWITCHES (add after the APPROVAL_NOTIFY_ENV entry so former spellings like 1 or on get the startup warning):
    (crate::audit_publisher::SPACE_MEMORY_DELETE_AUDIT_ENV, "true"),
    (crate::orchestration_store::TODO_RECITATION_ENV, "true"),

E. Migrations: none taken. Golden, proto and admission golden: none needed.

## P2-sc-07-rel1

Flag registry (apps/Control Plane/flag-registry/flag-registry.json): no row exists for SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT. The single-row switch from F-f08-08 was built without one, and this item reuses it. The lead should add a row shaped like SESSION_CORE_AUTHZ_DECISION_AUDIT: name SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT, kind switch, onLiteral "true", owner model-plane, source code, default "false", purpose naming the literal true and covering both the single-row delete and the binding-revocation erase, codePath apps/Model Plane/rust/services/session-core/src/audit_publisher.rs. No compose line is needed for a code-sourced switch. Then run the Go flag-registry tests in apps/Control Plane/flag-registry. Optional, not done: add the switch to exactSwitchPins in exact_literal_switch_pins_test.go and to EXACT_SWITCHES in session-core switch_spellings.rs; both need the registry row first. No migration, proto, golden, or admission changes. Commit dreaming.rs and memory_grpc.rs together with the P2-sc-07 hunks already in them.

## P2-sc-08-rel1

["No registry rows, compose lines, migrations, proto, golden, or flag declarations are changed by this item. No new switch is added; the gate is inside the existing SESSION_AGENT_MEMORY_ENABLED branch.", "Required later, owned outside session-core: a new scope session:agent-memory:write granted to service:execution-core on audience session-core, via the Control Plane principal-train, edited in apps/Control Plane/config/plane-service-principals.json (scopes and scopesByAudience['session-core']). Also an execution-core mint plus a save_memory bearer switch for AGENT_SELF, in session_terminal_auth.rs and runtime_loop/mod.rs. These are the lead's to dispatch.", "Lead to add the compose line and flag-registry row for SESSION_CORE_AGENT_MEMORY_RUN_BOUND (currently missing in both)."]

## P2-sc-09

1) apps/Control Plane/flag-registry/flag-registry.json: add this row to `flags` (next to MODEL_GATEWAY_GOAL_JUDGE_ENABLED / MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED):
{"name":"MODEL_GATEWAY_GOAL_SEGMENT_TOPUP","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"F-f09-11 goal budget top-up: each /continue segment of a goal run debits its metered tokens and cost to the goal through session-core TopUpGoalTurn, once per segment. Needs MODEL_GATEWAY_THREAD_GOALS_ENABLED and SESSION_CORE_THREAD_GOALS_ENABLED. Never for a ZDR or waived run. Only the literal true turns it on.","codeDefault":"false","declarations":[{"service":"model-gateway","env":"MODEL_GATEWAY_GOAL_SEGMENT_TOPUP","default":"false"}]}
(session-core adds no new switch: its RPC rides SESSION_CORE_THREAD_GOALS_ENABLED.)

2) apps/Model Plane/deploy/docker-compose.yml, model-gateway service environment, after the MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED line (about line 567):
      # F-f09-11: each /continue segment of a goal run debits its usage to the goal (session-core TopUpGoalTurn). Needs
      # MODEL_GATEWAY_THREAD_GOALS_ENABLED here and SESSION_CORE_THREAD_GOALS_ENABLED on session-core. Only the literal true.
      MODEL_GATEWAY_GOAL_SEGMENT_TOPUP: ${MODEL_GATEWAY_GOAL_SEGMENT_TOPUP:-false}

3) Migration number: session-core 0136_thread_goal_turn_topups.sql plus revert/0136_thread_goal_turn_topups.down.sql (both created; 0135 was the last). Re-check 0136 is still free before committing, since other agents may add migrations.

4) Proto: thread_goals.proto is edited (the TopUpGoalTurn rpc and the TopUpGoalTurnRequest message, additive). Regenerate the checked-in Go (go/gen/model_plane/v1/thread_goals*.pb.go and thread_goals_grpc.pb.go) and Python (mp_events/_gen/.../thread_goals_pb2.py) with the usual buf recipe, and run buf lint/breaking. I did not touch generated files. The Rust side is generated at build time.

5) Admission golden: no change needed (no agent-action admission surface touched).

## P2-sc-11

A) apps/Control Plane/flag-registry/flag-registry.json: add this row (for example next to APPROVAL_WAIVER_REFUSE / the SESSION_CORE_APPROVAL_* rows):
{
  "name": "SESSION_CORE_APPROVAL_DECISION_RECORD",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "F-f03-16: a decided approval reads back its content-free decision record (source, scope, rule id, step-up, cause code), and the gateway projects it as Approval.decision. Off, every approval reads exactly as before. Only the literal true turns it on.",
  "declarations": [
    { "service": "session-core", "env": "SESSION_CORE_APPROVAL_DECISION_RECORD", "default": "false" }
  ],
  "codePath": "apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs"
}
B) apps/Model Plane/deploy/docker-compose.yml, session-core environment block: insert after the line `      APPROVAL_AUDIT_EVENTS_ENABLED: ${APPROVAL_AUDIT_EVENTS_ENABLED:-false}` (about line 1372):
      # F-f03-16: a decided approval reads back its content-free decision record. Off by default; only the literal true.
      SESSION_CORE_APPROVAL_DECISION_RECORD: ${SESSION_CORE_APPROVAL_DECISION_RECORD:-false}
C) Generated bindings (FU-11, lead runs buf generate; never hand-edit): orchestration.proto DecideApprovalRequest.step_up_verified = 14; Approval.decision = 17; new message ApprovalDecisionRecord. The Rust bindings regenerate automatically through mp-contracts build.rs.
D) Golden or migration changes: none needed. The admission golden is untouched. proto_wire_parity passes. session-core migrations: none.
E) Already done in this unit (not a shared file): api/openapi.yaml Approval.decision and the new ApprovalDecision schema.

## P2-ic-01

A. apps/Control Plane/flag-registry/flag-registry.json: insert in the main switch list (the "flags" array, not unregisteredSwitches) right after the closing "}," of the APPROVAL_SHELL_RULE_PROPOSALS_ENABLED row (that row ends at line 251), and before the APPROVAL_WAIVER_REFUSE row (line 252). The new row:
    {
      "name": "SESSION_CORE_APPROVAL_STEP_UP_CAUSE",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "model-plane",
      "source": "compose",
      "default": "false",
      "purpose": "WS13-S5 (F-f03-12): session-core stamps step_up_required on an approval cause in the v1 step-up set (owner_effect_reserved, provider_write, capability_high_risk with class destructive), and the chat posture never grants a stamped cause (DEC-F03-3). Rollout order: APPROVAL_STEP_UP_ENFORCED=shadow first, then this switch, then enforce. Only the literal true turns it on.",
      "declarations": [
        { "service": "session-core", "env": "SESSION_CORE_APPROVAL_STEP_UP_CAUSE", "default": "false" }
      ],
      "codePath": "apps/Model Plane/rust/crates/mp-contracts/src/approval_cause.rs"
    },

B. apps/Model Plane/deploy/docker-compose.yml (LF): in the session-core environment block, after line 1223 (SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE) and before the ZDR Z-17 comment at line 1224, add these two lines with six-space indent:
      # WS13-S5 (F-f03-12): stamps the step-up bit on approval causes in the v1 set (owner effect, provider_write, destructive high-risk capability); the chat posture never grants them. Rollout: APPROVAL_STEP_UP_ENFORCED shadow, then this, then enforce. Only the literal true.
      SESSION_CORE_APPROVAL_STEP_UP_CAUSE: ${SESSION_CORE_APPROVAL_STEP_UP_CAUSE:-false}

C. No migration number (no schema change). No proto change (ApprovalCause.step_up_required = 7 already exists in orchestration.proto). No change to api/contracts/agent-action-admission.json, its .sha256 or vendored copies, the approval-causes golden, or any .proto. F-f09-14 and F-f01-26 add no switch and need no registry or compose row: APPROVAL_SHELL_RULE_PROPOSALS_ENABLED and INFERENCE_OPENAI_PROMPT_CACHE_KEY are already registered.

## P2-ic-02

A. apps/Model Plane/deploy/docker-compose.yml: add one line directly after each existing LOOP_GUARD_MODE line, with the same 6-space indent. After line 465 (model-gateway block): LOOP_GUARD_RESULT_AWARE: ${LOOP_GUARD_RESULT_AWARE:-false}. After line 1905 (execution-core block): the same line. B. apps/Control Plane/flag-registry/flag-registry.json: (1) In flags[], insert these two objects between the LETTA_TOOL_SEARCH_MODE object (ends about line 1886) and the MCP_APPS_ENABLED object. The text is validated (scratch copy loads, 389 flags, sorted): {"name":"LOOP_GUARD_MODE","kind":"choice","choiceRead":"anyCase","owner":"model-plane","source":"compose","default":"","values":["","off","shadow","enforce"],"purpose":"WS25-S1/S2: unset or off builds no guard. shadow logs verdicts only; enforce reminds the model and ends a loop. shadow and enforce read in any case; any other value, OFF included, reads as shadow. Enforce only after shadow data (D2-824).","declarations":[{"service":"execution-core","env":"LOOP_GUARD_MODE","default":""},{"service":"model-gateway","env":"LOOP_GUARD_MODE","default":""}]} and {"name":"LOOP_GUARD_RESULT_AWARE","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"WS25-S2 (F-f05-08): with LOOP_GUARD_MODE on, a call whose result changed since its last run is progress, not a repeat or cycle, so polling is not stopped. Off keeps today's rules. Only the literal true turns it on; enable with enforce.","declarations":[{"service":"execution-core","env":"LOOP_GUARD_RESULT_AWARE","default":"false"},{"service":"model-gateway","env":"LOOP_GUARD_RESULT_AWARE","default":"false"}]} (format them one property per line, as the neighbours are). (2) In unregistered[] (about line 4890), remove "LOOP_GUARD_MODE". C. apps/Control Plane/flag-registry/choice_read_test.go: add one entry to choiceReadPins between the LETTA_TOOL_SEARCH_MODE and MCP_DO_TOOLS entries: {"LOOP_GUARD_MODE", ChoiceReadAnyCase, "model-gateway tool_loop.rs and execution-core agent.rs: LoopGuardMode::from_env_value trims and folds shadow and enforce; off matches exactly after trimming; any other value reads as shadow"}, D. Migrations: none. Admission golden: none. Proto: none from my unit. The sandboxes.proto change belongs to another agent.

## P2-ic-02-rel1

- LOOP_GUARD_RESULT_AWARE has no flag-registry row and no compose line yet. A grep finds no match in apps/Control Plane/flag-registry/flag-registry.json or apps/Model Plane/deploy/docker-compose.yml. LOOP_GUARD_MODE is declared at apps/Control Plane/flag-registry/flag-registry.json line 4890 and in the compose file, so the lead should add the matching default-off declaration for LOOP_GUARD_RESULT_AWARE, read as the literal 'true'. The switch spelling is already listed at apps/Model Plane/rust/services/model-gateway/src/switch_spellings.rs line 301. This is P2-ic-02's shared edit, not this item's.
- No migration, golden, proto or registry change comes from this item.
- Ledger (lead): record the digest normalisation rule (the key list, the text rules and the decision to keep the restart cap unbuilt) in apps/Model Plane/docs/decisions/ledger.md before any enforce flip.

## P2-ic-03

F-f07-04: no registry row, compose line, migration number, admission golden or proto change. The lead must regenerate the fixture and its pin with FU-11: cd "C:/dev/mp-w345/apps/Model Plane/rust" && MP_UPDATE_PROMPT_MANIFEST=1 cargo test -q -p mp-contracts --test prompt_tool_manifest. Then clear the scenario-touch gate for the 43 changed entries (see leftovers 1). F-f07-08: no registry row, compose line, migration number, admission golden or proto change. There is no new switch (ROUTING_POLICY_PROMOTION_REQUIRED already gates the path). COST_CORE_URL already reaches inference-core: deploy/docker-compose.yml, inference-core service, COST_CORE_URL line 1647.

## P2-ic-03-rel1

Golden (lead): once the in-flight manifest-affecting source edits settle, from 'C:/dev/mp-w345/apps/Model Plane/rust' run: MP_UPDATE_PROMPT_MANIFEST=1 CARGO_TARGET_DIR=C:/dev/mp-target-w345 cargo test -q -p mp-contracts --test prompt_tool_manifest. This rewrites apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/prompt-tool-manifest.json and its .sha256 pin. This item adds tool/tool_loop.rs/read_page and tool/tool_loop.rs/find_in_page (source services/model-gateway/src/tool_loop.rs). Check the change report first, because it also includes the other in-flight items. Scenario gate (lead): scripts/gates/scenario-touch.py treats the regenerated fixture as a trigger. The same change must add an eval case under 'apps/Model Plane/python/eval-lab-py/cases/' or a replay cassette under 'apps/Model Plane/rust/crates/mp-inference-replay/cassettes/', or a reviewed exception for the new manifest digest in scripts/gates/baselines/scenario-touch-exceptions.json (CODEOWNERS-reviewed). I did not touch the fixture, the pin, the exceptions file, or any scenario. This item changes no behaviour, so a reviewed exception is the honest route unless a scenario is already in the change. No registry rows, compose lines, migration numbers or proto changes for this item.

## P2-ic-04

A. apps/Control Plane/flag-registry/flag-registry.json: insert after the MODEL_GATEWAY_USER_PROFILE_PREFIX row (both new, default false, strict true):
{ "name": "MODEL_GATEWAY_PROMPT_SECTION_NAMES", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f08-16/F-f08-15: an Ask turn sends one named system message per authored source; the standing profile and memory blocks carry their section tags. execution-core needs EXECUTION_PROMPT_SECTION_NAMES on as well for a run to render them. Only the literal true turns it on.", "declarations": [ { "service": "model-gateway", "env": "MODEL_GATEWAY_PROMPT_SECTION_NAMES", "default": "false" } ] },
{ "name": "EXECUTION_PROMPT_SECTION_NAMES", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false", "purpose": "F-f08-16/F-f08-15: a run seeds its tagged authored sections, profile and memory as named system messages. The gateway needs MODEL_GATEWAY_PROMPT_SECTION_NAMES on to send them. Only the literal true turns it on.", "declarations": [ { "service": "execution-core", "env": "EXECUTION_PROMPT_SECTION_NAMES", "default": "false" } ] }

B. apps/Model Plane/deploy/docker-compose.yml, two env lines:
- service model-gateway (block starts ~line 194), beside MODEL_GATEWAY_USER_PROFILE_PREFIX (~line 515), 6-space indent: MODEL_GATEWAY_PROMPT_SECTION_NAMES: ${MODEL_GATEWAY_PROMPT_SECTION_NAMES:-false}
- service execution-core (block starts ~line 1810), beside PROVIDER_READ_TOOL_ENABLED (~line 2019): EXECUTION_PROMPT_SECTION_NAMES: ${EXECUTION_PROMPT_SECTION_NAMES:-false}

C. F-f01-21: no change. INFERENCE_CORE_CODEX_WIDE_EFFORT already has its registry row (source "code", codePath effort.rs). It has no compose line, which is correct for source "code".

D. F-f04-14: no registry, compose, golden or proto change. PROVIDER_READ_TOOL_ENABLED and CONNECTOR_ASK_READS_ENABLED are unchanged.

E. Migrations: none. Admission golden and .proto files: none changed by these units.

## P2-ic-05-rel1

Commit split. The P2-ic-05 golden and the rel1 golden must be separate commits. Snapshots are in C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\p2ic05-rel1\ . Commit A (P2-ic-05 golden): fixture prompt-tool-manifest.json.before (digest d1872a705d7a067871be276806359f8424951dc7babfd1d996587b95c4c65e28), pin prompt-tool-manifest.json.sha256.before, exceptions scenario-touch-exceptions.json.before (only the d1872a70 entry), plus the working-tree scripts/gates/tests/test_scenario_touch.py, which is valid for both states. Commit B (this item): the working-tree fixture prompt-tool-manifest.json (copy kept as prompt-tool-manifest.json.rel1, digest 48754273a6baf5eb73861c77417a110e22c37462d04855e383b3fde5696b9d50), the working-tree pin (copy .sha256.rel1), and the working-tree scenario-touch-exceptions.json (both entries). Check each commit state with: cargo test -q -p mp-contracts --test prompt_tool_manifest, and pytest scripts/gates/tests/test_scenario_touch.py. Do not commit prompt_tool_manifest.rs in either commit, since it belongs to the other agent. No registry rows, compose lines, migration numbers, proto or generated-code changes come from this item.

## P2-go-01

1. Migration: `apps/Model Plane/go/services/sandbox-manager/migrations/0009_workspace_overlay_tombstones.up.sql` and `.down.sql` (already created by me; number 0009, no other sandbox-manager migration appeared during the run).

2. Compose, `apps/Model Plane/deploy/docker-compose.yml`, `sandbox-manager` service environment block. Add right after the `SANDBOX_MANAGER_EGRESS_TIERS` line (about line 2833):
```
      # F-f05-16 (D-F05-6): a Space overlay can carry deletions (tombstones) that PromoteWorkspace honours with compare-and-swap. Only the exact literal true enables it.
      SANDBOX_MANAGER_OVERLAY_TOMBSTONES_ENABLED: ${SANDBOX_MANAGER_OVERLAY_TOMBSTONES_ENABLED:-false}
```

3. Registry, `apps/Control Plane/flag-registry/flag-registry.json`, new entry in the `flags[]` array, same shape as `SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM`:
```json
{
  "name": "SANDBOX_MANAGER_OVERLAY_TOMBSTONES_ENABLED",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "off",
  "purpose": "F-f05-16: SnapshotSandbox records a Space lease's deleted_paths as tombstones on its overlay and PromoteWorkspace deletes the Space row with compare-and-swap. Only the exact literal true enables it; off, deleted_paths is ignored and behaviour is unchanged.",
  "declarations": [
    { "service": "sandbox-manager", "env": "SANDBOX_MANAGER_OVERLAY_TOMBSTONES_ENABLED", "default": "off" }
  ],
  "codePath": "apps/Model Plane/go/services/sandbox-manager/cmd/main.go"
}
```
(Add the name to any parallel name list or ratchet the registry keeps, as for EGRESS_TIERS.)

4. Proto and generated code: `sandboxes.proto` is already edited (`repeated WorkspaceDeletedPath deleted_paths = 5;` on SnapshotRequest, plus `message WorkspaceDeletedPath { string path = 1; string base_hash = 2; }`). The lead or F-f05-99 must regenerate `apps/Model Plane/go/gen/model_plane/v1/sandboxes.pb.go` (and any vendored or other-language copies, for example the Rust gen for execution-core's later caller). Then do the follow-up in leftovers.

5. Admission golden (`agent-action-admission.json`): no change needed; this unit adds no action or admission surface."

## P2-go-02

Unchanged from the verifier's SHARED notes, all of which are the lead's to apply. (1) Go regen: from apps/Model Plane/proto run buf generate with the local v2 template at C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/buf.gen.local.yaml, with --path model_plane/v1/sandboxes.proto. Copy only sandboxes.pb.go and sandboxes_grpc.pb.go into apps/Model Plane/go/gen/model_plane/v1. Delete line 1 (`//go:build retention_erase_gen`) of retention_erase_grpc.go and retention_erase_grpc_test.go in sandbox-manager/internal/server, then run go test ./... in sandbox-manager. Also regenerate the Python stub apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/sandboxes_pb2.py with the mp_events recipe. The Rust stubs come from mp-contracts/build.rs and need no action. (2) Compose, in the sandbox-manager environment block right after SANDBOX_MANAGER_EGRESS_TIERS (about line 2833): `SANDBOX_MANAGER_SERVICE_THREAD_ERASE: ${SANDBOX_MANAGER_SERVICE_THREAD_ERASE:-false}`. (3) Flag registry: add the SANDBOX_MANAGER_SERVICE_THREAD_ERASE entry from the verifier's SHARED block after SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM (kind switch, onLiteral "true", owner model-plane, source compose, default "false", stage dark, one declaration for sandbox-manager, codePath apps/Model Plane/go/services/sandbox-manager/cmd/main.go). (4) Principals: the session-core sandbox-manager audience is already in the tree. Run the PRINCIPAL-TRAIN checks once with a single editor: principal-registry-parity.test.sh, zdr-contract-sync.test.sh, control-train-repin.sh. (5) Migration: none. (6) No admission golden change. (7) Stage by hunk: server.go, cmd/main.go, main_test.go and plane-service-principals.json also carry hunks from F-f06-02 (closeBlobErasure, threadErasureFromEnv) and F-f05-16 (tombstones, overlay tombstones), plus the cost-core audience line in the principals file.

## P2-go-03

Compose and registry edits for the lead (I changed neither):

1. `apps/Model Plane/deploy/docker-compose.yml`, service `cost-core` (next to `COST_CORE_AUTH_AUDIENCE`, about line 3108):
   `      COST_CORE_ACCEPT_API_KEY: ${COST_CORE_ACCEPT_API_KEY:-false}`

2. Same file, service `model-gateway` (next to `MODEL_GATEWAY_AGENT_API`, about line 549):
   `      # O-18: budget-check Agent API key turns before the model call. Only the literal true.`
   `      AGENT_API_BUDGET_CHECK: ${AGENT_API_BUDGET_CHECK:-false}`
   `      # O-18: lets the key's own cost-core bearer be the fallback. Must match cost-core's value. Only the literal true.`
   `      COST_CORE_ACCEPT_API_KEY: ${COST_CORE_ACCEPT_API_KEY:-false}`

3. `apps/Control Plane/flag-registry/flag-registry.json`, two rows in alphabetical position:
   {"name":"AGENT_API_BUDGET_CHECK","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"O-18 / F-f10-23b: Agent API key turns are checked against the org's spend ceilings (cost-core budget check for apikey:<key_id>) before the model call. Off, a key turn is not budget-checked. Only the literal true turns it on.","declarations":[{"service":"model-gateway","env":"AGENT_API_BUDGET_CHECK","default":"false"}]}
   {"name":"COST_CORE_ACCEPT_API_KEY","kind":"switch","onLiteral":"true","owner":"model-plane","source":"compose","default":"false","purpose":"O-18: cost-core admits an api_key principal on POST /api/v1/budget/check only, for its own key principal (user_id forced to apikey:<key_id>, org pinned to the token). model-gateway reads the same literal to allow the key's own cost-core bearer as the fallback path. Off, the principal type is refused as before. Only the literal true turns it on.","declarations":[{"service":"cost-core","env":"COST_CORE_ACCEPT_API_KEY","default":"false"},{"service":"model-gateway","env":"COST_CORE_ACCEPT_API_KEY","default":"false"}]}

4. Migrations: none. Proto: none. Admission golden: none.
   `agent-api-v1.json`, its OpenAPI spec and `.sha256` are NOT touched (the refusal is a failed run, not a new public error code).

5. The principal-train pins for `plane-service-principals.json` (the `cost-core` entry in the `apiKeyExchange` row) were already in the tree. If the repo script re-pins a digest, re-run it. The policy contract spec passes as is.

## P2-go-04

(a) Migration: number 0045 (apps/Model Plane/go/services/capability-core/migrations/0045_learning_review_counters.up.sql and .down.sql, already in the tree and untracked). No golden, proto or admission-golden change. (b) apps/Control Plane/flag-registry/flag-registry.json: add this object to the `flags` array, in name order, immediately before the LEARNING_REVIEW_ENABLED row (the array is sorted by name; that row is at index 146):
{
  "name": "LEARNING_PII_SCREEN_ENABLED",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "P2-go-04 (F-f01-23): installs the deterministic PII and secret screen (learning.PIIScreen) on every learned skill candidate. A candidate carrying an e-mail address, a national id, a payment card, an IBAN, a phone number or a secret is dropped, never redacted; learning_review_candidates_screened_total counts the drops by class only. Off, learning.Screen stays a no-op. Only the literal true.",
  "declarations": [
    { "service": "capability-core", "env": "LEARNING_PII_SCREEN_ENABLED", "default": "false" }
  ]
}
(c) apps/Model Plane/deploy/docker-compose.yml: in the capability-core service (the block starts at line 2511), add after the LEARNING_REVIEW_MAX_PER_ORG_PER_DAY line (line 2619) and before the "# EP-11 S7 /learn" comment:
      # F-f01-23 (P2-go-04): the deterministic PII and secret screen on every learned skill candidate
      # (learning.PIIScreen). Off unless exactly true; a candidate that carries personal data or a secret
      # is dropped, never redacted.
      LEARNING_PII_SCREEN_ENABLED: ${LEARNING_PII_SCREEN_ENABLED:-false}
(d) Optional, same file: the comment above LEARNING_REVIEW_COST_CONTROLS (lines 2611 to 2613) says "With no counter store wired the day caps skip every review (budget_unavailable), so keep the switch false." That is stale now that the store is built whenever the switch is on. Suggested replacement: "The day caps are counted in learning_review_counters (migration 0045), which is built only while this switch is on." (e) After (b) and (c) are in, run `go test ./...` in apps/Control Plane/flag-registry. TestShippedRegistryMatchesComposeFiles needs both rows. The literal-reader cross-check now sees installLearningPIIScreen's == "true" read and checks onLiteral. I did not run this after the shared edits because I must not edit those files.

## P2-go-06

All of these are the lead's. I touched none of them.

1. Control principal train (O-27, P2-cp-10), `apps/Control Plane/config/plane-service-principals.json`. Insert a new principal alphabetically (after `quarry*`, before `session-core`). Do not hand-edit pins.
```
"sandbox-manager": {
  "tenancy": "directory",
  "audiences": ["session-core"],
  "consumerEnv": "SANDBOX_MANAGER_SERVICE_API_KEY",
  "orgIds": [],
  "retentionByAudience": { "session-core": "persistent" },
  "scopes": ["session:thread-owner:resolve"],
  "scopesByAudience": { "session-core": ["session:thread-owner:resolve"] }
}
```
Held by exactly one principal on exactly one audience. Never put a credential value in the file; the launcher mints `SANDBOX_MANAGER_SERVICE_API_KEY`, and the `.env.example` entry only names it. Then run the train acceptance commands in PRINCIPAL-TRAIN.md.

2. flag-registry.json: add one row (format as the `SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM` row):
```
{ "name": "SANDBOX_MANAGER_THREAD_OWNER_CHECK", "kind": "switch", "onLiteral": "true", "owner": "model-plane", "source": "compose", "default": "false",
  "purpose": "O-13: sandbox-manager asks session-core ResolveThreadOwner before every thread-scoped lease, read, write and erase (NOT_FOUND when not the owner, UNAVAILABLE when session-core is unreachable) and sweeps workspaces of threads that no longer exist. Only the exact literal true enables it.",
  "declarations": [ { "service": "sandbox-manager", "env": "SANDBOX_MANAGER_THREAD_OWNER_CHECK", "default": "false" } ],
  "codePath": "apps/Model Plane/go/services/sandbox-manager/cmd/thread_owner.go" }
```
Also add the name to the literal-only switch list near line ~4953, the one holding `SANDBOX_MANAGER_EGRESS_TIERS`, if that list applies to new switches.

3. `apps/Model Plane/deploy/docker-compose.yml`, `sandbox-manager` service environment (and the production compose if it mirrors this block):
```
SANDBOX_MANAGER_THREAD_OWNER_CHECK: ${SANDBOX_MANAGER_THREAD_OWNER_CHECK:-false}
SANDBOX_MANAGER_THREAD_ORPHAN_GRACE_HOURS: ${SANDBOX_MANAGER_THREAD_ORPHAN_GRACE_HOURS:-24}
SESSION_CORE_ADDR: ${SESSION_CORE_ADDR:-session-core:9091}
AUTH_CORE_URL: ${AUTH_CORE_URL:-http://auth-core:3011}
SANDBOX_MANAGER_SERVICE_ID: ${SANDBOX_MANAGER_SERVICE_ID:-sandbox-manager}
SANDBOX_MANAGER_SERVICE_API_KEY: ${SANDBOX_MANAGER_SERVICE_API_KEY:-}
```
The key line is empty by default and only has to be set when the switch is on (`threadOwnerFromEnv` stops startup otherwise). The Control launcher mints it once the principal row exists.

4. Proto, additive RPC (the .proto and generated Go/Rust/Python code are the lead's). In `proto/model_plane/v1/runs.proto`, in `RunService` next to `ResolveThreadOwner`:
```
// ThreadsExist: read-only batch existence read for sandbox-manager's orphan sweep. Ids only.
rpc ThreadsExist(ThreadsExistRequest) returns (ThreadsExistResponse);
message ThreadsExistRequest { string org_id = 1; repeated string thread_ids = 2; }  // at most 100 ids
message ThreadExistence { string thread_id = 1; bool exists = 2; }
message ThreadsExistResponse { repeated ThreadExistence threads = 1; }
```
- Regenerate Go, Rust and Python code. Add buf-lint-baseline entries if the doc-comment rule requires them.
- The session-core handler must answer with explicit rows per requested id (exists=false only for an id it knows is absent or deleted). It must call `caller.authorize_org(org_id)`, cap the request at 100 ids, and return no content.

5. session-core Rust (not my track; needed for the switch to work end to end). In `session-core/src/run_service_grpc.rs`, `resolve_thread_owner` and the new `threads_exist` must accept a service caller holding `session:thread-owner:resolve`. Today `resolve_thread_owner` only checks `authorize_operation(&caller, "session:read")`. Keep `session:read` accepted for existing callers (capability-core). Simplest form: for a service caller, pass if it holds either scope. The `:read` suffix keeps it permitted under a ZDR caller. Without this, a token minted with the narrow scope is refused and every thread-scoped call returns UNAVAILABLE once the switch is on.

6. Existence transport adapter, a new file after the proto is regenerated: `apps/Model Plane/go/services/sandbox-manager/cmd/thread_exists_transport.go`
```
package main

import (
	"context"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadowner"
	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
)

func init() {
	newExistenceTransport = func(cc grpc.ClientConnInterface) threadowner.ExistenceTransport {
		return existenceTransport{client: mpv1.NewRunServiceClient(cc)}
	}
}

type existenceTransport struct{ client mpv1.RunServiceClient }

func (t existenceTransport) ThreadsExist(ctx context.Context, bearer, orgID string, ids []string) (map[string]bool, error) {
	ctx = metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+bearer)
	resp, err := t.client.ThreadsExist(ctx, &mpv1.ThreadsExistRequest{OrgId: orgID, ThreadIds: ids})
	if err != nil {
		return nil, err
	}
	out := make(map[string]bool, len(resp.GetThreads()))
	for _, row := range resp.GetThreads() {
		out[row.GetThreadId()] = row.GetExists()
	}
	return out, nil
}
```
7. Migrations: none. `thread_workspaces.created_at` (0005) already exists, and the tombstone table is 0010.
8. Golden: no agent-action-admission golden change.

## P2-go-07-rel1

1. Registry row. Insert into apps/Control Plane/config/plane-service-principals.json between "retrieval-engine" and "session-core" (2-space indent, LF). I did not write this file, because other agents have edits in it.

  "sandbox-manager": {
    "tenancy": "directory",
    "audiences": [
      "session-core"
    ],
    "consumerEnv": "SANDBOX_MANAGER_SERVICE_API_KEY",
    "orgIds": [],
    "retentionByAudience": {
      "session-core": "persistent"
    },
    "scopes": [
      "session:thread-owner:resolve"
    ],
    "scopesByAudience": {
      "session-core": [
        "session:thread-owner:resolve"
      ]
    }
  },

2. Auth Core contract pin. In apps/Control Plane/auth-core/src/auth/plane-service-principals-policy.contract.spec.ts, the test 'leaves every other principal as it was: the registry still names the same principals' (around line 2376) lists principal names. Insert 'sandbox-manager' between 'retrieval-engine' and 'session-core'. Without this, that test fails once row 1 lands. I did not edit this file, because another agent has it modified.

3. Compose. In apps/Model Plane/deploy/docker-compose.yml, sandbox-manager block:
   - SANDBOX_MANAGER_SERVICE_API_KEY: ${SANDBOX_MANAGER_SERVICE_API_KEY:-}
   - SANDBOX_MANAGER_THREAD_OWNER_CHECK: ${SANDBOX_MANAGER_THREAD_OWNER_CHECK:-false}
   - SESSION_CORE_ADDR: session-core:9091 (bare host:port, not http://)
   - networks: default and inter-plane-bus. The existing pin cmd/thread_owner_compose_test.go (TestSandboxManagerJoinsInterPlaneBus) needs this, and it currently fails.
   docker-compose.production.yml overlays sandbox-manager; I believe the base environment keys carry over through the overlay merge. Please confirm.

4. Flag registry. apps/Control Plane/flag-registry/flag-registry.json has no row for SANDBOX_MANAGER_THREAD_OWNER_CHECK. Add one modelled on SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM: kind "switch", onLiteral "true", owner "model-plane", source "compose", default "off", declarations [{service: sandbox-manager, env: SANDBOX_MANAGER_THREAD_OWNER_CHECK, default: off}], codePath apps/Model Plane/go/services/sandbox-manager/cmd/thread_owner.go. The compose-pin tests in apps/Control Plane/flag-registry check this.

5. Launcher. The credential for the new principal is generated from consumerEnv by scripts/plane-principal-registry.jq and run-control-plane.sh, with no code change. Make sure the stack env the sandbox-manager container reads carries SANDBOX_MANAGER_SERVICE_API_KEY.

6. After 1 and 2 land, go test ./cmd/ in sandbox-manager should pass the registry pin. It also needs 3 for TestSandboxManagerJoinsInterPlaneBus. Optional: apps/Control Plane/config/README.md lists principals and may need the row.

No golden, proto or migration changes are needed for this item.

## P2-ec-01

- Flag registry: none. The existing switch SKILL_EXT_POLICY_ENFORCED (model-gateway, needs ORG_AGENT_POLICY_MODE=enforce) is already declared. Execution-core acts only on a non-empty token, so it has no new switch. The deploy/docker-compose.yml needs no new line, but confirm SKILL_EXT_POLICY_ENFORCED is in the model-gateway block.
- Migration: none.
- Admission golden (agent-action-admission.json, its .sha256 and vendored copies): no change needed from this unit.
- Proto: `string ext_skills = 28;` is already in apps/Model Plane/proto/model_plane/v1/execution.proto (comment text included). The lead regenerates with the buf recipe: go/gen/model_plane/v1/execution.pb.go and apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/execution_pb.ts. There is no buf breaking issue, since the field is additive. The max tag before this was 27 (`coordinator`).

## P2-ec-01-rel1

Lead must make these; I did not edit them. (1) apps/Control Plane/flag-registry/flag-registry.json, the SKILL_EXT_POLICY_ENFORCED row (around line 3859): add declaration { "service": "capability-core", "env": "SKILL_EXT_POLICY_ENFORCED", "default": "false" } next to the model-gateway one, because capability-core now reads the switch (cmd/main.go). The registry test passes either way, but the project rule is that every reading service is declared. (2) apps/Model Plane/deploy/docker-compose.yml, the capability-core environment block (starts at line 2520; the SKILL_* switches are around lines 2605-2646, for example after SKILL_SPACE_SCOPE at 2646): add SKILL_EXT_POLICY_ENFORCED: ${SKILL_EXT_POLICY_ENFORCED:-false}. Without it the capability-core read never sees the switch in a deployment. (3) Python generated code apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/execution_pb2.py has no ext_skills at all, so it lacks ExecuteScheduledStepRequest.ext_skills (14) and RunAgentRequest.ext_skills (28). Regenerate it with the repo's python proto recipe if that package is kept in sync. Go gen (go/gen/model_plane/v1/execution.pb.go) already has both fields. Rust gets them at build time from the proto through mp-contracts build.rs. (4) No proto, golden or migration number change is needed by this item beyond what is already in the worktree.

## P2-ec-02

- Flag registry, compose, admission golden and its .sha256: nothing. There is no new switch; MCP_APP_TOOL_CALLS and MCP_DO_TOOLS are already declared.
- Migrations: none.
- Proto: I added `bool side_call = 16;` to ExecuteStepRequest in apps/Model Plane/proto/model_plane/v1/execution.proto (the spec names that file). Rust generates it at build time. Regenerate the committed Go and Python bindings with the buf recipe: apps/Model Plane/go/gen/model_plane/v1/execution.pb.go and apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/execution_pb2.py. The change is additive, so buf breaking should be green.
- Optional: apps/Model Plane/proto/baseline.binpb is the buf-breaking image, so it needs no update for an additive field.

## P2-ec-03

1. `apps/Control Plane/flag-registry/flag-registry.json`: insert directly before the `MCP_ELICITATION_ENABLED` entry (after `MCP_DO_TOOLS`, around line 1945):
{
  "name": "MCP_DO_ELICITATION",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "EP-4 S7: a Do run's MCP call may carry a server's form to the run's owner as a resume question. Only the literal true turns it on. Needs the gateway's MCP_ELICITATION_ENABLED and AGENT_ASK_USER_RESUME_ENABLED. Run a single model-gateway replica while it is on (DEC-F08-06).",
  "declarations": [
    { "service": "execution-core", "env": "MCP_DO_ELICITATION", "default": "false" }
  ],
  "codePath": "apps/Model Plane/rust/services/execution-core/src/mcp_gateway.rs"
}

2. `apps/Model Plane/deploy/docker-compose.yml`, execution-core block (starts around line 1810). Directly after its `AGENT_ASK_USER_RESUME_ENABLED` line (around line 2249; not the model-gateway one at 484 or the session-core one at 1330):
        # EP-4 S7: a Do run's MCP call may carry a server's form to the run's owner as a resume question. Only the literal true; needs model-gateway MCP_ELICITATION_ENABLED and AGENT_ASK_USER_RESUME_ENABLED
        MCP_DO_ELICITATION: ${MCP_DO_ELICITATION:-false}

3. Operational: single model-gateway replica while MCP elicitation is on (DEC-F08-06). Do not turn `MCP_DO_ELICITATION` on until W3-EP-4-S7-b (the resume that answers the held call) is in. The in-place continuation refuses a form's question until then, and the held call only lapses at the gateway's hold TTL.

4. P2-cp-11 (W3-EP-4-S7-a0) is not in the tree. Grant execution-core the scope `orchestration:question:service-origin` on audience session-core, in `apps/Control Plane/config/plane-service-principals.json` (scopes plus `scopesByAudience`, with its policy spec). session-core's `create_run_question` must accept it for a service origin. Until then every form is cancelled with FORM_NOT_ASKED, which fails closed.

5. No migration, no golden or proto change. The golden cases `e_zdr_run_mcp_elicitation_not_advertised` and `e_waived_run_mcp_elicitation_not_advertised` are FUGEN's.

## P2-ec-05-rel1

"None. The change is confined to the #[cfg(test)] scheduled_agent_lane_tests module in grpc.rs: no registry rows, compose lines, migration numbers, proto or golden changes. A production fix for the managed-run gap in leftovers item 1 would touch session-core (terminalization.rs and grpc.rs) or agent.rs, and needs the lead's decision first."

## P2-ec-06

Lead to apply (none of these files were touched):

A. apps/Control Plane/flag-registry/flag-registry.json, add to the "flags" array in name order:
{
  "name": "EXECUTION_SCREEN_INFERENCE_CREDENTIAL",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "model-plane",
  "source": "compose",
  "default": "false",
  "purpose": "F-f04-12 (DEC-F04-3): a Do run's tool-result screening call may present the content-screening principal's classifier credential (execution-core-screening, inference:invoke on inference-core), so the gateway's inline classifier can run for the Do loop. Off, the call carries none and the gateway keeps its zero classifier budget. Needs CONTENT_SAFETY_SCREEN_RPC on and the gateway's CONTENT_SAFETY_SEMANTIC_INLINE. Never presented for a ZDR or waived run. Only the literal true turns it on.",
  "declarations": [
    { "service": "execution-core", "env": "EXECUTION_SCREEN_INFERENCE_CREDENTIAL", "default": "false" }
  ]
}

B. apps/Model Plane/deploy/docker-compose.yml, execution-core environment block, directly after the EXECUTION_CORE_SERVICE_API_KEY line (about line 2110):
      # F-f04-12 (DEC-F04-3): content-screening principal. Off by default; only the literal `true` lets a Do verdict call present its classifier credential. An empty key keeps that credential off.
      EXECUTION_SCREEN_INFERENCE_CREDENTIAL: ${EXECUTION_SCREEN_INFERENCE_CREDENTIAL:-false}
      EXECUTION_CORE_SCREENING_SERVICE_ID: ${EXECUTION_CORE_SCREENING_SERVICE_ID:-execution-core-screening}
      EXECUTION_CORE_SCREENING_SERVICE_API_KEY: ${EXECUTION_CORE_SCREENING_SERVICE_API_KEY:-}

C. Migrations: none for either unit. Goldens and protos: none. The admission golden (apps/Model Plane/api/contracts/agent-action-admission.json and its vendored copies) is unchanged because no pin was added.

D. Already done by me, do not repeat: the apps/Control Plane/config/PRINCIPAL-TRAIN.md batch 9 row. The apps/Control Plane/config/README.md "Content-screening principal (dark)" section. The apps/Model Plane/docs/ZDR.md row.

E. Optional, lead's call: apps/Model Plane/deploy/docker-compose.production.yml explicit pin for the switch (goal-judge precedent). Not required, because the base default is false.

## P2-bff-01

1. flag-registry (apps/Control Plane/flag-registry/flag-registry.json). Insert the object below just before the row named WIDE_RESEARCH_CHILD_BUDGET_ENFORCED (~line 4763) in the top-level "flags" array. Do NOT add the name to the "unregistered" list. There is no org-core switch, so there is no second row. The row is unchanged from the lead's instructions.
{
  "name": "WEB_DOMAIN_POLICY_BFF",
  "kind": "switch",
  "onLiteral": "true",
  "owner": "frontend-plane",
  "source": "code",
  "default": "false",
  "stage": "dark",
  "purpose": "EP-9 O-23 / D-f10-3: the org's web domain rules also bind the BFF's web search (web, similar, images, videos, answer stream), the single-page URL query and every crawl start (UI crawl, crawl action, recrawl, Ingestions crawl run). Only the literal true turns it on; off, no handler calls Model Gateway for a domain check and every search and crawl is what it was. The verdicts bind only when Model Gateway's WEB_DOMAIN_POLICY_MODE is enforce.",
  "declarations": [
    { "service": "gateway", "env": "WEB_DOMAIN_POLICY_BFF", "default": "false" }
  ],
  "codePath": "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs"
}

2. Compose (apps/Frontend Plane/verevonv3/docker-compose.yml, service `gateway`, right after the line `CHAT_RESEARCH_RUNS: ${CHAT_RESEARCH_RUNS:-false}`, ~line 298):
      # EP-9 O-23: the org's web domain rules also bind the BFF web search and every crawl start. Exact literal true, default off. Binds only when Model Gateway's WEB_DOMAIN_POLICY_MODE is enforce.
      WEB_DOMAIN_POLICY_BFF: ${WEB_DOMAIN_POLICY_BFF:-false}

3. Org-core: no compose change. model-gateway already holds org:browser-site:read:any in the compose org-core registry, and the compose contract test confirms it. The org-core rules route has no switch. It must be deployed (org-core image rebuilt) BEFORE the BFF switch goes on, or every crawl start answers 503.

4. Deploy order and notes (no file change):
   - Quarry: set QUARRY_EDGE__CRAWL_SCOPE_MODE=enforce so followed links are dropped. The Ingestion compose default is observe, which only reports. With the scope mode off, a crawl that carries domain_rules is a 400, so Quarry must be on observe or enforce before the BFF switch goes on.
   - Model Gateway: WEB_DOMAIN_POLICY_MODE must be enforce for the rules to be carried.

5. No migrations, no admission golden change, no .proto change. The space-surface golden, its two vendored copies and the three .sha256 pins are already in the tree, and the contract-sync pin at line 235 is done.

## P2-bff-01-rel1

"None required for this item: no migration, golden, proto, or new switch. The lead should (a) make sure P2-bff-01 declares WEB_DOMAIN_POLICY_BFF in the gateway compose block (default false) and adds its flag-registry.json row, since neither exists in this worktree; (b) decide the pre-org onboarding question in leftovers item 1 before enabling the switch with Model Gateway enforce; (c) confirm Quarry QUARRY_EDGE__CRAWL_SCOPE_MODE is observe or enforce wherever the switch is on."

## P2-bff-02

1. Flag registry, apps/Control Plane/flag-registry/flag-registry.json: add this object to the "flags" array, with the separating comma, after the CREDENTIAL_BINDINGS_UI_ENABLED entry: {"name": "AUTH_STEP_UP_ROUTE_ENABLED", "kind": "switch", "onLiteral": "true", "owner": "frontend-plane", "source": "code", "default": "false", "stage": "dark", "purpose": "F-f03-15: the gateway's relay of Auth Core's password re-check behind a high-risk Model Plane approval (POST /api/v1/auth/step-up). Off, the route is not registered and answers 404. Only the literal true turns it on.", "declarations": [{"service": "gateway", "env": "AUTH_STEP_UP_ROUTE_ENABLED", "default": "false"}], "codePath": "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs"}
2. Compose, apps/Frontend Plane/verevonv3/docker-compose.yml, service gateway, environment block: add this line directly after `      CREDENTIAL_BINDINGS_UI_ENABLED: ${CREDENTIAL_BINDINGS_UI_ENABLED:-false}` (line 304): `      AUTH_STEP_UP_ROUTE_ENABLED: ${AUTH_STEP_UP_ROUTE_ENABLED:-false}`. apps/Model Plane/deploy/docker-compose.yml declares no gateway switch today, so the verevonv3 file is the gateway's compose block (the one config_zdr_switch_tests.rs reads).
3. Optional, after items 1 and 2: add "AUTH_STEP_UP_ROUTE_ENABLED" to PHASE_SWITCHES in apps/Frontend Plane/verevonv3/apps/gateway/src/config_zdr_switch_tests.rs (array length 11 to 12). Not applied here, because that file is outside this item and its compose and registry tests fail until items 1 and 2 are in.
4. No migrations, admission golden, vendored copies or proto changes are needed.

## P2-bff-03

(a) apps/Control Plane/flag-registry/flag-registry.json: in "flags", next to SPACE_THREAD_SCOPE_RESOLVE, add { "name": "GATEWAY_CHAT_PROJECTS_ENABLED", "kind": "switch", "onLiteral": "true", "owner": "frontend-plane", "source": "code", "default": "false", "stage": "dark", "purpose": "WS18-S4 (owner D-f06-C): a project start (the first send of a chat in a Space, no thread_id) is refused with 404 chat_projects_disabled before any upstream call while off, on the chat stream, the JSON turn and the AG-UI stream; the composer capabilities report chat_projects only while on. First release is append-only; live following, approvals and plan hydration for project threads are not in it. Only the literal true; flipped together with VITE_CHAT_PROJECTS_ENABLED.", "declarations": [ { "service": "gateway", "env": "GATEWAY_CHAT_PROJECTS_ENABLED", "default": "false" } ], "codePath": "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs" }. (b) apps/Frontend Plane/verevonv3/docker-compose.yml: gateway environment, directly after the line "      CHAT_ARTIFACT_PROMOTE_ENABLED: ${CHAT_ARTIFACT_PROMOTE_ENABLED:-false}" (about line 283), add exactly one line: "      GATEWAY_CHAT_PROJECTS_ENABLED: ${GATEWAY_CHAT_PROJECTS_ENABLED:-false}". (c) apps/Frontend Plane/verevonv3/docker-compose.production.yml: gateway environment next to 'SPACE_THREAD_SCOPE_RESOLVE: "false"' (about line 42), same 12-space indent: 'GATEWAY_CHAT_PROJECTS_ENABLED: "false"'. (d) apps/Frontend Plane/verevonv3/.env.example: add GATEWAY_CHAT_PROJECTS_ENABLED=false. NOT done by me, because .env* files may not be read or edited. (e) No migration, no admission golden, no vendored copy and no proto change. (f) Optional, after (a) and (b) are in: apps/Frontend Plane/verevonv3/apps/gateway/src/config_zdr_switch_tests.rs: PHASE_SWITCHES [&str; 11] becomes [&str; 12] with "GATEWAY_CHAT_PROJECTS_ENABLED" appended; the strict-parser table [(&str, fn(Option<&str>) -> bool); 8] becomes 9 with ("GATEWAY_CHAT_PROJECTS_ENABLED", super::chat_projects_from), added. Not done by me. Then run cargo test --bin verevon-gateway-rs -- zdr_switch_tests. F-f06-09 needs no shared edit.

## P2-bff-05-rel1

"None needed. There is no new switch (so no flag-registry row), no new env name (so no compose line), and no migration, golden, admission or proto change; no doc change is needed either. Staging note for the lead, since config.rs also holds other agents' uncommitted hunks. My hunks in config.rs (line numbers are approximate and may shift) are: (a) the upstream_secret fn, inserted just before required_service_token (around line 1298); (b) the seven call sites in build_state (anbud_api_key, marketing_core_service_key, application_convex_service_key, zammad_api_token, retention_decision_service_api_key, mail_task_ingest_service_token, mail_task_nats_password), around lines 1023-1144; (c) the retention field doc line that now starts 'Trimmed; a `change-me*` placeholder refuses boot'; (d) the test block from the upstream_secret_trims... test through upstream_secrets_refuse_change_me_at_boot_unless_the_switch_is_exactly_one, which includes the helpers UPSTREAM_SECRETS, FAKE_CORE_TOKENS, EnvOverlay, boot_with and stored_upstream_secret, around lines 2479-2686."

## P2-app-01

(a) apps/Control Plane/flag-registry/flag-registry.json: insert after line 3211 (the closing '    },' of NATS_ALLOW_TOKEN_FALLBACK, verified) and before the ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED object. This is the requested row, laid out as the file lays out its objects (braces at 4 spaces, keys at 6):\n    {\n      \"name\": \"NOTIFICATION_ORG_ERASURE_ENABLED\",\n      \"kind\": \"switch\",\n      \"onLiteral\": \"true\",\n      \"owner\": \"application-plane\",\n      \"source\": \"code\",\n      \"default\": \"false\",\n      \"stage\": \"dark\",\n      \"purpose\": \"Purges an organization's Web Push subscriptions and every org-keyed notification-core row on GDPR erasure. Needs the Control Plane durable notification-core-org-erasure (F-f05-11) first. Only the literal true turns it on.\",\n      \"codePath\": \"apps/Application Plane/notification-core/internal/config/config.go\"\n    },\nThe loader validated this row in C:/Users/IMAFER~1/AppData/Local/Temp/claude/ea61e972061a3412/f1f497d1-6d47-4d60-9b30-06f5aa0faac7/scratchpad/p2app01/proposed/flag-registry.json. After inserting, run GOWORK=off go test ./... -count=1 in apps/Control Plane/flag-registry.\n(b) apps/Application Plane/docker-compose.yml: the notification-core service block starts at line 800. Add after line 838 (NOTIFICATION_NATIVE_TYPES_ENABLED), with 12 spaces of indent:\n            NOTIFICATION_ORG_ERASURE_ENABLED: ${NOTIFICATION_ORG_ERASURE_ENABLED:-false}\n(c) No migration (this unit adds none, so no migration number is taken), no golden change (agent-action-admission.json, its .sha256 and vendored copies), no proto change, and no other compose or registry row. F-f05-09 needs no new switch.

## P2-app-01-rel1

No registry rows, migration numbers, proto or golden changes are needed for this behaviour. Optional, pre-existing gap for the lead to decide; I did not edit either file. (a) apps/Application Plane/docker-compose.yml, notification-core service block (NOTIFICATION_DELIVERY_MODE is at line 819 and NOTIFICATION_WEB_PUSH_ENABLED at line 842): add NOTIFICATION_ORG_ERASURE_ENABLED: ${NOTIFICATION_ORG_ERASURE_ENABLED:-false}. (b) apps/Control Plane/flag-registry/flag-registry.json has no notification-core or NOTIFICATION_* rows today; if the switch is declared, add one with default "false" for service notification-core. (c) At commit time, git add the untracked files listed in files, since they belong to the org-erasure work as well as this item.

## P2-app-02

Shared files for the lead. Search by the anchor named; line numbers are from the current tree and may shift.

A. apps/Control Plane/flag-registry/flag-registry.json. Insert the first object after the NATS_ALLOW_TOKEN_FALLBACK object (its closing "}," is line 3211), before the ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED object:
    {
      "name": "NOTIFICATION_ORG_ERASURE_ENABLED",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "application-plane",
      "source": "code",
      "default": "false",
      "stage": "dark",
      "purpose": "F-f05-10: on, a GDPR erasure of an organization, or of one user on subject user or user_anonymize, hard-deletes its notification-core rows, Web Push keys included. Off, nothing is erased. Only the literal true turns it on.",
      "codePath": "apps/Application Plane/notification-core/internal/config/config.go"
    },
Insert the second object after the SPACE_AUDIT_DECISION_EVENTS object (its closing "}," is line 3977), before the SPACE_DELETION_HOLD_RELEASE object:
    {
      "name": "SPACE_DELETION_AUTH_SWEEP",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "application-plane",
      "source": "code",
      "default": "false",
      "stage": "dark",
      "purpose": "Space deletion: on, an authorization claim that died before its acknowledgement is re-driven every 5 minutes, and a claim that throws after it is released for retry. Off, it waits for another run. Only the literal true turns it on.",
      "codePath": "apps/Application Plane/convex-core/convex/spaceDeletionAuthorization.ts"
    },
Both objects were validated: a scratch copy of the registry with both inserted loads with the Control Plane's own loader (389 flags). Source "code" matches the existing SPACE_DELETION_HOLD_RELEASE and SPACE_DELETION_PERMANENT_REFUSAL_PARK rows. The compose ratchet reads only the Control and Model Plane compose files, so these two rows are not ratchet-checked.

B. apps/Application Plane/docker-compose.yml. Insert after line 858 (NOTIFICATION_GDPR_SHARED_NATS_PASSWORD), inside the notification-core environment, 12-space indent:
            # Organization and user erasure (F-f05-10): off by default and read as the literal "true". It rides the
            # shared Control-Plane broker above and binds the notification-core-org-erasure durable (F-f05-11).
            NOTIFICATION_ORG_ERASURE_ENABLED: ${NOTIFICATION_ORG_ERASURE_ENABLED:-false}

C. apps/Application Plane/convex-core/docker-compose.yml. Insert after line 181 (SPACE_DELETION_PERMANENT_REFUSAL_PARK), inside the convex-gateway environment, 6-space indent:
      # Space deletion flag SPACE_DELETION_AUTH_SWEEP: on, spaceDeletionAuthorization:sweepAuthorizationEvents re-drives an
      # authorization claim whose worker died before its acknowledgement, and a claim that throws after it is released for
      # retry. Off by default; only the literal true turns it on. Read at call time.
      SPACE_DELETION_AUTH_SWEEP: ${SPACE_DELETION_AUTH_SWEEP:-false}

D. Migrations: none written. The next free number in apps/Application Plane/notification-core/migrations is 019 (018 is the highest). Only needed if the lead adopts leftover item 4.

E. Golden and proto: none.

F. Already in place, no action: apps/Application Plane/convex-core/startup.sh sets SPACE_DELETION_AUTH_SWEEP (line 181, default false). Also see leftover item 7 for SPACE_DELIVERY_LEASE_SWEEP.

## P2-app-02-rel1

Apply by insertion only, to the two tracked LF files below. Use the exact text, because the test pins the default, stage, onLiteral and codePath. Do not add a row or compose line that is already present.

(1) C:/dev/mp-w345/apps/Control Plane/flag-registry/flag-registry.json, inside the "flags" array.
(a) Insert the SPACE_DELIVERY_LEASE_SWEEP row (this item) immediately before the line "    {" that precedes '      "name": "SPACE_DM_REGISTRATION",' (it goes after the SPACE_DELETION_PERMANENT_REFUSAL_PARK row).
(b) Insert the SPACE_DELETION_AUTH_SWEEP row (P2-app-02's switch) immediately before the line "    {" that precedes '      "name": "SPACE_DELETION_HOLD_RELEASE",' (it goes after the SPACE_AUDIT_DECISION_EVENTS row). Skip (b) if P2-app-02 has already added it.
The loader refuses unsorted or duplicate names, so keep these positions.

Row (a), SPACE_DELIVERY_LEASE_SWEEP:
    {
      "name": "SPACE_DELIVERY_LEASE_SWEEP",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "application-plane",
      "source": "code",
      "default": "false",
      "stage": "dark",
      "purpose": "Space delivery: on, spaces:sweepExpiredDeliveryLeases re-drives an expired claim on a Space deletion, registration or audience delivery. Off, such a claim waits for another run's claim. Only the literal true turns it on.",
      "codePath": "apps/Application Plane/convex-core/convex/spaces.ts"
    },

Row (b), SPACE_DELETION_AUTH_SWEEP:
    {
      "name": "SPACE_DELETION_AUTH_SWEEP",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "application-plane",
      "source": "code",
      "default": "false",
      "stage": "dark",
      "purpose": "Space deletion: on, spaceDeletionAuthorization:sweepAuthorizationEvents re-drives an authorization claim that died before its acknowledgement. Off, such a claim waits for another request. Only the literal true turns it on.",
      "codePath": "apps/Application Plane/convex-core/convex/spaceDeletionAuthorization.ts"
    },

(2) C:/dev/mp-w345/apps/Application Plane/convex-core/docker-compose.yml, in the convex-gateway "environment:" block.
Insert the block below immediately after the line
      SPACE_DELETION_PERMANENT_REFUSAL_PARK: ${SPACE_DELETION_PERMANENT_REFUSAL_PARK:-0}
(line 181), and before the comment line "    # G49 (verevon-gap.md §8.33)". Six-space indent. The two SPACE_DELETION_AUTH_SWEEP lines belong to P2-app-02; drop those three comment lines and the AUTH key line if that item already added them.

      # Space deletion flag SPACE_DELETION_AUTH_SWEEP: on, spaceDeletionAuthorization:sweepAuthorizationEvents re-drives an
      # authorization whose claim died before its acknowledgement, and releases a claim that throws after it is taken for retry.
      # Off by default everywhere; with it off such a claim waits for another request's run, as before.
      SPACE_DELETION_AUTH_SWEEP: ${SPACE_DELETION_AUTH_SWEEP:-0}
      # Space delivery flag SPACE_DELIVERY_LEASE_SWEEP: on, spaces:sweepExpiredDeliveryLeases re-drives an expired claim on a
      # Space deletion owner delivery, Space registration or recipient-audience registration whose worker died before its
      # acknowledgement. Off by default everywhere; with it off such a claim waits for another run's claim, as before.
      SPACE_DELIVERY_LEASE_SWEEP: ${SPACE_DELIVERY_LEASE_SWEEP:-0}

Then run:
  cd "C:/dev/mp-w345/apps/Application Plane/convex-core" && node ./node_modules/tsx/dist/cli.mjs --test test/space-delivery-lease-sweep.test.cjs   (expect tests 32, pass 32)
  cd "C:/dev/mp-w345/apps/Control Plane/flag-registry" && GOWORK=off go test ./...   (expect pass)
Verified on scratch copies: C:\Users\IMAFER~1\AppData\Local\Temp\claude\ea61e972061a3412\f1f497d1-6d47-4d60-9b30-06f5aa0faac7\scratchpad\shared\flag-registry.json and ...\scratchpad\shared\docker-compose.yml (script ...\scratchpad\verify-shared-edits.cjs).

## P2-app-03-rel1

No registry row, compose line, migration number, golden or proto change belongs to this item.

The lead's commit set for this item is the rel1 part of apps/Application Plane/convex-core/convex/spaces.ts plus the new test file apps/Application Plane/convex-core/test/space-delivery-legacy-rows.test.cjs. In spaces.ts, the rel1 lines are the comment at lines 1110-1116 and the four reads at lines 1130-1134, all inside claimableDueRows. That function is new relative to HEAD, so these lines cannot be committed without the P2-app-03 helper block around them.

The same file also carries other uncommitted hunks (the lease sweep, the deletion and parked-list changes, and others). Do not take those for this item.

## P2-app-04

SHARED EDITS FOR THE LEAD. Apply these as exact edits; do not copy whole shared files, since other units edit them too.

A) apps/Control Plane/flag-registry/flag-registry.json (3 edits; no SQL migration, no golden or proto change).
 A1. SPACE_DELETION_HOLD_RELEASE: replace the "purpose" value with:
 "Lets a space deletion re-drive after a legal-hold release, for the Model, Data and Ingestion owners. Read at call time. Only the literal true turns it on; off, a legal-hold block is terminal."
 A2. SPACE_DELETION_PERMANENT_REFUSAL_PARK: replace the "purpose" value with (239 runes):
 "Permanent refusals park as failed_permanent. Re-drive covers Model, Data and Ingestion. failed_permanent rows are listed by listParkedDeletionOwnerDeliveries, re-armed by rearmParkedDeletionOwnerDelivery. Only the literal true turns it on."
 A3. Insert this object immediately BEFORE the SPACE_REACTIONS_API object, so names stay sorted ("SPACE_REACTIONS" sorts before "SPACE_REACTIONS_API"):
    {
      "name": "SPACE_REACTIONS",
      "kind": "switch",
      "onLiteral": "true",
      "owner": "application-plane",
      "source": "code",
      "default": "false",
      "purpose": "WS12-S8: the Convex half of the gateway's SPACE_REACTIONS_API pair, message reactions (ids, one emoji, a time). Off, writes are refused and reads return nothing. Never an authorization input. Only the literal true turns it on.",
      "codePath": "apps/Application Plane/convex-core/convex/spaceReactions.ts"
    },

B) apps/Application Plane/convex-core/docker-compose.yml, service convex-gateway (service key at line 109).
 B1. Line 164: "      SPACE_REACTIONS: ${SPACE_REACTIONS:-0}" becomes "      SPACE_REACTIONS: ${SPACE_REACTIONS:-false}".
 B2. Replace lines 174 through 181 (8 lines, the SPACE_DELETION_HOLD_RELEASE and SPACE_DELETION_PERMANENT_REFUSAL_PARK comment blocks and keys) with:
      # Space deletion re-drive flag SPACE_DELETION_HOLD_RELEASE: the re-drive covers Model, Data and Ingestion owners,
      # re-asking each one after its legal hold is lifted, and the trusted receipt path may leave a legal-hold block.
      # Off by default everywhere; only the literal true turns it on. With it off a hold stays terminal, as before.
      SPACE_DELETION_HOLD_RELEASE: ${SPACE_DELETION_HOLD_RELEASE:-false}
      # Space deletion flag SPACE_DELETION_PERMANENT_REFUSAL_PARK: on, a permanent owner refusal (400, 401, 403, 412)
      # parks that owner's delivery as failed_permanent. The re-drive covers Model, Data and Ingestion owners;
      # failed_permanent rows are listed by listParkedDeletionOwnerDeliveries and re-armed by
      # rearmParkedDeletionOwnerDelivery. Off by default everywhere; only the literal true turns it on. With it off
      # such a refusal releases for a retry, as before.
      SPACE_DELETION_PERMANENT_REFUSAL_PARK: ${SPACE_DELETION_PERMANENT_REFUSAL_PARK:-false}

C) apps/Application Plane/docker-compose.yml (root compose, service convex-gateway, key at line 117), line 171: "            SPACE_REACTIONS: ${SPACE_REACTIONS:-0}" (12-space indent) becomes "            SPACE_REACTIONS: ${SPACE_REACTIONS:-false}". No SPACE_DELETION_* keys exist in this file.

D) Migrations: none. The only schema change is the Convex index by_space_and_state on spaceDeletionRequests, already in convex/schema/spaceDeletion.ts. No SQL migration number was taken.

E) Golden, admission golden, and proto: none.

Already in the tree, uncommitted, no shared action: convex-core/startup.sh (warning block after the SPACE_REACTIONS set line, :-false defaults, rewritten comments).

After the lead applies A to C, the expected flag-registry result is all green once the org-compliance rows from their own item exist. Run: cd "apps/Control Plane/flag-registry" then GOWORK=off go test -count=1 ./...
