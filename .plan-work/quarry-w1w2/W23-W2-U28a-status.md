# W2-U28a status: control-internal secret split, finalize org check, Go internal routes in openapi-internal (D2-050)

Plane: Ingestion (quarry-control, quarry-orchestrator, quarry-edge, compose). Lane c, branch w23/lane-c (fast-forwarded to w23/integ first; the ff succeeded).

## Built
Nothing of this existed beforehand (one shared HMAC group, `finalize` took only the job id, the 8 Go routes were undocumented). All of it built.

1. Secret split (control). `httpx.SplitHMAC` (new `internal/httpx/hmac_split.go`) verifies every path under `/v1/internal/jobs/` and `/v1/internal/schedules/` with `QUARRY_CONTROL_INTERNAL_SECRET`, and every other route (including `/v1/jobs/*`, `/v1/runs/*`, the edge change webhook) with `QUARRY_INTERNAL_SECRET`, with separate nonce caches. The group is a prefix, so an internal route added later under those prefixes is strict from its first day. `cmd/control/main.go` builds the second verifier and swaps `r.Use(hmacVerifier.Middleware)` for `r.Use(httpx.SplitHMAC(...))`. Both verifiers share the enforce/rollout mode (`QUARRY_INTERNAL_HMAC_REQUIRED` / prod).
2. `POST /v1/internal/zdr-tombstones` stays on the EDGE secret. Its caller is the edge (`crates/quarry-edge/src/zdr_tombstone.rs`), which must not hold the orchestrator secret. So the split is 7 routes on the new secret and 1 on the old one; the spec says which.
3. Fallback with one warning. Unset `QUARRY_CONTROL_INTERNAL_SECRET` falls back to `QUARRY_INTERNAL_SECRET` on control (one startup warning, "unset") and on the orchestrator (`controlauth`, one warning per process via `sync.Once`). A value equal to the old secret warns once too ("same"). Compose sets it to `${QUARRY_CONTROL_INTERNAL_SECRET:-${QUARRY_INTERNAL_SECRET}}` on both control and the orchestrator, so today's deploy runs unchanged and logs the "same" warning. Remove the fallback after the soak.
4. Orchestrator signing (`controlauth`). `Sign` now picks the secret by path: `IsControlInternalPath` (every `/v1/internal/jobs/...`, `GET /v1/internal/schedules/desired`, `POST /v1/internal/schedules/{id}/runs`) signs with `QUARRY_CONTROL_INTERNAL_SECRET`; everything else (control `/v1/jobs`, `/v1/runs`, the edge `/v1/internal/schedules/preflight`, queues, run_page) keeps the shared secret passed by the caller. No call site changed (the callers still pass the shared secret).
5. Finalize org check. `POST /v1/internal/jobs/{id}/finalize` takes `org_id` (body, or the query parameter; the body wins). A job of another org answers 404 (same as a missing one) and is not changed. A finalize that names no org is still accepted by default (an orchestrator not yet redeployed keeps working); `QUARRY_CONTROL__FINALIZE_ORG_REQUIRED=1` makes it 400. The orchestrator's two finalize bodies (forced cancel after grace, sweep) now send `org_id` from the job it read.
6. Edge `job_lifecycle_routes.rs`: it needs NO new secret. Its control calls are `/v1/jobs/{id}/summary|errors|control` and `/v1/jobs/ongoing`, all outside the orchestrator-only group, so it keeps the edge secret. Giving the edge the control-internal secret would let a leaked edge secret call finalize again, which is the thing D2-050 closes. Added a unit-test module: a source scan proves no lifecycle call targets the orchestrator-only prefixes, the four expected paths are present, and a control 401/403 reaches the caller as a 403.
7. OPENAPI-INTERNAL, Go half: 8 paths added to `docs/openapi-internal.yaml` (operations `controlInternal*`), two security schemes (`controlInternalHmac`, `edgeInternalHmac`), header parameters, and schemas for the job, finalize request/answer, schedule wire, register-run and the tombstone summary. Guard: `scripts/openapi-routes.mjs` gained `controlInternalRoutes(root)` (scans the Go `r.Get/Post(...)` literals); `openapi-internal-guard.test.mjs` now compares documented paths to edge + control routes, and a new test checks the 8 methods and that each route names the secret the Go prefix list in `hmac_split.go` implies; `openapi-route-coverage.test.mjs` counts the control routes as registered.

## Commits (lane c, w23/lane-c)
- 00f3115c3 feat(quarry-control): own secret for the orchestrator-only internal routes; finalize refuses another org
- f4a7c105d feat(quarry-orchestrator): sign control's internal job routes with the control-internal secret; finalize names the org
- 031be75e0 docs(quarry): the eight Go control internal routes in openapi-internal.yaml, with the parity guard
- 962f72ec5 feat(ingestion): QUARRY_CONTROL_INTERNAL_SECRET for control and the orchestrator (compose)
- 60dc2d506 test(quarry-edge): lifecycle calls never target control's orchestrator-only routes
- (this file and the depth rows: the docs commit after these)

