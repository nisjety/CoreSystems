# SC1 prep on live dev: the four hops switched to the gate posture (2026-10-29)

Owner order in chat: "Forbered SC1" (prepare SC1) on the live dev stack ("dette er kun dev"). Nothing was built, committed or changed in a tracked file. Helpers live in the session scratchpad `stack/` directory (full path below as `$ST`):
`C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/stack`

Result: the read-only preflight (`GATEWAY_CONTAINER=verevon-gateway-rs bash scripts/tests/browser-grant-prod-contract.sh --preflight-only`) went from 10 FAIL lines (`preflight-before.txt`) to `preflight passed` at 11:08:16Z (`preflight-after.txt`). `status-after.txt` is `sc1-mode.sh status` at 11:09:10Z.

Rollback images (tagged 10:41:25Z, before any change), `docker tag <image id>`:

| Container | Rollback tag | Image id |
|---|---|---|
| auth-service | `rollback/auth-service:pre-sc1-20261029` | 60c620875b78 |
| model-plane-browser-broker-1 | `rollback/model-plane-browser-broker-1:pre-sc1-20261029` | ea91868594dd |
| verevon-gateway-rs | `rollback/verevon-gateway-rs:pre-sc1-20261029` | dd6bce326f33 |
| quarry-edge | `rollback/quarry-edge:pre-sc1-20261029` | b1782860bea1 |

No hop changed image: every recreate reused the running image id. Pre-change non-secret snapshots (env key -> sha256 prefix, never values; runtime shape; registry per principal) are in `$ST/sc1-before/`, post-change in `$ST/sc1-after/`.

## What changed, per hop

### 1. auth-core (`auth-service`), registry row
- How: `apps/Control Plane/scripts/run-control-plane.sh up -d --no-deps --no-build auth-core` (no rebuild needed: the running image already serves `scopesByAudience` and `retentionByAudience` for every principal). Before it, `run-control-plane.sh config` resolved auth-core's environment and the only key whose value differed from the running container was `PLANE_SERVICE_PRINCIPALS_JSON` (150 keys compared by hash).
- 1st recreate 10:48:20Z to 10:54:12Z: registry assembled from main's committed policy. Per-principal comparison (names, credential hashes, policy hashes) found a SECOND change besides the wanted quarry-edge row: capability-core lost `session:memory-controls:read`. The live registry had that scope appended to capability-core's `scopes` and to its `session-core` list (proved by reproducing the old row's hash exactly); main's policy lacks it, and the model-plane branch's capability-core asks for it (`capability-core/cmd/main.go:934`, policy commit 3bff5e8dc on branch model-plane, not on main).
- 2nd recreate 10:57:34Z to 11:03:01Z with `$ST/auth-registry-recreate.sh`: the registry is taken from the running container, the quarry-edge row is set from the committed policy, and capability-core keeps the appended scope. The value travels in a shell variable only (shell env wins over `--env-file` in compose interpolation).
- Verified: healthy, 0 restarts, `/users/public` 200; same 19 principal names; every credential hash unchanged; only quarry-edge's policy hash changed (now equal to main's assembled row: audiences browser-broker + data-plane, `browser:read` on browser-broker, retention zdr there); capability-core's row hash equals the pre-change one; preflight `registry_ok` = ok. No refusals in auth-core, capability-core, execution-core, model-gateway or quarry-control logs in the 10:53 to 11:04 gap beyond the pre-existing finetune-poller refusal (same count before).
- Side effects: `run-control-plane.sh` rewrites the `PLANE_SERVICE_PRINCIPALS_JSON` line of the untracked `apps/Control Plane/.env.generated-secrets` from main's policy on every run (now: with the quarry-edge row, without capability-core's memory-controls scope). Compose's recreate also removed a stopped container `auth-service-rollback-d1-789` (it carried auth-core's compose labels; made by an earlier session).
- Rollback: `sc1-mode.sh dev` keeps this hop on purpose (the task allows it). The image did not change, so the rollback tag is only a safety copy; the change is the registry env value alone.

### 2. broker (`model-plane-browser-broker-1`), grant TTL 900 -> 45
- How, 11:03:41Z to 11:03:57Z: `BROWSER_BROKER_GRANT_TTL_SECONDS=45 bash $ST/broker-compose.sh up -d --no-deps --no-build browser-broker`. `broker-compose.sh` is `compose.sh` in the model-plane worktree (`epic-shaw-4d613b`) with the same three live env files as `recreate-with-live-env.sh` (the broker has no NATS password to carry). A `config` dry run first: with the default chain the env equalled the running container key for key; with 45 only the TTL differed.
- Verified: healthy, same image, same shape (ports, networks, aliases); env diff = the TTL only; log `browser-broker grant limits grant_ttl_seconds=45`, durable Postgres grant store; model-gateway reaches it (`browser-broker:8087/healthz` 200 from inside model-gateway; its configured `BROWSER_BROKER_ADDR` host `model-plane-browser-broker-1:9095` answers h2).

