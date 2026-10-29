# SC1 prep: independent read-only verification (2026-10-29, 11:11Z to 11:15Z)

This verification ran no build, recreate, commit or DB write, and did not change any tracked file. The only containers it started were two throwaway `curlimages/curl:8.10.1` probes (`--rm`, already present locally), and both are gone (`docker ps -a` shows no `sc1-verify*` or `grant-contract-tunnel`). It printed no env value except non-secret flags and URLs.
Preflight transcript: `preflight.txt` in this folder.

| Check | Result | Evidence |
|---|---|---|
| Wrapper preflight (`GATEWAY_CONTAINER=verevon-gateway-rs ... --preflight-only`) | PASS | exit 0, `preflight passed` at 11:11:28Z, 19 `ok` lines, none FAIL; it printed no secrets (`preflight.txt`) |
| quarry-edge healthy | PASS | healthy, restarts=0, started 11:07:30Z, image b1782860bea1 (= the rollback tag, unchanged) |
| broker healthy | PASS | `model-plane-browser-broker-1` healthy, restarts=0, started 11:03:44Z, image ea91868594dd (= the rollback tag) |
| model-gateway healthy | PASS | `model-plane-model-gateway-1` healthy, up since 2026-10-28 17:03Z (not touched) |
| gateway healthy | PASS | `verevon-gateway-rs` healthy, restarts=0, started 11:06:32Z, image dd6bce326f33 (= the rollback tag) |
| auth-service healthy | PASS | healthy, restarts=0, started 11:03:00Z, image 60c620875b78 (= the rollback tag) |
| execution-core and SPA healthy | PASS | `model-plane-execution-core-1` and `frontend-plane-verevonv3-frontend-1` healthy |
| Rollback tags exist | PASS | `rollback/{quarry-edge,verevon-gateway-rs,model-plane-browser-broker-1,auth-service}:pre-sc1-20261029` = b1782860bea1 / dd6bce326f33 / ea91868594dd / 60c620875b78 |
| `http://localhost:5173/` | PASS | 200 |
| `/api/v1/auth/session`, signed out | PASS | 200 with body `null` (null user, not 5xx) |
| Unauthenticated `POST /api/v1/browser/sessions` | PASS | 401 `{"error":{"code":"unauthorized",...}}` through :5173 and directly on gateway 127.0.0.1:3185 |
| SPA proxies to the gateway under test | PASS | frontend `GATEWAY_PROXY_TARGET=http://gateway:3185`; `verevon-gateway-rs` carries the alias `gateway` on inter-plane-bus (the frontend's only network) |
| quarry-edge has no host port | PASS | `docker port` empty, `PortBindings {}`, read_only=true, cap_drop=[ALL] |
| quarry-edge refuses a non-JWT bearer | PASS | throwaway probe on ingestion-net: `/v1/scrape` and `/v1/browsers`, no auth 401, `Bearer not-a-jwt` 401; edge log `failed to decode JWT header InvalidToken` (the bearer reached JWT validation, so there is no dev bypass) |
| example.com reachable from quarry-edge's network | PASS | probe on ingestion-net: `https://example.com/` 200 (172.66.147.243) |
| Edge to broker, gateway to edge and model-gateway | PASS | probe on inter-plane-bus: `browser-broker:8087/healthz` 200, `:9095` answers h2 (415 to a bare POST), `quarry-edge:8082/health` 200; from inside the gateway, `model-gateway:8080/healthz` 200 and `quarry-edge:8082/health` 200 |
| `sc1-mode.sh status` works | PASS | exit 0 at 11:12:38Z: registry `ok capability-core-memory-controls=kept`, broker TTL 45 (sc1), gateway strict (sc1), edge mode=sc1 (production, both dev bypasses 0, validator `http://browser-broker:9095`, REQUIRE_BROWSER_GRANTS=1, DURABLE_EVENT_HISTORY=1, ARTIFACT_BACKEND=filesystem, NO_SANDBOX=0, host_ports=0); spa 200, unauth sessions 401. The script prints only fixed non-secret keys (read in full) |
| Broker TTL in the log | PASS | `browser-broker grant limits grant_ttl_seconds=45`, durable Postgres grant store |
| Broker image has the grantlog lines (needed for H.3) | PASS | the binary `/usr/local/bin/service` contains `browser grant issued` and `browser grant revoked`. Its revision label e603fde78 is not a descendant of d7a04e07a, but the strings are in the binary |
| Gateway `BROWSER_HUMAN_BROWSE` | PASS | unset, so off. A.4 and B.1 can pass (SC1-RUN.md difference 3) |
| Recent error lines | INFO | 0 ERROR/WARN in broker, gateway and auth-service since recreate. quarry-edge: 1 ERROR, the Data Plane NATS `authorization violation` (pre-existing, noted in PREP). WARNs are startup notices plus the probe's own 401s |
| `sc1-legs.js` syntax | PASS | `node --check` exit 0 |
| `sc1-legs.js` TTL | PASS | `legA6(ttl)` defaults to 45 and waits T+5=50 s, under the 120 s run cap. It matches broker TTL 45 and SC1-RUN step 4 `L2.legA6(45)` |
| PREP.md vs the stack | PASS | container names, image ids, rollback tags, TTL 45, strict, edge production posture, networks (ingestion-net + inter-plane-bus) and no host port all match what was observed |
| SC1-RUN.md container names | FAIL (doc) | Lines 28 and 77 use `frontend-plane-verevonv3-gateway-1`, but no such container exists; the live gateway is `verevon-gateway-rs`. The section 0 preflight would fail `running`, and the section 3 H.1/H.2 scan would count `0` on the missing container (a false PASS, because `docker logs` errors go into `grep -c`). Use `GATEWAY_CONTAINER=verevon-gateway-rs` and `GW=verevon-gateway-rs` |
| SC1-RUN.md stack note | INFO | Line 7 says doc section 1 reserves the live stack for no step, and live dev is the owner's call. The owner said "Forbered SC1" / "dette er kun dev", so the run header must state that SC1 runs on live dev |
| Wrapper full run: tunnel port | FAIL (pre-run) | The wrapper's default `TUNNEL_PORT=18082` is already bound on the host by `model-plane-inference-core-1` (127.0.0.1:18082->8082). Start the full wrapper with `TUNNEL_PORT=<free port>`, for example 18182, or its `docker run -p` fails |
| Wrapper full run: tunnel image | WARN | `alpine/socat` (default `TUNNEL_IMAGE`) is not present locally, so the first full run pulls it from Docker Hub. Allow that pull, or set `TUNNEL_IMAGE` to a local image that has socat (none found) |

## What blocks SC1

1. **Sign-in, the user's step:** the page legs (SC1-RUN sections 1 to 9) and the wrapper without `--preflight-only` need the user signed in to the built-in browser at `http://localhost:5173` with a seeded dev account. The stack itself is ready.
2. **SC1-RUN.md gateway name:** replace `frontend-plane-verevonv3-gateway-1` with `verevon-gateway-rs` at lines 28 and 77 before running sections 0 and 3. If you do not, the preflight fails and the H.1/H.2 counts are false zeros.
3. **Wrapper tunnel port 18082 is taken** by `model-plane-inference-core-1`. Run the full wrapper with `TUNNEL_PORT=18182` (or any other free port), and allow the one-time `alpine/socat` pull.

Not blocking, but record them: the Data Plane NATS refusal of the edge (pre-existing); capability-core's `session:memory-controls:read` lives only in the live registry (PREP open issue 1: never recreate auth-core with the bare `run-control-plane.sh`); leg C may spend model tokens if a planner is configured (weekly usage line); the restart rows R.* are out of scope (TTL >= 300 needed).
