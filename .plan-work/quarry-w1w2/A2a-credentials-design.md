# A2a-credentials design note (Opus review, 2026-10-21)

Scope: WS1.A2 part 1 (breakdown item a, lane A steps 1 and 2, plus the service_tokens half of 3). Every claim below is from a file I read at HEAD 473d0a78. G1/G6 validator semantics, boot gates, `prod_*` tests and the overlay are A2b's.

## 0. One ordering correction (load-bearing)

The task orders "(2) registry + manifest" before "(3) service_tokens". That order breaks the build: `TokenRequestManifest::parse` runs `row.check()` on every row and refuses an audience `from_registry_audience` does not know (service_tokens.rs:226-233, 290-295). A `browser-broker` row before the `BrowserBroker` variant makes `token_request_manifest()` fail, so every `from_manifest` (ingest, vector-retrieve, ...) returns `Internal` at runtime. Conversely the auth-core spec "authorized by the real policy exactly where a row says granted" (policy spec ~2040) fails if the manifest says `granted:true` before the registry grants it. So the commits are:

1. **C1** trait + call sites (alone, unblocks lane B).
2. **C2** Control Plane registry + auth-core spec only (vendored manifest untouched: its rows are unchanged, so the manifest block stays green).
3. **C3** Quarry `BrowserBroker` audience + manifest row + all digests + vendored copy + Rust and shell pins, in ONE commit.

## 1. Contracts that change

**C1, `grant_validator.rs`** (trait at 177-180):
```rust
async fn validate_for_org(&self, org_id: &str, grant_id: &str) -> QuarryResult<GrantValidation> {
    let _ = org_id; self.validate(grant_id).await
}
```
Default body only, so `NoopGrantValidator`, `HttpGrantValidator`, `GrpcGrantValidator` and the test fakes (`tests/start_run_grants.rs:39`, `tests/grant_budget_claim.rs:50`) compile unchanged. Call sites, org always from the verified run, never from a body:
- `admission.rs:33` `validate_run_grant(state, org_id, grant_id, run_domains)`; callers `admission.rs:371` and `tabs.rs:145` pass `&entry.org_id`.
- `sensitive_approval.rs:152` `validate_sensitive_approval(state, org_id, approval)`; caller `admission.rs:396` passes `&entry.org_id`.
- `lifecycle.rs:218` `validate_for_org(&org_id, grant_id)` where `org_id = claims.org_id` (lifecycle.rs:100). On resume this equals the checkpoint org, already checked by `caller_owns_browser_run` (lifecycle.rs:156).
No enforcement logic changes. Leave the `lifecycle.rs:3-13` doc comment ("keyed to Quarry's static bearer") for A2b, which makes it false.

**C2, `apps/Control Plane/config/plane-service-principals.json`** `quarry-edge` (line 380): `audiences += "browser-broker"`, `scopesByAudience["browser-broker"] = ["browser:read"]`, `retentionByAudience["browser-broker"] = "zdr"`, top-level `scopes = ["browser:read","data:read","data:rerank","documents:write","org:data:read_all"]`. The top-level edit is mandatory: the parser requires `scopes` to equal the union of `scopesByAudience` (plane-service-principal.ts:294-304). tenancy, consumerEnv and tokenExchange stay as they are. Spec edits: the P1 test at ~1508 `expect(row?.scopes).toEqual(ceiling)` must become `['browser:read', ...ceiling]`; the `scopesByAudience['data-plane']` pin stays. New `describeIfPolicy('principal bundle P2: browser:read for quarry-edge')`: held by quarry-edge alone on browser-broker alone (`holdersOfScope`, `lanesHoldingScope`), retention `zdr`, mints for a directory org; no principal holds `browser:write` on any audience (loop over `everyPlaneAudience`); quarry-edge refused `browser:read` on every other audience (`expectScopeConfinedTo`).

**C3, Quarry.** `ServiceTokenAudience::BrowserBroker`: `endpoint` `/api/browser-broker/internal-token` (served by `@Controller('api')` + `@Post(':audience/internal-token')`, plane-token.controller.ts:144/281; `browser-broker` is a known audience with TTL 300 s, convex-token.service.ts:446), `registry_audience` `browser-broker`, and the variant in the `from_registry_audience` array (63-67). Manifest row, appended last:
`{"name":"browser-grant-validate","audience":"browser-broker","mint_path":"/api/browser-broker/internal-token","scopes":["browser:read"],"reason":"validate verified browser grant","granted":true,"used_by":["crates/quarry-runtime/src/grpc/browser_broker.rs"],"feature_flags":["QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL"],"notes":"..."}`.
`used_by` must name a file that calls `from_manifest("browser-grant-validate")`, or the row must carry `pending` (token_request_contract.rs:217-248, 530-585). To avoid a second digest churn, C3 adds one call site and nothing else to `grpc/browser_broker.rs`: `pub fn browser_grant_token_request() -> QuarryResult<ServiceTokenRequest> { ServiceTokenRequest::from_manifest("browser-grant-validate") }` (pub, so no dead-code warning). A2b uses it from `validate_for_org`. Tests to update: `EXPECTED_REQUESTS` `[..; 7]` (service_tokens.rs:718), the `"browser-broker" =>` arm before `other => panic!` (~805), `expected_path` arm (token_request_contract.rs:153-159). Pins: `contracts/cross-plane-token-requests.json.sha256`, the auth-core copy and its `.sha256` (must be byte-identical: spec compares the strings), the digest at `scripts/tests/zdr-contract-sync.test.sh:116`. Digests are over LF bytes (auth-core `sha256OfLf`): compute `tr -d '\r' < file | sha256sum`; write the new JSON LF.

