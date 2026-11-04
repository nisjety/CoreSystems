# W2-FE-GW1 status: INNH-10 GW-1, gateway x-api-key ingress for browser agent runs (lane d, branch w23/lane-d)

## Built
All of the scope, CODE ONLY. Checked first: nothing of GW-1 existed in the gateway (no `x-api-key` handling, no `api-key-exchange` or `/v1/browser/key-grants` caller). Only the gap was built.

- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/api_key_agent.rs` [NEW]: routes `POST /api/v1/api-key/agent/runs`, `POST /api/v1/api-key/agent/runs/{run_id}/step`, `DELETE /api/v1/api-key/agent/runs/{run_id}`. Order on every route: configured (service credential present, else 503 `api_key_agent_not_configured`) -> `x-api-key` present and printable, at most 512 (401 `api_key_missing` / `api_key_invalid`) -> per-key ingress limit keyed by `api_key_agent:<sha256(key)>` (429 `rate_limited`, reason `api_key_rate_limited`, Retry-After). Start: strict body (`deny_unknown_fields`, so `grant_id`, `profile_id`, `resume_run_id` are refused), hosts 1..32 and plain, budget within 10 USD / 500 steps / 900 s, `initiated_by` only `agent`, `start_url` http(s) only -> exchange at auth-core for `browser-broker` -> `POST {model-gateway}/v1/browser/key-grants` -> exchange for `quarry` -> `POST {quarry}/v1/agent/runs` (`constraints` from the tighter of asked and granted budget, granted hosts, `zdr` = asked OR grant ZDR (`z_` id or `zdr:true`), `grant_id`, `initiated_by: agent`) -> optional navigate step to `start_url` -> 201 `{ data: { run_id, budget, zdr, expires_at?, allowed_domains? (omitted under ZDR), initial_step? } }`. Any failure after the grant revokes it (and closes the run if it had started). Step: binding lookup (another key or unknown run -> 404, no upstream call), JSON-object body forwarded as is, 404/410 from Quarry drops the binding and revokes. Close: Quarry DELETE then always revoke. Exchanged tokens are cached until min(expiresInSeconds, 120) - 15 s, keyed by `sha256(key) NUL org NUL audience`; the raw key is only ever in the auth-core exchange body. Every upstream request is built fresh (no caller header copied). Upstream refusals are rebuilt: our code, the upstream reason only when it is a snake_case token, never an upstream message. Logs carry only a 12-hex key ref and status, never the key, a token or a host.
- Tests (in the same file, wiremock doubles for auth-core, model-gateway and Quarry): flag off mounts nothing; flag on without credentials 503; missing key 401; 8 invalid bodies 400 with no upstream call; happy start with caller `Authorization`, `x-browser-authorization`, `cookie`, `x-org-id`, `x-user-id`, `x-verevon-org-id`, `x-service-id` all absent upstream and the key only in auth-core's body; ZDR grant omits hosts and starts a ZDR run; exchange refusal keeps `api_key_org_mismatch` and acquires nothing; dark exchange (404) -> 503 `exchange_disabled`; failed Quarry start revokes (no upstream message leaks); step reuses the cached token; another key cannot step or close (404, nothing upstream); close deletes and revokes; run gone on step revokes; per-key rate limit with a separate bucket per key; cache key never contains the key; reasons are tokens only; key, tokens and hosts never logged (captured subscriber).
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mod.rs` [C]: one line `pub mod api_key_agent;` (alphabetical, after `anbud`, so rustfmt's module ordering is kept).

## Commits (lane d)
- 853ff3403 feat(gateway): x-api-key ingress for browser agent runs behind API_KEY_AGENT_RUNS (INNH-10 GW-1)
- (this file, MP ask and depth rows: the docs commit that adds this file)

## Code only, not compiled
No cargo build, check, clippy or test was run (owner order 2026-11-03). Only `rustfmt` was run on the new file (a formatter, not a build; it also confirmed the file parses). Integration must run `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets api_key_agent` and clippy.

## INTEGRATION STEP NEEDED (outside this unit's files)
`src/main.rs` `build_router` needs `.merge(domains::api_key_agent::router(state.clone()))` (alphabetical, near `agents_runs` / `ai`). Until then `router` carries `#[allow(dead_code)]` and nothing is reachable, flag or not. The router is empty when the flag is off, so the merge cannot collide with another route.

## Flag and default
`API_KEY_AGENT_RUNS` (gateway), default off (only `true` or `1` turns it on): off, no route is mounted (ordinary 404). Read once when the router is built (`ApiKeyAgentConfig::from_env`), never in a handler; not on `AppState` because `config.rs` is outside this unit. Knobs: `API_KEY_AGENT_SERVICE_ID` (default `verevon-gateway`), `API_KEY_AGENT_SERVICE_API_KEY` (empty -> 503 with the flag on), `API_KEY_AGENT_RATE_PER_HOUR` (default 600). The whole path also needs auth-core `API_KEY_ENABLED` + `API_KEY_AGENT_EXCHANGE_ENABLED`, the registry row's `apiKeyExchange`, MP-1..3 and Quarry's `QUARRY_EDGE__API_KEY_AGENT_RUNS`.

## Assumed decisions
- Path `/api/v1/api-key/agent/runs` (the design leaves it to this session); three routes only (start, step, close). Receipts, tabs and the events stream (allow-listed in Quarry U1) are not exposed in v1.
- The caller names the org in the start body (`org_id`): the exchange needs `orgId` and auth-core refuses a key bound to another org (`api_key_org_mismatch`), so the body cannot widen anything. Step and close take the org from the gateway's own run binding.
- The run binding (run_id -> key hash, org, grant id) is in gateway process memory, bounded (4096) and expiring (900 s + 120 s). See the depth row on multiple replicas.
- The gateway service credential is a dedicated env var, not `MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY` (the R13-1 security review asked for a separable credential). The operator sets it to the credential of the registry row that holds `apiKeyExchange`.
- Model-gateway key-grant door: the broker-audience key token is sent as `x-browser-authorization: Bearer <token>` with no `Authorization` header (there is no model-gateway-audience key token); body `{ allowed_domains, budget{micro_usd,max_steps,max_runtime_s}, zdr }`; answer `{ grant_id, expires_at, allowed_domains?, budget, key_id }` bare or in `{ data }`; revoke `DELETE /v1/browser/key-grants/{grant_id}` with the same header, 2xx or 404 is success.
- Revoke on failure: after the grant on any start failure; on step only when Quarry answers 404/410 (the run is gone). A refused step (policy, budget, sensitive action) keeps the run and the grant; the grant TTL still bounds it.
- Status codes: auth-core 401/403/429 keep their status (`api_key_refused` + reason), 400 -> 400 `invalid_request`, 404 (exchange dark) -> 503 `exchange_disabled`, other -> 503. Upstream 4xx keep their status, 5xx -> 502.
- Per-key ingress limit uses the gateway's `RateLimiter::from_cache_with_rpm` (Dragonfly-backed, in-process fallback), burst = per-minute rate (10 at 600/h).

## MP asks
Appended to MP-ASKS.md under "W2-FE-GW1": the exact request/response/header shape the gateway sends to `POST`/`DELETE /v1/browser/key-grants` (MP-3).

## Not verified
- Nothing compiled or run. Risks for the compile guard: axum 0.8 `Router::with_state` into `Router<AppState>`, wiremock 0.6 `Request` fields, `reqwest::RequestBuilder::timeout` overriding the client's 25 s.
- No live auth-core exchange, no model-gateway key-grant door (MP-3 not built), no Quarry U1-U4 path. The MP-3 answer shape is the design's proposal, not a built contract.
