# quarryv2 deployment verification, 2026-10-16

Branch `quarryv2`, HEAD at start `dd4bc8b7` (image tags `qv2v/*:dd4bc8b7`). Local only; nothing pushed.

## Method and deviation from "swap the live stack"

The live Docker stack (project containers `quarry-edge`, `model-plane-*`, `verevon-gateway-rs`, ...) is bind-mounted
from the main checkout, runs the user's ~324 uncommitted live-tree changes, and is in use by other sessions
(`model-plane-model-gateway` was rebuilt ~2 h before this run). The branch is cut from committed main, so swapping
branch images into it would have regressed that work and applied control migrations 019/020 to the live
`quarry_v2` database. So every service under test was rebuilt from the branch worktree under an isolated tag
`qv2v/<service>:dd4bc8b7` and run **side by side** as `qv2v-*` containers on a private network `qv2v-net` with its
own Postgres, Dragonfly, NATS and Temporal. The live stack was only read: auth-core JWKS and token minting
(`host.docker.internal:3011`) for real user JWTs. No live container, volume, image tag or database was changed.

Consequence: this is "rebuilt image + running container + end to end flow", on an isolated stack, not on the user's
live stack. The live stack still runs the old images.

## Log

### Images built (all `docker build` from the worktree at `dd4bc8b7`; build args SOURCE_REVISION=dd4bc8b7)
| image | id |
|---|---|
| qv2v/browser-broker:dd4bc8b7 | 2f9adbef56f0 |
| qv2v/quarry-control:dd4bc8b7 | d3d6fbb224fe |
| qv2v/quarry-orchestrator:dd4bc8b7 | d012ff60c633 |
| qv2v/model-gateway:dd4bc8b7 | c87f5762100b |
| qv2v/verevon-gateway:dd4bc8b7 | 2ea1b8cb18cc |
| qv2v/quarry-edge:dd4bc8b7 | (see below) |

Test identity: the repo's own seeded dev account `local@verevon.dev` (credentials from
`apps/Frontend Plane/verevonv3/tests/e2e/local-auth.setup.ts`), signed in through the live gateway on `:5173`; plane
tokens minted from the live auth-core (`GET /api/<aud>/token`, session cookie + the gateway's internal key) exactly
as the gateway does. Org `eDJiveH1XUJWTEMkia2CIMVu68bcMS4z`.

## Slice WS1.A1 (browser-broker + model-gateway grants): VERIFIED
Containers `qv2v-broker` (qv2v/browser-broker, :9095/:8087) and `qv2v-mg` (qv2v/model-gateway, :38080,
`BROWSER_BROKER_ADDR=http://qv2v-broker:9095`).

    POST /v1/browser/grants  (no auth)                         -> 401
    POST ... authorization only (no x-browser-authorization)    -> 401   (delegated bearer is required)
    POST {"allowed_domains":["example.com"],"mode":"cloud"}     -> 201 {"allowed_domains":["example.com"],"expires_at":"2026-10-16T12:05:32Z","grant_id":"c3f3c4da787022bf9d9c4b1a57b3a52e","kind":"session","zdr":false}
    POST {... "zdr":true, two domains}                           -> 201 {"expires_at":"2026-10-16T12:05:34Z","grant_id":"z_72245aced75c37be8e12a4bac6d5dbb1","kind":"session","zdr":true}   (z_ id, no allowed_domains in the body)
    POST {"allowed_domains":["metadata.google.internal"]}        -> 400 {"error":"browser_host_not_grantable"}
    DELETE /v1/browser/grants/c3f3...                            -> 200 {"revoked":true}   (again: 200 {"revoked":true})
    20 per-owner cap: 1 ZDR + 19 more -> 201 x19, then 429 x3

Deviation from the plan's evidence text: the broker log has **no per-call `AcquireGrant` line** (only the four boot
lines), so "broker log shows one AcquireGrant and no domain list" cannot be shown; the no-domains half holds
trivially. Not treated as a defect (the plan's WS1.A1 logging requirement is the effective TTL and caps at boot,
which is present: `browser-broker grant limits grant_ttl_seconds=900 max_session_domains=8 max_active_grants_per_owner=20`).
The gRPC `ValidateGrant` leg (Quarry side) is WS1.A2 and is not part of this branch.

