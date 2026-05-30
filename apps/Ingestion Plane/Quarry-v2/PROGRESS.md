# Quarry-v2 Progress Log

## Gap Closure Status

| Gap # | Description | Status | Closed |
|-------|-------------|--------|--------|
| 2 | Donor scrape removal (V1 hot path) | ✅ Closed | 2026-05-05 |
| 5 | Browser action schema in edge API | ✅ Closed | 2026-05-04 |
| 7 | `/v1/map` endpoint | ✅ Closed | 2026-05-04 |
| 8 | LLM `summary`/`query`/`json` formats | ✅ Closed | 2026-05-04 |
| 14 | `zeroDataRetention` option | ✅ Closed | 2026-05-04 |
| 17 | Browser path SSRF via chromiumoxide (DnsGuard not called) | ✅ Closed | 2026-05-05 |

## 2026-05-05

### Gap #2 — Donor scrape removal (V1 hot path)

**Closed.**

- `AppState` no longer carries any donor URL field
- The scrape handler routes 100% through `PageRunner::run()`; no donor code path remains
- `cargo check` passes clean (warnings only, no errors)
- Both occurrences of Gap #2 in `docs/GAP.md` updated to `Closed`

## Next Open Gaps (by priority)

| # | Gap | Priority |
|---|-----|----------|
| 1 | Eval harness + warm/cold benchmarks | P0 |
| 3 | Coverage CI gate (≥ 80 %) | P0 |
| 4 | Sitemap + robots.txt in crawl BFS | P1 |
| 6 | Mobile emulation + geo targeting (full fingerprint proof) | P1 |
| 9 | SDK (Python + TypeScript) | P2 |
| 10 | OpenAPI spec | P2 |
