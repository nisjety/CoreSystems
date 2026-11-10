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

## 4. Webhook delivery: what a subscriber's endpoint receives (INNH-5, 2026-10-31)

This section used to describe a `{delivery_id, signature, event}` wrapper signed with an "org webhook secret". Neither was ever built. What control sends is below; it is what `internal/dispatcher` does and what its tests pin. The subscription API that creates the endpoint's registration is the section "Webhook subscriptions" further down.

**Request.** `POST <the subscription's url>`, `Content-Type: application/json`, one header `X-Quarry-Signature`, and the body below. No redirect is followed. The connection is made to an address vetted at connect time: a private, loopback, link-local, metadata or carrier-grade-NAT address is refused (a blocked target is dead-lettered at once, `last_error` is `blocked_target`). The timeout is 30 seconds.

**Body** is the control event as JSON, byte for byte what was signed:

```json
{
  "event_id": "evt_...",
  "run_id": "run_...",
  "job_id": "job_...",
  "type": "change_detected",
  "ts": "2026-10-31T10:00:06Z",
  "seq": 0,
  "payload": {
    "subject": "quarry.change.detected",
    "emitted_at": "2026-10-31T10:00:06Z",
    "source_url": "https://example.com/pricing",
    "org_id": "org_...",
    "status": "changed",
    "new_baseline": { "baseline_id": "bln_...", "fingerprint": "blake3:...", "artifact_id": "art_...", "captured_at": "..." },
    "prev_baseline": { "baseline_id": "bln_...", "fingerprint": "blake3:...", "captured_at": "..." },
    "diff_id": "diff_...",
    "checked_at": "2026-10-31T10:00:05Z"
  },
  "idempotency_key": "..."
}
```

`type` is the stored event type (`change_detected`); `payload.subject` keeps the dotted subject `quarry.change.detected`. `run_id` and `job_id` are present only when the event belongs to one. `new_baseline`, `prev_baseline`, `diff_id` and `artifact_id` are present when the change monitor recorded them (see "Change text diff"). No page text is in the body; `GET /v1/change/diff?diff_id=` reads it, org-scoped. A page URL and two content fingerprints are in it, which is why a Zero Data Retention organization cannot subscribe.

**Signature.** `X-Quarry-Signature: t=<unix seconds>,v1=<hex>`, where `<hex>` is the lowercase hex of HMAC-SHA256 keyed with the subscription secret (the whole `whsec_...` string, as UTF-8 bytes) over the ASCII string `<t>` + `.` + the raw request body. To verify: split the header on `,`, read `t` and `v1`, recompute over the raw bytes you received (not a re-serialised JSON), compare in constant time, and reject a `t` more than 5 minutes from your clock (replay). The timestamp is that of this attempt, so a retry carries a fresh signature over the same body. After `rotate-secret` the old secret stops verifying at once.

**Outcome and retries.** A `2xx` is delivered. `408`, `429` and `5xx` and a transport failure are retried after 5 s, 30 s, 5 min and 1 h; the fifth failed attempt makes the delivery `dlq`. Any other `4xx` is `dlq` at once. A `dlq` delivery is visible in the delivery log and can be re-queued from the API; it is not retried on its own. Delivery is at least once and not ordered: de-duplicate on `event_id`. One detected change reaches control by more than one path; control keeps one delivery per subscription for it (the key is the org, the URL and the new baseline),; that de-duplication applies only when the event names a baseline id.

**Tenancy.** An event is delivered only to the active subscriptions of the organization that owns it. An event whose owner is unknown is delivered to nobody. A subscription without an organization (rows from before migration 025) receives nothing, and the dispatcher re-checks that at send time.

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

The assistant gateway calls `POST /v1/browser/redirect-hop` with `{url}` to inspect one
public HTTP(S) response. Service actors require `browser:execute`. The response
envelope carries `data.target` as an absolute URL for a validated 301, 302, 303, 307
or 308 redirect, or `null` for other statuses. The probe pins public DNS addresses,
checks Quarry web policy, and reads the response headers without cookies or storage.
The gateway checks site access and the next hop before continuing the chain.
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

This is a planned shape and nothing produces it. What a change monitor stores, with `QUARRY_EDGE__CHANGE_TEXT_DIFF` on, is the paragraph diff in "Change text diff" below.

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

Root-level, all optional, snake_case with a camelCase alias: `formats` (`markdown`, `html`, `rawHtml`, `links`, `metadata`; `links` and `metadata` change nothing, since both are already in every response; `screenshot` and `pdf` are in section 10 and the AI formats `json`, `summary`, `question` and `highlights` in "Scrape AI formats"), `only_main_content`, `include_tags`, `exclude_tags`, `remove_base64_images`, `extractor` (`readability`, `clean_full`, `raw`), `timeout_ms` (1000 to 55000, default 55000), `max_inline_bytes` (1 to 8 MiB, default 1 MiB), and `max_age` (milliseconds, 0 to 30 days; INNH-7, see "`max_age`" below).

All eight are applied. `exclude_tags` (CSS selectors) removes every match and its subtree. `include_tags` keeps only the matching subtrees, in document order, each once; an explicit selection beats the main-content heuristic, so the heuristic does not run on it, and a list that matches nothing gives empty markdown and the warning `include_tags_matched_nothing`, never the whole page. The selection is then cleaned the way `clean_full` cleans a page (lazy images resolved, inline images stripped), whatever `extractor` and `remove_base64_images` say. A selector that does not parse is a 400 `invalid_selector` at `include_tags[i]` or `exclude_tags[i]`. `only_main_content` (default true) and `extractor` say one thing two ways: `readability` is the main-content heuristic, `clean_full` keeps the whole body minus head, scripts, styles, noscript, templates and iframes (lazy images resolved, inline images stripped) and `raw` converts the document as it is. An unset one follows the other; setting them to disagree is a 400 `conflicts_with_only_main_content` on `extractor`. `remove_base64_images` strips inline `data:` images on the readability and raw paths too. With default options the markdown is byte-identical to what a scrape returned before these options existed. `html` is the HTML the markdown was made from (after exclusion, inclusion and cleaning); `rawHtml` is the document as fetched. `effective_options.ignored_fields` now lists only unknown request keys. The AI formats (`json`, `summary`, `question`, `highlights`) are in "Scrape AI formats" below; `reserved_format` is gone.

Response additions on `data`: `markdown`, `html`, `raw_html` (inline, only when asked for; `markdown` also by default under ZDR), `truncated[]` (ZDR: cut at the cap), `omitted[]` (not ZDR: over the cap, read the `formats.*` artifact), `warnings[]`, `effective_options`. `formats.*` artifact references are unchanged. A request that names `formats`, `include_tags`, `exclude_tags`, `remove_base64_images`, `only_main_content: false` or an `extractor` other than `readability` neither reads nor writes the page cache. So does a request whose `signals` select the browser driver (any `actions`, `screenshot`, `pdf`, `prior_block_signals` of 2 or more under `QUARRY_BLOCK_ESCALATION=legacy` (the field is deprecated and ignored under `honest`), or `profile_required`): a cached static result cannot carry what the browser renders or captures. `timeout_ms` and `max_inline_bytes` change delivery, not the result, so they do not bypass it. ZDR never touches it. This paragraph is about the page cache (`quarry:page:...`, read only through `prev_fingerprint`); the URL result cache that `max_age` reads is a separate key space with its own rules (below), and both are untouched by a request without `max_age` and without `prev_fingerprint`.

Unknown keys: ignored with a `warnings[] {code: "unknown_field", field}` entry, `effective_options.ignored_fields`, `Deprecation: true` (and `Sunset` when `QUARRY_EDGE__SCRAPE_STRICT_AFTER` is set) and a `quarry.scrape.unknown_field` log line naming the keys. `QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION=strict` (or a passed strict-after date) makes them a 400. `skipTlsVerification`, `skip_tls_verification`, `proxy`, `proxyUrl`, `headers`, `mobile` and `stealth` are always a 400 `reserved_field`. A bad value is a 400 `BAD_REQUEST` with `details.fields[] {path, code, message}`, including a body that is not JSON. `QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED=0` turns the option keys back into unknown keys (the kill switch); `QUARRY_EDGE__SCRAPE_INLINE_MAX_BYTES` changes the default inline cap.