## Slice WS1.A0 (production overlay boots quarry-edge): VERIFIED, after one fix (commit `37f9d278`)
Edit: `docker-compose.production.yml` sets `QUARRY_EDGE__ARTIFACT_BACKEND` (default `filesystem`, override
`QUARRY_EDGE_ARTIFACT_BACKEND=s3`), `QUARRY_EDGE__ARTIFACT_ROOT=/app/data/artifacts` on named volume
`quarry-edge-artifacts`, `QUARRY_EDGE__DURABLE_EVENT_HISTORY=1`, and requires
`QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL` (`:?`). `docker compose ... config` of base + production renders
(env chain from the deploy recipe, `--project-directory` at the live Ingestion Plane dir for the `.env` files).
`docker-compose.yml` untouched. RUNBOOKS section "Starting the edge on the production overlay".

Defect found on the rebuilt image, not by any unit test: first run (image `2e9ed447608f`, rev `dd4bc8b7`):
`/ready` -> `{"durable":true,"status":"ready"}` 200, log has no "durable runtime could not be established",
BUT `/app/data/artifacts` was `root:root 755` (Docker creates a missing mount point root-owned and the edge runs as
uid 65532), so the first `POST /v1/scrape` returned
`500 INTERNAL "fs write /app/data/artifacts/org=.../html.html: Permission denied (os error 13)"`.
Fix: `Dockerfile.edge` creates `/app/data/artifacts` owned by 65532 so a fresh named volume inherits it.
Rebuilt (image `782656b0ec18`, `edbf13ce` later `c2421b96bfe5`), fresh volume: dir is `quarry quarry`, scrape of
example.com -> 200 with html + markdown artifacts.

