# A2a-credentials progress (final, for A2b)

All steps committed locally on quarryv2-next. Nothing uncommitted of mine.

## Commits
- 24ff66d7 C1  GrantValidator::validate_for_org (DEFAULT body = validate) + agent-route call sites + tests
- 82225bf2 C2  Control Plane registry row quarry-edge: audience browser-broker, scope browser:read ONLY, retention zdr; auth-core P2 spec; config README
- 79330ea1 C3  ServiceTokenAudience::BrowserBroker, manifest row browser-grant-validate, `browser_grant_token_request()`, all pins and the vendored copy, wire + mint tests
- b9182fea     docs: CROSS_PLANE_INTEGRATION row
(The zdr-contract-sync.test.sh digest line for the manifest was carried into 8b3c108b by another unit's whole-file commit; C3 made HEAD consistent again.)

## What A2b builds on (exact hunks)
- crates/quarry-runtime/src/grant_validator.rs: trait `GrantValidator` now has
  `async fn validate_for_org(&self, org_id: &str, grant_id: &str) -> QuarryResult<GrantValidation>` with a default body `let _ = org_id; self.validate(grant_id).await`.
  GrpcGrantValidator must OVERRIDE it (token_for_org(org_id, &browser_grant_token_request()?, false), Bearer on the gRPC call, one retry on Unauthenticated with force_refresh=true + provider.invalidate) and, when service tokens are configured, make the org-blind `validate` refuse (G1) instead of falling back to the static bearer.
- crates/quarry-runtime/src/grpc/browser_broker.rs:26 `pub fn browser_grant_token_request() -> QuarryResult<ServiceTokenRequest>` (the row's one call site; A2b uses it, adds no second from_manifest call). Needs `with_service_tokens(SharedServiceTokenProvider)` or similar on GrpcGrantValidator (crates/quarry-runtime/src/service_tokens.rs: `ServiceTokenProvider::token_for_org(org, &request, force_refresh)`, `invalidate`, `ServiceBearer::expose()`; Debug redacted).
- agent-route call sites, all already on validate_for_org with the org from the verified run: admission.rs:39 (validate_run_grant gets org_id; callers admission.rs ~371 `&entry.org_id`, tabs.rs ~145 `&entry.org_id`), sensitive_approval.rs:159 (validate_sensitive_approval org_id; caller admission.rs ~396), lifecycle.rs:218 (`&org_id` = claims.org_id). The lifecycle.rs:3-13 doc comment ("keyed to Quarry's static bearer") is left for A2b to correct.
- Tests to extend: tests/start_run_grants.rs has RecordingBroker + recording_edge(grants, require) (records ("<plain>" | org, grant)); `known_gap_a_service_principal_is_not_bound_to_the_grants_owner` still pins the owner gap and stays green.
- Registry: quarry-edge|browser-broker holds browser:read only, zdr. No principal holds browser:write; GrpcGrantValidator::revoke (needs browser:write) will be refused for a service token: correct, do not widen.
- Mint path: POST {AUTH_CORE_URL}/api/browser-broker/internal-token, headers x-service-id: quarry-edge, x-service-api-key: QUARRY_SERVICE_API_KEY; body {orgId, scopes:["browser:read"], reason:"validate verified browser grant"}; token TTL 300 s (convex-token.service.ts:446). Manifest feature flag: QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL.
- Feature note: crate quarry-runtime's `grpc` feature is off by default; `cargo test -p quarry-runtime --lib token_request_contract` passes with and without it (the wire test picks the call site via cfg).

## Digests
- cross-plane-token-requests.json (Quarry contracts/ and the auth-core fixtures/ copy, byte-identical, LF): sha256 30ec31e3a92a015df80b921bf0b27b70d463b148154863ae26903bd36df813ee (was 0849784e418ef475b18f39a58850a0e06b2a0973032d2c2a60fa80908ef30359). .sha256 files are CRLF in the working tree like the others.
- Digest line in scripts/tests/zdr-contract-sync.test.sh:116 updated.

## Test commands and results
- qtest `cargo test -p quarry-runtime --lib grant_validator` 12 passed; `--lib service_tokens` 12; `--lib token_request_contract` 14 (default) and 14 (--features grpc); `--features grpc --lib grpc` 13.
- qtest `cargo test -p quarry-edge --test start_run_grants` 7; `--test grant_budget_claim` 8; `--lib agent_routes` 86; `cargo build --workspace --all-targets` compiles (warnings in other units' files).
- auth-core (pnpm install done in this worktree; node_modules is untracked/ignored): `npx jest src/auth/plane-service-principals-policy.contract.spec.ts` 60 passed (P2 = 6); five other registry specs 65 passed.
- `bash scripts/tests/principal-registry-parity.test.sh` 37 passed (~15 min on Windows); `node --test scripts/tests/browser-grant-contract.test.mjs` 19 passed; `bash scripts/tests/zdr-contract-sync.test.sh` 52 passed.
- Not run: full auth-core suite diff vs HEAD (not needed: only the policy JSON and one spec changed; the 5 specs reading the registry were run); `zdr-contract-sync.test.sh --self-test`.

## Round 4 review fix (2181da59)
- tests.rs: new `admission_validates_the_run_and_its_approval_for_the_runs_org` drives admit_action (gate 4 + gate 10) with entry.org_id "org-of-the-run", grant_id Some("grant_run_1"), observed dlg_1; asserts recorded validate_for_org calls = [(org, grant_run_1), (org, grant_approval_1)] and the approval consumed. Shared `RecordingGrantValidator` helper (A2b can reuse). Old helper test renamed `sensitive_approval_helper_forwards_the_org_it_is_handed`.
- qtest `cargo test -p quarry-edge --lib agent_routes`: 87 passed. Mutation (gate 10 passing "") makes the new test fail; reverted.

## Round 4 review fix 2 (79fa08c5)
- start_run_grants.rs: the org-binding test now sends org_id/orgId/org/organization_id/organizationId = org-stranger in the start, step and new-tab bodies (helpers conflicting_org_fields, merge_fields) and still asserts every recorded pair is (org-standard-golden, grant-org). qtest `--test start_run_grants`: 7 passed.