**`max_age` (INNH-7, `QUARRY_EDGE__SCRAPE_MAX_AGE`, off by default).** `max_age` is an integer in milliseconds (0 to 2592000000, alias `maxAge`) and means what Firecrawl's `maxAge` means: serve a stored result no older than this instead of fetching. It reads a second cache, `quarry:scrape:{org}:{blake3(canonical URL, options key)}`, never the page cache. The key is the organization (a hard boundary; an empty org id is never a key), the canonical URL (host case, default port, fragment and `utm_*`/`gclid`-style parameters do not matter; a trailing slash and the query order do) and the options that change a result (`formats`, `include_tags`, `exclude_tags`, `only_main_content`, `extractor`, `remove_base64_images`), so one URL under two option sets is two entries. `max_age`, `timeout_ms` and `max_inline_bytes` are not in the key.

- Absent: the cache is not read; the response is what it was. `0`: not read, but the fresh result is written. `N`: a stored result for the key is served when `now - fetched_at <= N`; otherwise the page is fetched and the entry replaced.
- A hit does not fetch, is not metered as `scrape.page` (the existing page-cache hit is not either; whether a cache hit bills is a product decision, recorded as open), makes no new record (`scrape_id` is absent), and returns the ORIGINAL `data.fetched_at`. `effective_options` gains `max_age` (the echo), and on a hit `cache_state: "hit"`, `cached_at` (RFC 3339, the original fetch) and `cache_age_ms`. On a miss `cache_state` is `miss` (it is `bypass` when the cache was not consulted: `max_age` 0, `cache.mode` `bypass` or `write_only`, or the request is not cacheable).
- The entry holds the output (with its artifact references) and the run's warnings, never inline text. A hit rebuilds `markdown`, `html` and `rawHtml` from the stored artifacts of the run that produced it, cut at the request's `max_inline_bytes` as a run would; an artifact that is missing makes the entry a miss and the page is fetched again, never an error.
- Written after every cacheable 2xx success of the static or TLS driver that the run did not flag as a bot wall or a soft-404 (the pipeline marks both in every `QUARRY_CHALLENGE_ENFORCE` mode: under the default `log` such a page is still returned, with `cache_state` `miss`, but it is never written, so the next `max_age` call fetches again; under `enforce` a bot wall fails the run), for the TTL `cache.max_age_s` (default 3600 s). A `max_age` longer than the TTL therefore behaves as the TTL. `cache.mode` gates it: `read_only` never writes, `write_only` and `bypass` never read. `cache` with only `max_age_s` (no `mode`) is valid and means `read_write`; `cache.max_age_s` is the write TTL in seconds and `max_age` the read freshness in milliseconds.
- Never cached, with the warning `max_age_ignored` when `max_age` was sent: a capture (`screenshot`, `pdf`), `actions`, an AI format (`json`, `summary`, `question`, `highlights`), a request planned on the browser driver (and a result the browser served), a non-default web policy, `ingest: true`, a privacy class of `zdr_ephemeral` or `credential_or_secret`, a non-2xx result. Under ZDR (body flag or the token's floor) the cache is neither read nor written and no key is computed, not even a hashed one: the warning is `max_age_ignored_zdr`, `cache_state` is `bypass`. With no cache backend (no Redis) `max_age` is ignored with the warning `cache_unavailable`, never an error.
- The URL is still gated by the web policy before the cache is read, so a denied page is never served from it.
- Switch off: `max_age` is a 400 `feature_disabled` at `max_age`. `QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED=0` turns it, like every option, back into an unknown key. In strict validation `max_age` is a known key. Errors: `out_of_range` (above 30 days), `invalid_type` (not a non-negative integer), `conflicting_aliases` (`max_age` and `maxAge` with different values).
- Not in this version: crawl, batch and `/v1/internal/run_page` have no `max_age` (they keep `prev_fingerprint`), nor `stale_while_revalidate_s`, `min_age` or a `store` switch.

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

## 9.6 Sensitive ref action `fill_otp_ref`: a one-time code into a page (E3.1-ab, dark)

Status: dark, and not wired end to end. The edge refuses the action with 403 `FORBIDDEN` while `QUARRY_EDGE__FILL_OTP` is off (the default). The executor that fills the page is not connected yet (`docs/OTP_HANDOFF.md`, B3 and B4). The model may propose the action; Quarry checks the approval and the page, then executes or rejects it. A model never grants the approval. The OpenAPI schema is `FillOtpRefAction` in `docs/openapi.yaml`, and the row below is the same contract.

| Action | Wire fields | Approval | Refused | Receipt |
|---|---|---|---|---|
| `fill_otp_ref` | `snapshot_id`, `generation`, `ref_id`, `approval_grant_id`. No value field: a value, code or text key is ignored, never kept or echoed. | A single-use `fill_otp` approval grant (child of the run grant) for the exact top-frame target, used once per live run. | 403 `FORBIDDEN` while the flag is off. 409 `TARGET_REPAIR_REQUIRED` for a stale or absent target. An API-key run is refused with `api_key_sensitive_action_refused`. The page checks below refuse with 403 `FORBIDDEN` and a closed `otp_*` `details.reason`. | Outcome `otp_filled` only. The code is never in a receipt, event or response. |

The fill checks these before any code is read, in this order (`crates/quarry-browser/src/actionability.rs`, and the driver's `otp_fill_precheck`):

1. The top document is `https`. Loopback `http` is for that crate's own tests only.
2. The target is in the top frame. A child-frame target fails closed, because there is no frame variant.
3. The frame origin equals the top origin, and the grant origin is `https` and equals the top origin too.
4. The target is one visible, required, non-password text `<input>`, and the only visible text field on the page.
5. The page offers no other way to get the code: no `datalist` on the target, no visible `select`, radio or listbox, and no visible QR code.
6. After the usual actionability checks, the insert is trusted input (focus, select all, `Input.insertText`). A field that would need the in-page value setter is refused, never set from script.

After a fill step, succeeded or not, the frame's reads (accessibility tree, DOM and screenshot) are withheld until the frame navigates. ZDR: the code is held only in a zeroed buffer and dropped after the insert; it is not stored. A ZDR run keeps no receipts today, and the boolean receipt that a ZDR run needs is not built (`docs/OTP_HANDOFF.md`, sections 3 and 8).

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

- `status` `ok`: the result field is present. `skipped`: no model call was made for this format (`ZDR_UNAVAILABLE`, `PRIVACY_POLICY` when the request's `privacy.privacy_classification` is `credential_or_secret` or `zdr_ephemeral`, `BUDGET_EXHAUSTED`, `EMPTY_SOURCE`, or `MODEL_RATE_LIMITED` for formats not started when a sibling was rate limited). `error`: a call was made and failed (`MODEL_UNAVAILABLE` with a `reason`, `MODEL_RATE_LIMITED` with `retry_after_s`, `SCHEMA_INVALID` with `schema_issues`).
- `reason` is one of `model_lane_denied`, `model_lane_unauthorized`, `model_rejected_request`, `upstream_unavailable`, `timeout`, `transport`, `bad_reply`, `not_json`, `too_large`, `deadline`.
- A model failure never fails the scrape: the status stays 200 with the page fields. The codes are strings inside a 200 body, not wire `ErrorCode`s.
- `untrusted_source` is true on every `ok` entry: the output is model text derived from the page and is data, never instructions. `repair` (`none`, `local`, `model`) says how a `json` output was made valid; the one model repair call carries the invalid output, the issues and the schema, never the page. `grounded` says the model's supporting quote for an `answer` is verbatim on the page; `dropped` counts highlight quotes that are not on the page.
- `model` is what the gateway answered with (a routing alias or what it resolved to), never a vendor id chosen by the caller. `cost_usd` is added to `usage` when the Model Plane returns it.

Model output is untrusted and checked by Quarry, not by the model: `json` is validated against the caller's schema after a local repair and at most one model repair; `highlights` and the `question` quote are verified verbatim against the page text (whitespace runs, no-break spaces and typographic quotes are folded, minimum 8 characters, de-duplicated, non-overlapping); `summary` cannot be verified and is labelled. The page reaches the model inside a per-call random delimiter it cannot know, with rules that call it data. AI output is inline only: never an artifact, never in the scrape record (`GET /v1/scrape/{id}` has no AI fields), never in the page cache, an event, a log or a span.

**Zero Data Retention.** Effective ZDR (the token floor or the body flag) plus any AI format: every AI entry is `skipped` with `ZDR_UNAVAILABLE`, no Model Plane call is made, nothing is written, and the page itself is returned as an ordinary ZDR scrape. `QUARRY_EDGE__AI_FORMATS_ZDR` accepts only `refuse` in v1 (ADR-4); `allow` is reserved and behaves as `refuse`. The rows `scrape` and `scrape_stream` in `docs/ZDR_MATRIX.md` say so.

**Privacy block.** The request's `privacy` block is honoured before the model hop, by the rule the page-title hop uses (`model_hop_allowed`): a `privacy_classification` of `credential_or_secret` or `zdr_ephemeral` skips every requested AI format with `PRIVACY_POLICY`, makes no Model Plane call, and returns the page itself as a normal scrape. Any other classification (or none) calls the model as usual. ZDR is answered first (`ZDR_UNAVAILABLE`). `truncated_input: true` is set on an entry that made a model call when the page text was longer than `QUARRY_EDGE__AI_FORMATS_MAX_INPUT_CHARS`; a page of exactly the cap is not truncated.

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

**Errors.** 400 for the prompt rules above. 429 short-circuit unchanged. 501 `UNSUPPORTED` (INNH-3) when a `schema` or a `prompt` is sent and the Model Plane cannot be used. The body is exactly `{error, code, hint, next_actions}`: `code` is `UNSUPPORTED`, `hint` starts with "omit `schema` and `prompt` to receive cleaned markdown" and points at `docs/RUNBOOKS.md`, "Enable structured extract and AI formats", and `next_actions` has two entries, the first being the retry without `schema` and `prompt`. There is no `details`; the four cases differ in `error`: (0) `structured extraction is not available on this edge: QUARRY_EDGE__AI_FORMATS is off` (QB-T17: a URL is set, the switch is off or was refused at boot; checked before the others when a URL is set); (1) `structured extraction needs the Model Plane, and QUARRY_EDGE__MODEL_PLANE_URL is not set on this deployment` (no URL; also an empty or blank URL); (2) `structured extraction needs a granted Model Plane lane, and this deployment holds no model credential` (a URL, no service key and no development token); (3) `structured extraction needs a granted Model Plane lane, and the Model Plane refused this service` (the gateway answered 401 or 403 to the first model call, or Auth Core would not mint the token). Cases 0, 1 and 2 are answered before any page is fetched. Case 3 is answered after one page fetch and one model call, only while no source has been extracted; it replaces the former 200 with one `status: error` per source. A refusal after a source came back `ok`, a timeout, a 5xx, a non-JSON answer and a ZDR or privacy refusal stay per source in the 200. No case names a URL, host or token. `QUARRY_EDGE__AI_FORMATS` is named only in case 0: QB-T17 (merged from main) makes it the gate of the model arm of `/v1/extract` as well as of the AI formats of `/v1/scrape`. A deployed stack gets one of these until WS16.S1 to S3 and decision X-7 land.

**ZDR (ADR-4).** The model hop carries no ZDR posture, so under ZDR (the body flag, or the token's floor for a user of a ZDR org) no page text is sent to a model. The page is still fetched under ZDR; each source answers status `error`, `error` starting `ZDR_UNAVAILABLE:`, with its `structured` harvest, which needs no model. This is not a 501 and does not need a Model Plane. The engine refuses a ZDR call as well (`AiFormatRunner::extract_for_org`), so a future caller cannot reach the model by skipping the route check. The `extract` row of the ZDR matrix says so.

**Privacy block (INNH-1).** The request's `privacy` block is honoured the same way as on `/v1/scrape`: a `privacy_classification` of `credential_or_secret` or `zdr_ephemeral` sends no page text to the model. The source answers status `error`, `error` starting `PRIVACY_POLICY:`, with its `structured` harvest; no model call is made and it is not a 501. Any other classification calls the model as before.

**Time budget.** Sources are read one after another and every model call is paid. After 50 seconds from the start of the request a source that has not been read by the model answers status `error` with a message that names the budget, instead of the router cutting the whole request at 60 seconds. Send fewer URLs per request, or `max_urls` lower.

**Not in this version.** No per-call output cap and no `cost_usd` (WS16.S1). No LLM usage metric: metering is unchanged (`quarry.extract.url` per `ok` or `fetched` source). The Verevon gateway re-adding `prompt` to its own contract is the Innhenting session's change. The model lane for a deployed stack (a grant on the Model Plane, decision X-7) is not built, so a deployed stack still cannot run this arm.

## Change text diff: stored page text, the paragraph diff and `GET /v1/change/diff` (INNH-4, 2026-10-31)

A change monitor (a schedule with `target_kind=change_monitor`) used to keep a fingerprint per version and nothing a person could read. With `QUARRY_EDGE__CHANGE_TEXT_DIFF` on (default off) it keeps the page text and the difference between two versions. Off, nothing below exists: the recorder is what it was and `GET /v1/change/diff` answers 501. The flag is read per request.

**What is stored.** `PageRunner` already writes a `markdown` artifact on every non-ZDR page run. `POST /v1/internal/run_page` now returns its id as `markdown_artifact_id` (general: any caller, and the per-run results of a crawl, can use it; absent when nothing was stored, always so under ZDR). The orchestrator passes it on `POST /v1/internal/change/record`. With the flag on and the body not `zdr: true`, a new or changed check:
1. checks the id is a markdown artifact the body's `org_id` can read (a 400 otherwise, nothing saved; another org's id reads as not found, so it fails here);
2. saves the new baseline with `artifact_id` set to it (an unchanged check saves nothing);
3. on a change, computes the paragraph diff of the older baseline's markdown and the new one and stores it as an artifact of kind `change_diff` (extension `change_diff.json`, `application/json`), then a `DiffRecord` with `format: "json"`, `artifact_id` pointing at it and `summary` (at most 1 KB, for example `+3 -1 paragraphs, 2 moved`, `no paragraph change (the page markup changed, its text did not)` or `text diff unavailable: no_previous_text`);
4. puts the saved baseline in `ChangeRecord.new_baseline` and the diff record in `ChangeRecord.diff_id` before the event and the webhook go out. Both were empty before: the `change_detected` event's idempotency key ended in an empty baseline id. The `quarry.change.detected` body is the same `ChangeRecord` plus `subject` and `emitted_at`; it now carries `new_baseline` (with `artifact_id`) and `diff_id`. No page text is in it.

A baseline recorded before the flag has no `artifact_id`. The first change after it stores an unavailable diff (`reason: no_previous_text`) and the next one diffs. Unchanged, unreachable and ZDR-declared results persist no text.

**The stored diff** (artifact kind `change_diff`, field names pinned by `quarry-core` `change_history`):

```json
{
  "version": 1,
  "source_url": "https://example.com/pricing",
  "from_baseline_id": "bln_...", "to_baseline_id": "bln_...",
  "from_fingerprint": "blake3:...", "to_fingerprint": "blake3:...",
  "available": true,
  "reason": "no_previous_text",
  "diff": {
    "format": "paragraph", "real_delta": true,
    "added": 1, "removed": 1, "moved": 0, "unchanged": 14,
    "ops": [
      { "op": "added", "index": 6, "text": "Pro plan: 12 EUR per seat" },
      { "op": "removed", "index": 6, "text": "Pro plan: 10 EUR per seat" },
      { "op": "moved", "from": 2, "to": 5 }
    ],
    "truncated": false
  }
}
```

`reason` is present only when `available` is false (then there is no `diff`). The diff is paragraph-level and set-based: paragraphs are blocks separated by a blank line (a block over 8192 bytes is cut into units), compared by a hash of their text with whitespace collapsed and line endings ignored. `added`: in the new text and not the old; `removed`: the opposite; `moved`: kept its text but changed place among the paragraphs both texts hold (an inserted or dropped paragraph does not make the rest move; a repeated paragraph counts once). `real_delta` is `added + removed > 0`. The change fingerprint is still the raw-byte one, so a page whose bytes changed and whose text did not (an ad, a timestamp in the markup) is `status: changed` with `real_delta: false`; switching the monitor to a text fingerprint is a separate slice. Caps: 500 ops (added and removed first, then moved), 2048 bytes of text per op (cut on a character boundary), 1 MiB of markdown read per side; the counts are always complete and `truncated` says anything was cut.

**`GET /v1/change/diff`** (bearer; org from the token). Query: `url`; `from` and `to` (baseline ids, together, with `url`); `diff_id`; `include=markdown`.
- `url` alone: the latest two baselines. `url` + `from` + `to`: those two, in that order. `diff_id`: the diff a webhook announced (optionally with its `url`).
- 200 `data`: `{ source_url, from: {baseline_id, fingerprint, captured_at, artifact_id|null} | null, to: {...}, diff_id: string|null, available, reason?, diff?, markdown?: {from, to, truncated} }`. `diff_id` is the stored record when the pair has one and its artifact reads back (the diff the webhook announced); otherwise the diff is computed from the two stored pages for this request and `diff_id` is null. `reason` is one of `no_previous_version` (one baseline; `from` is null), `no_previous_text`, `no_text`, `artifact_gone` (retention evicted a page; a stored diff still answers, it holds its own text). `markdown` only with `include=markdown`, each text cut at 1 MiB.
- 400: no `url` and no `diff_id`; `from` without `to`; `diff_id` with `from` or `to`; baselines of another url than `url`, or the same baseline twice; `include` other than `markdown`; an id over 128 characters. 404 (`NOT_FOUND`, one message): no baseline for the org and url, or an unknown baseline or diff id, or one of another organization; there is no 403 for another tenant. 501: the flag is off (the message names `QUARRY_EDGE__CHANGE_TEXT_DIFF`) or the edge has no baseline store. 403: a Zero Data Retention caller (the token's floor or the body flag), refused like a stored scrape record: stored page text is not readable under ZDR.

**ZDR.** A ZDR run stores no markdown, so `run_page` returns no id and nothing is kept; the record route also ignores a markdown id on a `zdr: true` body (the baseline row of an observe-mode ZDR body is still written, with no text). Row `change_diff_read` in `ZDR_MATRIX.md`: refused. No hash of a page is ever returned as a stand-in.

**Not in this version.** `POST /v1/change/check` takes a caller-supplied fingerprint, fetches nothing and stores nothing, so it has no text diff; only scheduled monitors do. No consumer exists for `POST /v1/change/refresh`. The artifact janitor deletes by age only: if it is armed, old baselines lose their text (`artifact_gone`) until it learns to spare baseline artifacts. A retried record activity can write a second baseline (the record route ignores the idempotency key).

## Webhook subscriptions: the API (INNH-5, 2026-10-31)

A subscription is an HTTPS endpoint that receives `change_detected` events of the caller's organization. Off, nothing below exists: `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` and `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS` are unset in the default composes. The edge routes answer 501 while the edge switch is off, and the edge reports control's plain 404 (switch off there) as 501 naming `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS`. The fan-out tenancy fix and the retirement of the old global routes are not behind a switch: they close a hole that has no legitimate user.

Edge routes (bearer; the organization is the token's, a caller-supplied `org_id` is ignored; control mirrors them under the same paths with `?org_id=` and the internal HMAC):

| Method and path | Answer |
|---|---|
| `POST /v1/webhook-subscriptions` | 201 `data`: the subscription plus `secret` |
| `GET /v1/webhook-subscriptions?limit&cursor` | 200 `data: {items, next_cursor?}` |
| `GET /v1/webhook-subscriptions/{id}` | 200 `data`: the subscription |
| `PATCH /v1/webhook-subscriptions/{id}` | 200 `data`; body names at least one of `active`, `events`, `description`, `notify_on_change` |
| `DELETE /v1/webhook-subscriptions/{id}` | 204, no body |
| `POST /v1/webhook-subscriptions/{id}/rotate-secret` | 200 `data: {id, secret, secret_hint}` |
| `GET /v1/webhook-subscriptions/{id}/deliveries?status&limit&cursor` | 200 `data: {items, next_cursor?}` |
| `POST /v1/webhook-subscriptions/{id}/deliveries/{delivery_id}/retry` | 202 `data`: the delivery, `pending` |

Create body: `{ "url": "https://hooks.example.com/q", "events": ["change_detected"], "description": "prod" }`; an unknown field is a 400. `notify_on_change` is optional (see the bullet below). A subscription is:

```json
{ "id": "whk_...", "url": "...", "events": ["change_detected"], "active": true, "description": "prod",
  "secret_hint": "whsec_...ab12", "created_at": "2026-10-31T10:00:00Z",
  "last_delivery": { "status": "success", "at": "2026-10-31T10:05:00Z", "event_id": "evt_..." } }
```

`description` and `last_delivery` are absent when empty. A delivery is `{ id, event_id, event_type, status, attempt, next_attempt_at?, last_error?, created_at }`; there is no payload in it, and `last_error` is a closed token: `http_<status>`, `timeout`, `blocked_target`, `transport`, `inactive`, `error`.

- **Secret.** Minted by control (32 random bytes, `whsec_` + 64 hex characters). Returned by create and by rotate-secret and by nothing else: the reads are typed views with no secret field, and the edge rebuilds every answer from a typed view as well. Control stores it in plaintext (the HMAC needs the key); see the owner decision in `RUNBOOKS.md`.
- **URL.** `https` only; no user info or fragment; at most 2048 characters; every address the host resolves to must be public (checked at create with the egress guard, and again at every connect). The URL cannot be edited; delete and create.
- **Events.** A closed list, `change_detected` today. The stored event type is `change_detected`; the `quarry.change.detected` subject appears in the delivery body only. Run-finished events (INNH-13) join the list without a contract change.
- **notifyOnChange (E3.5-7).** An optional `notify_on_change`, `{ "types": ["changed"], "min_paragraph_delta": 3 }`, on create. On PATCH an absent key keeps the filter and `null` removes it. `types` is a non-empty list of `new`, `changed` or `removed`; `min_paragraph_delta` is 0 to 1000, and 0 means no threshold. The filter applies to `change_detected` events only, and a subscription without one takes every change. The subscription returns the filter as `notify_on_change`, and the field is absent when there is none. A refused value is a 400 with `details.field` `notify_on_change`; a body that is not the documented shape is `body_invalid`. The semantics, and the run-event batch path that is not filtered yet, are in `docs/CHANGE_TRACKING.md`, "Webhook emission".
- **Limits.** 10 subscriptions per organization (429 `subscription_limit`); the same URL with the same events twice is a 409 `subscription_exists`, whatever their filters.
- **Errors.** 400 with `error.details.field` and `error.details.reason` (`docs/ERROR_REASONS.md`); 404 for an unknown, foreign or malformed id (one answer); 403 for a service principal and for a Zero Data Retention caller on create, patch and retry (`details.reason` is `webhook_off_process`; reads, delete and rotate-secret stay open so an organization can wind down); 409 `delivery_not_retryable` for a delivery that is pending, in flight or delivered.
- **Retention.** A delivery row holds the event payload. Finished deliveries (`success`, `failed`, `dlq`) are deleted after 14 days (`QUARRY_CONTROL__WEBHOOK_DELIVERY_RETENTION_DAYS`, 1 to 3650). Deleting a subscription deletes its deliveries. Organization erasure deletes both tables for the organization.
- **Retired.** `GET/POST /v1/webhooks`, `GET /v1/webhooks/{id}` and `/v1/webhook-deliveries...` on control answer 410 `GONE`: they listed every organization's rows and returned secrets in cleartext, and nothing called them. `POST /v1/webhooks/change` (the edge's signed receiver) is unchanged.

**Not in this version.** A test-ping route; auto-disable after repeated dead letters; two secrets during a rotation (the old one stops at once); encryption of the secret at rest; a per-subscription rate limit; run-finished and job events (the store and fan-out are ready, the list is closed).

## Crawl `sitemap` and `scrape_options` (INNH-6, built, 2026-10-31)

Status: built and tested in tests, dark. Two flags, one per half, both default off: `QUARRY_EDGE__CRAWL_SITEMAP` (6a) and `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS` (6b), read once, `on`, `1` or `true` (any case) enables and anything else is off. With a flag off the handoff answers the 400 `unsupported_option` it always did for that option and nothing else changes. Neither is on any stack. Code: `crates/quarry-edge/src/{crawl_request,crawl_scrape,crawl_sitemap,crawl_policy}.rs` and `routes.rs`; `services/quarry-orchestrator/internal/{activities/crawl_sitemap.go,workflows/crawl_sitemap.go}`.

**Request (`POST /v1/crawl`, response envelope unchanged).**
- `sitemap`: `skip` (absent, today's behaviour), `include` or `only`. `include` seeds the crawl with the sitemap's URLs and still follows links. `only` crawls the seed and the sitemap's URLs and follows no page link. A wrong string is 400 `invalid_option`, a non-string 400 `invalid_type`. It needs the crawl scope switch: with `QUARRY_EDGE__CRAWL_SCOPE_MODE=off` (a legacy crawl) `include` and `only` are 400 `unsupported_option`.
- `scrape_options`: an object with `formats` (`markdown`, `html`, `rawHtml`, `links`, `metadata`), `only_main_content`, `include_tags`, `exclude_tags`, `remove_base64_images`, `extractor` (`readability`, `clean_full`, `raw`) and `timeout_ms` (1000 to 55000), each with its camelCase alias. It is validated by the validators `/v1/scrape` uses (`ScrapeOptions::from_value` and `resolve`), so the limits and the codes are the same: 32 selectors of 256 characters (`too_many`, `empty`, `too_long`), `out_of_range`, `unsupported_extractor`, `conflicts_with_only_main_content`, `invalid_type`, `conflicting_aliases`. The keys that cannot apply per page are 400 `unsupported_option`, with the path: `actions`, `actions_on_error`, `max_inline_bytes`, `screenshot`, `pdf`, `json`, `summary`, `question`, `highlights`, and a capture, object or AI entry of `formats` (`scrape_options.formats[1]`). The reserved keys are `reserved_field`. An unknown key inside is a warning in `effective.request_warnings` (`scrape_options.<key>`) in lenient mode and 400 `unknown_field` in strict. Error paths are prefixed `scrape_options.`.
- The handoff stamps the canonical, resolved form into the job params (`sitemap` only when not `skip`; `scrape_options` only when present, so a job without them is byte-identical to before) and echoes both in `data.effective`.

**Per-page semantics (6b).** Every page of the crawl is cleaned with the options: the stored `markdown` and `html` artifacts, the excerpt, and the text `ingest: true` sends to the Data Plane are the cleaned content. Links are discovered from the whole page whatever `include_tags` and `exclude_tags` cut. `formats` is accepted for parity and changes nothing that is stored, because a crawl page stores its artifacts whatever the list says. `timeout_ms` bounds one page's run (504 `TIMEOUT` when exceeded; the activity retries as for any timeout). A page cleaned differently from the default neither reads nor writes the page cache (the cache is keyed by the fingerprint alone); a page whose options only name `formats` or `timeout_ms` still uses it. `POST /v1/internal/run_page` takes the stamped object as `scrape`, re-validates it (400 with `details.fields` under `scrape.`) and answers `scrape_applied: true`. An orchestrator that sent options and does not read the echo (an edge that predates the field, or has the flag off) ends the run with `run_failed` code `edge_scrape_options_unsupported`, like `edge_scope_unsupported` does for the scope.

**Sitemap discovery (6a).** `POST /v1/internal/crawl/sitemap` (internal; runtime token and the `x-quarry-run-sig` org binding over the org and the seed URL, as `run_page`; 501 with the flag off). Body `{url, org_id, crawl, max_urls}`, answer `{urls[], discovered, sitemaps_fetched, nested_fetched, skipped{out_of_scope, robots_disallowed, duplicate, cap, invalid, sitemaps_refused, sitemaps_unavailable}, truncated}`. The edge reads the seed origin's robots.txt `Sitemap:` lines (at most 5), else `/sitemap.xml`; reads gzip bodies (compressed cap 10 MiB, decoded cap 50 MiB); follows an index to 2 levels, 5 children per index, 20 files in all, 30 seconds. Every sitemap file goes through the web policy, the static SSRF heuristics, the security preflight and the public-address DNS guard, and must be on one of the crawl's hosts (the seed hosts, with subdomains only when the scope allows them; `allow_external_links` does not widen it). A `Sitemap:` line or an index child on another host, or at an internal address, is counted in `sitemaps_refused` and never fetched. Each `<loc>` has its XML entities decoded, is canonicalised (`canonical_url`, default options), de-duplicated, and run through the crawl scope in enforce mode whatever the crawl's own mode is (host, section, include and exclude patterns, extensions), then robots.txt when the gate enforces. Page URLs are never fetched here.

**Orchestrator.** A crawl whose params carry `sitemap: include|only` calls the `SitemapSeeds` activity once, after `run_started` and before its loop, under `workflow.GetVersion(ctx, "crawl-sitemap-seeds-v1", DefaultVersion, 1)`; a crawl without the param never reaches the call, so an in-flight history replays unchanged. On the durable frontier the activity enqueues the URLs itself in batches of 500 (request ids shared with the link path, so a URL reached both ways is one entry, and a retry adds nothing) and returns counts only: no URL passes through the workflow history. On the in-memory frontier the URLs come back in the result and are capped at 500. Sitemap URLs are queued at depth 1, one level below the seed: a redirect off the host cannot add a host to the scope (that is a depth-0 right) and the depth bound counts their links. The seed is still fetched at depth 0 in `only`. The counts ride the existing events: `run_started.sitemap` (the mode) and `run_completed.sitemap` (`mode`, `discovered`, `sitemaps_fetched`, `nested_fetched`, `seeded`, `skipped{...}`, `truncated`, or `error`); no new event type. A failed discovery ends an `only` crawl with `run_failed` code `sitemap_unavailable` (nothing else to crawl) and lets an `include` crawl go on with links only, with `run_completed.sitemap.error` set. The pages it seeds count against `max_pages` and are metered as any crawl page (`quarry.crawl.page`); the sitemap fetches themselves are not metered.

**ZDR.** Crawls are refused under ZDR, so neither option has a ZDR path. The internal route declares non-ZDR and answers 400 to a body that sets `zdr`; it stores nothing. See the row `internal_crawl_sitemap` in `docs/ZDR_MATRIX.md`.

## Saved job definitions (E4.1-6, built, NOT run, 2026-11-10)

Status: built in code, dark behind `QUARRY_EDGE__SAVED_JOBS` (default off, read from the edge's process environment, so a change takes a restart of the edge). The module is `crates/quarry-edge/src/saved_job.rs`. The routes are `GET /v1/sources/{source_id}` and `PUT /v1/sources/{source_id}/definition`, a definition on `POST /v1/sources` (`config.definition`), and `source_id` on `POST /v1/schedules`. Their wire is in `docs/openapi.yaml`. Operator steps and the refusals are in RUNBOOKS, "Saved job definitions". No test covers these routes yet, and none was run.

**What a definition is.** A source (`src_`, kind `crawl`, `scrape` or `search`) holds at most one saved definition, at `config.definition`:

```json
{"version": 1, "job_kind": "crawl", "options": {"max_pages": 20, "ingest": true}}
```

- `version` is `1`. Any other value is `unsupported_version`.
- `job_kind` is `scrape` or `crawl`, and it must equal the source's kind (otherwise `unsupported_kind`). A `search` source takes no definition. `batch` is not a saved kind: a source has one URL.
- `options` holds the job's options. The URL is not one of them: the source's URL is the seed, and a `url` key is `not_saveable`.
- Only the keys `version`, `job_kind` and `options` are allowed (`unknown_field`). The definition is at most 16 KiB of compact JSON (`too_long`).

**Validation (on the edge, before anything is stored).** The validators the live routes use:
- `crawl`: the options, with the source URL as the seed, go through the `POST /v1/crawl` body parser in strict mode, whatever the process setting. An option whose feature the live route has off stays refused as `unsupported_option`: `sitemap` other than `skip` without `QUARRY_EDGE__CRAWL_SITEMAP`, and `scrape_options` without `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS`.
- `scrape`: the options go through the `POST /v1/scrape` option reader (`ScrapeOptions::from_value`) and its default limits (`resolve`). Reserved keys are `reserved_field`, unknown keys are `unknown_field`, and two spellings of one key are `conflicting_aliases`.
- Every problem found at one level is reported together, in `details.fields[]` as `{path, code, message}`. The paths start at `definition`: `definition.job_kind`, `definition.options.<key>` and so on.
- Not saveable: `url`, `zdr`, `webhook` and `ignore_robots_txt` (with the camelCase spellings `ignoreRobotsTxt` and `ignoreRobots`) are `not_saveable`, whatever the process allows. A robots override, a webhook target and a ZDR declaration change what a run may do or keep, so a saved job never carries one. These keys are refused before the crawl parser runs.
- The stored form is canonical: the options the validators accepted, re-serialised under their canonical names. `allow_subdomains` and `allow_external_links` are written only when `true`, and `crawl_entire_domain` only when `false`.

**Storage and the routes.**
- `POST /v1/sources` with `config.definition` checks the definition before the row is written, and control keeps the canonical form. A repeat registration of the same URL returns the row that exists and does not rewrite its config (in the Postgres and the in-memory store alike). A definition sent with a repeat POST is therefore checked and not stored. Change a stored definition with PUT.
- `PUT /v1/sources/{source_id}/definition` with `{"definition": <definition>}` replaces `config.definition`, and with `{"definition": null}` it clears it. The key is required. The edge reads the source through control (its kind and URL; another organization's id, or a deleted one, is a 404), checks the definition against them, applies the org web policy to the URL, and forwards the canonical form. Control replaces that one key of `config` and keeps the rest. Control checks the shape again (version 1, `job_kind` equal to the source's kind, an `options` object, 16 KiB), so a definition the edge accepts passes it.
- `GET /v1/sources/{source_id}` returns the source with its config, so the stored definition can be read back. It is not behind the switch.
- The read and the write of the definition proxy Control Plane records and keep no copy.

**Schedules from a saved definition.** `POST /v1/schedules` with `source_id` and no `config` makes an executable schedule from a source's definition:
- `target_ref` is the source id. The schedule's config is built when the schedule is created, from the definition as it is stored, and the edge checks the stored row again first, because the row is not trusted. A crawl's config is `url`, the keys the definition sets out of `max_pages`, `max_depth`, `ingest` and `scrape_options`, then `definition_version` and `source_id`. A scrape with no options is `url`, `definition_version` and `source_id`.
- A snapshot: control copies the schedule's config into each fire's params, so a fire reads the schedule and not the source. A later PUT reaches only the schedules created after it. A schedule's config has no update route (PATCH sets the tag only), so to change a schedule's options, create a new one and delete the old one.
- The request `kind` must equal the definition's `job_kind`, otherwise `saved_job_kind_mismatch`. A `batch` schedule is `saved_job_not_schedulable`: a batch has no saved form.
- Only a crawl is schedulable, and a scrape only with no options. A scheduled crawl carries only `max_pages`, `max_depth`, `ingest` and `scrape_options` from its config. Any other option in the definition (`allow_subdomains`, `allow_external_links`, `crawl_entire_domain: false`, `include_paths`, `exclude_paths`, `deny_extensions`, `sitemap` other than `skip`) is refused as `not_schedulable`, naming the key, and never dropped without a word.
- Why a scrape with options is refused: a scheduled scrape runs with the URL alone (`ScrapeJobWF` reads only `url` and `ingest`). Saving and firing such a definition would silently drop its options, so it is refused at create, naming the first option key. A scrape with no options is schedulable, and it fires as the plain scrape of the URL.
- The source's URL is gated as a plain schedule's URL is: a denied URL is `403 policy_denied`.
- Everything else is as for any executable schedule: a 5-field cron, created disabled unless a user token sends `activate: true`, and the ZDR refusal of the `schedules` row, which runs before the source is read. `source_id` with `preset` (a change monitor) is `source_id_not_for_monitor`, and a non-empty `config` with `source_id` is `config_conflicts_with_source_id`.

**ZDR.** A definition holds options only: no page content, no title, and no URL but the source's own. It needs no ZDR matrix row of its own. Creating a schedule is the `schedules` create as before: the row of `docs/ZDR_MATRIX.md` applies, and with `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` on, a ZDR organization is refused before the source is read. The saved path adds no exception to that row.

**The switch.** While `QUARRY_EDGE__SAVED_JOBS` is off, these answer `501 UNSUPPORTED`, with a message that names the switch: a `config.definition` on `POST /v1/sources` (null included), any `PUT .../definition` (null included), and `source_id` on `POST /v1/schedules`. `GET` is not behind it. The fire path does not read the switch, so a schedule made from a definition keeps firing while the scheduled-jobs switch on control allows it.

**Error reasons.** No new `ErrorCode`.
- `400 BAD_REQUEST` with `details.fields[]`: a definition the validators refuse. The codes are `unsupported_version`, `unsupported_kind`, `not_saveable`, `unknown_field`, `reserved_field`, `conflicting_aliases`, `too_long`, `invalid_type`, `required`, and the validator codes of `/v1/crawl` and `/v1/scrape` (for example `invalid_option`, `unsupported_option`, `out_of_range`, `invalid_pattern`).
- `400 BAD_REQUEST` with `details.fields[]` code `required` at `definition`: a `PUT` body with no `definition` key.
- `400 BAD_REQUEST` with `details.reason`: `invalid_source_id`, `config_conflicts_with_source_id`, `source_id_not_for_monitor`, `source_has_no_definition`, `saved_job_kind_mismatch`, and `saved_job_not_schedulable`. For a batch that is the whole answer; for an option the fire path cannot honour, `details.fields[]` carries code `not_schedulable` at `definition.options.<key>`.
- `400 BAD_REQUEST` with `details.reason` `source_definition_invalid` and `details.fields[]`: at schedule create, the stored definition no longer passes the validators (for example, the process's crawl limits were lowered after it was saved). Save it again.
- `400 BAD_REQUEST` with the message `invalid source id`: a malformed id on the source routes.
- `403 FORBIDDEN` with `details.reason` `policy_denied`: the URL is denied by the org web policy, on a definition save or a schedule create.
- `404 NOT_FOUND`: no source of the token's organization has that id, or it is deleted.
- `403 FORBIDDEN`: a service principal. The route scope fails closed for `/v1/sources` and for the new path, as it does for the other source routes.

**Not in this build.**
- Nothing runs a saved definition once. It is a schedule template: its only consumer is `source_id` on `POST /v1/schedules`.
- Batch definitions, the `dataset` result kind, and an update to a schedule's config.
- Firing a saved schedule waits for X-9 and for `QUARRY_SCHEDULED_JOBS_ENABLED` on control (RUNBOOKS, "Schedules go-live").
- The generated SDKs (`sdks/generate.sh`) do not carry these routes until the generator runs.

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

## `POST /v1/parse`: a PDF or DOCX to text, nothing stored (INNH-11, built, 2026-10-31)

Status: built and tested in tests, dark. Default off: `QUARRY_EDGE__PARSE_ROUTE` (`on`, `1`, `true` or `yes`, read per request). With it off the route answers 501 `UNSUPPORTED` with `details {reason: "parse_route_disabled", env: "QUARRY_EDGE__PARSE_ROUTE"}`, after the auth chain. It is on no stack. Code: `crates/quarry-edge/src/parse_routes.rs`, `routes.rs` (its own router, because the body is up to 25 MiB), `auth.rs` (the scope), `crates/quarry-runtime/src/content_route.rs` (`parse_document_info`, the page facts).

**Request.** The body is the document's raw bytes. Not a URL (a URL is `POST /v1/scrape` with `QUARRY_EDGE__DOC_ROUTING=on`), and not multipart: multipart is out of v1 (it needs a multipart parser in the edge, and the gateway can forward raw bytes). `Content-Type` is absent, `application/octet-stream`, `application/pdf` or `application/vnd.openxmlformats-officedocument.wordprocessingml.document`; any other type is a 415 before the body is read. The bytes decide the kind (`quarry_transform::sniff`, magic numbers only): a PDF marker in the first kilobyte, or a ZIP whose `[Content_Types].xml` names WordprocessingML. A declared PDF or DOCX type that the bytes contradict is a 415 `content_type_mismatch`; bytes that are neither are a 415 `unsupported_media` with `details.detected`. No header is read for a filename and none is echoed, stored or logged. No JSON body.

**Response 200**, in the typed envelope, with `Cache-Control: no-store`:
```
{ "data": { "markdown": "...", "kind": "pdf" | "docx", "bytes": 1599, "structure": "plain",
            "truncated": false, "pages": 1, "warnings": [ { "code": "...", "message": "..." } ] },
  "meta": { "request_id": "..." } }
```
`pages` is for a PDF only (absent for a DOCX, never null). `structure` is always `plain`: the text is paragraphs, with no headings, lists or tables marked up, and a PDF whose text sits in `Tj` and `TJ` runs loses its layout. `warnings` is always present, usually empty: `document_has_no_text_layer` (no page of the PDF has a text layer, a scan, so the text is empty), `some_pages_have_no_text_layer` (a mixed PDF; the message says how many pages), `document_has_no_text` (a DOCX with no text), `truncated_at_8mib` (the text was cut at 8 MiB, `truncated` is true). There is no OCR. **No redaction is applied** (`pii.rs` is not wired): personal data in a document comes back as it is, to the authenticated caller. No artifact id, no scrape id: nothing was kept to point at.

**Errors**, in the standard envelope: 400 `empty_body` (or `body_unreadable`); 413 `too_large` with `max_bytes`, by the declared `Content-Length` before the body is read, or by the bytes when none was declared (the limit is 25 MiB, the parsers' own; `QUARRY_EDGE__PARSE_MAX_BYTES` lowers it, never raises it); 415 as above, and `pdf_parse_failed`, `docx_parse_failed` or `too_large` from the parser (a file it cannot read, or that its limits stopped), each with `details.kind` and `details.supported: ["pdf","docx"]`; 429 `RATE_LIMITED` with `Retry-After`; 501 as above, or `isolation_required`; 504 `TIMEOUT` (the 15 second deadline). 401 and 403 come from the auth chain.

**Limits.** One organisation may have `QUARRY_EDGE__PARSE_CONCURRENCY_PER_ORG` parses in flight (default 1, at most 8) and the process `QUARRY_EDGE__PARSE_MAX_INFLIGHT` uploads (default 4, at most 32); past either the answer is a 429 (`org_parse_busy`, `parse_capacity`) with `retry_after_seconds` 2 and `Retry-After: 2`, before the body is read. A request that has a slot then waits at most 3 seconds for one of the two process-wide parse slots (shared with the scrape pipeline): past that, 429 `parser_busy` with `Retry-After: 5`. The parse itself: a child process under 768 MiB of address space, 20 s of CPU and 16 file descriptors, the 15 second deadline, the 8 MiB text cap, 25 MiB of input.

**Isolation.** The route refuses to serve (501 `isolation_required`) while documents would be parsed inside the edge process (`QUARRY_EDGE__DOC_PARSE_ISOLATION=inprocess`, or an edge that is not a Unix `quarry-edge-rs`): a hostile file could take the edge down. `QUARRY_EDGE__PARSE_ALLOW_INPROCESS=1` lifts that for a single-tenant development box. The boot line `document parse route is on` reports `isolation` and `serving`.

**Auth.** A user token needs no scope. A service principal needs `parse:write` (like `scrape:write`, it is not in the scope registry of the Control Plane yet: a cross-plane ask, see the INNH-11 status). No other scope stands in for it. An `api_key` principal (INNH-10) is refused with 403 `api_key_route_refused`: the route is not on a key's allow-list.

**ZDR and storage.** The handler takes no `AppState`: it has no artifact store, cache, index, event sink, usage meter or ingest to write to, and a test reads its source to keep it so. It logs nothing about a request. A ZDR caller is served the same way. See the row `parse` in `docs/ZDR_MATRIX.md`. **Billing:** the route is not metered in this version (no usage record, no Lago metric); the owner decides whether a parse is billable.

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

**Job results (INNH-12, an index of the pages a run fetched).** `GET /v1/crawl/{id}/results`,
`/v1/batch/{id}/results` and `/v1/jobs/{id}/results` list the pages a job fetched, one row
for each fetch, in the order they were fetched. The edge answers `{data: {kind, items,
next_seq?}, meta: {request_id, page: {next?, limit}}}`. A row is `{seq, url, status, content_type?,
title?, fetched_at, word_count?, driver?, markdown_artifact_id?, source_artifact_id?,
bytes?}`: `url` is the URL the job fetched (as requested), `bytes` the size of the stored
Markdown, `driver` one of `static`, `tls`, `browser`. The rows come from the job's
`page_fetched` events, which now carry the artifact ids, `bytes`, `word_count` and `driver`
(content-free, closed fields). The page content is not in the response: read it with
`GET /v1/artifacts/{id}` (org-bound, 404 when gone). Failed and blocked pages are not rows;
they stay on `/errors`.

Query: `limit` (1 to 500, default 100; anything else is a 400 naming the field),
`after_seq` (the `next_seq` of the previous page, also `meta.page.next`; `next_seq` is
absent on the last page) and `latest` (default false). A URL fetched twice (a retry, an
escalation to a browser) has two rows; `latest=true` keeps the latest fetch of each URL, in
`seq` order, and pages the kept rows by `after_seq`. A `latest=true` read looks at the first
20 000 fetches of a job at most and sets `scan_capped: true` when there were more. A row
written before the fields existed has `url`, `status`, `title` and `fetched_at` only; the
ids are also absent when nothing was stored, which is every ZDR run.

**Decision B2(a): results are a page index first.** `data.kind` names what the rows are, a
closed set: `page_index` or `dataset`. Every read answers `page_index` today: a row says
which page was fetched and which artifacts hold its content, and carries no extracted
fields. `dataset` (rows of extracted records) is reserved and nothing produces it; it needs
WS5.I (the extract step a crawl or batch can embed) and WS16.J (the dataset store) first.
The CSV and JSON Lines export carries the same value in `X-Quarry-Result-Kind`, taken from
the same field. A client must not read a row set as page content or as a dataset unless
`kind` says so. Zero Data Retention is unchanged: the 403 floor stays in the route gate.

**Export (E4.1-4).** `format=csv` or `format=jsonl` on the same three routes returns the same rows as text instead of the envelope: CSV (RFC 4180: a header line of the field names, CRLF line ends, quoting for a comma, quote or line break) or JSON Lines (one object per row). `fields` projects the columns: a comma-separated list of row fields from the list above, each named at most once, written in the order given; the default is all eleven in the order listed. A CSV cell that a spreadsheet would read as a formula (a leading `=`, `+`, `-`, `@`, tab or carriage return) gets a leading apostrophe, so a page title or URL stays text. A JSON Lines field the row does not have is `null`. An export reads 1 to 100 rows (`limit` above 100 is a 400 naming `limit`) and its body is at most 10 MiB (over that, a 400 with `details.reason: export_too_large`; the file is never cut short). `fields` without an export `format` is a 400 naming `fields`; an unknown `format` is a 400 naming `format`. Headers: `Content-Type` (`text/csv; charset=utf-8` or `application/x-ndjson; charset=utf-8`), `Content-Disposition: attachment`, `Cache-Control: private, no-store`, `X-Quarry-Result-Kind: page_index` (an index of the fetched pages, not the page content), `X-Quarry-Next-Seq` when another page exists, and `X-Quarry-Scan-Capped: true` when a `latest` scan stopped at its ceiling. The gates are the JSON read's: the flag (501), the ZDR floor (403) and the service-principal refusal, all checked before the export options are read.

Off unless `QUARRY_EDGE__JOB_RESULTS=1` (501 `UNSUPPORTED`, checked first). 403 `FORBIDDEN`
with `details.reason: job_result_urls` under the token's ZDR floor, because a row names a
URL and a title (the same floor as `/errors`); crawl and batch are refused for ZDR callers at
handoff, so no ZDR job has rows. A service principal gets 403 from the route scope, as on
`/errors`. The organization is the token's: control answers a foreign or unknown id, a
batch id on the crawl path and the reverse with the same 404. The artifact ids dangle after
an edge restart on the in-memory artifact backend (the compose default), and an S3 backend
cannot list artifacts (the ids still resolve by `GET /v1/artifacts/{id}`). Org erasure does
not purge the events a row is read from (the existing posture of `/events` and `/errors`).

**Artifact list filters (INNH-12).** `GET /v1/artifacts` takes `kind` (one of `markdown`,
`html`, `raw`, `pdf`, `screenshot`, `extract`, as the OpenAPI always said and now enforced)
and `run_id` (`run_...`), alone or together, on top of the paging filters. Another value of
either is a 400 with `details.field`. `ArtifactSummary` gains `run_id` and `page_hash`
(additive). The filters work on the in-memory and filesystem backends; they are not behind
the results flag.

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
- `docs/openapi-internal.yaml` describes the `/v1/internal/*` routes the edge registers: run_page, change/record, page-images, schedules/preflight, orgs/{org}/profiles, and the queue routes (enqueue, enqueue_batch, pop, ack, seen). It is repository documentation only. It is never served, no SDK is generated from it, and no code or script loads it. The guard fails if the internal spec and the registered internal routes differ.
- Who can reach an internal route is decided by gateway routing, not by the file (D2-312, not proven live). The page-images route has no token: its content hash is the capability on the trusted bus, so it must never be routed publicly.
- The eight `/v1/internal/*` routes of the Go control service (quarry-control: jobs claim, pending-control, control-ack, running, finalize; schedules desired and runs; zdr-tombstones) are not described yet. They move with the D2-050 internal secret split.
- Every route the edge registers is in one of the two spec files or on `scripts/openapi-route-allowlist.json` with a reason (`scripts/tests/openapi-route-coverage.test.mjs`).
- The generator does not delete files it no longer writes. After a spec change, delete the generated files that no longer exist (the gitignored `sdks/python/quarry_client`, `sdks/python/docs`, `sdks/python/test`, `sdks/typescript/src`, and the tracked `sdks/typescript/docs`).

## E3.7 additions: batch idempotency, the 408 envelope, job get and cancel, the search scrape block, sitemap options (W3-E3.7.11, 2026-11-03)

Status: a record of the E3.7 units as the code stands on 2026-11-03. Code only, not compiled, not run. `docs/openapi.yaml` carries the wire shapes: `Idempotency-Key` on `POST /v1/batch` and `POST /v1/crawl`, the batch `409`, the route-deadline `408` on `GET /v1/jobs/{job_id}`, and `SearchRequest.scrape` with `SearchResponse.scrape`. This section says what the edge serves, what it does not, and under which switch. Every switch named here is read by the code and defaults off.

| Item | State | Switch (default) |
|---|---|---|
| Batch `Idempotency-Key`, org-scoped (E3.7.1) | built | `QUARRY_EDGE__BATCH_IDEMPOTENCY` (off); migration 028 is not flagged |
| Job get and cancel (WS15.L1, WS15.L2) | built, documented in openapi | cancel needs `QUARRY_EDGE__JOB_CONTROL` (off) |
| Wait-or-resume job get with a 408 envelope and a resume token (E3.7.3) | not built | none yet |
| `scrape` block on `POST /v1/search` (E3.7.7a) | built | `QUARRY_EDGE__SEARCH_SCRAPE` (off) |
| Answer reuse of the hydrated pages (E3.7.7b) | not wired on the edge | none |
| Crawl `sitemap` skip, include, only (INNH-6a, which is E3.7.5) | built, documented in the INNH-6 section | `QUARRY_EDGE__CRAWL_SITEMAP` (off) |
| Gzip sitemaps and the Sitemap-line parse on `/v1/map` (E3.7.4) | not built | none yet |

### Batch `Idempotency-Key` (E3.7.1)

- The header is read on `POST /v1/batch` and on `POST /v1/crawl`. It is trimmed and used when it is 1 to 128 bytes. An empty, longer or not visible ASCII value is ignored, and the request proceeds as if no key were sent.
- Keys are unique per organization. Control migration 028 replaces the global unique index with one on `(org_id, idempotency_key)`. The migration is not behind a flag: it only stops two organizations colliding on one key.
- Flag off (the default). A repeated key answers `200` with the job the first request created, the same `job_id`. The repeat is not billed. The body is not compared, so a repeat with a different body re-attaches to the first job.
- Flag on (`QUARRY_EDGE__BATCH_IDEMPOTENCY`, batch only). The edge stores a SHA-256 of the batch as sent (the URL list in order, and `ingest`) on the job as `idempotency_payload_hash`, inside the signed body. A repeat with the same hash re-attaches exactly as with the flag off. A repeat with a different hash answers `409`: `error.code` is `CONFLICT` and `error.details` is `{"reason": "idempotency_key_reused", "job_id": "<the held job>"}`. Nothing is created and nothing is billed.
- The hash is taken before the URL check and the allowance clamp, so a retry is not refused because the plan or the policy changed between attempts.
- A held job with no stored hash (created with the flag off, by the crawl path or by another client) re-attaches as before, because it cannot be compared.
- Crawls are forwarded with the key but are not hashed. With the flag on, a crawl key reused for a different URL still re-attaches silently.
- A fresh request is unchanged by the flag: `200` with the `HandoffAck`.

### The 408 envelope (E3.7.3)

- Served today: the route deadline. Each edge route has one: 60 seconds, or 125 seconds for `POST /v1/scrape` while actions or AI formats are on. Past it the edge answers `408` with an empty body and no error envelope, and echoes `x-request-id`. It is the only `408` the edge answers itself. `GET /v1/jobs/{job_id}` documents it in openapi.
- Not built: the wait-or-resume answer. The E3.7.3 design gives `GET /v1/jobs/{job_id}` a wait: when the wait ends before the job does, the answer is a `408` with a JSON envelope and a resume token, and the caller calls again with the token. Neither the wait parameter, the token nor the envelope exists in the edge or in control today. Do not build a client against it until E3.7.3 lands and this section changes.
- A route-deadline `408` on `POST /v1/batch` or `POST /v1/crawl` can be retried with the same `Idempotency-Key`. If the first attempt created a job, the retry re-attaches to it. Without a key, a retry can create a second job.

### Job get and cancel (WS15.L1, WS15.L2)

- Read: `GET /v1/jobs/{job_id}` for any kind. `GET /v1/crawl/{job_id}` and `GET /v1/batch/{job_id}` check the kind: a job of the other kind is `404`. Errors are at `/v1/jobs/{job_id}/errors` and at the kind-specific `errors` paths. Reads are org-scoped by the verified token: another organization's job is `404`.
- Cancel: `DELETE /v1/crawl/{job_id}` and `DELETE /v1/batch/{job_id}`. The replies are `202` when recorded and not yet applied (`control.pending`), `200` when the job already reflects the request, `404`, `409` (`details.reason` is `ended`, `not_running` or `not_controllable`), and `501` when `QUARRY_EDGE__JOB_CONTROL` is off. Pause and resume are `POST` on `/pause` and `/resume` under the same paths.
- There is no generic `DELETE /v1/jobs/{job_id}`. A caller cancels through the path of the kind it created.
- Non-ZDR only: crawl and batch are durable jobs. Both handoffs answer `403` under zero data retention (`zdr_refusal`), so no ZDR caller has a job for these routes to read or cancel.

### The search `scrape` block (E3.7.7a)

Request: `SearchRequest.scrape`, optional.

- `limit`: 1 to 10, default 5.
- `max_chars` (also `maxChars`): 1 to 50 000, default 8 000.
- `timeout_ms` (also `timeoutMs`): 1 000 to 55 000, default 15 000. One budget for the whole block.
- `formats`: `markdown` only, which is the default. Any other entry is `400 UNSUPPORTED_OPTION`.

Out-of-range values are `400 BAD_REQUEST`. They are not clamped. Both refusals use the flat search error body (`error`, `code`, optional `hint`), and the message never repeats the caller's input. The checks run only with the flag on.

Response: `SearchResponse.scrape`, present only when the request sent a block.

- Flag `QUARRY_EDGE__SEARCH_SCRAPE` (default off, read per request). Off: the block is not validated, nothing is fetched, and `scrape` is `{"status": "disabled", "requested": 0, "hydrated": 0, "failed": 0}`. Turning the flag off never turns a search into a `400`.
- On: the first `limit` results, in the order the caller sees (after diversity and rerank), are fetched concurrently under one deadline. Each URL passes the request's web policy, then the security preflight (blocklists, private hosts, host rules), then a static markdown fetch with a 2 MB body cap. The browser is not used: no render and no actions.
- `scrape` is `{"status": "ok", "requested", "hydrated", "failed", "items": [...]}`, with `items` in result order. An item carries `url`, `position` (1-based in `results`), and either `status: "ok"` with `markdown`, `chars` and `truncated`, or `status: "failed"` with `code` (and `http_status` when the code is `http_status`).
- Failure codes are closed. `policy_denied`, `blocked` and `invalid_url` are refused before the fetch and are not billed. `deadline_exceeded`, `http_status`, `too_large`, `decode`, `extract`, `timeout`, `transport`, `empty` and `too_short` happen after it. The search never fails because a hydration did.
- Zero data retention: hydration runs inline only. Nothing is cached (it runs after the cache step, on hits and misses alike), written to an event or logged. The page text is in this response and nowhere else.
- Metering: one `quarry.scrape.page` event per request, with the quantity of fetch units that left the edge (successes, and failures after the fetch began). The estimate in its metadata uses placeholder rates from the job pricing helper.
- Not yet: `format=context` and `include_answer` do not use the hydrated text. A request with a scrape block and `include_answer` fetches the answer's sources a second time, and those fetches are not metered as `quarry.scrape.page`, until the edge passes the hydrated pages into the answer path (E3.7.7b).

### Sitemap options (INNH-6a covers E3.7.5; E3.7.6 and E3.7.4 as noted)

- Crawl `sitemap` (built; see the INNH-6 section above): `skip` (absent) follows links only; `include` seeds the crawl from the site's sitemaps and still follows links; `only` crawls the seed and the sitemap's URLs and follows no page link. Gzip sitemaps and sitemap indexes are read within bounds (`quarry_transform::sitemap::parse_body`, 50 MiB decoded cap). With `QUARRY_EDGE__CRAWL_SITEMAP` off, `include` and `only` are `400 unsupported_option`.
- Canonical URL helper (E3.7.6): `quarry_transform::canonical_url` already canonicalises and de-duplicates the `<loc>` values of a crawl sitemap (INNH-6a). Its wiring into the frontier enqueue of page links and into redirect seen-marking is not built.
- `/v1/map` (E3.7.4, not built): the route still reads its sitemaps as text (`quarry_transform::sitemap::parse`), with no gzip handling and no Sitemap-line parse. No switch is added for it here.

### What the SDKs carry

`sdks/generate.sh` generates the Python and TypeScript clients from `docs/openapi.yaml` in Docker (openapi-generator 7.22.0). This unit did not regenerate them. Until the integration step runs the generator, the in-tree clients may lack the `Idempotency-Key` parameter, the `scrape` fields and the batch `409`.
