# Phase 0 — Frozen Contracts

Authoritative across Rust + Go. No plane may invent new IDs or envelopes.

## 1. ID scheme

All IDs: ULID (Crockford base32, 26 chars). Prefix = resource type.

| Prefix    | Resource            |
|-----------|---------------------|
| `run_`    | execution run       |
| `queue_`  | request queue       |
| `cp_`     | checkpoint          |
| `sch_`    | schedule            |
| `store_`  | named store         |
| `snap_`   | snapshot            |
| `art_`    | artifact            |
| `lease_`  | browser lease       |
| `prof_`   | browser profile     |
| `job_`    | durable job (wf)    |
| `evt_`    | event record        |
| `whk_`    | webhook delivery    |

Rust: `quarry_core::ids::Id<Kind>` newtype.
Go: `quarrycontracts.ID` with kind tag.

## 2. REST resource envelope

```json
{
  "data": <resource | [resource]>,
  "meta": {
    "request_id": "req_...",
    "page": { "cursor": "...", "next": "...", "limit": 50 }
  },
  "error": null
}
```

Error path:

```json
{
  "data": null,
  "meta": { "request_id": "req_..." },
  "error": {
    "code": "SECURITY_BLOCKED",
    "message": "Host on blocklist",
    "details": { "host": "example.com" },
    "retryable": false
  }
}
```

### Request id (`x-request-id`, WS10.S1)

Every Quarry response and every gateway response carries an `x-request-id` header. A valid inbound id is adopted: 1 to 128 bytes from `[A-Za-z0-9._:-]`. Anything else (overlong, empty, spaces, control characters, non-ASCII) is replaced by a server-minted `req_<ULID>` (`req_<uuid>` in quarry-control). A bad id is never a 400. The envelope `meta.request_id` equals the header on every envelope response, success and error alike.

The id is a support-lookup key only. It is not unique across tenants, so it is never an authorization, tenancy or idempotency key; support lookups filter by `(org_id, request_id)`. Operator switch: `QUARRY_EDGE__REQUEST_ID_ADOPT=0` mints every id.

## 3. Event envelope (SSE + webhook + durable history)

```json
{
  "event_id": "evt_...",
  "run_id": "run_...",
  "job_id": "job_...",
  "type": "page.fetched",
  "ts": "2026-04-22T10:15:00.123Z",
  "seq": 42,
  "payload": { ... },
  "idempotency_key": "..."
}
```

### Event types (frozen list)

- `run.started`, `run.paused`, `run.resumed`, `run.cancelled`, `run.completed`, `run.failed`
- `page.queued`, `page.fetched`, `page.failed`, `page.blocked`, `page.retried`, `page.escalated`
- `driver.planned`, `driver.fallback`, `transport.probed`
- `action.started`, `action.completed`, `action.failed`
- `preview.screenshot`, `preview.markdown`, `preview.artifact`
- `agent.started`, `agent.delta`, `agent.completed`, `agent.failed`, `agent.expired`, `agent.budget_exhausted`
- `artifact.written`, `snapshot.created`, `store.record.written`
- `lease.acquired`, `lease.released`, `profile.restored`, `profile.captured`
- `change.detected`, `change.unchanged`
- `schedule.fired`

#### Job backlog refusal

`POST /v1/crawl` and `POST /v1/batch` answer `429 RATE_LIMITED` with `error.details = {reason: "org_backlog_full", scope: "org", limit, queued, retry_after_seconds}` when job admission is on and the organization already has `limit` jobs waiting. No job is created. A job that waits longer than the backlog TTL ends with `run.failed` (`payload.reason = "backlog_expired"`).

#### Run statistics on a terminal event

`run.completed` and `run.cancelled` of a **crawl** or **batch** run carry `payload.statistics`, and control copies it onto the job, so `GET /v1/jobs/{id}` returns it as `statistics` (absent while the job runs, and for job kinds that do not report it):

```json
{ "pages_visited": 5, "pages_failed": 1, "duration_ms": 8423,
  "status_classes": { "2xx": 4, "4xx": 1 },
  "drivers": { "static": 4, "browser": 1 },
  "error_classes": { "client_4xx": 1 } }
```

Every key comes from a closed vocabulary: status classes `2xx`..`5xx` or `other`; drivers `static`, `browser`, `tls` or `other`; error categories `network`, `timeout`, `server_5xx`, `client_4xx`, `validation`, `blocklist`, `auth` or `unknown`. It never carries a URL, a title, a fingerprint or an error message. Empty breakdowns are omitted, and control drops a `statistics` value that is not an object or is over 4 KiB. A crawl is refused under ZDR, so no ZDR job has statistics.

### Agent run trace: `seq`, replay and resume (WS10.S3, built, NOT run)

An open agent run has its own trace. Its events are numbered densely per run from 1 (the `AgentStarted` of a run is `seq` 1). The number is minted together with the event id, under one lock that also writes the event into the run's replay ring and into the live channel, so a resume that replays and then tails sees no gap and no duplicate.

- The SSE `id:` of `GET /v1/agent/runs/{run_id}/events/stream` is the event's `seq`. Resume with `Last-Event-ID` (or `?from_seq=`, which wins). Replay returns the retained events with a greater `seq`.
- `replay_gap` is sent first when the resume point is older than the ring: `{requested, oldest_retained, fallback: "browser-session/timeline"}`. The durable timeline covers the older part.
- `done` with `run_closed` follows the terminal event when the run closes.
- The envelope's `type` field is the snake_case serde name (`agent_started`, `action_started`). The SSE event name is the dotted name the App Shell dispatches on (`agent.started`, `action.started`); both spellings are in use, and the dotted list above is the SSE name, not the envelope value.
- The NATS subject receives the same event id and seq as the SSE path (the transport is handed the minted event). A ZDR event never reaches the durable HTTP publisher or NATS; it stays in the in-memory ring for the run's lifetime and is never persisted.
- Runs that were never opened keep the global counter and have no replay.

Flags: `QUARRY_EDGE__TRACE_RING_EVENTS` (default 512; `0` disables the ring, so a resume that missed events gets `replay_gap`) and `QUARRY_EDGE__TRACE_RING_MAX_BYTES` (default 1 MiB, must be at least 1). An unparsable value aborts boot.

## 4. Webhook payload

```json
{
  "delivery_id": "whk_...",
  "signature": "sha256=<hmac>",
  "event": { <event envelope> }
}
```

HMAC over raw body using org webhook secret. Retries: exp backoff, max 24h, DLQ to `quarry-control`.

## 5. Artifact naming

`s3://<bucket>/org=<org>/run=<run_id>/page=<page_hash>/<kind>.<ext>`

Kinds: `html`, `md`, `raw`, `links.json`, `images.json`, `screenshot.png`, `screenshot_annotated.png`, `pdf`, `trace.zip`, `extract.json`, `summary.txt`, `attributes.json`, `branding.json`, `audio.json`, `change.json`, `visual_change.json`, `visual_observation.json`, `thumbnail.png`, `tiles.json`, `page_image_clean.png`, `ocr_preprocessed.png`, `logo_candidate.png`, `rendered_palette.json`, `chunks.json`, `meta.json`.

`page_hash = blake3(normalized_url + content_fingerprint)`.

## 6. Cache policy schema

```json
{
  "mode": "bypass | read_only | read_write | write_only",
  "max_age_s": 3600,
  "vary_on": ["url", "headers.accept-language", "render.js"],
  "stale_while_revalidate_s": 30
}
```

## 7. Run policy schema

```json
{
  "concurrency": { "per_run": 8, "per_domain": 2 },
  "delay": { "min_ms": 500, "max_ms": 2000, "jitter": true },
  "retry": { "max": 3, "backoff": "exp", "base_ms": 1000 },
  "proxy": { "strategy": "rotate | sticky | none", "pool": "residential_eu" },
  "robots": "strict | respect | ignore",
  "ordering": "fifo | priority | lifo",
  "block": { "on": "challenge", "action": "escalate | retry | abort" },
  "checkpoint": { "every_n_pages": 50, "every_s": 60 },
  "determinism": "strict | best_effort | off"
}
```

### Run policy donor notes

- Quarry V1 contributed the separation of immediate vs. scheduled execution, security scan, rate limits, and crawl robots behavior.
- Firecrawl contributed feature-gated engine fallback, per-team concurrency, proxy modes, cache age controls, and action wait-budget limits.
- Crawlee/Scrapy contributed adaptive per-host/session feedback: fast failures must not increase crawl rate; blocked sessions should be marked bad or retired.

## 8. Browser lease schema

```json
{
  "lease_id": "lease_...",
  "profile_id": "prof_...",
  "session_affinity_key": "...",
  "proxy_affinity": { "pool": "...", "sticky_key": "..." },
  "ttl_s": 1800,
  "capabilities": ["js", "screenshots", "pdf", "actions"],
  "artifact_bucket": "..."
}
```

### Browser profile snapshot schema

Additive profile/snapshot metadata. The binary snapshot body is an artifact referenced by `artifact_id`.

```json
{
  "snapshot_id": "snap_...",
  "profile_id": "prof_...",
  "created_at": "2026-04-29T12:00:00Z",
  "expires_at": "2026-05-29T12:00:00Z",
  "artifact_id": "art_...",
  "state": {
    "cookies": true,
    "local_storage": true,
    "session_storage": true,
    "indexed_db": true,
    "cache": false
  },
  "browser": {
    "engine": "chromium",
    "user_agent": "...",
    "viewport": { "width": 1280, "height": 800 },
    "locale": "en-US",
    "timezone": "Europe/Oslo"
  },
  "affinity": {
    "session_affinity_key": "...",
    "proxy_affinity": { "pool": "...", "sticky_key": "..." },
    "remote_reconnect_url": "..."
  },
  "validation": {
    "probe_url": "https://example.com/account",
    "last_success_at": "...",
    "status": "valid | stale | expired | unknown"
  },
  "storage": {
    "backend": "postgres: quarry_profiles row (WS17.E1)",
    "row": "org_id=org_... profile_id=prof_...",
    "envelope": "bytea, NULL snapshot when encryption is on",
    "enc_v": 1,
    "key_id": "env:v1 | oc:<n> (oc is reserved for WS17.E2)"
  },
  "encryption": {
    "scheme": "aes-256-gcm (ring), envelope JSON with enc_v 1 and alg A256GCM",
    "dek": "one per profile, wrapped by the key provider, new DEK on every 256th stored save",
    "nonce": "base64 of 12 random bytes, one per seal",
    "aad": "\"quarry-profile\0v1\0\" || len32(org) || org || len32(profile) || len32(snapshot_id) || snapshot_id || enc_v (u16 BE)"
  }
}
```

Browserless-style session URLs (`connect`, `browserQL`, `stop`, reconnect URL) are secrets. They may be stored only in encrypted snapshot metadata or a secret store, never in public event payloads.

Production snapshot storage is the `quarry_profiles` Postgres table. With `QUARRY_EDGE__PROFILE_ENCRYPTION=on` the snapshot is an envelope and the plaintext column is NULL; the interim key provider is the edge environment (`QUARRY_PROFILE_MASTER_KEYS`, `QUARRY_PROFILE_MASTER_KEY_ACTIVE`). The Control Plane key provider (`org_core`, WS17.E2) is not built. The S3 archive store is plaintext and is not wired.

## 9. Normalized output envelope (runtime → edge/control)