## Tests and counts
Go on the host, GOWORK=off:
- quarry-control `go test ./... -count=1`: all packages ok; `go vet ./...` clean. New in `internal/httpx`: 5 tests (`TestIsOrchestratorOnlyPath`, `TestSplitHMAC_RoutesEachGroupToItsOwnSecret` with 7 cases, `TestSplitHMAC_UnsignedRefusedWhenRequired`, `TestSplitHMAC_SameVerifierIsTheOldSharedSecretBehaviour`, `TestControlInternalSecretPosture`). New in `internal/resources`: 6 tests (`TestSecretSplit_EdgeSecretRefusedOnOrchestratorRoutes` over the 7 routes, `TestSecretSplit_UnsignedAndGarbageRefused`, `TestSecretSplit_ControlSecretRefusedOnEdgeRoutes` (incl. the tombstone route), `TestSecretSplit_RouteTableMatchesGuard` (chi.Walk: exactly 8 internal routes, each in exactly one group), `TestFinalize_OtherOrgRefused` (body and query, job stays running, owner's finalize works), `TestFinalize_OrgIDOptionalByDefaultRequiredByFlag`).
- quarry-orchestrator `go test ./... -count=1`: all ok. New in `internal/controlauth`: 7 tests (path matcher table, control route signs with the control secret and not the shared one, other routes keep the shared one, unset falls back with exactly one warning, equal warns once, no secret leaves it unsigned, control secret alone signs). `internal/jobs`: two existing finalize tests now assert `org_id` in the body.
- Node: `node --test scripts/tests/openapi*.test.mjs` from Quarry-v2: 40 tests, 38 pass, 2 fail, both PRE-EXISTING and unrelated (see Not verified): `every_registered_route_is_documented_or_allowlisted_with_a_reason` (`/v1/audit/page`, a route from another unit that is in neither spec nor the allowlist) and `the SDK files that are tracked or generated carry no internal API` (stale ignored generated files `sdks/typescript/src/apis/InternalApi.ts` and two models are on disk in this worktree). The new and changed guard tests pass.
- Rust via the lane c wrapper: `cargo test -p quarry-edge --lib job_lifecycle_routes`: 4 passed, 0 failed.

## Flags and defaults
- `QUARRY_CONTROL_INTERNAL_SECRET`: new secret, unset = old behaviour (fallback + one warning). Not a behaviour flag; it is the split itself.
- `QUARRY_CONTROL__FINALIZE_ORG_REQUIRED`: default OFF (compose `:-0`). A wrong-org finalize is refused regardless (leak fix, no flag).

## Assumed decisions
- Prefix-based orchestrator-only group (`/v1/internal/jobs/`, `/v1/internal/schedules/`), tombstones stay edge-signed.
- The edge does NOT receive the new secret (the scope line listed `job_lifecycle_routes.rs` among the users of it; the code shows its calls are outside the group, and the secret in the edge would void the split).
- Cross-org finalize is a 404 (indistinguishable from missing), not a 403.
- No-org finalize accepted until the owner flips `QUARRY_CONTROL__FINALIZE_ORG_REQUIRED`; the orchestrator already sends the org.
- `control-ack` is not org-checked (the scope names finalize only); depth row.
- The orchestrator reads the new secret from its own environment inside `controlauth` (no `main.go` / Config change, to stay in the unit's files).
- Files outside the listed scope that had to change: control `internal/resources/jobs_control.go` (the finalize handler), new `internal/httpx/hmac_split.go` and tests, orchestrator `internal/jobs/control.go` (org_id in the two finalize bodies) and its test, `scripts/openapi-routes.mjs` and the two node tests.

## Rollout order
Deploy control and the orchestrator in the same step with the same `QUARRY_CONTROL_INTERNAL_SECRET` (compose already feeds both from one variable). If control is given a distinct value and the orchestrator is not, the orchestrator signs with the old secret and control answers 401 on the job-control loop, the claim and the schedule routes. Never set it on the edge. Then, after the soak, remove the fallback (`ControlInternalSecretPosture`'s "unset" branch and `secretFor`'s empty branch) and make the variable required.

## Model Plane asks
None.

## Not verified
- No live stack run, no compose `config` parse (Docker not used); the compose edit is two `environment:` entries plus comments, with nested `${A:-${B}}` interpolation, and was not parsed by compose.
- The two failing node tests above were not re-run at the base commit; both are about files this unit did not touch (`routes.rs` `/v1/audit/page`, ignored generated SDK files).
- The older compose at `Quarry-v2/deploy/compose/docker-compose.yml` was not edited (outside the unit's files); it passes only `QUARRY_INTERNAL_SECRET`, so it runs on the fallback.
- `docs/CROSS_PLANE_AUTH.md`, `docs/ARCHITECTURE.md` and `docs/RUNBOOKS.md` still describe one shared secret (outside the unit's files).
