# Stack-evidence session 2026-10-28: the qv2e evidence stack

Owner order 2026-10-28: "Start stack-økten" (after the list A/B build). Runbook: `../STACK-SESSION-RUNBOOK.md`. This folder holds one evidence file per leg, `flags-log.md`, and the review.

## Stack facts

- **Isolated, not the live stack.** Compose project `qv2e`, containers `qv2e-*`, private network `qv2e-net`, fixture network `qv2e-fixture` (192.88.99.0/24, the same non-private subnet the repo's crawl-fixture overlay uses so the SSRF guard lets it through). Own Postgres (`qv2e-pg`, published 127.0.0.1:38432, databases `quarry_edge` and `quarry_v2`), Dragonfly, NATS (JetStream, token), Temporal (`qv2e-temporal` plus its own Postgres). The live stack is only read: the edge fetches Auth Core's JWKS from `host.docker.internal:3011` (the live auth-core) so a real user token can verify in the user legs.
- **Images**, built from a frozen `git archive` of `quarryv2-next` at `bd61260ee` (no worktree strays) plus the worktree's `Cargo.lock` (gitignored in Quarry-v2; sha256 prefix e7d3ee56b535b574):
  - `qv2e/quarry-edge:bd61260ee` (id fb4f6b4d115e), `/version` revision `qv2e-bd61260ee`
  - `qv2e/quarry-control:bd61260ee` (c482bbb8463a)
  - `qv2e/quarry-orchestrator:bd61260ee` (b028915d44a4)
  - `qv2e/fixture-a:bd61260ee`, `qv2e/fixture-b:bd61260ee`: nginx with the repo's `deploy/compose/crawl-fixture/{a,b}` baked in. A bind mount from the Temp folder gave nginx `stat() ... (5: I/O error)` on Docker Desktop, so the content is copied into the image.
- **Ports (host loopback):** edge 38082, control 38081, Postgres 38432.
- **Edge mode:** `ENVIRONMENT=dev`, `QUARRY_EDGE_AUTH_DEV_BYPASS=1`. A bearer that is not a valid Auth Core JWT becomes the stub identity `org_placeholder` / `user_placeholder` (principal `user`, profile import scope). A real JWT still verifies first. Use `Authorization: Bearer dev-evidence` in dev legs.
- **No** browser broker, model gateway, gateway, SPA, Data Plane, searxng or billing-core in this stack. `MODEL_PLANE_URL` is empty.
- **Metrics:** `GET /metrics` answers 401 without the metrics token. The token is in the stack's secrets file (never print it).
- **Helper:** `bash <scratchpad>/stack/qv2e.sh ps | up | restart-edge | logs <service> | config`. The edge reads its flags from `<scratchpad>/stack/qv2e-flags.env` (baseline = the committed compose defaults; every change is logged in `flags-log.md`).
- **Page cache:** the edge has a 300 s page cache (`QUARRY_EDGE__CACHE_TTL_SECS`). The lead's smoke test scraped `/`, `/robots.txt`, `/docs/a` and `/private/secret` on fixture-a at about 16:57 UTC.

## Smoke test (lead, 2026-10-28 ~16:55 UTC)

- All ten services healthy at the first `up`.
- `/ready` 200; `/version` `qv2e-bd61260ee`.
- `POST /v1/scrape` of the four fixture-a paths with the dev bearer: each 200 upstream after the fixture images were baked.
- The edge reaches the JWKS at `host.docker.internal:3011`.
- Boot log: 0 ERROR lines. Two WARN lines worth a look: "no BrowserBroker validator is configured" (expected in dev) and `profile tombstone sweep failed` with `error_class: INTERNAL` at boot (to be explained by the D16 leg).

## P0 notes (2026-10-28, leg P0-preflight, 16:57 to 16:59 UTC)

- **Metrics are not on port 38082.** `/metrics` runs on an internal listener (`:9092` inside `qv2e-edge`, bearer `QUARRY_EDGE__METRICS_TOKEN`). The public port answers 401 without a bearer and 404 with one. Read counters with this shape (the token is read inside the container and never printed):
  `docker exec qv2e-edge sh -c 'wget -q -O - --header="Authorization: Bearer $QUARRY_EDGE__METRICS_TOKEN" http://127.0.0.1:9092/metrics'`
- **Counters appear at the first increment.** At 16:58 only `quarry_robots_decisions_total` (5 series, all 0) existed of the five watched families. Treat a missing line as 0.
- **Scrapes do not move the robots counters.** Only crawl and map do.
- **Answer routes:** `POST /v1/answer` and `/v1/answer/stream` both return 501 `UNSUPPORTED` (no SearchProvider, no Model Plane URL). Each 501 writes one ERROR line (tower_http) in the edge log.
- **Egress:** `https://example.com/` scrapes fine (200). `offhost.example.net` does not resolve from the edge.
- **Fixture fetches** show in `docker logs qv2e-fixture-a` with user agent `Quarry/2.0`.
- **Flag baseline** copy for diffing: `<scratchpad>/stack/p0-flags-start.env` (same as `qv2e-flags.baseline.env` content at 16:57). P0 changed no flag.
- The `profile tombstone sweep failed` WARN from the smoke test is not in the current edge boot log.
