# Stack session on live dev, 2026-10-28

Owner order 2026-10-28: merge quarryv2-next into main, rebuild live dev, test there ("dette er kun dev").
The isolated qv2e stack (results one level up: P0-preflight.md, L-X10-robots.md, L-SC7-billing.md) is torn down.
This folder holds the evidence of the same legs run again on the live dev stack, plus the legs qv2e did not reach.

## What runs

- `main` = `quarryv2-next` (`bd61260ee`). The live checkout `C:\dev\CoresSystem` is on `main` with the
  integration commit `7c082825a` (branch `integ/main-20261028`) applied as uncommitted changes: that is the
  live tree's own earlier work plus the 24 resolved conflicts. Snapshot of the earlier live tree: `0b3dee61f`
  (branch `wip/live-tree-2026-10-28`).
- Rebuilt from that tree at 2026-10-28 ~19:45 UTC: quarry-edge, quarry-control, quarry-orchestrator, the
  verevonv3 gateway image (verevon-gateway-rs), and the SPA container (restart).
- NOT rebuilt, on purpose: Model Plane (browser-broker, model-gateway, ...) runs builds from the `model-plane`
  branch (worktree epic-shaw-4d613b); building them from main would drop that branch's work. auth-core: the
  only change is a principal-registry row (quarry-edge -> browser-broker, the X-9 ask), which lives in a secret
  env value; not changed.
- Quarry `Cargo.lock` in the live checkout was stale (2026-10-06, html2md 0.2.15, the build failed); replaced
  with the integration worktree's lock (html2md 0.2.17). Old copy: `C:/dev/backups/pre-rebuild-20261028-1900/`.
- Backups before the rebuild: `C:/dev/backups/pre-rebuild-20261028-1900/` (DB dumps, rollback image tags
  `rollback/<container>:pre-rebuild-20261028-1900`).

## Stack facts for the legs

- Edge `quarry-edge` at http://127.0.0.1:8082 (ENVIRONMENT=dev, QUARRY_EDGE_AUTH_DEV_BYPASS=1: a bearer that
  is not a real Auth Core JWT becomes the stub org_placeholder / user_placeholder, principal user; use the
  literal `Bearer dev-evidence`). `/version` reports revision "unverified" (no build arg on live).
- Control `quarry-control` at http://127.0.0.1:28081. Orchestrator `quarry-orchestrator`. Postgres
  `ingestion-postgres` (127.0.0.1:15432, user ingestion_user; databases quarry_edge, quarry, quarry_v2, ...);
  use `docker exec ingestion-postgres psql -U ingestion_user -d quarry_edge`.
- Crawl fixture: the official TEST-ONLY overlay `apps/Ingestion Plane/docker-compose.crawl-fixture.yml`:
  containers crawl-fixture-a (fixture-a.crawl.test, docs.fixture-a.crawl.test) and crawl-fixture-b
  (fixture-b.crawl.test) on network crawl-fixture (192.88.99.0/24); quarry-edge joined it.
- Helper: `bash <scratchpad>/stack/live.sh edge-up | fixtures-up | flags | compose <args>`. Flags: edit
  `<scratchpad>/stack/live-flags.env` (empty = committed compose defaults), log in `flags-log.md` here, then
  `live.sh edge-up`.
- Edge metrics: internal :9092 with a bearer: `docker exec quarry-edge sh -c 'wget -q -O - --header="Authorization: Bearer $QUARRY_EDGE__METRICS_TOKEN" http://127.0.0.1:9092/metrics'`
  (the token stays inside the container).
- Live has billing-core-service, Lago, and several NATS servers (ingestion-nats is the Ingestion one).
- Baseline switches (live.sh flags at 19:46 UTC): ENVIRONMENT=dev, AUTH_DEV_BYPASS=1, AI_FORMATS 0,
  ANSWER_SYNTHESIS 0, ARTIFACT_BACKEND memory, AUTOPROMPT 0, BROWSER_GRANT_VALIDATOR_GRPC_URL empty,
  BUDGET_ENFORCEMENT shadow, CRAWL_ROBOTS observe, CROSS_PLANE_AUTH_DEV_BYPASS 0, LLM_CLASSIFY_INTENT 0,
  MODEL_AUDIO 0, MODEL_PLANE_URL empty, PAGE_TITLE_ENRICH 0, PROFILE_ENCRYPTION off, ROBOTS_UNREACHABLE
  disallow, RUN_SWEEPER_DRY_RUN 1, SCRAPE_ACTIONS off, SCRAPE_CAPTURE off, SEARCH_SUGGEST 0,
  SEMANTIC_RERANK 0, ZERO_SAAS_SEARCH 0. Not set (code default): PROFILE_OWNER_ENFORCE,
  TAKEOVER_ENFORCEMENT, RECEIPT_TEXT_MODE, CRAWL_SCOPE_MODE, AGENT_DEFAULT_MAX_STEPS, DOC_ROUTING,
  VECTOR_RETRIEVAL.
- First smoke: POST /v1/scrape of http://fixture-a.crawl.test/ with the dev bearer: 200 with markdown.
