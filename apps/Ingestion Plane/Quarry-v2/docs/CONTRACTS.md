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
- `agent.started`, `agent.delta`, `agent.completed`, `agent.failed`, `agent.expired`
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

Today `formats` holds `html`, `markdown`, `raw`, `links`, `screenshot`, `pdf` and `extract` (`quarry_core::output::OutputFormats`); the other entries above are the planned shape.

### Requested formats schema

What `POST /v1/scrape` accepts today is in 9.1: `markdown`, `html`, `rawHtml`, `links` and `metadata`, where `links` and `metadata` change nothing because both are in every response. `json`, `screenshot`, `summary`, `question` and `highlights` are reserved and refused with 400 `reserved_format`. Any other name, and any object form such as `{ "type": "json" }`, is refused with 400 `unsupported_format`. The example and the rules below are the target design for the formats not built yet.

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

Agent events use `agent.started`, `agent.delta`, `agent.completed`, `agent.failed`, and `agent.expired` (the run sweeper released an idle or over-age run; payload `{reason: idle|ttl|takeover_idle|max_age, idle_s, age_s}`, no URL; a non-ZDR run stays resumable). Agent tool calls must go through the same action/runtime policy path as user-supplied `actions[]`.

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

## Who holds the browser: `USER_TOOK_OVER` (409)

A run has a control mode (`agent_control` or `human_takeover`) and an epoch that counts real mode changes (it never goes down). While a person holds the browser, an action proposed by an agent is refused with `409 USER_TOOK_OVER`, `details {control_mode: "human_takeover", control_epoch}`; a person's own actions go through. The initiator is derived, never taken on trust: a service principal is always an agent; a person's token is a person unless the request says `initiated_by: "agent"` (or, on the WebSocket, `actor`), which can only tighten. Only a person may change the mode (`403 control_transition_requires_human` otherwise), and `expected_epoch` on the control request makes the change a compare-and-set (`409 CONFLICT` with the current epoch). Receipts and the owner timeline carry `initiated_by` and `control_epoch`; a refused agent action leaves one `skipped` receipt per takeover. Enforcement is `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` (default off: the edge logs `would_reject` and refuses nothing until the planner side maps the 409).

## 9.1 `POST /v1/scrape` request options (WS5.D)

Root-level, all optional, snake_case with a camelCase alias: `formats` (`markdown`, `html`, `rawHtml`, `links`, `metadata`; `links` and `metadata` change nothing, since both are already in every response), `only_main_content`, `include_tags`, `exclude_tags`, `remove_base64_images`, `extractor` (`readability`, `clean_full`, `raw`), `timeout_ms` (1000 to 55000, default 55000), `max_inline_bytes` (1 to 8 MiB, default 1 MiB).

All eight are applied. `exclude_tags` (CSS selectors) removes every match and its subtree. `include_tags` keeps only the matching subtrees, in document order, each once; an explicit selection beats the main-content heuristic, so the heuristic does not run on it, and a list that matches nothing gives empty markdown and the warning `include_tags_matched_nothing`, never the whole page. The selection is then cleaned the way `clean_full` cleans a page (lazy images resolved, inline images stripped), whatever `extractor` and `remove_base64_images` say. A selector that does not parse is a 400 `invalid_selector` at `include_tags[i]` or `exclude_tags[i]`. `only_main_content` (default true) and `extractor` say one thing two ways: `readability` is the main-content heuristic, `clean_full` keeps the whole body minus head, scripts, styles, noscript, templates and iframes (lazy images resolved, inline images stripped) and `raw` converts the document as it is. An unset one follows the other; setting them to disagree is a 400 `conflicts_with_only_main_content` on `extractor`. `remove_base64_images` strips inline `data:` images on the readability and raw paths too. With default options the markdown is byte-identical to what a scrape returned before these options existed. `html` is the HTML the markdown was made from (after exclusion, inclusion and cleaning); `rawHtml` is the document as fetched. `effective_options.ignored_fields` now lists only unknown request keys. `json`, `screenshot`, `summary`, `question` and `highlights` are reserved: a 400 `reserved_format`.

Response additions on `data`: `markdown`, `html`, `raw_html` (inline, only when asked for; `markdown` also by default under ZDR), `truncated[]` (ZDR: cut at the cap), `omitted[]` (not ZDR: over the cap, read the `formats.*` artifact), `warnings[]`, `effective_options`. `formats.*` artifact references are unchanged. A request that names `formats`, `include_tags`, `exclude_tags`, `remove_base64_images`, `only_main_content: false` or an `extractor` other than `readability` neither reads nor writes the page cache. `timeout_ms` and `max_inline_bytes` change delivery, not the result, so they do not bypass it. ZDR never touches it.

Unknown keys: ignored with a `warnings[] {code: "unknown_field", field}` entry, `effective_options.ignored_fields`, `Deprecation: true` (and `Sunset` when `QUARRY_EDGE__SCRAPE_STRICT_AFTER` is set) and a `quarry.scrape.unknown_field` log line naming the keys. `QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION=strict` (or a passed strict-after date) makes them a 400. `skipTlsVerification`, `skip_tls_verification`, `proxy`, `proxyUrl`, `headers`, `mobile` and `stealth` are always a 400 `reserved_field`. A bad value is a 400 `BAD_REQUEST` with `details.fields[] {path, code, message}`, including a body that is not JSON. `QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED=0` turns the option keys back into unknown keys (the kill switch); `QUARRY_EDGE__SCRAPE_INLINE_MAX_BYTES` changes the default inline cap.

**Scrape record (WS5.F).** A non-ZDR scrape stores one small `scrape_record` artifact (the normalized output and `effective_options`, never inline text) and returns its id as `data.scrape_id`. `GET /v1/scrape/{id}` returns it again for the same organization: another organization's id is a 404, an id that is not a scrape record is a 404, and a reader whose organization is under ZDR gets a 403 even for an earlier record. A ZDR scrape stores nothing and has no `scrape_id`; a failed write adds the warning `scrape_record_unavailable`. Service principals need `scrape:read` or `scrape:write`. `QUARRY_EDGE__SCRAPE_RECORDS=0` stops storing records. Records live as long as artifacts do.
