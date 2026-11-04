# P0-preflight

- Leg: P0-preflight (runbook section 1 steps 1 and 5, section 4 row DATAPLANE-OPTION-B), adapted to the qv2e stack.
- Time window UTC: 2026-10-28 16:57 to 16:59.
- Image revision: `GET /version` returned `{"build_date":"2026-10-28T16:14:47Z","cargo_version":"0.1.0","revision":"qv2e-bd61260ee","service":"quarry-edge-rs"}`.
- Flags changed: none. The flag file was identical before and after the leg (diff clean). No restart. `flags-log.md` has no new line.
- Auth used for the dev calls: `Authorization: Bearer dev-evidence`.

## Results

| id | what | result | evidence |
|---|---|---|---|
| P0.a | every service healthy or running | PASS | `qv2e.sh ps`: control, dragonfly, edge, nats, orch, pg, temporal, temporal-pg all `(healthy)`. fixture-a and fixture-b are `Up` (they have no healthcheck). 10 of 10. |
| P0.b | edge `/health`, `/ready`, `/version` | PASS | `/health` 200 `ok`. `/ready` 200 `status: ready`, `durable: true`. Both browser pools `state: idle`, slot 0 `status: starting`, `healthy: null`, 0 sessions (there is no browser broker in this stack). `/version` 200, revision `qv2e-bd61260ee`. |
| P0.c | flag defaults inside the running edge | PASS | `docker exec qv2e-edge env` shows: SCRAPE_ACTIONS=off, SCRAPE_CAPTURE=off, CRAWL_ROBOTS=observe, ROBOTS_UNREACHABLE=disallow, BUDGET_ENFORCEMENT=shadow, AUTOPROMPT=0, SEMANTIC_RERANK=0, LLM_CLASSIFY_INTENT=0, PAGE_TITLE_ENRICH=0, MODEL_PLANE_URL empty, PROFILE_OWNER_ENFORCE=observe. TAKEOVER_ENFORCEMENT is not set (code default). This equals the runbook section 1 step 5 list. Also not set: CRAWL_SCOPE_MODE, RECEIPT_TEXT_MODE, AGENT_DEFAULT_MAX_STEPS, DOC_ROUTING. |
| P0.d1 | how `/metrics` is served | PASS | `crates/quarry-edge/src/metrics.rs` lines 9 to 15: a separate listener on `QUARRY_EDGE__METRICS_ADDR` (not set here, so the default `0.0.0.0:9092`), bearer `QUARRY_EDGE__METRICS_TOKEN`. The port is not published to the host. The public port 38082 answers `GET /metrics` 401 with no bearer and 404 with the dev bearer (it is not a public route). |
| P0.d2 | metrics curl shape recorded | PASS | Shape for all legs (token never printed, it is read inside the container from its own env): `docker exec qv2e-edge sh -c 'wget -q -O - --header="Authorization: Bearer $QUARRY_EDGE__METRICS_TOKEN" http://127.0.0.1:9092/metrics'`. Check: wrong bearer gives 401, the container token gives 200 with 10769 bytes. Sample saved as `P0-preflight-metrics.log` (no secrets). |
| P0.d3 | metric families present | PASS | Families are created at the first increment, so a family that nothing has hit yet is absent. At 16:58: `quarry_robots_decisions_total` present (5 series, all 0); `quarry_agent_budget_exhausted_total`, `quarry_agent_takeover_exempt_steps_total`, `quarry_profile_owner_decisions_total`, `quarry_profile_owner_stamps_total` absent (0 series). All four are defined in `metrics.rs` (lines 177, 257/277, 337, 406, 427) so the later legs must read "absent" as 0 before the first event. Other families present: quarry_scrape_outcomes_total (ok=7), quarry_http_requests_total, quarry_lease_pool_*, quarry_robots_cache_entries, quarry_agent_runs_active/pending, quarry_build_info, quarry_edge_draining. |
| P0.e1 | plain scrape of fixture-a | PASS | `POST /v1/scrape {"url":"http://fixture-a.crawl.test/","formats":["markdown"]}` returned 200, `status` 200, title "Fixture A", driver `static`. The markdown artifact `GET /v1/artifacts/<id>` returned 200 with the text "Fixture A" and the page links. The fixture-a nginx log shows the fetch (`GET / 200 410`, user agent `Quarry/2.0`), so it was a live fetch, not a cache hit. |
| P0.e2 | public egress `https://example.com/` | PASS | `POST /v1/scrape` returned 200, title "Example Domain", driver `static` 93 ms. The edge has public internet egress. |
| P0.f1 | DATAPLANE-OPTION-B: answer routes with `MODEL_PLANE_URL` empty | PASS | `POST /v1/answer` 501 `{"code":"UNSUPPORTED","error":"answer pipeline not configured","hint":"configure a SearchProvider AND a Model Plane URL to enable /v1/answer"}`. `POST /v1/answer/stream` 501 with the same body. The boot log says `answer pipeline disabled (search or model_plane_url unset)`. |
| P0.f2 | no Model Plane token mint or call in the edge log | PASS | `docker logs qv2e-edge` (111 lines, boot to 16:58:41): grep for model plane, model_plane, mint, model-gateway gives 1 line, and it is the INFO line above (a status message, not a call). 0 mints, 0 calls. |
| P0.g | stack facts written for later legs | PASS | "P0 notes" section appended to `README.md`. |

## Depth

1. **`/metrics` is not on the published port.** The README line "`GET /metrics` answers 401 without the metrics token" is true for port 38082 only because the auth layer runs before the router. The real endpoint is the internal listener on 9092 inside the container. Legs that read counters must use the `docker exec` shape above. A plain `curl 127.0.0.1:38082/metrics` with any bearer gives 404 and cannot read counters.
2. **Counters appear lazily.** A leg that expects `quarry_agent_budget_exhausted_total`, `quarry_agent_takeover_exempt_steps_total`, `quarry_profile_owner_*` must treat "no line" as 0 before the first event, and must not count the absence as a FAIL.
3. **A plain `/v1/scrape` does not touch the robots counters.** After 7 successful scrapes all five `quarry_robots_decisions_total` series are still 0, and the fixture log shows no `/robots.txt` fetch for the scrape of `/` at 16:58. Robots decisions come from crawl and map only. The X-10 leg should not expect scrape traffic to move them.
4. **Boot WARN about the profile tombstone sweep is not in this edge's log.** The lead's smoke note listed `profile tombstone sweep failed` (INTERNAL). The current boot log (from 16:55:32) has only the BrowserBroker WARN and the dev-bypass WARN lines. Possibly the edge was restarted after the smoke test, or the sweep ran before. The D16 leg should check again.
5. **ERROR lines in the edge log.** There are 3 ERROR lines at 16:58:41 (`tower_http::trace::on_failure`, status 501) for the answer calls made by this leg. They are the expected 501 traces, not faults. Later legs that count ERROR lines should subtract them.
6. **Dev-bypass WARN per request.** Every call with `Bearer dev-evidence` logs two WARN lines (`failed to decode JWT header`, then the dev-bypass fallback). Expected; do not read it as an auth fault.
7. **Offhost name.** `offhost.example.net` does not resolve from inside qv2e-edge (`wget` exit 4), so the ADR-6 leg's "egress does not allow offhost" condition holds without any change.

## Not run

- Nothing from the P0 scope was skipped.
- Out of scope for this leg and not touched: `docker compose config` against the repo compose files (the stack uses `qv2e-compose.yml`; the live env files must not be read), and `browser-grant-prod-contract.sh --preflight-only` (runbook step 6; needs the repo root and is not in the P0 brief).
