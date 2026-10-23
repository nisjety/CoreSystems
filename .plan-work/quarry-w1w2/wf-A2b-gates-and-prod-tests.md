# A2b-gates-and-prod-tests hand-over (WS1.A2 part 2)

## What was built (commits f8153418, 720ae31f, 543730ba on quarryv2-next)
- G1: `GrpcGrantValidator::with_service_tokens`; every ValidateGrant carries a broker token minted for the run's org (manifest row `browser-grant-validate`, browser:read). One refresh on Unauthenticated, then Forbidden. Org-blind `validate` and `revoke` are refused when tokens are configured. Transient statuses: 3 attempts (50/200 ms) then DRIVER_FAILED 502. Mint failure never reaches the broker.
- G6: `FailedPrecondition` (revoked/expired) and `NotFound` -> `active:false` -> callers answer 403 "browser grant is inactive or expired" (SECURITY_BLOCKED).
- Boot gates: `quarry-edge/src/grant_boot.rs::decide` (pure) wired in main.rs. Production refuses an HTTP-only validator and no validator; a gRPC validator needs `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` (dev bypass is the only exception, never in production). Manifest row parsed at boot; gRPC connect already eager.
- `tests/browser_grants_prod.rs` (8 tests incl. the six plan names), `test_state_with_grants`.
- `docker-compose.grant-contract.yml` (env only) + `scripts/tests/grant-contract-overlay.test.mjs` (renders compose, diffs hardening keys), L1 asserts, `scripts/tests/browser-grant-prod-contract.sh`, `tests/e2e/browser-grants-prod-contract.spec.ts`, `.github/workflows/browser-grant-contract.yml`, base compose key, RUNBOOKS section.

## Traps found
- The RUNBOOKS.md hunk of commit 720ae31f also contains another unit's (C2, WS4.J) appended section: two units appended to one file in the same window and my whole-file commit took both. Harmless text, but that section is in HEAD before its code is.
- `build_server(true)` in quarry-runtime's build.rs: needed for a fake broker in tests. The edge's grpc test build is cold ~14 minutes in the container.
- A plain `cargo test -p quarry-edge` (no `--features grpc`) runs only the boot tests of browser_grants_prod.rs; the validator-path tests are `#[cfg(feature = "grpc")]`. quarry-v2-ci.yml's workspace job does not enable grpc, so only the new workflow job runs them.
- The wrapper/spec assumptions that nobody has verified on a stack: Auth Core's `GET /api/<aud>/token` accepts the session cookie when called on :3011 directly; model-gateway answers 201 with `grant_id`/`kind`; the container names (`model-plane-browser-broker-1`, `frontend-plane-verevonv3-gateway-1`, `auth-service`, ...); `alpine/socat` is pullable; `docker inspect` of auth-service exposes `PLANE_SERVICE_PRINCIPALS_FILE` or `_JSON`.
- Legs B and C use `test.fail()`. WS1.B is already largely built, so on the exit-gate stack they may pass unexpectedly: that fails the suite on purpose; item b removes `test.fail()` and the `@legBC` handling in the wrapper (CONTRACT_BC_GREEN=1).
- The log scan prints counts only. Legs need 45 s broker TTL (`BROWSER_BROKER_GRANT_TTL_SECONDS=45`), set by `--boot`; preflight checks it.

## Left
- Run the wrapper at the exit gate (needs the rebuilt production-overlay stack and seeded accounts). Not run here.
- Mutation evidence: three planted mutations (FailedPrecondition not inactive; org-blind validate falling through; HTTP-only accepted in production) each fail the intended tests (runtime 2 + 1, edge prod_step_with_revoked 1, grant_boot 1 + binary prod_boot_refuses_http_only_validator 1), files restored byte-identical.

## Not run (honest list)
- Full quarry-edge and quarry-runtime suites and `cargo check --workspace --all-targets` (Docker stopped answering; see progress file). Only the targeted suites listed in the commits were run.

## Round 4 additions
- The validator construction is `grant_boot::build_grpc_validator`; the prod tests and main use the same function. Removing the tokens arm now fails the prod_* tests (reviewed by hand; not yet run, see the progress file).
- Wrapper: the no-sandbox probe reads /proc (the quarry-edge image has no ps); all five hops must exist, a missing one fails the run. Spec: legs B and C no longer use test.fail(); `untilWs1b` accepts only an HTTP refusal in KNOWN_BC_STATUSES (400,401,403,409,502,503, override CONTRACT_BC_EXPECTED_STATUSES). The real pre-WS1.B status is a guess until the exit gate shows it in the `observed` annotations.
- Correction: 543730ba's message says 18 planted mutations, the test has 19.

## Round 5 update (2026-10-23)
The earlier "Not run" list is now closed except the exit-gate stack run: full quarry-edge and quarry-runtime suites (1854 passed, 0 failed, default features), cargo check --workspace --all-targets, the log lint (22 passed) and the 3fb780d8 change (8 prod tests, mutation detected by 4) all ran in the container. Commit fffe3627 is the last one. Remaining: L2 wrapper/spec on the rebuilt production-overlay stack; the `--features grpc` full edge suite beyond browser_grants_prod was not run (only default features for the full suite).