Boot gates, rebuilt image, production env (each run as a throwaway container, exit 1 with these messages):
- `ARTIFACT_BACKEND=memory` -> `Error: durable runtime is required but QUARRY_EDGE__ARTIFACT_BACKEND is in-memory; configure fs or s3`
- `DURABLE_EVENT_HISTORY=0` -> `Error: durable runtime could not be established (artifact/profile/history backend fell back to ephemeral storage)`
- validator URL pointing at a host that does not exist -> `Error: DriverFailed: grpc connect failed: transport error`
  (answers the plan's [UNVERIFIED]: a placeholder URL is NOT enough, the gRPC connect is eager; the broker must be up)
- `METRICS_ENABLED=1` with empty token -> `Error: metrics: QUARRY_EDGE__METRICS_TOKEN is required in production when QUARRY_EDGE__METRICS_ENABLED=1`
Running container: `ENVIRONMENT=production`, `docker ps` `Up ... (healthy)`, validator `http://qv2v-broker:9095` (branch broker),
`GET /version` -> `{"revision":"edbf13ce",...}`. Not tested: the literal `docker compose ... up quarry-edge` (that
would swap the live container); the container was started with the flags the rendered overlay produces
(read_only, cap_drop ALL, the three tmpfs, seccomp profile from the worktree, `init`, 45 s stop timeout).

## Slice WS3.A (Chromium sandbox + seccomp, production hardening): VERIFIED
Same container (read-only root, `cap_drop ALL`, no `no-new-privileges`, seccomp-chromium.json, tmpfs uid 65532).
`POST /v1/scrape` with `signals.screenshot=true` (forces the browser driver) -> 200, `"driver":{"kind":"browser","duration_ms":1321}`.
`ps -eo args` sampled every 0.5 s for 30 s in the container during the request: 83 lines mentioning chromium,
**0** containing `--no-sandbox` (`grep -c -e "[-]-no-sandbox"` -> 0). Chromium + crashpad running from
`/var/lib/quarry/chromium/xdg-config/...`.
Not covered: the `navigate` step of an agent run (needs a grant in production; WS1.A2 not on this branch). The
scrape path exercises the same launcher.

## Slice WS5.A/B (content sniff, 415, PDF/DOCX): VERIFIED after one fix (commit `edbf13ce`)
DOC_ROUTING default `shadow`: `https://example.com` -> `content_kind:"html"` (`content_kind_source:"header"`), fingerprint
`blake3:f26a1fdd...ab049` identical on a second scrape; W3C `dummy.pdf` -> `content_kind:"pdf"` (`source:"magic"`).
`skip`: `https://www.w3.org/Icons/w3c_home.png` -> 415 `UNSUPPORTED_MEDIA`, `details.kind:"image"`, `subtype:"png"`;
a GitHub ZIP -> 415 `kind:"archive"`, `bytes:4059197`.
`on`: DOCX (`https://calibre-ebook.com/downloads/demos/demo.docx`) -> `content_kind:"docx"`, markdown artifact begins
"Demonstration of DOCX support in calibre / This document demonstrates the ability of the calibre DOCX Input plugin...",
raw artifact 1311881 bytes. PDF raw artifact is byte-identical to the downloaded file (`cmp`).
**Defect:** `dummy.pdf` with `on` returned a 19-byte markdown artifact of control characters
(`\001 \002 \003 \003 \n\n \004 ...`) and no warning. The PDF has a subset TrueType font with a `/ToUnicode` map, and
`quarry-transform::pdf::extract_text` read the raw string operands. No unit test used a PDF with real text.
Fix: use lopdf's font-aware `Document::extract_text` per page, raw walk as fallback; test with a generated
subset-font PDF (`subset_font_text_is_read_through_the_tounicode_map`; `cargo test -p quarry-transform` 144+11 pass in
the rust:1 container). Rebuilt edge (`c2421b96bfe5`): markdown artifact is `Dummy PDF file` (14 bytes).
ZDR (`zdr:true`, same PDF): `data.markdown == "Dummy PDF file"`, `formats` has no artifact ids, `GET /v1/artifacts`
count 0 before and after, Redis db 6 `quarry:page:*` count 56 before and 56 after.
Not run: 20 MB PDF timing (f), crawl with a PDF link and a PNG link (e) (see crawl below), `html` format inline under ZDR
(`data.html` was absent for `formats:["markdown","html"]`; that is WS5.D, not on the branch).

## Slice WS6.I (metrics listener): VERIFIED, one wording nit
`METRICS_ENABLED=1` + 48-hex token, production. From a `curlimages/curl` container on `qv2v-net` (stand-in for
`inter-plane-bus`; the listener is not published, `docker port qv2v-edge` shows only 8082):
no header -> 401; wrong token -> 401; right token -> series
`quarry_http_requests_total{route="scrape",method="POST",status_class="2xx"} 4`, `quarry_scrape_outcomes_total{outcome="ok"} 4`,
`quarry_lease_pool_{in_flight,capacity=2,...}{pool="chromium"}`, `quarry_build_info{version="0.1.0"} 1`; after one
browser-rendered scrape `quarry_lease_pool_acquired_total{pool="chromium"} 1`, `peak_in_flight 1`.
Grep of the whole body for `example.com|w3.org|dummy.pdf` -> 0.
Nit: the public port answers **401** to `/metrics` without a bearer (the auth layer runs before routing, so any unknown path
is 401) and **404** with a valid user bearer. RUNBOOKS/plan say 404; both deny it.

## Slice WS2.A (raw page script denied): VERIFIED
Agent run on the rebuilt edge (dev env, hardened container), `navigate` to example.com -> 200, then
`{"action":{"type":"evaluate","script":"1+1"}}` -> `403 FORBIDDEN "raw page script execution (evaluate) is not permitted; use typed browser actions"`,
`details {"denied_action":"evaluate","reason":"raw_script_denied"}`. Through the HTTP step route only; the other entry points are unit-tested only.

## Slice WS6.A (crawl bounds): VERIFIED
`POST /v1/crawl {"url":"https://example.com/","max_pages":999999,"max_depth":99}` -> 200
`limits: {"max_pages":{"requested":999999,"effective":5000,"clamped_by":["hard"]},"max_depth":{"requested":99,"effective":10,"clamped_by":["hard"]}}`;
no limits given -> `effective 1000 / 5, clamped_by ["default"]`.

## Slice WS4.K (edge page cache keyed by org): VERIFIED
Same URL scraped as org A (`eDJive...`) and org B (`rfDfmP...`): Redis db 6 holds two keys,
`quarry:page:rfDfmPhdN7YWC...:blake3:f26a1fdd...` and `quarry:page:eDJiveH1XUJWT...:blake3:f26a1fdd...`; org B's response
was its own fresh scrape (`change.status:"new"`).

## Slice WS4.A (gateway result cache org scope + ZDR bypass): NOT VERIFIABLE here
The plan's check drives `/api/v1/search/web` (needs a SearXNG or Brave backend; none in the isolated stack) and flips an
org's `interactiveRetention.zdr` in org-core metadata, which would mutate live org data. Not done. The branch gateway
ran with `GATEWAY_CACHE_REDIS_URL` pointing at the scratch Dragonfly (`gateway result cache connected content_cache:true`).

## Slice WS6.B (agent budgets): VERIFIED (some plan expectations do not hold)
Edge in the dev env (agent runs need a grant in production and WS1.A2 is not on the branch), `AGENT_DEFAULT_MAX_STEPS=5`.
- `BUDGET_ENFORCEMENT=on`: `POST /v1/agent/runs {"constraints":{"max_steps":0,"allowed_domains":["example.com"],"max_cost_usd":50},"initiated_by":"agent"}` ->
  `constraints {"class":"agent","enforcement":"on","max_steps":5,"max_runtime_s":900,"max_cost_usd":20.0,"estimated":true}`,
  `clamped [{max_steps default 5},{max_runtime_s default 900},{max_cost_usd requested 50 effective 20 by ceiling}]`.
  navigate + 4 screenshots -> 200 x5; the sixth and seventh -> `402 {"code":"BUDGET_EXHAUSTED","message":"agent run budget exhausted: steps (limit 5, used 5 count)","details":{"dimension":"steps","limit":5,"limit_source":"run_budget","resumable":false,"spent":5,"unit":"count"},"retryable":false}`.
  `GET /v1/agent/runs/{id}/receipts`: exactly 5 receipts, `cost_micro_usd` 1000 (navigate) and 5000 x4.
  Checkpoint row `state.budget = {"class":"agent","steps":5,"attempts":5,"active_ms":14743,"spent_micro_usd":21000}`, status `active`.
- Resume: `DELETE` the run, then `POST /v1/agent/runs` with `resume_run_id` -> 200, first step -> `402 BUDGET_EXHAUSTED steps used 5` at once.
- Human not capped: the same user without `initiated_by` -> `constraints {"class":"interactive"}`; navigate + 8 screenshots all 200.
- `BUDGET_ENFORCEMENT=shadow`, default 3 steps: navigate + 5 screenshots all 200, edge log once:
  `WARN agent run budget would be exhausted (shadow: not enforced) class=agent dimension=steps limit=3 spent=3`.
Does not hold: the plan's `state->>'budget_exhausted' = 'steps'` (no such key; the ledger is `state.budget`), `cost_source: flat_table`
on receipts and the `agent.budget_exhausted` SSE event (both listed as not done in the ledger).
Not tested: a service-principal token (none minted here); `initiated_by:"agent"` with a user token covers the class.

## Slice WS6.G (per-org pool permits): VERIFIED by serialisation, not by the 429
`POOL_ORG_MAX_PERMITS=1`, pool of 2: three concurrent browser-rendered scrapes of `https://httpbin.org/delay/9` as one org
finished at 11.4 s, 20.9 s, 30.4 s (one at a time; without the cap two would overlap and finish together). The
`org_render_cap` 429 was not reached because the wait budget is longer than a 9 s page.

## Slice WS6.J (drain): VERIFIED
Edge with one active run and its browser session, and an SSE stream open on `GET /v1/agent/runs/{id}/events/stream`
(keep-alive `:` frames received). `docker stop qv2v-edge` (stop timeout 45 s) took **31 s**, `State.ExitCode=0`, `OOMKilled=false`.
Log: `shutdown: SIGTERM received`, `drain: no longer accepting new runs, crawls or batches`, after 30 s
`drain: deadline reached with connections still open; closing them deadline_s=30`,
`drain: browser sessions released released=1 failed=0 in_use=0`, `local index: flushed on shutdown`. The SSE client got curl
exit 18 when the stream was closed. After the restart `POST /v1/agent/runs` with `resume_run_id` of the non-ZDR run -> 200; a ZDR
run has no row in `quarry_agent_run_checkpoints` (count 0). With no open stream the stop took 1.1 s, exit 0, `released":2`.
Not measured: the pre-slice baseline (exit 137); no `agent.expired` event (known not done).

## Slice WS6.H (job admission) and WS6.K (job statistics): VERIFIED
Control `qv2v/quarry-control:dd4bc8b7` (migrations 019 and 020 visible: `claimed_until` and `statistics` in `\d jobs`), orchestrator
`qv2v/quarry-orchestrator:dd4bc8b7`, edge, own Temporal and Postgres. `QUARRY_JOB_ADMISSION=on`, cap 1, backlog 2, TTL 60 s.
Round 1: org A crawl x3 and org B x1, then A's fourth:
`A1 running, A2 accepted, A3 accepted, B1 running` (B runs at once, own cap). The fourth A submission ->
`429 RATE_LIMITED "this organization has too many jobs waiting; try again shortly" details {"limit":2,"queued":2,"reason":"org_backlog_full","retry_after_seconds":30,"scope":"org"}`,
no job row. `claimed_until` is set on claimed rows (the orchestrator used the claim endpoint). 60 s later A2 and A3 became `failed` with
`params.failure_reason = backlog_expired` (12:22:57) while A1 was still running.
Round 2 (cap 1, backlog 100, TTL 10 m): 40 org A jobs then one org B job: B1 `completed` while A still had `accepted` rows
(A's jobs ran one at a time). No starvation.
Statistics: crawl of quotes.toscrape.com, 5 pages -> `{"drivers":{"static":5},"duration_ms":11994,"pages_failed":0,"pages_visited":5,"status_classes":{"2xx":5}}`;
a batch of `[example.com, example.com/does-not-exist..., no-such-host....invalid]` ->
`{"drivers":{"static":2},"duration_ms":16424,"pages_failed":1,"error_classes":{"server_5xx":1},"pages_visited":2,"status_classes":{"2xx":1,"4xx":1}}`
(the unresolvable host is an edge `500 INTERNAL "dns lookup failed"`, hence `server_5xx`: inside the vocabulary, imprecise).
Control ran with `QUARRY_INTERNAL_HMAC_REQUIRED=0`: see F1.

## Slice WS6.F (gateway rate classes): VERIFIED
Branch gateway `qv2v/verevon-gateway:dd4bc8b7` (`2ea1b8cb18cc`) with `GATEWAY_RATE_LIMIT_RPM_BROWSER_START=3` and
`GATEWAY_CACHE_REDIS_URL` = the scratch Dragonfly, signed in as org A (`local@verevon.dev`) and org B (`e2e@verevon.dev`):
`POST /api/v1/browser/sessions` x4 per org: 1 to 3 passed the limiter (they returned `502 upstream_unavailable` because the edge
panicked on example.com at that time, F2), the fourth -> `429 Too Many Requests`, `retry-after: 17` (org A) and `18` (org B), gateway
log `inbound rate limit exceeded key_source=org`. Meanwhile org A `GET /api/v1/browser/sessions/{run}`, `/frame`, `/timeline` -> 200 x3.
Dragonfly keys: `verevon:gw:ratelimit:browser_start:org:eDJive...`, `verevon:gw:ratelimit:browser_start:org:rfDfmP...`,
`verevon:gw:ratelimit:interactive:org:eDJive...`, `verevon:gw:ratelimit:anonymous`. No `verevon:gw:ratelimit:org:*` default key
appeared in this flow.
Deviation: the plan wants the first creates to return 200 and `run_action` to stay 200 on a session made through the gateway; the
creates were refused upstream, so the 200s are GETs on a run created directly on the edge.

## Findings and fixes on the branch
Commits added to `quarryv2` by this run:
- `37f9d278` WS1.A0: production overlay, Dockerfile artifact dir, RUNBOOKS.
- `edbf13ce` quarry-transform: PDF text through ToUnicode (WS5.B produced control characters on a real PDF).
- `3269c377` quarry-runtime: DOM text snippet cut on a char boundary (F2).
- `b3ec826c` quarry-runtime: Redis password no longer logged (F3); RUNBOOKS /metrics wording.

F1 (not fixed; pre-existing, affects the production overlay): the edge does not sign its calls to quarry-control
(`crates/quarry-edge/src/handoff.rs` `forward_to_orchestrator`; `quarry_runtime::publisher` posts events with a bearer only). The
production overlay sets `QUARRY_INTERNAL_HMAC_REQUIRED=1` on control, so there `POST /v1/crawl` and `/v1/batch` answer
`502 UPSTREAM_BLOCKED "control 401: internal auth failed: missing HMAC headers"` and run events are dropped
(`non-retryable error posting event batch - discarding ... 401 Unauthorized`). The live tree has no signing there either; the live
stack never noticed because it runs `ENVIRONMENT=dev` and `HMAC_REQUIRED=0`. Needs an owner: sign these two calls the way
`resource_routes.rs` already signs its GETs.
F2 (fixed, `3269c377`): pre-existing panic at `observation.rs:2144` (`&trimmed[..500]` inside a multi-byte character) on the first
`navigate` to example.com in the hardened container; it killed the request (the gateway saw 502). Reproduced twice, gone on the new image.
F3 (fixed, `b3ec826c`): `ProfileStore Redis cache attached url=redis://:<password>@...` logged the Dragonfly password at INFO.
F4 (observation): `ZDR` with `formats:["markdown","html"]` returns inline markdown only; `html` is absent (WS5.D, not on the branch).
F5 (observation): the plan's WS1.A1 evidence expects an `AcquireGrant` line in the broker log; the broker logs nothing per call.

## Final images (branch HEAD `b3ec826c` plus the docs-only commit `c0b7abb7`)
| image | id | note |
|---|---|---|
| qv2v/quarry-edge:b3ec826c (`:latest`) | 7960c594bae3 | `GET /version` revision b3ec826c; final smoke on the production config: `/ready` `{"durable":true,"status":"ready"}`, html + browser scrape 200, ZDR PDF inline text, 0 `--no-sandbox`, Redis log line `redis://***@qv2v-redis:6379/6`, 0 occurrences of the password |
| qv2v/quarry-edge:3269c377 | 12568453076b | image used for the budget, drain, pool, WS2.A, WS6.A, WS4.K runs |
| qv2v/quarry-edge:edbf13ce | c2421b96bfe5 | PDF fix, WS5 and metrics runs |
| qv2v/quarry-edge:37f9d278, :dd4bc8b7 | 782656b0ec18, 2e9ed447608f | WS1.A0 first run (artifact dir defect) and the pre-fix image |
| qv2v/quarry-control:dd4bc8b7 | d3d6fbb224fe | |
| qv2v/quarry-orchestrator:dd4bc8b7 | d012ff60c633 | |
| qv2v/browser-broker:dd4bc8b7 | 2f9adbef56f0 | |
| qv2v/model-gateway:dd4bc8b7 | c87f5762100b | its `/version` shows the live container's SOURCE_REVISION because the env was copied from the live container; the image id is the evidence |
| qv2v/verevon-gateway:dd4bc8b7 | 2ea1b8cb18cc | `/version` revision dd4bc8b7 |
Only the edge changed after `dd4bc8b7`; the Go, model-gateway and gateway images are from `dd4bc8b7` and the later commits do not touch them.

## Status per slice
VERIFIED: WS1.A0 (after a fix), WS1.A1, WS2.A, WS3.A, WS4.K, WS5.A/B (after a fix), WS6.A, WS6.B, WS6.F, WS6.H, WS6.I, WS6.J, WS6.K.
VERIFIED, partly: WS6.G (serialisation seen, the `org_render_cap` 429 not reached).
NOT VERIFIABLE: WS4.A (needs a search backend and a change to a live org's metadata).
NOT RUN: WS5.A/B items (e) crawl with PDF and PNG links, (f) 20 MB PDF timing; the Model Plane compose change for the broker's
`inter-plane-bus` alias (needs recreating the live broker); the pre-slice drain baseline.
FAILED then FIXED on the branch: WS1.A0 artifact volume ownership; WS5.B PDF text.
Teardown: the `qv2v-*` containers, the volumes `qv2v-edge-*`, and the network `qv2v-net` were removed; the `qv2v/*` images were kept.

## Follow-up run: F1 fixed and verified
Commit `8b4b7de1` signs the crawl/batch handoff (`handoff.rs`: the exact body bytes and the path with its `org_id` query) and run-event
publishing (`quarry_runtime::EventPublisher::with_signer`, signing on every attempt so a retry gets a fresh nonce; `main.rs` builds the
`InternalSigner` once and shares it with `AppState`). Unit tests: signature recomputed over the received body and query (wiremock),
no headers without a signer, publisher passes method/path/body to the signer (`cargo test -p quarry-runtime --lib publisher`, `-p quarry-edge --lib handoff`: pass).
Rebuilt edge image `528126e6e13a` (rev `8b4b7de1`) on the production overlay config against control with
`QUARRY_INTERNAL_HMAC_REQUIRED=1` ("HMAC cross-plane auth: ENFORCED"): `POST /v1/crawl` -> 200 and the job `completed pages=4`;
`POST /v1/batch` -> 200, `completed pages=2`; single scrape 200; `events` table 72 rows, 0 occurrences of `401 Unauthorized`/`missing HMAC`
in the edge log (before the fix: `502 UPSTREAM_BLOCKED control 401 missing HMAC headers` and discarded event batches).
Also `unsupported_media_message` ("an image file", test passes; not rebuilt into an image, trivial string change).
Crawl of `rfc-editor.org/rfc/rfc2616` (12 pages, DOC_ROUTING=on) fetched only HTML (`content_kind` html x18), so item (e) (PDF and PNG links inside a crawl) is still not shown.
