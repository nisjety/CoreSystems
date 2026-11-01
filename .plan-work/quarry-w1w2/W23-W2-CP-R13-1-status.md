# W2-CP-R13-1 status: principal registry PR 1 (lane d, branch w23/lane-d)

## Built
All of the scope; none of it existed beforehand. The lane started from `w23/integ` (ff merge clean).

- `apps/Control Plane/config/plane-service-principals.json`
  - `quarry-edge`: audiences `inference-core`, `ingestion`, `model-gateway` appended; retention `tenant` for the two model lanes, `zdr` for `ingestion`; scopes `models:invoke` on `model-gateway`, `inference:invoke` + `models:invoke` on `inference-core`, `integration:otp:read` on `ingestion` (WS16.S2, ADR-2a / E3.1-g). `consumerEnv` unchanged, so no new credential.
  - `execution-core`: `parse:write` on `quarry` (INNH-11); `webPolicyByAudience {"quarry":"tenant"}`.
  - `model-gateway`: `webPolicyByAudience {"quarry":"tenant"}`.
  - `verevon-gateway`: `apiKeyExchange ["browser-broker","quarry"]` (no new audience, no service-token lane).
- `auth-core/src/auth/plane-service-principal.ts`: optional fields `webPolicyByAudience` and `apiKeyExchange`, validated (a bad value invalidates the registry, like `retentionByAudience`); accessors `webPolicyPostureFor` (WS7.I reads it) and `authorizePlaneApiKeyExchange` (CP-1 reads it). `AuthorizedPlaneServicePrincipal` is deliberately unchanged (an exact `toEqual` in plane-service-principal.spec.ts).
- `apps/Control Plane/scripts/plane-principal-registry.jq` and `scripts/tests/principal-registry-parity.test.sh` (not in the unit's file list, but necessary): the assembler is a closed key whitelist, so without this the launcher and the bootstrap REFUSE the registry ("unknown key(s)"). Whitelist, validation and copy-through added; the parity policy view compares the two new fields.
- Quarry manifest (`crates/quarry-runtime/contracts/cross-plane-token-requests.json`): rows `model-extract` (model-gateway, `models:invoke`) and `model-extract-inference` (inference-core, `inference:invoke` + `models:invoke`), `granted:true`, `pending` WS16.S3 (empty `used_by`, which the contract test requires for a pending row). `model-invoke`, `model-stream`, `model-audio` stay `granted:false`; their notes and the manifest description now say the closed gate is Quarry's. `.sha256` refreshed (`2d139b8f...`), auth-core copy re-vendored byte for byte, digest pin in `zdr-contract-sync.test.sh` updated.
- Rust: `ServiceTokenAudience::InferenceCore` (`/api/inference-core/internal-token`) and `Ingestion` (`/api/ingestion/internal-token`, no manifest row yet, E3.1-h adds it), `from_registry_audience`, `EXPECTED_REQUESTS` 7 -> 9.
- Tests added or updated: `token_request_contract.rs` (`model_extract_rows_are_the_only_granted_model_rows`, `the_two_extract_rows_are_exactly_the_registry_grant`, `the_extract_rows_are_pending_until_the_model_client_requests_them`, wire test `a_model_extract_asks_auth_core_for_exactly_the_two_extract_rows`, scope-rule test generalised to the inference-core audience), `service_tokens.rs` (`every_audience_round_trips_and_mints_on_the_path_of_its_own_name`), the auth-core policy contract spec (new blocks "principal bundle R-13 PR 1: quarry-edge model lanes / the ingestion audience / parse:write" and "registry schema fields of R-13 PR 1"; P1, P2, web-tool, tenant-lane and manifest pins updated), `docs-contract.test.mjs` (model-plane row, registry and manifest agreement, ingestion lane).
- Docs: `CROSS_PLANE_AUTH.md` model-plane row, `CROSS_PLANE_INTEGRATION.md` (two table rows, the grant paragraph), `SELF_HOST.md` (the model-gateway sentence), `config/README.md` (new section, tenant-lane sentence).

## Commits (lane d)
- e0a65f01a feat(control): principal registry R-13 PR 1 (registry, TS, jq, parity test, spec, config README)
- c0ff63d74 feat(quarry-runtime): model-extract manifest rows, InferenceCore and Ingestion audiences (WS16.S2) (manifest, pins, vendored copy, Rust, docs, docs-contract)
- docs(plan-work) commit with this file and the depth rows

## Tests run (all passed unless stated)
- auth-core: `npx jest plane-service-principals-policy.contract plane-service-principal.spec plane-principal-registry-assembly quarry-token-zdr-claims plane-token-scopes inference-service-token data-plane-service-principal`: 7 suites, 224 passed, 0 failed (policy contract spec 88, plane-service-principal.spec 70 plus 1 not run, assembly 15). No failure showed in these suites on this checkout, so there was nothing to diff against the 15 known failures; the suites deployment-contract and model-plane-token.controller were not part of this run.
- `bash scripts/tests/principal-registry-parity.test.sh`: 37 passed, 0 failed.
- `bash scripts/tests/zdr-contract-sync.test.sh`: 122 passed, 5 failed, all five on files this unit did not touch (zdr-feature-matrix.json digest = INNH-11's matrix not regenerated; telemetry-field-contract and usage-envelopes `.sha256`, and the zdr-refusals pair = CRLF worktree pins). Every cross-plane-token-requests check (golden, pin, auth-core copy and its pin) passes.
- `node --test scripts/tests/docs-contract.test.mjs`: 33 passed. All `scripts/tests/*.test.mjs`: 70 passed, 2 failed (SDK internal-API check, route-coverage allowlist: untracked SDK files and the INNH-11 route, not this unit).
- Rust through the wrapper, lane d: `cargo test -p quarry-runtime --lib -- token_request_contract service_tokens`: 31 passed, 0 failed.

## Flag and default
None. Inert: nothing mints for the new lanes and nothing reads `webPolicyByAudience` or `apiKeyExchange` until WS16.S3, E3.1-h, WS7.I and CP-1. Rollback: revert the registry file, re-run the plane runner, flip the two manifest rows back.

## Assumed decisions
- `parse:write` goes on `execution-core`'s `quarry` lane (the queue names the scope, not a principal; it is the existing Quarry write-scope holder).
- Ingestion lane retention is `zdr` (E3.1-f memo recommendation).
- `apiKeyExchange` is not tied to `audiences` and is restricted to `quarry` / `browser-broker`; `webPolicyByAudience` is restricted to those two audiences with value `tenant`, and the audience must be one of the row's audiences.
- `model-invoke`, `model-stream`, `model-audio` keep `granted:false` although the registry now authorizes the same request (depth row 1); the vendored-manifest spec carries an explicit `quarrySideGates` list.
- Extra files outside the unit's list (jq filter, parity test, CROSS_PLANE_INTEGRATION.md, SELF_HOST.md) were required: the launcher would refuse the registry otherwise, and docs-contract needs every manifest row in the integration table.

## Model Plane asks
None new, nothing appended to MP-ASKS.md. For WS16.S3 and the live model lane: the gateway's `/v1/ai/chat` must accept the quarry-edge tenant token pair (model-gateway + inference-core); `/v1/invoke` still refuses service principals (unchanged).

## Not verified
- No live Auth Core: the real-controller mints in the spec (inference-core, ingestion) use a faked org-core. The model-gateway tenant mint through `ModelPlaneTokenController` is covered only by the all-lanes `mintEveryLane` case, not a dedicated one.
- The quarry-edge crate was not rebuilt. A grep finds the enum only in the loader, the contract test and `tests/dp_rerank_contract.rs` (not an exhaustive match). Only quarry-runtime lib tests were run.
- The deployed registry (auth-service recreated with the new PLANE_SERVICE_PRINCIPALS_JSON) is not done. Tenant lanes need the Auth Core image that knows `tenant` (already deployed for the Data Plane lanes).
- `.github/workflows/contract-goldens.yml` path lists were not reviewed for the manifest copy.

## Depth rows
Eight rows appended to `.plan-work/quarry-w1w2/W23-depth-lane-d.md` (tag W2-CP-R13-1).