```json
{
  "run_id": "run_...",
  "url": { "requested": "...", "final": "...", "canonical": "..." },
  "status": 200,
  "fetched_at": "...",
  "fingerprint": "blake3:...",
  "formats": {
    "html": { "artifact_id": "art_...", "bytes": 12345 },
    "markdown": { "artifact_id": "art_...", "bytes": 6789 },
    "raw": { "artifact_id": "art_..." },
    "links": [ { "href": "...", "text": "...", "rel": "..." } ],
    "images": [ { "src": "...", "alt": "...", "title": "...", "width": 1200, "height": 630 } ],
    "screenshot": { "artifact_id": "art_..." },
    "visual_observation": { "artifact_id": "art_..." },
    "visual_change": { "artifact_id": "art_..." },
    "pdf": { "artifact_id": "art_..." },
    "extract": { "artifact_id": "art_...", "schema_id": "..." },
    "summary": { "artifact_id": "art_..." },
    "attributes": { "artifact_id": "art_..." },
    "branding": { "artifact_id": "art_..." },
    "audio": { "artifact_id": "art_..." },
    "change": { "artifact_id": "art_..." }
  },
  "change": { "status": "new | changed | unchanged", "prev_fingerprint": "..." },
  "metadata": { "title": "...", "lang": "...", "content_type": "..." },
  "driver": { "kind": "static | browser | tls", "duration_ms": 1234 }
}
```

Today `formats` holds `html`, `markdown`, `raw`, `links`, `screenshot`, `pdf` and `extract` (`quarry_core::output::OutputFormats`); the other entries above are the planned shape.

### Requested formats schema

What `POST /v1/scrape` accepts today is in 9.1: `markdown`, `html`, `rawHtml`, `links` and `metadata`, where `links` and `metadata` change nothing because both are in every response. `screenshot` and `pdf` (WS13.4b) and `json`, `summary`, `question` and `highlights` (INNH-1, "Scrape AI formats" below) are accepted as strings or objects behind their flags, each refused with 400 `feature_disabled` at its field while its flag is off; the bare strings `json`, `question` and `highlights` are 400 `missing_field`. `reserved_format` no longer exists. Any other name, and any object whose `type` is not one of these six, is refused with 400 `unsupported_format`. The example and the rules below are the target design for the formats not built yet.

```json
{
  "formats": [
    "markdown",
    "html",
    "rawHtml",
    "links",
    "images",
    "summary",
    { "type": "json", "schema": {}, "prompt": "..." },
    { "type": "changeTracking", "modes": ["json", "git-diff"], "tag": "...", "schema": {}, "prompt": "..." },
    { "type": "screenshot", "fullPage": true, "quality": 90, "viewport": { "width": 1280, "height": 800 } },
    { "type": "attributes", "selectors": [ { "selector": "a", "attribute": "href" } ] },
    { "type": "query", "prompt": "..." },
    "branding",
    "audio"
  ],
  "only_main_content": true,
  "include_tags": ["main", "article"],
  "exclude_tags": ["nav", "footer"],
  "remove_base64_images": true
}
```

Rules:

- `changeTracking` requires `markdown`.
- At most one `screenshot` format.
- `json`, `summary`, and `query` may depend on control/lab LLM jobs, but runtime must store deterministic source artifacts first.
- `branding` requires browser/CDP-capable driver unless a future static CSS parser proves enough.
- `audio` is a specialty handler behind a feature gate.

### Action schema

```json
{
  "actions": [
    { "type": "wait", "milliseconds": 1000 },
    { "type": "wait", "selector": "#ready" },
    { "type": "click", "selector": "#accept", "all": false },
    { "type": "write", "text": "query" },
    { "type": "press", "key": "Enter" },
    { "type": "scroll", "direction": "down", "selector": null },
    { "type": "screenshot", "fullPage": true, "viewport": { "width": 1280, "height": 800 } },
    { "type": "scrape" },
    { "type": "executeJavascript", "script": "document.title" },
    { "type": "pdf", "format": "Letter", "landscape": false, "scale": 1 }
  ],
  "limits": {
    "max_actions": 50,
    "max_total_wait_ms": 60000
  }
}
```

Action results are written as artifacts and referenced from `meta.json`. Unsupported actions must return `UNSUPPORTED` rather than silently no-op.

### Driver plan schema

### TLS profile schema

The code implementation uses the rquest upstream package published as `wreq`. Profiles are semantic and portable; the first hot-path implementation owns its profile presets through `wreq::Emulation`. `wreq-util` profile catalogs stay behind explicit license review.

```json
{
  "tls_profile": {
    "mode": "auto | chrome | firefox | safari",
    "implementation": "wreq",
    "version_hint": "chrome_latest | firefox_latest | safari_latest",
    "boringssl": true,
    "probe": {
      "enabled": false,
      "endpoint": "https://tls.peet.ws/api/all",
      "artifact_id": "art_..."
    }
  }
}
```

`auto` defaults to Chrome-family emulation unless policy, host history, or target corpus scoring chooses otherwise. User-agent overrides are allowed only as explicit request/config fields and must not silently downgrade the TLS profile to plain `reqwest`. The first `wreq` adapter disables redirects explicitly; redirect-following can be enabled only after each hop is re-run through SSRF/DNS preflight.

### Driver plan schema

```json
{
  "requested": "static | tls | browser | auto",
  "chosen": "static | tls | browser | document | specialty | index",
  "fallbacks": [
    {
      "driver": "tls",
      "reason": "needs browser-like TLS/H2 fingerprint",
      "unsupported_features": []
    },
    {
      "driver": "browser",
      "reason": "actions require CDP",
      "unsupported_features": ["audio"]
    }
  ],
  "signals": {
    "cache_hit": false,
    "requires_js": true,
    "known_static": false,
    "blocked_previous_attempt": false
  }
}
```

This is inspired by Firecrawl's engine waterfall but must be exposed as an inspectable runtime artifact.

### `/v1/map` request and response schema

`/v1/map` discovers URLs without returning full page content. It shares crawl policy, cache policy, TLS profile, profile, and denial-reason semantics with `/v1/crawl`, but has a lighter response envelope.

```json
{
  "url": "https://example.com/docs",
  "search": "install",
  "limit": 500,
  "max_depth": 3,
  "include_paths": ["/docs/**"],
  "exclude_paths": ["/admin/**"],
  "allow_external_links": false,
  "allow_subdomains": true,
  "ignore_sitemap": false,
  "sitemap_only": false,
  "ignore_robots_txt": false,
  "tls_profile": { "mode": "auto" },
  "cache": { "mode": "read_write", "max_age_s": 3600 }
}
```

```json
{
  "data": {
    "url": "https://example.com/docs",
    "links": [
      { "url": "https://example.com/docs/install", "title": "Install", "source_url": "https://example.com/docs", "depth": 1 }
    ],
    "denied": [
      { "url": "https://example.com/admin", "code": "exclude_pattern", "message": "Excluded by /admin/**", "source_url": "https://example.com/docs", "depth": 1 }
    ],
    "driver_plan": { "chosen": "tls", "fallbacks": [] }
  },
  "meta": { "request_id": "req_..." },
  "error": null
}
```

### Implemented bounded public discovery and audit evidence (2026-10-22)

The richer map example above is a design contract, not the current bounded
implementation. The current `POST /v1/map` accepts only `url`, optional `search`,
`limit`, and `include_subdomains`; it returns the plain object
`{url,links:[{url,title?,score?}],count,ranked}`. The `url` is the requested URL;
each returned link retains the URL actually discovered. Canonical apex/`www`
redirects can therefore produce `www` links for an apex request.

Both map and `POST /v1/audit/page` use static credentialless single-hop requests:
public HTTPS only, no cookies or proxy, public-address DNS resolution pinned
into the connection, 1 MiB decoded response limit, maximum three redirects,
and fresh URL/security preflight on each hop. Redirects stay on the original
origin or its exact apex/`www` pair with the same port. Sensitive paths,
credentials, queries, encoded paths, fragments, redirect loops, nested `www`
expansion and arbitrary subdomains are rejected. A redirect destination's own
robots rules are fetched before its document; unavailable/forbidden/rate-limited
robots cannot be silently treated as permission. Generic `/v1/scrape` and
`fetch_bounded` do not gain redirect permission.

`POST /v1/audit/page` requires authenticated `scrape:read` for service actors.
It permits at most eight in-flight captures per process and rejects excess
requests with 429 without queueing. This is a local transport capacity guard,
not durable per-organization billing or scheduled-job quota management.
It accepts exactly `{"url":"https://example.com/"}`. Its fixed 10 second deadline
includes robots and redirect requests. Successful transport returns a plain
`bounded_page_v1` object:

```json
{
  "version": "bounded_page_v1",
  "url": "https://example.com/",
  "final_url": "https://www.example.com/",
  "status": 200,
  "fetched_at": "2026-10-22T19:00:00Z",
  "run_id": "run_...",
  "fingerprint": "blake3:<64 hexadecimal characters>",
  "metadata": {"title": null, "description": null, "content_type": "text/html"},
  "canonical": null,
  "links": [],
  "headings": [],
  "partial": false,
  "limits": {"body_bytes": 1048576, "timeout_ms": 10000, "max_links": 100}
}
```

`metadata.title` and `description` describe the actual HTML title and standard
description tag, not Open Graph fallbacks. Oversized metadata fails rather than
becoming a fabricated missing-field finding. Links contain `{url,text?}` and
headings `{level,text}`; these lists are capped at 100 and 200 respectively,
with `partial=true` when the source exceeds a list cap. Capture requires HTML
for successful 2xx sources. An upstream 404/500 is evidence in `status`, not a
successful page; transport denial, size overflow and timeout remain non-200 API
failures. No raw HTML, artifacts, browser fallback, ingest, or durable storage
is produced by this route. Marketing Core owns interpretation, snapshots,
comparisons, prioritized findings and the work lifecycle.

### Real-time preview event payloads

Preview events are SSE-only by default and may be stored when `trace` or audit mode is enabled.

```json
{
  "type": "preview.screenshot | preview.markdown | preview.artifact",
  "payload": {
    "run_id": "run_...",
    "page_url": "https://example.com",
    "action_index": 3,
    "artifact_id": "art_...",
    "chunk": "partial markdown when small enough",
    "truncated": false
  }
}
```

### Agent config and event payloads

Gemma 4 local is the default model provider. OpenAI and Anthropic are additive provider integrations configured through CLI/control setup later.

```json
{
  "agent": {
    "provider": "gemma-local | openai | anthropic",
    "model": "gemma-4",
    "goal": "Find pricing tables and scrape them",
    "tools": ["scrape", "map", "click", "type", "screenshot"],
    "limits": { "max_steps": 25, "max_tokens": 8192, "max_runtime_s": 120 }
  }
}
```

Agent events use `agent.started`, `agent.delta`, `agent.completed`, `agent.failed`, and `agent.expired` (the run sweeper released an idle or over-age run; payload `{reason: idle|ttl|takeover_idle|max_age, idle_s, age_s}`, no URL; a non-ZDR run stays resumable); the owner timeline's `lifecycle` item for that expiry carries `initiatedBy: "system"` (a start and an owner's close carry none). `agent.budget_exhausted` is sent once per segment of a run when budget enforcement `on` refuses a call (see "Agent run budgets" at the end). Agent tool calls must go through the same action/runtime policy path as user-supplied `actions[]`.

### Crawl denial reason schema

```json
{
  "url": "https://example.com/admin",
  "code": "exclude_pattern | include_pattern | depth_limit | robots_txt | file_type | url_parse_error | backward_crawling | social_media | external_link | section_link | non_web_protocol | security_blocked | duplicate",
  "message": "Operator-readable explanation",
  "source_url": "https://example.com/",
  "depth": 2
}
```

Denial reasons are emitted as `page.blocked` payloads or stored in crawl status artifacts. Firecrawl's operator-readable explanations are the UX reference; Quarry owns the typed code list.

### Change detail artifact

```json
{
  "status": "new | changed | unchanged",
  "fingerprint": "blake3:...",
  "text_fingerprint": "blake3:...",
  "prev_fingerprint": "blake3:...",
  "semantic_diff": {
    "added": 1,
    "removed": 0,
    "moved": 2,
    "unchanged": 18,
    "ops": []
  },
  "modes": {
    "json": { "artifact_id": "art_..." },
    "git_diff": { "artifact_id": "art_..." }
  }
}
```

