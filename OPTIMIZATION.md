# Optimization & Code Quality Guide

A consolidated reference of essential performance optimization and clean code techniques, applied to the Triodelab CoreSystem.

---

## Checklist

### Performance

| # | Technique | Status | Notes |
|---|---|---|---|
| 1 | **Profile first** | ✅ Done | pprof live on user-core (:6060), org-core (:6061), billing-core (:6062) under real traffic — heap, goroutine, block, allocs, mutex all collected |
| 2 | **Algorithm selection** | ✅ Done | DB indexes replace linear scans; `scanOrg` DRY helper replaces repeated O(n) scans |
| 3 | **Data structure selection** | ✅ Done | `sync.Map` session cache in user-core; Redis cache-aside across all services |
| 4 | **Database query reduction** | ✅ Done | `SetDefaultEntitlements` N→1 (pgx.Batch); `UpdateUser` pre-fetch removed; all indexes added |
| 5 | **Implement caching** | ✅ Done | Redis in user-core, org-core, billing-core; metric usage 30s; session 30s; **imports-core quota 60s** |
| 6 | **Loop optimization** | ✅ Done | Entitlement batch; ListOrganizations LIMIT; `run_job` serial loop → `asyncio.gather()` |
| 7 | **Memory management** | ✅ Done | pgxpool MaxConns tuned; SQLAlchemy pool tuned; `make([]Org, 0, limit)` pre-allocated slice |
| 8 | **Async / concurrency** | ✅ Done | Shared `httpx.AsyncClient` in imports-core; `run_job` serial loop → `asyncio.gather()` + `Semaphore(5)` |
| 9 | **Compiler optimization** | ✅ Done | Go: `-ldflags="-s -w" -trimpath` on all services |
| 10 | **Use libraries** | ✅ Done | pgx, zerolog, fiber, SQLAlchemy, httpx — no reinventing the wheel |

### Code Quality

| # | Technique | Status | Notes |
|---|---|---|---|
| 11 | **DRY principle** | ✅ Done | `shared-entrypoint.sh`; duplicate `ImportService` removed; `scanOrg()` helper extracted |
| 12 | **Remove unused code** | ✅ Done | Dead `ImportService` stub removed; 6× `await Promise.resolve()` no-ops removed from auth-core; `go mod tidy` |
| 13 | **Short, single-purpose functions** | ✅ Done | `scanOrg` extracted from 4 functions; `UpdateUser` simplified; `GetQuotaStatus` refactored |
| 14 | **Meaningful naming** | ✅ Done | zerolog replaces stdlib `log.Printf` in billing-core + user-core; constants commented |
| 15 | **Remove dead imports** | ✅ Done | `go mod tidy` run on all 3 Go modules; dead `uuid` dep removed from org-core |

---

## Open Items

| Priority | Item | Effort |
|---|---|---|
| � Low | **auth-core Node.js profiling** — use `0x` or `clinic` for flame graph under session-heavy load if latency regressions appear | Medium |

---

## What Was Done (Summary)

### Control Plane — Round 2 (this pass)
- Fixed auth-core rate-limit `storage: 'database'` → `'secondary-storage'` (Redis) — was hitting `rate_limit` table on every request
- Enabled session cookie cache by default in auth-core (`SESSION_COOKIE_CACHE_ENABLED=false` to opt out)
- Removed 6× dead `await Promise.resolve()` no-ops from auth-core hot paths
- `SetDefaultEntitlements` N individual SQL Exec calls → single `pgx.Batch` round-trip
- `ListOrganizations` now paginated with `LIMIT/OFFSET` (default 100, max 500 guard)
- Extracted `scanOrg()` helper — eliminated ~50 lines of duplicated scan code across 4 repository functions
- `GetQuotaStatus` metric usage now cached 30s in Redis (`billing:usage:{org}:{metric}`)
- `retry_processor.go` stdlib `log.Printf` → zerolog structured logging
- `BetterAuthClient` 3× verbose `log.Printf` → zerolog Debug/Warn; added `GetUserCached()` with 30s in-process `sync.Map` cache
- `go mod tidy` on billing-core (zerolog v1.34.0 pulled), org-core (uuid removed), user-core (clean)

### Ingestion Plane — Round 2 (this pass)
- `check_quota` 60s in-process TTL cache — repeated upload calls skip org-core HTTP entirely
- `run_job` serial `for` loop → `asyncio.gather()` + `Semaphore(5)` + `asyncio.Lock()` for thread-safe counters
- Validated: quota cache logic (4 unit tests); concurrency correctly bounded at max=5 across 20 tasks
- Replaced per-request `httpx.AsyncClient` → shared pooled client
- Tuned SQLAlchemy pool (size 5→3, overflow 10→2, added recycle/timeout)
- Added DB indexes: imports-core `002`
- Added non-root `appuser` to both Dockerfiles
- Bumped Quarry alpine `3.20→3.21`
- Added `init: true` for Chromium zombie reaping
- Added log file size limits across all app services
### pprof Live Profiling (this pass)
- Instrumented user-core (:6060), org-core (:6061), billing-core (:6062) with `net/http/pprof` — opt-in via `PPROF_ENABLED=true`
- Exposed debug ports in docker-compose; wrote `pprof-profile.sh` load + collect script
- **Findings**: heap 1.5MB each (validator init + NATS/Redis connection buffers) — no leaks; block 100% idle waiters (pgxpool health ticks, retry processor sleep) — no pathological contention; CPU unsampled because all handlers complete <2ms (sub-sample-tick) — confirms services are not CPU-bound