## 2. Security and concurrency

- Least privilege: `browser:read` is what the broker requires for `ValidateGrant` (authz.go:62-77); `AcquireGrant`, `RevokeGrant`, `DeriveRunGrant` need `browser:write`, which no principal gets (X-1; L1 test `no principal in the registry holds browser:write`). `GrpcGrantValidator::revoke` (browser_broker.rs:95) will therefore be refused for a service token: correct, do not widen.
- Org binding becomes real: the broker resolves `GetScoped(grant, principal.OrganizationID, OwnerFilter)` (server.go:191); a token minted for the run's org makes another org's grant `NotFound`, mapped to `active:false` (browser_broker.rs:138-153) and so a 403. A service principal's `OwnerFilter` is empty, so owner binding stays Quarry's (`bind_validated_grant`); the known-gap test stays green.
- Fail closed: `token_for_org` refuses an empty org before network I/O (service_tokens.rs:487-493); a 401/403 from auth-core is `Forbidden`, a directory miss is 403 and org-core down is 503 (plane-token.controller.ts, `assertServiceOrganizationBinding`); every error propagates through `?`, no call site turns an `Err` into allow.
- Caching and rotation already exist: per (audience, org, scopes) key, singleflight mint lock, refresh 30 s before expiry, `force_refresh` bypass, redirects off, `ServiceBearer` Debug redacted (service_tokens.rs:19-37, 394-399, 481-554). Nothing new to build; tokens for different orgs never share a key.
- ZDR: `retentionByAudience: zdr` means the minted token says `zdr:true` (plane-service-principal.ts:477, 510). The broker reads `principal.ZeroDataRetention` only in `AcquireGrant`, so validation is unaffected; the token carries no content. Auth-core refuses a request that names a posture, and the manifest never does.
- No token in logs: no new tracing macro in C1 to C3; the log-field lint stays at baseline.

## 3. Migration and idempotence

No SQL. Deploy order (plan): auth-core with the registry before a quarry-edge that mints for browser-broker. C1 to C3 change no runtime behaviour until A2b wires `with_service_tokens`: the default body calls `validate`, and the new row has no caller that mints. Re-running a pin refresh is idempotent (same bytes, same digest). The registry is additive; removing it again affects no other audience.

## 4. Hot files, order of edits

C1: `grant_validator.rs`, `agent_routes/{admission,lifecycle,sensitive_approval,tabs}.rs` (exact-anchor edits; lane B waits for this commit). C2: registry JSON, auth-core spec. C3: `service_tokens.rs`, `token_request_contract.rs`, `grpc/browser_broker.rs` (one fn), manifest + `.sha256`, auth-core fixture + `.sha256`, `zdr-contract-sync.test.sh` (one digest line; lane C appends rows there later), optionally the L1 registry/manifest asserts in `scripts/tests/browser-grant-contract.test.mjs`. Never main.rs.

## 5. Tests that prove each rule

- C1: `default_validate_for_org_delegates_to_validate` (grant_validator.rs); in `tests/start_run_grants.rs` a recording `FakeBroker` overriding `validate_for_org`: `start_run_validates_the_grant_for_the_callers_org`, `step_revalidates_the_grant_for_the_runs_org`, `new_tab_revalidates_for_the_runs_org`, `sensitive_approval_is_validated_for_the_runs_org`; existing `start_run_grants` (5) and `known_gap_...` green. Run `cargo test -p quarry-edge --test start_run_grants` and `--lib agent_routes` via qtest.
- C2: `npx jest plane-service-principals-policy.contract` (diff the failing set against HEAD); `bash scripts/tests/principal-registry-parity.test.sh` (policy changes need no script edit); `node --test scripts/tests/browser-grant-contract.test.mjs`.
- C3: `cargo test -p quarry-runtime service_tokens token_request_contract` (`the_shipped_manifest_is_accepted_unchanged`, `from_manifest_agrees_with_the_file_read_as_plain_json`, `the_pin_names_exactly_these_bytes`, `every_call_site_names_a_row...`), plus new `mints_an_org_scoped_browser_broker_token_on_its_own_path` (wiremock on `/api/browser-broker/internal-token`, body `{orgId, scopes:["browser:read"], reason}`, no `zdr`); auth-core manifest block again; `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`.

## 6. Not doing

G1 (`GrpcGrantValidator` token source, per-call `token_for_org`, retry once on `Unauthenticated` with `force_refresh`, and refusing plain `validate` when service tokens are configured), G6 (`FailedPrecondition` to `active:false`), main.rs wiring and boot gates, `browser_grants_prod.rs`, compose, overlay, workflow, L2: all A2b. No change to the broker, `revoke`, `HttpGrantValidator`, or the live auth-core.
