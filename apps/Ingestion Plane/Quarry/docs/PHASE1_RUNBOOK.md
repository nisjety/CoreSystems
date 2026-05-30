# Quarry Phase 1 Operational Runbook

## Scope
This runbook covers the Phase 1 API surface (`/health`, `/ready`, `/v1/scrape`) and baseline hardening/observability.

## Health checks
- `GET /health`: process-level health.
- `GET /ready`: dependency readiness (ai-core and Redis when configured).
- `GET /metrics`: baseline request/error/latency telemetry.

## Common incidents
### 1) `401 invalid or missing API key`
- Verify `QUARRY_API_KEY` and `QUARRY_API_KEY_HEADER`.
- Confirm caller sends the header expected by the API.

### 2) `429 rate limit exceeded`
- Check `RATE_LIMIT_MAX` and `RATE_LIMIT_WINDOW_SEC`.
- Verify caller retry/backoff behavior.

### 3) `503` on `/ready`
- If `ai_core` is `unreachable`: verify `AI_CORE_GRPC_ADDR` and ai-core process status.
- If `redis` is `unreachable`: verify `REDIS_URL`, network route, and Redis service health.

### 4) scrape blocked by security
- Review security payload in API response.
- Tune `SECURITY_BLOCK_THRESHOLD` only with explicit risk acceptance.

## Graceful shutdown
- SIGTERM/SIGINT triggers graceful shutdown in `cmd/api/main.go`.
- Active requests get up to 10s to complete.

## Logging fields to watch
- `event=auth_denied|auth_granted`
- `event=rate_limit_exceeded`
- `request_id`, `path`, `ip`

## Escalation checklist
- Capture request ID and timestamp.
- Capture `/ready` and `/metrics` output.
- Capture relevant env values (without secrets).

## Module API (Phase 2 Day 5)

### Endpoints
- `GET /v1/modules`
	- Returns registered module names and default module.
	- Example response:
		- `{"success": true, "modules": ["multi", "quick", "seo"], "default": "multi"}`
- `POST /v1/scrape`
	- Supports optional `module` in request body.

### Request field
- `module` (optional): selects scraping strategy.
	- `quick`: single-page fast pass (`maxPages=1`, no enrichment)
	- `seo`: SEO-oriented crawl (`maxPages>=3`, no enrichment)
	- `multi`: multi-page enriched crawl (default)

### Compatibility behavior
- If `module` is omitted, Quarry uses `multi`.
- If `module` is unknown, Quarry falls back to `multi`.
- Existing clients without `module` continue to work unchanged.