### 3. gateway (`verevon-gateway-rs`), `BROWSER_GRANT_ENFORCEMENT=strict`
- How, 11:06:29Z to 11:06:40Z: `python $ST/gateway_env_swap.py --tag sc1 --set BROWSER_GRANT_ENFORCEMENT=strict`. It is `C:/dev/tools/swap_gateway_image.py` (which has no env override, and whose fixed rollback name `verevon-gateway-rs-pre-fix-rollback` is already taken, so it would fail at its rename) with `--set/--unset`, the running image id instead of `:latest`, a per-run rollback name, and removal of the stopped old container once the new one is healthy.
- Verified: healthy; env diff = the one added key; shape identical (network, aliases, port 127.0.0.1:3185) except `Config.Image` now reads the image id instead of `frontend-plane-verevonv3-gateway:latest` (same image). `:5173/` 200, gateway `/health` 200, unauthenticated `/api/v1/browser/sessions` 401.

### 4. quarry-edge, production posture
- How, 11:07:27Z to 11:07:37Z: `bash $ST/edge-sc1-compose.sh up -d --no-deps --no-build --force-recreate --wait --wait-timeout 300 quarry-edge`. Same env-file chain as `live.sh`; files `docker-compose.yml` + `docker-compose.production.yml` + `docker-compose.grant-contract.yml` (no crawl-fixture, no live-flags override); `ENVIRONMENT=production`, `QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL=http://browser-broker:9095` as `boot()` sets them; `SOURCE_REVISION`/`BUILD_DATE` only satisfy the overlay's interpolation (no build). A `config` diff first: the env keys that change are exactly ENVIRONMENT, QUARRY_EDGE_AUTH_DEV_BYPASS (1 -> 0), ARTIFACT_BACKEND (memory -> filesystem), BROWSER_GRANT_VALIDATOR_GRPC_URL (empty -> broker), XDG_CONFIG_HOME, plus new DURABLE_EVENT_HISTORY=1, REQUIRE_BROWSER_GRANTS=1, XDG_DATA_HOME; database, Dragonfly/Redis URLs and every other key unchanged.
- Verified: healthy, 0 restarts, same image; read-only root, cap_drop ALL, no published port; networks ingestion-net + inter-plane-bus; logs: Postgres profile store and step receipts (durable), durable event history enabled; resolves `browser-broker`; gateway -> `quarry-edge:8082/health` 200; `:5173/` 200; unauthenticated `/api/v1/browser/sessions` 401 (not 5xx). New named volume `ingestion-plane_quarry-edge-artifacts` (artifact root).
- Startup WARN/ERROR lines: the Data Plane NATS `authorization violation` (page-image producer disabled) is pre-existing (same finding in r2sc4 and earlier evidence); ZDR-refuse-durable off in production (observe only), brave paid backup egress, rerank shadow without rerank are notices.

## What normal dev use loses while in SC1 mode
- No dev bearer on the edge: `QUARRY_EDGE_AUTH_DEV_BYPASS=0` and production mode, so direct calls to the edge need a real Auth Core token; dev shortcuts and scripts that relied on the bypass get 401.
- No host port 8082: `http://localhost:8082` is gone (production publishes no port). Scratchpad scripts that call it (for example `rx12-sse.sh`) do not work; reach the edge through the gateway, or via the wrapper's throwaway `grant-contract-tunnel` (127.0.0.1:18082) when it runs.
- Strict grants: every in-app browser session needs a broker grant; a host that cannot be granted is refused (400 `browser_host_not_grantable`), and a session or run without a grant is 403 at the edge (`QUARRY_EDGE__REQUIRE_BROWSER_GRANTS=1`).
- Grants expire after 45 s (broker TTL), so a long manual browser session loses its grant quickly.
- No crawl fixtures: the edge is off the `crawl-fixture` network (X-10 and other fixture-host crawl tests need dev mode).
- Live-flags overrides (`live-flags.env`) do not apply; the edge runs the committed compose defaults plus the two overlays.
- Artifacts now go to the filesystem volume instead of memory (kept across restarts while in SC1 mode; the dev edge goes back to memory).

## How to go back
`bash $ST/sc1-mode.sh dev`: quarry-edge via `live.sh edge-up` (dev, crawl-fixture overlay, live-flags.env), broker back to the compose default TTL 900 (same live env chain), gateway recreated without `BROWSER_GRANT_ENFORCEMENT` (resolves to permissive), auth-core left as it is (the quarry-edge row is the committed policy). `sc1-mode.sh sc1` re-applies this posture (idempotent, ends with the preflight); `sc1-mode.sh status` prints the posture (non-secret keys only). `dev` was not run.

## Open issues and what was not done
1. The live auth-core registry carries a hand patch that main's policy lacks: capability-core `session:memory-controls:read` (model-plane branch, 3bff5e8dc). ANY `run-control-plane.sh` recreate of auth-core from main drops it and breaks capability-core's session-core token mint. Until the model-plane policy row reaches main, recreate auth-core with `$ST/auth-registry-recreate.sh` (or `sc1-mode.sh sc1`), not the bare recipe.
2. A stopped container `auth-service-rollback-d1-789` (an earlier session's rollback copy with auth-core's compose labels) was removed by compose during the first recreate; its image was not checked.
3. Capability-core ran without the memory-controls scope for about 9 minutes (10:54:12Z to 11:03:01Z); no refusal is visible in the logs of that window.
4. The Data Plane NATS refuses the edge (page-image producer disabled): pre-existing, not touched.
5. Not run: the SC1 legs themselves (user signs in, `L2-signed-in-steps.md` sections 2 to 7, and the wrapper without `--preflight-only`).