### Visual observation artifact

Deterministic browser-action visual evidence. Quarry may use OpenCV in an
isolated sidecar to compute this, but it must not perform VLM reasoning, OCR
interpretation, anti-bot bypass, or model-provider replacement.

```json
{
  "version": 1,
  "backend": "opencv5-sidecar",
  "step": 4,
  "previous_available": true,
  "changed": true,
  "change_ratio": 0.18,
  "regions": [
    { "x": 120, "y": 300, "width": 480, "height": 220, "score": 0.92, "label": "changed_region" }
  ],
  "metrics": {
    "threshold": 18,
    "morphology": "close_3x3",
    "source": "before_after_screenshot"
  },
  "annotated_artifact_id": "art_...",
  "change_artifact_id": "art_...",
  "related_artifacts": {
    "visual_change": "art_...",
    "screenshot_annotated": "art_...",
    "page_image_clean": "art_...",
    "thumbnail": "art_...",
    "tiles": "art_...",
    "ocr_preprocessed": "art_...",
    "logo_candidate": "art_...",
    "rendered_palette": "art_..."
  }
}
```

### Visual change artifact

Pure deterministic before/after screenshot diff. This is intentionally smaller
than `visual_observation.json` so change tracking can consume it without pulling
thumbnail, OCR, or branding side outputs.

```json
{
  "version": 1,
  "backend": "opencv5-sidecar",
  "step": 4,
  "previous_available": true,
  "changed": true,
  "change_ratio": 0.18,
  "regions": [
    { "x": 120, "y": 300, "width": 480, "height": 220, "score": 0.92, "label": "changed" }
  ],
  "metrics": {
    "threshold": 18,
    "changed_pixels": 8120,
    "total_pixels": 2073600,
    "region_count": 1
  },
  "annotated_artifact_id": "art_..."
}
```

### Vision sidecar wire contract

Quarry edge calls the sidecar only after ZDR has been checked. The sidecar never
persists inputs or outputs; it returns deterministic bytes and metadata for edge
to persist or discard.

`POST /v1/visual/observe` accepts `previous_png_b64`, `current_png_b64`, `step`,
`max_regions`, and operation flags:

```json
{
  "run_id": "run_...",
  "page_hash": "blake3:...",
  "step": 4,
  "previous_png_b64": "...",
  "current_png_b64": "...",
  "max_regions": 32,
  "operations": {
    "diff": true,
    "screenshot_preprocessing": true,
    "thumbnail": true,
    "tiles": true,
    "ocr_preconditioning": true,
    "rendered_branding": true
  }
}
```

`POST /v1/visual/preprocess` accepts one rendered page PNG and returns
`clean_png_b64` for the Data Plane page-image CAS path. Optional derivative
fields are `thumbnail_png_b64`, `tiles`, `ocr_preprocessed_png_b64`,
`logo_candidate_png_b64`, and `rendered_palette`.

Rules:

- ZDR requests must not persist `visual_observation.json`, annotated screenshots,
  thumbnails, tiles, cleaned page images, OCR preprocessed images, logo crops, or
  rendered palette artifacts.
- Visual artifacts are deterministic evidence only. Model Plane owns visual
  reasoning and provider-backed VLM/OCR decisions.
- OpenCV must not be used for anti-bot bypass or challenge solving.

## 10. Versioning

Contracts frozen at `v1`. Additive only. Breaking change ⇒ `v2` path (`/v2/scrape`). Deprecate via `Sunset` header + event.

### Scrape captures and actions: the closed set and the flags (WS13.8, built, NOT run, 2026-10-24)

Everything here is off by default and additive to v1, except the refusals listed under "Changed refusals". The OpenAPI file (`docs/openapi.yaml`, `ScrapeRequest` and `ScrapeResponse`) describes the same fields.

Flags, each read once at process start:
- `QUARRY_EDGE__SCRAPE_CAPTURE`: only `on` admits the `screenshot` and `pdf` formats (WS13.4b).
- `QUARRY_EDGE__SCRAPE_ACTIONS`: `off` (the default); `user` admits user tokens only (dev only); `on` also admits a service principal that holds `scrape:interact` (not to be set before the ADR-6 ruling). WS13.5.
- `QUARRY_EDGE__SCRAPE_STITCH`: `on` tiles and stitches a full-page screenshot taller than one capture; off crops it and sets the `screenshot_truncated` warning (WS13.6).
- `QUARRY_EDGE__SCRAPE_CAPTURE_INLINE_MAX_BYTES`: the ZDR inline cap for one capture, 6 MiB by default, 16 MiB at most.

