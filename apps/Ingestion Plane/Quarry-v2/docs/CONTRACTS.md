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
- `agent.started`, `agent.delta`, `agent.completed`, `agent.failed`
- `artifact.written`, `snapshot.created`, `store.record.written`
- `lease.acquired`, `lease.released`, `profile.restored`, `profile.captured`
- `change.detected`, `change.unchanged`
- `schedule.fired`

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
    "backend": "s3 | minio | fs",
    "bucket": "quarry-snapshots",
    "key": "org=org_.../profile=prof_.../snapshot=snap_.../state.bin.enc"
  },
  "encryption": {
    "scheme": "xchacha20poly1305 | aes-256-gcm",
    "key_source": "env:QUARRY_PROFILE_KEY | kms:<key-id>",
    "nonce": "base64...",
    "aad": "org=org_...;profile=prof_...;snapshot=snap_..."
  }
}
```

Browserless-style session URLs (`connect`, `browserQL`, `stop`, reconnect URL) are secrets. They may be stored only in encrypted snapshot metadata or a secret store, never in public event payloads.

Production snapshot storage uses real S3. Local development and tests use MinIO-compatible S3 APIs. `QUARRY_PROFILE_KEY` is the required first key source; KMS integration is additive later.

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

### Requested formats schema

Firecrawl-compatible names are accepted at the edge and normalized internally.

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

Agent events use `agent.started`, `agent.delta`, `agent.completed`, and `agent.failed`. Agent tool calls must go through the same action/runtime policy path as user-supplied `actions[]`.

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