Request keys (new, all optional):
- `formats`: `screenshot` and `pdf`, as a string or as an object with `type` and options. Screenshot options: `fullPage`, `clip`, `quality` (JPEG when set), `viewport`. PDF options: `format`, `landscape`, `printBackground`, `scale` (0.1 to 2.0), `maxPages` (100 by default, 200 at most). At most one of each kind.
- `actions`: an array of at most 50 items of the closed set `wait`, `click`, `write`, `press`, `scroll`, `select`, `screenshot`, `pdf`. A selector or time wait uses the wire type `wait` (`milliseconds`, or `selector` with `timeoutMs`). The types `executeJavascript`, `evaluate`, `navigate`, `back`, `forward`, `clickPoint`, `mouseWheel`, `upload`, `download` and `dialog` are refused by name with `action_denied`; any other unknown type is `unknown_action_type`. A `write` or `select` whose selector looks like a credential field is refused (a heuristic, ADR-2). The total wait budget is 60 s and one wait is at most 30 s; a selector wait without `timeoutMs` waits 5 s; a selector is at most 512 bytes; one `write` text at most 4096 bytes and all of them 16384 bytes; a `scroll` at most 20000 px. The environment can only lower these caps.
- `actions_on_error`: `abort` (the default) or `continue`. There is no retry.
- `initiated_by`: a tighten-only label. `"agent"` (a Model-proposed list relayed under the user's token) is refused for `actions` with 403 `scrape_interact_scope_required`, whatever the token. The edge reads it. It is not yet a row of the contract key table, so it can appear in `warnings` as `unknown_field`; that is a known gap.
- Captures and actions share one cap of eight: `too_many_captures` when `formats` and `actions` together ask for more.

Field errors (400, in `errors[]` with a `path` and a `code`; a value is never echoed): `feature_disabled` (at `formats` or `actions` while its flag is off), `too_many` (two screenshots or two PDFs), `too_many_captures`, `unsupported_format` (an object without `type` `screenshot` or `pdf`), `too_many_actions`, `unknown_action_type`, `action_denied`, `invalid_action` (a missing or empty selector, or a wait or scroll with the wrong fields), `selector_too_long`, `text_too_long`, and the option decode error for an unknown key or a wrong value type inside an option object.

Response (`data`, all optional):
- `screenshot` and `pdf`: inline `{content_type, bytes, width?, height?, data_base64}` under ZDR only; otherwise read `formats.screenshot` or `formats.pdf`.
- `captures`: in both modes, the measured `{screenshot?: {content_type, width, height, bytes, tiles, stitched, truncated}, pdf?: {bytes}, blocked_requests}`; counts and sizes only, never a URL.
- `scrape_id`: the stored record for a non-ZDR scrape, for `GET /v1/scrape/{id}`.
- `effective_options.actions`: `{count, types, total_wait_ms, capture_count, on_error}`. Never a selector or a text.
- `warnings[]` codes for captures: `screenshot_failed`, `pdf_failed`, `screenshot_truncated`, `capture_exceeds_inline_cap`. `omitted[]` may name `screenshot` or `pdf` (over the ZDR cap).

Not built yet: the per-action outcomes `data.actions[]` of a successful run, and the bytes of a screenshot or PDF that an action makes, are neither stored nor returned. On a failed run under `abort` the outcomes are in `error.details.actions`.

Changed refusals (an edge that had enabled the WS5 options flag sees these):
- `formats: ["screenshot"]` was `400 reserved_format`; it is now `400 feature_disabled` while `QUARRY_EDGE__SCRAPE_CAPTURE` is not `on`.
- `actions` was an unknown key (ignored with a warning); it is now `400 feature_disabled` at `actions` while `QUARRY_EDGE__SCRAPE_ACTIONS` is `off`.

Errors: the `details.reason` values and their status codes are in `docs/ERROR_REASONS.md` (section "Scrape captures and actions"). `UNSUPPORTED` (501) means the browser cannot capture or act. `/v1/scrape/stream` refuses captures and actions with `unsupported_in_stream`.

## Who holds the browser: `USER_TOOK_OVER` (409)

A run has a control mode (`agent_control` or `human_takeover`) and an epoch that counts real mode changes (it never goes down). While a person holds the browser, an action proposed by an agent is refused with `409 USER_TOOK_OVER`, `details {control_mode: "human_takeover", control_epoch, taken_over_at}` (`taken_over_at` is RFC 3339 and is absent for a run resumed into a takeover, whose start the checkpoint does not carry); a person's own actions go through. The initiator is derived, never taken on trust: a service principal is always an agent; a person's token is a person unless the request says `initiated_by: "agent"` (or, on the WebSocket, `actor`), which can only tighten. A tab open takes the same hint as `initiatedBy` in its body, and a tab select or close (they have no body) as the `initiatedBy` query, `?initiatedBy=agent`; a value that is neither `human` nor `agent` is a `400` in the tab select/close query and a `422` in a body, and both spellings in one body or one query are refused too (`422` in a body; a `4xx` in the select/close query). Only a person may change the mode (`403 control_transition_requires_human` otherwise), and `expected_epoch` on the control request makes the change a compare-and-set (`409 CONFLICT` with the current epoch). Receipts carry `initiated_by` and `control_epoch`, and the owner timeline items (which are camelCase throughout) carry `initiatedBy` and `controlEpoch`; a refused agent action leaves one `skipped` receipt per takeover. Enforcement is `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` (default off: the edge logs `would_reject` and refuses nothing until the planner side maps the 409). A person's step while the person holds the browser never counts against the run's step budget and the run's runtime clock is paused (the cap bounds the agent, not the person who supervises it; decided 2026-10-28, TAKEOVER-BUDGET); those steps and the seconds they ran are metered apart in `quarry_agent_takeover_exempt_steps_total` and `quarry_agent_takeover_exempt_step_duration_seconds`. The exemption relies on agents using service principals: a person's token labelled `agent` is still a person's session and can itself switch to takeover (accepted residual, D2-012). An agent may not reload the page (WS9.B): a `reload` from the derived agent is `403 FORBIDDEN` with `details.reason: "agent_reload_denied"` on the step route, its `interact` alias, the browser socket and the attached delegate lane, and consumes nothing. Quarry decides who the agent is from the verified token; the Model Plane has no credential to the verevonv3 gateway (decided 2026-10-28, MP-GATEWAY-AUTH).

Which agent opened a tab. Every request that carries the initiator hint takes it as `initiated_by` or `initiatedBy` (a tab open, a step, a run start, a socket action, and the tab select/close query); both in one body is a `422`, never a silent pick. Quarry records who did each tab open, select and close, and who drove each step: the class (`human` or `agent`) and, for an agent, an `agent` object of ids and classes only, never a URL or a title. `agent.basis` is `service_principal` when the call carried a service principal's own token, and then `agent.principalId` is that token's signed service id: the one verified part, which no request field can set. A person's token with the `agent` hint gives basis `user_session` and no `principalId`: the token proves the person, not which agent. The request fields `aiRunId` and `agentLabel` (body fields or query parameters; `ai_run_id` and `agent_label` accepted) are what the caller said, recorded as `agent.aiRunId` and `agent.label`: advisory, kept only when identifier-shaped (`[A-Za-z0-9._:-]`, 1 to 64 characters, otherwise dropped), and never used to decide anything. The record is on the timeline items (`agent` next to `initiatedBy` on a tab item and on an action item), on the step receipt (`agent`, with `initiated_by`), and on the owner projection's tab as `openedBy: {initiatedBy, agent?}` for a tab opened through the tab routes (a tab the browser opened itself, such as the first one, has none; the record is process-local and dropped when the tab is closed; it keeps at most 256 full records, and a tab opened after that has `openedBy: {initiatedBy, unattributed: true}`, the class without the agent ids, so it is not mistaken for a tab the browser opened itself). The Verevon gateway forwards the headers `X-Agent-Run-Id` and `X-Agent-Label` as the advisory fields, on an agent's call only. The tab routes are in `openapi.yaml`.

## 9.1 `POST /v1/scrape` request options (WS5.D)

> WS13.8: this section lists the inline formats of v1. `screenshot` and `pdf` (WS13.4b) and `actions` (WS13.5) are described in section 10 below. Each is refused with 400 `feature_disabled` at its field until its flag is on.

Root-level, all optional, snake_case with a camelCase alias: `formats` (`markdown`, `html`, `rawHtml`, `links`, `metadata`; `links` and `metadata` change nothing, since both are already in every response; `screenshot` and `pdf` are in section 10 and the AI formats `json`, `summary`, `question` and `highlights` in "Scrape AI formats"), `only_main_content`, `include_tags`, `exclude_tags`, `remove_base64_images`, `extractor` (`readability`, `clean_full`, `raw`), `timeout_ms` (1000 to 55000, default 55000), `max_inline_bytes` (1 to 8 MiB, default 1 MiB).

All eight are applied. `exclude_tags` (CSS selectors) removes every match and its subtree. `include_tags` keeps only the matching subtrees, in document order, each once; an explicit selection beats the main-content heuristic, so the heuristic does not run on it, and a list that matches nothing gives empty markdown and the warning `include_tags_matched_nothing`, never the whole page. The selection is then cleaned the way `clean_full` cleans a page (lazy images resolved, inline images stripped), whatever `extractor` and `remove_base64_images` say. A selector that does not parse is a 400 `invalid_selector` at `include_tags[i]` or `exclude_tags[i]`. `only_main_content` (default true) and `extractor` say one thing two ways: `readability` is the main-content heuristic, `clean_full` keeps the whole body minus head, scripts, styles, noscript, templates and iframes (lazy images resolved, inline images stripped) and `raw` converts the document as it is. An unset one follows the other; setting them to disagree is a 400 `conflicts_with_only_main_content` on `extractor`. `remove_base64_images` strips inline `data:` images on the readability and raw paths too. With default options the markdown is byte-identical to what a scrape returned before these options existed. `html` is the HTML the markdown was made from (after exclusion, inclusion and cleaning); `rawHtml` is the document as fetched. `effective_options.ignored_fields` now lists only unknown request keys. The AI formats (`json`, `summary`, `question`, `highlights`) are in "Scrape AI formats" below; `reserved_format` is gone.

Response additions on `data`: `markdown`, `html`, `raw_html` (inline, only when asked for; `markdown` also by default under ZDR), `truncated[]` (ZDR: cut at the cap), `omitted[]` (not ZDR: over the cap, read the `formats.*` artifact), `warnings[]`, `effective_options`. `formats.*` artifact references are unchanged. A request that names `formats`, `include_tags`, `exclude_tags`, `remove_base64_images`, `only_main_content: false` or an `extractor` other than `readability` neither reads nor writes the page cache. So does a request whose `signals` select the browser driver (any `actions`, `screenshot`, `pdf`, `prior_block_signals` of 2 or more under `QUARRY_BLOCK_ESCALATION=legacy` (the field is deprecated and ignored under `honest`), or `profile_required`): a cached static result cannot carry what the browser renders or captures. `timeout_ms` and `max_inline_bytes` change delivery, not the result, so they do not bypass it. ZDR never touches it.

Unknown keys: ignored with a `warnings[] {code: "unknown_field", field}` entry, `effective_options.ignored_fields`, `Deprecation: true` (and `Sunset` when `QUARRY_EDGE__SCRAPE_STRICT_AFTER` is set) and a `quarry.scrape.unknown_field` log line naming the keys. `QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION=strict` (or a passed strict-after date) makes them a 400. `skipTlsVerification`, `skip_tls_verification`, `proxy`, `proxyUrl`, `headers`, `mobile` and `stealth` are always a 400 `reserved_field`. A bad value is a 400 `BAD_REQUEST` with `details.fields[] {path, code, message}`, including a body that is not JSON. `QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED=0` turns the option keys back into unknown keys (the kill switch); `QUARRY_EDGE__SCRAPE_INLINE_MAX_BYTES` changes the default inline cap.

**Scrape record (WS5.F).** A non-ZDR scrape stores one small `scrape_record` artifact (the normalized output and `effective_options`, never inline text) and returns its id as `data.scrape_id`. `GET /v1/scrape/{id}` returns it again for the same organization: another organization's id is a 404, an id that is not a scrape record is a 404, and a reader whose organization is under ZDR gets a 403 even for an earlier record. A ZDR scrape stores nothing and has no `scrape_id`; a failed write adds the warning `scrape_record_unavailable`. Service principals need `scrape:read` or `scrape:write`. `QUARRY_EDGE__SCRAPE_RECORDS=0` stops storing records. Records live as long as artifacts do.

## 9.2 `POST /v1/scrape` refuses a bot wall (WS3.J)

A fetched page that is itself a bot challenge fails the scrape: `403`, `error.code: "CHALLENGE_DETECTED"`, `retryable: false`, `details {reason, block_reason, recovery: "not_applicable", next: "other_source"}`. `block_reason` is one of `captcha_widget`, `waf_interstitial`, `unexpected_bot_error` (the closed `BlockReason` set; `challenge_loop` belongs to agent runs) and `reason` repeats it, the key the agent path uses. There is no `host`, `url` or page text in `details` or `message`. It is the only field a consumer may read to say that a site blocked automation: a bare 403, a 429 or a 5xx page is still a scrape result (`output.status`), and a login page, a paywall or a CAPTCHA on an ordinary form is not refused. Nothing is added to the success response, and the field names are pinned by `crates/quarry-core/tests/contracts.rs`. `QUARRY_CHALLENGE_CLASSIFIER=legacy` switches the classification off. The refusal is only made when `QUARRY_CHALLENGE_ENFORCE=enforce`; the default is `log` (the plan's setting while the corpus validates), which keeps the page, as before, and counts it by reason in a log line. A refused page emits `page_blocked` with `reason: "challenge_detected"` and `block_reason` and is not metered.


## 9.3 Ref actions: `driver_capabilities.trusted_input` and `TARGET_REPAIR_REQUIRED` reasons (WS3.K)

`driver_capabilities` (on the run start response) has one more boolean, `trusted_input`: `true` when the driver was started with `QUARRY_BROWSER_TRUSTED_INPUT=on`, `false` otherwise and for every driver that does not say. It is additive: a client that predates it reads a missing key as `false`, and it is reported only, not requestable (a `driver_requirements.trusted_input` is ignored).

With it `true`, a `click_ref`, `type_ref` or `select_ref` on a top-level ref is checked before it is performed (visible, enabled, still, not covered) and a click and a type are real browser input. A ref the check refuses is `409 TARGET_REPAIR_REQUIRED`, the same status, error code and `retryable: false` as every stale-ref answer, with `details.reason` from a closed list (`target_not_attached`, `target_not_visible`, `target_disabled`, `target_covered`, `target_unstable`, `target_has_no_box`, `target_unreachable_after_scroll`) and, for `target_covered` only, `details.covered_by_tag`, the tag name of what covers the target. The details never carry a selector, URL or text from the page. The list, the meaning of each reason and what to do next are in `docs/ERROR_REASONS.md`; the flag, the rollout rule and what stays on the old in-page path (child frames, downloads, uploads, `wait_for_ref`) are in `docs/RUNBOOKS.md`, "Trusted input for ref actions". A client may treat a reason it does not know as "observe again".

## 9.4 `POST /v1/crawl` bounds (WS6.A)

Every crawl has a finite page bound and a finite depth bound. The edge resolves the request's `max_pages` and `max_depth` before it hands the job to the orchestrator, and the orchestrator resolves them again when it dispatches the job, so a job row written without the values is bounded too.

- Absent or `0`: the default. The defaults are 1000 pages (`QUARRY_CRAWL_DEFAULT_MAX_PAGES`) and depth 5 (`QUARRY_CRAWL_DEFAULT_MAX_DEPTH`). Zero is never unlimited, and the edge does not answer `400` for it (the strict refusal of `0` is WS14.I and is opt-in).
- Above the hard ceiling: clamped to it. The ceilings are 5000 pages (`QUARRY_CRAWL_HARD_MAX_PAGES`; the value awaits the owner's ruling and is overridable by this variable) and depth 10 (`QUARRY_CRAWL_HARD_MAX_DEPTH`).
- Otherwise: used as asked.

A default of `0` disables the default, so an absent request takes the hard ceiling; it never disables a ceiling. A zero ceiling, an unparsable value or a default above its ceiling stops the edge or the orchestrator at start-up, and the error names the variable.

The response to `POST /v1/crawl` reports what runs, under `data.limits`:

```json
{ "max_pages": { "requested": null, "effective": 1000, "clamped_by": ["default"] },
  "max_depth": { "requested": 12, "effective": 10, "clamped_by": ["hard"] } }
```

`requested` is what the caller sent (`null` when absent), `effective` is what runs, and `clamped_by` is `default` or `hard`, empty when the request was used unchanged. The job's `params` carry the effective values as `max_pages` and `max_depth`, and the requested ones as `max_pages_requested` and `max_depth_requested`.

The workflow keeps its own fallback for a job that reaches it with a zero bound: 1000 pages and depth 5. The change sits behind `workflow.GetVersion("crawl-bounded-v1")`, so a history recorded before it keeps its old behaviour. A crawl stops with `reason: "max_failures"` once its failed pages reach its page bound.

The gateway's `POST /api/v1/knowledge/crawl` returns the edge's `limits` under `data`, beside `upstream`. It keeps its own rules: a `maxPages` of `0` is a `400`, and a larger value is clamped to 5000 before the request reaches the edge.

## 9.5 `/v1/extract` does not sniff content (deferred, WS5.B)

`POST /v1/extract` does not route by content. This is an explicit deferral, not an oversight. Each fetched source is read as text (`String::from_utf8_lossy`) and passed to readability and the structured-data harvester, as it was before the document route existed. A PDF or a DOCX fetched through it is not parsed and is not refused with `415 UNSUPPORTED_MEDIA`; its markdown is whatever readability finds in the lossy text of the bytes, which is not the document's text. Documents and images are read through `POST /v1/scrape`, whose route is decided by the bytes (section 9.1, `QUARRY_EDGE__DOC_ROUTING`).

Why it is deferred: the WS5.B definition of done allows the step to be "landed or explicitly deferred"; the open question on it (plan X-14, and the WS5.B default) records the deferral; and routing extract through the content sniff would change the per-source result of a batch, which needs a contract decision the plan has not taken. The W1 criteria for documents (SC6) are measured on `/v1/scrape`.

To pick it up: route `fetch_page` in `crates/quarry-edge/src/extract_routes.rs` through `quarry_transform::sniff::detect` and `quarry_runtime::content_route::parse_document`, answer a document it cannot read with a typed per-source `UNSUPPORTED_MEDIA` that uses the reason codes of `/v1/scrape`, and test that a PDF in a batch fails only its own source.

## Scrape AI formats: `json`, `summary`, `question`, `highlights` (INNH-1, U1 to U3, 2026-10-31)

Status: built and tested in tests, dark. The contract and its parser are in `quarry-core` (`ai_spec.rs`, `scrape.rs`, U1), the engine and the `/v1/ai/chat` client in `quarry-runtime` (`ai_scrape.rs`, `ai_verify.rs`, `mp_client.rs`, U2), and the edge wiring in `quarry-edge` (`scrape_contract/ai_formats.rs`, `routes.rs`, U3). The flag `QUARRY_EDGE__AI_FORMATS` exists and is off, so every AI format is `400 feature_disabled` until an operator sets it. It is on no stack: the compose files default it to `0`, and no deployed stack can turn it on yet (see "Not in this version"). No real Model Plane gateway has been called; the lane is proven against a strict double only. The design and its decisions are `.plan-work/quarry-w1w2/INNH-1-design.md`.

**Request (additive, `POST /v1/scrape`).** `formats[]` items may be these objects. Unknown keys inside an object are 400 `unknown_field` at `formats[i]`, as for `screenshot` and `pdf`.

```json
{ "type": "json",       "schema": { "...": "..." }, "prompt": "optional" }
{ "type": "summary",    "max_chars": 2000, "language": "nb" }
{ "type": "question",   "question": "..." }
{ "type": "highlights", "query": "...", "max_highlights": 3, "max_chars_each": 300 }
"summary"
```

`"summary"` as a string means the defaults. The strings `"json"`, `"question"` and `"highlights"` cannot be honoured (each has a required input) and are 400 `missing_field` at `formats[i].schema`, `formats[i].question` and `formats[i].query`. `reserved_format` no longer exists: while the flag is off the four formats are 400 `feature_disabled` at `formats[i]` (the same step `screenshot` took in WS13.4b).

All field errors are 400 `BAD_REQUEST` with `details.fields[] {path, code, message}`; a value is never echoed.

| Field | Rule | Code |
|---|---|---|
| any AI kind | at most one of each kind per request | `too_many` at `formats[i]` |
| `json.schema` | required; a JSON object; root `"type": "object"`; at most 65,536 bytes serialized; at most 10 levels (`properties` and `items` each add one, the root is 1); at most 200 `properties` entries in all | `missing_field`, `invalid_type`, `schema_root_not_object`, `schema_too_large`, `schema_too_complex` at `formats[i].schema` |
| `json.schema` keywords | allowed: `type`, `properties`, `required`, `items` (one schema), `enum`, `minItems`, `maxItems`, `minLength`, `maxLength`, `minimum`, `maximum`, `additionalProperties` (`true` or `false`), `description`, `title`; `$schema`, `$id` and `$comment` are dropped; anything else is refused | `schema_keyword_not_supported` at `formats[i].schema.<path to the keyword>`; a wrong value type is `invalid_type` or `invalid_value` at the keyword |
| `json.prompt` | optional string, 1 to 10,000 characters | `empty`, `text_too_long`, `invalid_type` |
| `summary.max_chars` | optional integer, 200 to 10,000, default 2,000 | `out_of_range`, `invalid_type` |
| `summary.language` | optional, `[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})?`; default the page's own language | `invalid_value` |
| `question.question` | required string, 1 to 10,000 characters | `missing_field`, `empty`, `text_too_long` |
| `highlights.query` | required string, 1 to 1,000 characters | `missing_field`, `empty`, `text_too_long` |
| `highlights.max_highlights` | optional integer, 1 to 10, default 3 | `out_of_range` |
| `highlights.max_chars_each` | optional integer, 40 to 1,000, default 300 | `out_of_range` |
| any AI kind, flag off | | `feature_disabled` at `formats[i]` |

The schema keywords that the validator in `quarry_core::json_schema` ignores (`$ref`, `anyOf`, `oneOf`, `allOf`, `not`, `const`, `pattern`, `format`, `if`/`then`/`else`, `patternProperties`, `dependent*`, `default`, `uniqueItems`, ...) are refused rather than accepted, because a schema Quarry cannot check would make `schema_valid: true` a lie. Widening the validator is a separate slice.

`effective_options.formats` lists an AI format by its wire name only (`json`, `summary`, `question`, `highlights`): no schema, prompt, question or query is echoed or stored. A request with an AI format is not a default request: it neither reads nor writes the page cache, and `canonical_options_key` carries one `ai:<kind>:<hash16>` per AI format (the first 16 hex digits of blake3 over the spec as canonical JSON, keys sorted, defaults filled in, `$schema`/`$id`/`$comment` dropped), so a future result cache cannot confuse two specs and the content never appears in a key. A request without an AI format keeps the key it had before.

**Request errors decided by the edge, before the page is fetched (INNH-1 U3).** 403 `FORBIDDEN` `details.reason: extract_ai_scope_required` (a service principal without the scope `extract:ai`, or a request labelled `initiated_by: "agent"`); 501 `UNSUPPORTED` `model_plane_unavailable` (no `QUARRY_EDGE__MODEL_PLANE_URL`) or `model_lane_not_granted` (no usable model credential); `POST /v1/scrape/stream` refuses any AI format with `unsupported_in_stream` (an SSE `error` event, before any fetch, whatever the switch says; it looks at the raw `formats` for an object whose `type` is one of the four, or the string `summary`). `details.field` of the 403 is `formats[i]` of the first AI format. The 403 comes first, then the 501s, both after the field validation (a malformed AI format is still a 400 with its path) and before the fetch, so a refused request spends no fetch. ZDR is never a request error for an AI format: the scrape succeeds and each AI entry says `ZDR_UNAVAILABLE`; the 501 checks are not made for a ZDR request, because it never reaches a model. Reasons are in `docs/ERROR_REASONS.md`.

**Response (`data`, all optional, present only when asked for).**

| Field | Type | Present when |
|---|---|---|
| `json` | object | `ai.json.status` is `ok`; then it always validates against the request's schema |
| `summary` | string, at most `max_chars` characters (cut on a character boundary) | `ai.summary.status` is `ok` |
| `answer` | string, at most 10,000 characters | `ai.question.status` is `ok` and `ai.question.found` |
| `highlights` | `[{text, start, end}]`, in page order, not overlapping, each 8 to `max_chars_each` characters | `ai.highlights.status` is `ok`; may be `[]`, an honest empty result |
| `ai` | `AiReport` | any AI format was asked for |

`start` and `end` are Unicode scalar (char) offsets, end-exclusive, into the AI source text: the page markdown Quarry produced, before any inline truncation (the same as `data.markdown` when that came back whole; otherwise the `formats.markdown` artifact). `text` is the exact slice. A `json` output that still fails the schema after repair is not put in `data.json`; invalid data travels only as `schema_issues` (paths and codes).

`ai` is `{json?, summary?, question?, highlights?: AiFormatReport, total: {calls, input_tokens, output_tokens}}`. An `AiFormatReport` is `{status, code?, reason?, retry_after_s?, model?, usage?: {input_tokens, output_tokens}, truncated_input?, untrusted_source, schema_valid?, repair?, schema_issues?, found?, grounded?, dropped?}`:

- `status` `ok`: the result field is present. `skipped`: no model call was made for this format (`ZDR_UNAVAILABLE`, `BUDGET_EXHAUSTED`, `EMPTY_SOURCE`, or `MODEL_RATE_LIMITED` for formats not started when a sibling was rate limited). `error`: a call was made and failed (`MODEL_UNAVAILABLE` with a `reason`, `MODEL_RATE_LIMITED` with `retry_after_s`, `SCHEMA_INVALID` with `schema_issues`).
- `reason` is one of `model_lane_denied`, `model_lane_unauthorized`, `model_rejected_request`, `upstream_unavailable`, `timeout`, `transport`, `bad_reply`, `not_json`, `too_large`, `deadline`.
- A model failure never fails the scrape: the status stays 200 with the page fields. The codes are strings inside a 200 body, not wire `ErrorCode`s.
- `untrusted_source` is true on every `ok` entry: the output is model text derived from the page and is data, never instructions. `repair` (`none`, `local`, `model`) says how a `json` output was made valid; the one model repair call carries the invalid output, the issues and the schema, never the page. `grounded` says the model's supporting quote for an `answer` is verbatim on the page; `dropped` counts highlight quotes that are not on the page.
- `model` is what the gateway answered with (a routing alias or what it resolved to), never a vendor id chosen by the caller. `cost_usd` is added to `usage` when the Model Plane returns it.

Model output is untrusted and checked by Quarry, not by the model: `json` is validated against the caller's schema after a local repair and at most one model repair; `highlights` and the `question` quote are verified verbatim against the page text (whitespace runs, no-break spaces and typographic quotes are folded, minimum 8 characters, de-duplicated, non-overlapping); `summary` cannot be verified and is labelled. The page reaches the model inside a per-call random delimiter it cannot know, with rules that call it data. AI output is inline only: never an artifact, never in the scrape record (`GET /v1/scrape/{id}` has no AI fields), never in the page cache, an event, a log or a span.

**Zero Data Retention.** Effective ZDR (the token floor or the body flag) plus any AI format: every AI entry is `skipped` with `ZDR_UNAVAILABLE`, no Model Plane call is made, nothing is written, and the page itself is returned as an ordinary ZDR scrape. `QUARRY_EDGE__AI_FORMATS_ZDR` accepts only `refuse` in v1 (ADR-4); `allow` is reserved and behaves as `refuse`. The rows `scrape` and `scrape_stream` in `docs/ZDR_MATRIX.md` say so.

**Flags (INNH-1 U3; each read once at the first request; default off).** `QUARRY_EDGE__AI_FORMATS` (`on`, `1` or `true`, ignoring case and spaces, enables; anything else is off, so a typo never widens the endpoint), `QUARRY_EDGE__AI_FORMATS_ZDR` (`refuse`; `allow` is reserved, is logged once at start as ignored and behaves as `refuse`), `QUARRY_EDGE__AI_FORMATS_MODEL_JSON` (routing alias, default `verevon-balance`), `QUARRY_EDGE__AI_FORMATS_MODEL_TEXT` (summary, question, highlights and the json repair call; default `verevon-budget`), `QUARRY_EDGE__AI_FORMATS_MAX_INPUT_CHARS` (page characters per call, 1,000 to 80,000, default 60,000; a value above the ceiling is the ceiling), `QUARRY_EDGE__AI_FORMATS_MAX_TOTAL_TOKENS` (reported tokens per request, at least 1,000, default 200,000), `QUARRY_EDGE__AI_FORMATS_MAX_CALLS` (1 to 8, default 8, repair included) and `QUARRY_EDGE__AI_FORMATS_CONCURRENCY` (1 to 4 per request, default 4; the process-wide cap is 16). The models are routing aliases, never vendor ids. A malformed number falls back to its default.

**The model lane (development only).** The edge calls `POST /v1/ai/chat` with the credential it holds for the Model Plane: `state.model_plane_token`, which `main.rs` sets only under the auth dev bypass (`QUARRY_EDGE__MODEL_PLANE_TOKEN`), sent as `Authorization`, and `QUARRY_EDGE__MODEL_PLANE_INFERENCE_TOKEN` (the token for the inference audience; when unset the one dev token goes in both headers) as `x-inference-authorization`. With no `QUARRY_EDGE__MODEL_PLANE_URL` the answer is 501 `model_plane_unavailable`; with a URL and no such credential, 501 `model_lane_not_granted`. The Auth Core minted path is not built (owner decision OD-1).

**Who may ask.** A user token. A service principal needs the scope `extract:ai`, which no registry principal holds today; a request labelled `initiated_by: "agent"` is refused whatever the token, the scope included. Both are the 403 above.

**Deadline.** While the switch is on, `POST /v1/scrape` gets the 125 s router deadline that scrape actions have (the fetch keeps its own cap, and the model phase has 45 s of its own); every other route keeps 60 s.

**Not in this version.** No billing for AI formats (token usage is reported only; pricing is an owner decision). No result cache. Not on `/v1/scrape/stream`, crawl or batch. `/v1/extract` has its own `prompt` (next section, INNH-2). The Model Plane lane for a deployed stack (a grant for Quarry on `/v1/ai/chat`, decision X-7) is not built: a deployed stack cannot turn the flag on until it is.

## `/v1/extract`: the `prompt` takes effect (INNH-2, 2026-10-31)

`POST /v1/extract` used to read `prompt` and drop it. It is now sent to the model. No new endpoint, no new switch: the model arm is still gated by `QUARRY_EDGE__MODEL_PLANE_URL` and a model credential, as the schema arm always was. Callers that never sent `prompt` see no change except the additive item fields below.

**Modes.** `schema` alone: schema-guided extraction, as before. `schema` + `prompt`: the same, with the prompt as extra instructions that say what to extract and cannot relax the schema. `prompt` alone: the model is asked for one JSON object that answers the instructions; the item is status `ok` and `data` is that object (a reply that is not an object is a per-source `error`). Neither: cleaned markdown, status `fetched`, unchanged.

**Prompt rules.** 1 to 10000 characters after cleaning (characters, not bytes). Control characters other than line breaks and tabs are dropped and the ends are trimmed. An empty result, or more than 10000 characters before cleaning, is a 400 `BAD_REQUEST` with a `hint`; the message never repeats the prompt, and a refused prompt costs no fetch and no model call. The prompt is trusted text and goes before the page, outside a fence whose id is 128 random bits and is not in the page, so text on the page cannot close the fence or pose as the instructions. The output is still validated against `schema` when there is one. The prompt is not echoed in any response and not logged.

**ExtractItem additions (additive).** `schema_valid` (bool): present when a `schema` was sent and the model answered; false means `data` does not conform, and `status` stays `ok` (the `fetched | ok | error` enum is unchanged). `schema_issues` (string[]): one line per issue, present only when `schema_valid` is false. `truncated_input` (bool): present and true when the page was cut at the 20000-byte input cap, whatever the status, so a caller knows the model (or the markdown) saw only the start of the page.

**Errors.** 400 for the prompt rules above. 501 `UNSUPPORTED` when a `schema` or a `prompt` is sent and there is no Model Plane (hint: set `MODEL_PLANE_URL`, or omit `schema` and `prompt` to receive markdown); the exact enablement text is INNH-3. 429 short-circuit unchanged.

**ZDR (ADR-4).** The model hop carries no ZDR posture, so under ZDR (the body flag, or the token's floor for a user of a ZDR org) no page text is sent to a model. The page is still fetched under ZDR; each source answers status `error`, `error` starting `ZDR_UNAVAILABLE:`, with its `structured` harvest, which needs no model. This is not a 501 and does not need a Model Plane. The engine refuses a ZDR call as well (`AiFormatRunner::extract_for_org`), so a future caller cannot reach the model by skipping the route check. The `extract` row of the ZDR matrix says so.

**Time budget.** Sources are read one after another and every model call is paid. After 50 seconds from the start of the request a source that has not been read by the model answers status `error` with a message that names the budget, instead of the router cutting the whole request at 60 seconds. Send fewer URLs per request, or `max_urls` lower.

**Not in this version.** No per-call output cap and no `cost_usd` (WS16.S1). No LLM usage metric: metering is unchanged (`quarry.extract.url` per `ok` or `fetched` source). The Verevon gateway re-adding `prompt` to its own contract is the Innhenting session's change. The model lane for a deployed stack (a grant on the Model Plane, decision X-7) is not built, so a deployed stack still cannot run this arm.

## Answer synthesis: `/v1/answer`, `/v1/answer/stream` and `include_answer` (INNH-8a, 2026-10-31)

One pipeline (search, read the top pages, synthesize through the Model Plane) serves three surfaces. Before INNH-8a, when it was not wired `/v1/answer` said 501 with a hint that named two of the gates and `include_answer` said `answer: null` with no reason, and cached that null. This section is the contract after INNH-8a, merged with the QB-T17 boot gate. The enablement steps are in `docs/RUNBOOKS.md`, "Answer synthesis".

**The switch (QB-T17, default off) and its boot gate.** `QUARRY_EDGE__ANSWER_SYNTHESIS` (`1`, `true` or `on` enables; anything else is off; compose `QUARRY_EDGE_ANSWER_SYNTHESIS`, default `0`) is one of the Model Plane switches that `crates/quarry-edge/src/model_plane_gate.rs` decides once at boot. A switch that is on is still REFUSED, and stays off, unless the edge has a model path: `QUARRY_EDGE__MODEL_PLANE_URL`, and either a service identity whose cross-plane token-request manifest row `model-invoke` is `granted`, or the development bypass credential. The shipped manifest grants nothing, so today a switch that is on is refused at boot with an `ERROR` log that names it. The answer pipeline is built only with the gate's enablement AND a search provider AND a Model Plane client that builds. The edge derives the FIRST closed gate from the gate's decision (there is no second switch) and reports it: `synthesis_disabled`, `model_path_refused`, `no_model_plane_url`, `no_search_provider`, `model_client_init_failed`. A deployment that already set `MODEL_PLANE_URL` for another consumer (autoprompt, rerank, page titles, AI formats) gets no answer synthesis until it also sets the switch.

**`POST /v1/answer` and `POST /v1/answer/stream`.** Request `{query, top_k?, country?, language?, zdr?}`. 200 `{query, answer, citations[], model, latency_ms, sources_used, sources_skipped, sources_snippet_fallback}`; the stream sends `citations`, `delta`, `done {answer, cached}` and `error {message, reason}` events. Errors keep the legacy body `{error, code, hint?}` (not the typed envelope) and gain one optional field, `reason`, a closed code that is never provider text:
| status and `code` | `reason` | meaning and fix |
|---|---|---|
| 400 `BAD_REQUEST` | none | empty query |
| 501 `UNSUPPORTED` | `synthesis_disabled` | the switch is off: set `QUARRY_EDGE__ANSWER_SYNTHESIS=1` |
| 501 `UNSUPPORTED` | `model_path_refused` | the switch is on but the boot gate refused it: the manifest row `model-invoke` is not granted (or there is no service identity), so the feature stays off and no Model Plane call is made. The boot log names the refusal; the fix is the cross-plane grant (owner decision INNH-8b, below) |
| 403 `FORBIDDEN` | `zdr_unavailable` | zero data retention (the body flag or the token's floor): the model hop carries no ZDR posture (ADR-4), so the request is refused before any work. See ZDR below |
| 501 `UNSUPPORTED` | `no_search_provider` | configure `SEARXNG_URL`, `BRAVE_SEARCH_KEY` or `SERPER_KEY` |
| 501 `UNSUPPORTED` | `no_model_plane_url` | set `QUARRY_EDGE__MODEL_PLANE_URL` (`MODEL_PLANE_URL` in compose) |
| 501 `UNSUPPORTED` | `model_client_init_failed` | the URL is set but no client could be built from it: read the edge boot log |
| 503 `UNAVAILABLE` | `model_plane_auth_refused` | wired, but Auth Core refused to mint the edge's Model Plane token or the gateway answered 401 or 403 to it. A deployment fact, never a 403 on the caller. The fix is cross-plane (owner decision INNH-8b, below) |
| 429 `RATE_LIMITED` | `model_plane_rate_limited` | Auth Core rate limited the mint. It is the edge's service call that was limited, not the caller's own quota |
| 502 `DRIVER_FAILED` (or the code the error had) | `model_plane_error` | the Model Plane call failed (transport, 5xx, undecodable) |
| the status the error had | `answer_search_failed` | the search or page-fetch half failed before a model was asked |
The `code` of an error is the wire name of the error code (`DRIVER_FAILED`, `RATE_LIMITED`, `FORBIDDEN`), the same string `ErrorBody.code` documents in `openapi.yaml`. The `hint` of a 501, of the 403 and of the 503 is fixed text that names the setting and `docs/RUNBOOKS.md`. The message of a 503 is fixed too; the gateway's own text never reaches the body. A stream whose pipeline is not wired answers the same 501 as JSON, not a stream.

**`include_answer` on `POST /v1/search`.** The search never fails because of the answer. Two optional response fields, present only when the request set `include_answer`:
| `answer_status` | `answer` | `answer_reason` | what happened |
|---|---|---|---|
| `ok` | the answer | absent | synthesized (or replayed from the search cache) |
| `no_sources` | `""` | absent | the pipeline ran and no source had text; no model was called |
| `skipped_zdr` | absent | absent | zero data retention (body flag or the token's floor): nothing was sent to a model |
| `unavailable` | absent | one of the closed codes of the table above (`synthesis_disabled`, `model_path_refused`, `no_search_provider`, `no_model_plane_url`, `model_client_init_failed`, `model_plane_auth_refused`, `model_plane_rate_limited`, `model_plane_error`, `answer_search_failed`) | no answer |
A request without `include_answer` carries neither field and is byte for byte what it was. ZDR is checked before the gates, so a ZDR request says `skipped_zdr` even when the switch is off. The `answer` and `citations` fields are unchanged.

**Metering.** `include_answer` is metered like `/v1/answer`: one `quarry.answer.synth` (quantity 1; metadata `sources_used`, `sources_skipped`, `model`, `latency_ms`, `via: "search.include_answer"`) per model call, on top of the one `quarry.search.query` the search always meters (whose metadata gains `answer_status`). Never on a cache hit, a refused or failed synthesis, a `no_sources` result or a ZDR request. The two events have different ids.

**Cache.** A search that asked for an answer and has none (switch off, refused, failed) is not written to the search cache, so turning the pipeline on shows on the very next query and not after the TTL (it used to cache `answer: null`). An older entry of that kind is treated as a miss. A real answer, and an empty `no_sources` answer, are cached as before; ZDR never reads or writes the cache.

**ZDR (ADR-4).** The model hop carries no ZDR posture, so no ZDR request reaches a model. `include_answer` under ZDR sends nothing to a model (`skipped_zdr`, 200, the search itself runs under ZDR). `POST /v1/answer` and `POST /v1/answer/stream` REFUSE a ZDR request (the body flag or the token's floor) with `403 FORBIDDEN`, `reason: zdr_unavailable`, before the deployment's gate, the answer cache and the pipeline: no search, no page fetch, no model call (a stream is answered as that JSON 403, not as a stream). The refusal comes before the 501 checks, so a ZDR caller learns nothing about this edge's switches. A refused ZDR run leaves only the metadata-only tombstone (outcome `refused`, class `FORBIDDEN`) when tombstones are on. A caller under ZDR has no synthesis today; `/v1/search` and `/v1/scrape` run under ZDR without a model.

**Owner decision INNH-8b (not built).** A deployed stack cannot turn synthesis on, whatever the edge is set to, until the cross-plane grant exists: the Control Plane registry must give quarry-edge the `model-gateway` audience (scope `models:invoke`, retention lane `zdr`), the Model Plane gateway must let a service principal reach the route Quarry calls (today only `POST /v1/ai/chat` and `/v1/ai/embeddings`; Quarry calls `/v1/invoke`), Quarry must send the second `x-inference-authorization` header, and the `model-invoke` manifest row flips to `granted: true` together with the registry grant. Until the row is granted, the boot gate REFUSES the switch (`model_path_refused`, with the ERROR log); with the dev bypass credential the switch takes effect and a gateway that refuses the call answers `model_plane_auth_refused`.

## API-key agent runs (INNH-10, Quarry side built dark, NOT run, 2026-10-31)

A caller that holds an API key (not a person's session, not a service) may drive a fenced browser run on `/v1/agent/*`. The switch is `QUARRY_EDGE__API_KEY_AGENT_RUNS` (default `0`); with it off, behaviour is exactly what it was and an API-key token is refused on every route (`api_key_agent_runs_disabled`). The whole path stays dark until Auth Core mints the token, the BrowserBroker issues key grants and the Verevon gateway composes them (design: `.plan-work/quarry-w1w2/INNH-10-design.md`). **Quarry consumes what is described here; it issues nothing.** An API key never reaches Quarry: Auth Core exchanges it for a short-lived token, the BrowserBroker issues the grant, Quarry admits the run.

**The token Quarry reads.** An RS256 JWT, `aud: quarry`, like every plane token, with these claims; anything else is a 401 (`principal_type` `api_key` is accepted only in this exact shape):

| claim | value |
|---|---|
| `principal_type` | `api_key` |
| `sub` | `apikey:<key_id>` (must equal `apikey:` plus `key_id`) |
| `key_id` | the key's id, 1 to 128 printable ASCII bytes with no space; never the key |
| `acting_user_id` | the user who owns the key, 1 to 256 bytes, no padding or control character |
| `org_id` | the organization the key is bound to |
| `scopes` | `["browser:agent"]`; a token without it is refused on every route (`api_key_scope_missing`) |
| `zdr` | the organization's posture raised by the key's own flag; counts as a floor like a user's |
| `user_id`, `service_id` | absent (either one makes the token ambiguous: 401) |
| `exp` | auth-core mints at most 120 s; Quarry rejects an expired token as for any other, and an event stream ends at `exp` |

The run's actor is `apikey:<key_id>`, so the ordinary scoping applies: a run a key starts belongs to that key, and neither its owner's session nor another key of the same owner can step, read or close it (404), and the key cannot touch the owner's runs. `acting_user_id` is read only for the grant owner check. A key is always an agent: the `initiated_by` hint cannot make it a person, the receipts say `initiated_by: agent` with `agent: {basis: "api_key", principalId: "apikey:<key_id>"}` (`basis` is a new value beside `service_principal` and `user_session`), and the web policy treats it as an autonomous principal.

**The route allow-list** (method and path, nothing else; anything else is `api_key_route_refused`, a trailing slash included):

| method | path |
|---|---|
| `POST` | `/v1/agent/runs` |
| `POST` | `/v1/agent/runs/{run_id}/step` |
| `GET` | `/v1/agent/runs/{run_id}/receipts` and `/egress-receipts` |
| `GET`, `POST` | `/v1/agent/runs/{run_id}/tabs` |
| `POST` | `/v1/agent/runs/{run_id}/tabs/{tab_id}/select` |
| `DELETE` | `/v1/agent/runs/{run_id}/tabs/{tab_id}` |
| `GET` | `/v1/agent/runs/{run_id}/events/stream` (closed with `done: token_expired` when the token that opened it expires) |
| `DELETE` | `/v1/agent/runs/{run_id}` |

Refused by omission: `interact`, `browser-session*`, `attach*`, `live-view`, `frame`, `frames/*`, `devtools`, `procedure*`, `tabs/{id}/control`, `/v1/profiles*`, `/v1/artifacts*` and every scrape, crawl, search, extract, job, schedule, change, team and fleet route.

**The grant Quarry consumes** (`ValidateGrant`, unchanged call, one new field). The grant must be an allow-list session grant issued to this key:

| field | rule |
|---|---|
| `key_id` | new, optional on the wire; must equal the token's `key_id`. Empty (every person's grant, an older broker, the gRPC validator until `proto/browser.proto` carries the field) never matches, so a key run is refused: fail closed |
| `owner_id` | must equal the token's `acting_user_id` |
| `kind` | `""` or `session`; `human_browse` is `api_key_open_web_refused`, any other kind `api_key_grant_kind_refused` |
| `policy_mode` | `""` or `allowlist`; `open_web` is `api_key_open_web_refused`, an unknown mode `api_key_grant_kind_refused` |
| `allowed_domains` | non-empty and canonical (the broker's list); they ALWAYS replace the request's `constraints.allowed_domains`, in every environment |
| `budget` | must bound at least one of `micro_usd`, `max_steps`, `max_runtime_s`; applied to the run whatever `QUARRY_EDGE__GRANT_BUDGET_CLAIM` says |
| `parent_grant_id`, `resource_id` | must be empty (no derived grants) |
| `zdr` | a ZDR grant cannot back a non-ZDR run (the existing rule) |

A key grant backs only its key's runs: a person (the owner included), another person or a service that presents one gets `api_key_grant_owner_mismatch`. Every step and every tab open revalidates the grant, and the run stops when the grant is inactive, expired, no longer carries the key's id or no longer matches the run's hosts.

**Start rules.** `POST /v1/agent/runs` with a key token: a `grant_id` is required whatever `REQUIRE_BROWSER_GRANTS` says; `resume_run_id`, `profile_id` and `persist_profile` are refused; the run is always an agent run (the organization's agent caps, plan caps and allowance apply); at most `QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY` (2) live runs per key (429 `per_key_run_cap`); and `QUARRY_EDGE__BUDGET_ENFORCEMENT` must be `on` (`api_key_budget_not_enforced`), because in `shadow` the grant's budget would bound nothing. Sensitive actions (a dialog reply, an upload, a download) are refused on the action's kind (`api_key_sensitive_action_refused`) before the driver or the broker is asked. Nothing the request carries (a hint, a body field) loosens any of this.

**ZDR.** A key run keeps exactly the persistence of any agent run: under ZDR no checkpoint, timeline row or receipt. A key run never resumes or keeps a profile in any posture. See the row `agent_run_api_key` in `docs/ZDR_MATRIX.md`.

**Reasons.** Closed list in `docs/ERROR_REASONS.md`, "API-key agent runs": `api_key_agent_runs_disabled`, `api_key_scope_missing`, `api_key_route_refused`, `api_key_grant_required`, `api_key_grant_kind_refused`, `api_key_open_web_refused`, `api_key_grant_owner_mismatch`, `api_key_grant_budget_required`, `api_key_grant_hosts_required`, `api_key_profile_refused`, `api_key_resume_refused`, `api_key_sensitive_action_refused`, `api_key_budget_not_enforced`; and 429 `per_key_run_cap`.

**Audit.** Quarry has no durable audit stream. The trail is Auth Core (who got a token), the BrowserBroker (who got a grant) and Quarry's receipts, which name `apikey:<key_id>` per step. Quarry logs `agent.run.started` and `agent.run.refused` with ids only (run, organization, key, grant, ZDR flag, class, reason): never a host, a token or the key.

## Crawl scope and robots: planned status and vocabulary (WS14.S0, 2026-10-23)

Read this with `docs/CRAWL_SCOPE_ROBOTS.md`, which is the contract for WS14. The examples earlier in this file that show crawl and map scope fields, and the crawl denial reason schema, describe the target shape. They are not what the edge applies today.

- **Scope fields are not applied on the crawl path.** `allow_external_links`, `allow_subdomains`, `include_paths`, `exclude_paths`, `ignore_robots_txt` and `sitemap` are planned for `POST /v1/crawl` under WS14.S6. Until WS14.S3 and S6 ship, the crawl follows cross-host links and does not read robots.txt. `/v1/map` reads `sitemap.xml` and applies a path-only robots matcher; the map scope fields in the schema above are not all applied there either. Check the code before relying on a field.
- **Denial codes.** The schema above lists codes from an earlier design (`exclude_pattern`, `include_pattern`, `depth_limit`, `robots_txt`, `file_type`, `url_parse_error`, `backward_crawling`, `social_media`, `external_link`, `section_link`, `non_web_protocol`, `security_blocked`, `duplicate`). The vocabulary in `crates/quarry-core/src/crawl_denial.rs` is the one to build against. It uses `out_of_scope`, `robots_disallowed`, `depth_exceeded`, `max_pages_reached`, `duplicate`, `external_host_disabled`, `security_rejected`, `include_pattern_miss`, `exclude_pattern_hit`, `content_type_rejected` and `not_crawlable`. Mapping from the old names: `robots_txt` to `robots_disallowed`; `external_link` to `external_host_disabled`; `exclude_pattern` to `exclude_pattern_hit`; `depth_limit` to `depth_exceeded`; `file_type` to `content_type_rejected`; `url_parse_error` and `non_web_protocol` to `not_crawlable`; `section_link` to `out_of_scope`. `backward_crawling` and `social_media` have no code in the enum today and are not planned. `security_blocked` at fetch time stays the `SECURITY_BLOCKED` (403) error code with `details.denial` (no new `ErrorCode`).
- **`page.blocked` and terminal payloads** are planned as in `docs/CRAWL_SCOPE_ROBOTS.md`, section 6. No new event type is added.
- **Crawl and batch under ZDR** are still refused, with the status and text of the refusal unchanged (WS14 keeps the refusal; ADR-4).

## Job lifecycle reads and origin (WS15.L1, unit W2-U11, built, NOT run, 2026-10-23)

Migration `016_jobs_lifecycle.sql` (control) adds the job record fields below. Every
column is additive and defaulted; rows written before it read as origin `unknown`.

**Job statuses (closed).** `accepted` (waiting; the phase reads `queued`), `running`,
`paused`, and the terminal statuses `completed`, `failed`, `cancelled`, `timed_out`,
`skipped`. A terminal job takes no further transition: a late `run_resumed` or
`run_failed` leaves it as it is, and `PUT /v1/jobs/{id}` refuses to leave a terminal
status with 409.

**Origin (closed).** `origin.type` is `user`, `agent`, `schedule`, `webhook` or `api`
(`unknown` only for rows written before migration 016). `origin.actor` is the verified
user or service principal; `origin.ref` is the Model Plane run id for `agent` origins
only, taken from `x-verevon-run-id` on service tokens. The origin is stamped from the
verified claims at the edge and never read from a request body. Control refuses any
other type with 400; an empty type is `api`.

**Handoff.** `POST /v1/crawl` and `POST /v1/batch` post `{kind, params, origin}` to
control's `POST /v1/jobs`, signed when the internal HMAC is configured, and forward the
caller's `Idempotency-Key` (1 to 128 characters) when one was sent.

**Job record (wire).** `origin`, `desired_state` and `desired_seq`/`applied_seq` (set by
WS15.L2; empty and zero until then), `workflow_id`, `deadline_at`, `terminal_reason`
(the event type that ended the job), `finished_at`. List rows (`GET /v1/{kind}/jobs`)
also carry `phase` and `stats` (`completed`, `failed`, `total`).

**Status document (edge).** `GET /v1/crawl/{id}`, `/v1/batch/{id}`, `/v1/jobs/{id}`:
`job_id`, `kind`, `status`, `phase`, `terminal`, `origin`, `created_at`, `finished_at`,
`deadline_at`, `counts` (`completed`, `failed`, `blocked`, `retried`), `total` and
`total_kind` (`exact` for a batch, the URL count; `budget` for a crawl, the effective
page ceiling, not a prediction), `warnings` (closed codes; `max_pages_clamped` when the
requested page limit was lowered), `control` (`desired`, `pending`), `schedule_id`,
`run_id`. A foreign id and a kind mismatch are both 404.

**Credits per job (INNH-9, built, NOT run, 2026-10-31).** `GET /v1/crawl/{id}`, `/v1/batch/{id}`,
`/v1/jobs/{id}` and the list rows (`GET /v1/{kind}/jobs`, `/v1/crawl/ongoing`, `/v1/batch/ongoing`)
gain an optional `credits` object:
`{"used": 12.0, "unit": "credit", "basis": "pages_fetched", "price_per_page": 1.0,
"estimated": true, "final": false}`. It is derived at read time, never stored: `used` is the
job's `page_fetched` event count (`counts.completed`) times `price_per_page`, so it is
retroactive for old jobs and a late page after the terminal event is still counted.
`final` is false while the status is `accepted`, `running` or `paused` and true once it is
terminal. It is an estimate (`estimated` is always true), not what billing-core invoices:
a flat price per fetched page, a browser-rendered page priced like any other, failed and
blocked pages free. The field is absent (not null) for `search`, `research` and `agent`
jobs, which are not page-priced, and for every job while control runs with
`QUARRY_CONTROL__JOB_CREDITS` off (the default, read per request). The edge forwards what
control sends and holds no price or flag. Additive: no request change, no new route, no new
error code. The one price is `store.CreditsPerPage` in quarry-control, used by both this
figure and `GET /v1/team/credit-usage`. That team figure changed on Postgres: it used to add
one credit for every event with a job id (`run_started`, `page_queued`, ...) and now counts
`page_fetched` events only, as the in-memory store always did (the unused `payload.pages`
multiplier is gone). Team credits on Postgres therefore read lower than before and equal the
sum of the per-job credits. Control's own `GET /v1/jobs/{id}/summary` carries `credits` first.

**Errors (edge).** `GET .../errors`: `errors` lists failed or retried pages whose final
attempt the orchestrator marked (`final: true`), with `code` and `category`, `status` and
`retryable` where known. `blocked` lists pages a rule blocked, with `reason`.
`robots_blocked` is empty until robots enforcement (WS14.S4). The raw error text a worker
recorded is not returned. `next_seq` is set when the page was full; pass it as
`after_seq`. Under the token's ZDR floor the whole route answers 403, because it returns
URLs.

**Ongoing.** `GET /v1/crawl/ongoing` and `/v1/batch/ongoing` list the org's jobs in
`accepted`, `running` or `paused`, oldest first.

**Service principals** have no route to these paths: `require_service_route_scope`
answers 403 by default, and these paths are not in its list.

**Error reasons.** No new `ErrorCode`: 404 `NOT_FOUND`, 400 `BAD_REQUEST` (unknown origin
type or status filter), 409 `CONFLICT` (a terminal job, or a failed `if_status`), 403
`FORBIDDEN` (ZDR floor on errors).

Goldens: none changed. The OpenAPI paths are in `docs/openapi.yaml`.

## Agent run budgets: `meta.budget`, `cost_source`, `usage` and `agent.budget_exhausted` (WS6.B, W2-U21, built, NOT run)

An agent-class run (a service principal, or `initiated_by: "agent"`) is bounded in steps, attempts, active runtime and estimated cost. A person's session is never capped. Enforcement is `QUARRY_EDGE__BUDGET_ENFORCEMENT` = `off` | `shadow` (default) | `on`; in `shadow` nothing is refused. The gate sits at position 9 of the admission order (after the budget-free checks, before the sensitive approval).

- **`meta.budget`** on a REST step response (`POST /v1/agent/runs/{id}/step`) of an agent-class run: `{class, steps, attempts, max_steps?, max_attempts?, elapsed_s, max_runtime_s?, spent_usd, max_cost_usd?, estimated: true, cost_source, exhausted?}`. The `max_*` bounds and `exhausted` are absent for a person. `cost_source` is `flat_table` (the flat action table) or `provider_billed` (the driver reported the figure). Absent when enforcement is `off`. The WebSocket reply carries no `meta.budget` yet (not done in W2-U21).
- **Receipts** carry `cost_source` (`flat_table` | `provider_billed`; the column `quarry_step_receipts.cost_source`, migration `0012`). A failed attempt is charged the flat cost and its receipt now carries that cost too (it used to record 0); a skipped receipt has no cost and no source. A receipt written before migration `0012` has no `cost_source`.
- **`DELETE /v1/agent/runs/{id}`** returns `data.usage` for an agent-class run: `{class, spent_usd, steps, attempts, active_s, exhausted?}`. A person's session returns `{closed: true}` without `usage`.
- **`agent.budget_exhausted`** (`EventType::AgentBudgetExhausted`, durable type token `agent_budget_exhausted`): emitted once per segment of a run, when `on` refuses. Payload `{dimension, limit, spent, unit, estimated, limit_source}`; a ZDR run emits it through `emit_for_zdr` (nothing persisted). It is user-facing (X-12): the run's SSE stream (`GET /v1/agent/runs/{run_id}/events/stream`, `is_user_facing_event`) sends it under the name `agent.budget_exhausted`, once per segment however many calls are refused; the payload carries the refusal's figures only (no URL, no page content, no prompt). Its path to the run trace is a WS10 projection row, not built here. The refusal also writes a checkpoint with the exhaustion latch, so a resume refuses at once and cannot refill.
- **Legacy request cap.** `constraints.max_steps` still refuses with `429 RATE_LIMITED` ("agent step budget exhausted") where the gate does not own the bound: when enforcement is `off` (for everyone), and for an agent run while enforcement is `shadow`. In `on` the typed `402 BUDGET_EXHAUSTED` replaces it for agent runs, and a person's session is never refused for its own step count (a UI session that sends `max_steps: 12` is no longer refused at the 13th action when enforcement is `on` or `shadow`).

**Error reasons.** `BUDGET_EXHAUSTED` (402, not retryable) is unchanged from the WS6.B gate; its `details` are `{dimension, limit, spent, unit, estimated, limit_source, resumable: false}`.

Goldens: none changed. The OpenAPI schemas are `BudgetMeta` (new), `Envelope_BrowserObservation.meta` and the `DELETE` response `usage` (additive).

## Usage events for browser, agent and crawl work (WS6.C-e)

The usage event shape is unchanged (`UsageEvent`, `event_id`, `metadata`). New here:

- Seven metric names (`quarry.browser.second.agent`, `quarry.browser.second.interactive`,
  `quarry.agent.run`, `quarry.agent.step`, `quarry.crawl.page`, `quarry.extract.url`,
  `quarry.map.call`), published only under `QUARRY_EDGE__USAGE_BROWSER_METRICS=on`. Full table and
  mode rules in `BILLING.md`, section "Browser and crawl metrics (WS6.C-e)".
- Event ids for run segments are `quarry:<run_id>:s<segment>:<metric>`. `segment` is a checkpoint
  field (`budget.segment`, absent on a checkpoint written before it existed and read as `0`).
- The orchestrator's run_page body may carry `kind` (`crawl` or `batch`). Absent means a scrape or a
  monitor. Only `crawl` is metered (`quarry.crawl.page`). The field is not in `openapi.yaml` as a
  documented request property yet (depth item).
- The `x-verevon-surface` header is read on the agent start route and honoured only on a service
  token, from the closed set in `BILLING.md`.

## Organization limits: allowance clamps and the 402 `org_allowance` (WS6.D-s, built, NOT run)

The edge holds a limits resolver (`AppState.limits`). Only the `static` source is built: it reports no
allowance for any organization, so every response below is what it was before, plus the additive fields.

- **Crawl.** `POST /v1/crawl` clamps the page bound to the organization's remaining
  `quarry.crawl.page` when an allowance is configured: `effective = min(request-or-default, hard,
  remaining)`. `limits.max_pages` gains `remaining` (absent when no allowance is configured; never 0)
  and `clamped_by` may now contain `entitlement` next to `default` or `hard`. A configured allowance
  with nothing left is refused before any job exists.
- **Batch.** `POST /v1/batch` keeps the first N URLs that fit the remaining `quarry.batch.url`
  allowance and answers `limits.urls = {requested, accepted, dropped, remaining?, clamped_by[]}`
  (counts only, never a URL). The block is always present on a batch acknowledgement; `accepted` equals
  `requested` and `clamped_by` is empty when no allowance is configured. Only the accepted URLs reach
  the orchestrator and the usage meter.
- **Agent run.** `POST /v1/agent/runs` for an agent-class run: a configured, spent
  `quarry.browser.second.agent` allowance refuses the start like the other budget bounds (402 when
  `QUARRY_EDGE__BUDGET_ENFORCEMENT=on`; `shadow` records it and serves). What is left becomes the
  runtime bound: `clamped[]` gains `{field: "max_runtime_s", by: "entitlement"}` and a later runtime
  refusal says `limit_source: "entitlement"`. An entitlement only lowers a bound; a person's session is
  never limited (ADR-3).
- **402.** `BUDGET_EXHAUSTED` (402, not retryable) with `details = {dimension: "org_allowance",
  metric, limit_source: "entitlement", resumable: false}`. `metric` is the allowance that ran out:
  `quarry.crawl.page`, `quarry.batch.url` or `quarry.browser.second.agent`. No count and no
  organization id is in the body.
- **Unreadable limits.** Never from the static source. Under a source that can fail, fail mode `floor`
  serves the floor, `open` drops the entitlement ceiling, and `closed` answers `RUNTIME_NOT_READY`
  (503, `details.reason = "limits_unreadable"`).
- **Gateway.** The 402 reaches the app as `crawl_allowance_exhausted` (HTTP 402; `error.metric` for the two
  crawl metrics and `error.retryAfterSeconds` when sent), with Norwegian and English copy.

Goldens: none changed. The OpenAPI changes are `CrawlBound.remaining`, `entitlement` in `clamped_by`,
`BatchUrlsBound` (new) and the 402 on `/v1/crawl` and `/v1/batch` (all additive).

## Scrape captures: `screenshot` and `pdf` formats (WS13.4b, unit W2-U45, built, NOT run, 2026-10-24)

Additive to section 10 of this document. Request: `formats` may carry `"pdf"` or `{"type":"pdf","format":"A4"}`
and `{"type":"screenshot", ...}` (at most one of each; the counts of captures and actions share one cap of eight).
Refused with a 400 field error `feature_disabled` at `formats` while `QUARRY_EDGE__SCRAPE_CAPTURE` is not `on`.

Response (`ScrapeResponse`, all optional, absent unless asked):
- `formats.screenshot` and `formats.pdf`: `{artifact_id, bytes}`; not ZDR only.
- `screenshot`: `{content_type, bytes, width?, height?, data_base64}`; ZDR only (`image/png` or `image/jpeg`).
- `pdf`: `{content_type: "application/pdf", bytes, data_base64}`; ZDR only.
- `captures`: `{screenshot?: {content_type, width, height, bytes, tiles, stitched, truncated}, pdf?: {bytes},
  blocked_requests}`; both modes; counts and sizes only, never a URL.
- `warnings[]` codes added: `screenshot_failed`, `pdf_failed`, `screenshot_truncated`, `capture_exceeds_inline_cap`.
- `omitted[]` may name `screenshot` or `pdf` (over the ZDR inline cap).

Errors: 501 `UNSUPPORTED` `details.reason = "browser_capability_unavailable"` (a browser that cannot capture);
`/v1/scrape/stream` refuses capture formats with `details.reason = "unsupported_in_stream"`. No new `ErrorCode`.

## OpenAPI: the public spec and the internal spec (OPENAPI-INTERNAL, decision 3.13, 2026-10-28)

- `docs/openapi.yaml` is the public spec. `sdks/generate.sh` generates the SDKs from it and from nothing else. It has no `/v1/internal/*` path, no operation tagged `internal` and no `Internal*` schema; `scripts/tests/openapi-internal-guard.test.mjs` fails if one appears.
- `docs/openapi-internal.yaml` describes the `/v1/internal/*` routes the edge registers: run_page, change/record, page-images, schedules/preflight, orgs/{org}/profiles, and the queue routes (enqueue, enqueue_batch, pop, ack). It is repository documentation only. It is never served, no SDK is generated from it, and no code or script loads it. The guard fails if the internal spec and the registered internal routes differ.
- Who can reach an internal route is decided by gateway routing, not by the file (D2-312, not proven live). The page-images route has no token: its content hash is the capability on the trusted bus, so it must never be routed publicly.
- The eight `/v1/internal/*` routes of the Go control service (quarry-control: jobs claim, pending-control, control-ack, running, finalize; schedules desired and runs; zdr-tombstones) are not described yet. They move with the D2-050 internal secret split.
- Every route the edge registers is in one of the two spec files or on `scripts/openapi-route-allowlist.json` with a reason (`scripts/tests/openapi-route-coverage.test.mjs`).
- The generator does not delete files it no longer writes. After a spec change, delete the generated files that no longer exist (the gitignored `sdks/python/quarry_client`, `sdks/python/docs`, `sdks/python/test`, `sdks/typescript/src`, and the tracked `sdks/typescript/docs`).
