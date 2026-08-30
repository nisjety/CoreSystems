# Quarry-v2 — follow-up tasks from parity-finishers verification

Date: 2026-08-28. Source: code-grounded audit of `apps/Ingestion Plane/Quarry-v2/`
against the "Firecrawl-parity finishers + Venice privacy tiers" plan.

Venice privacy-tier work (Streams 3-4) is already shipped: `PrivacyTier` proto
enum, `min_privacy_tier` enforcement, capability-core migration 0013, frontend
badges + sovereign notice + run-console residency line. The remaining work is
in Stream 1 (two real gaps) and Stream 2 (two real stubs + one wire-contract
document).

Four scoped follow-up items. Each is independent and can ship on its own branch.

---

## F1 — Rust proptest hardening (URL norm / fingerprint / cache-key / DNS-guard SSRF)

**Why.** Plan §"Stream 1.6" requires `proptest` suites for URL normalization,
fingerprint stability, and cache-key derivation, plus SSRF property tests over
the DNS-guard heuristics. Today only three proptest files exist
(`change_history_proptest`, `change_webhook_proptest`, `dom_summary_proptest`),
none of which exercise the four targets. Plain `#[test]` coverage exists for
some (e.g. `cache::fingerprint` stability at `crates/quarry-edge/src/cache.rs:178-203`,
`SearchCache::key` at `:221-226`, `content_fingerprint` at
`crates/quarry-transform/tests/transform.rs:8-23`), but these are
single-input unit tests, not property tests over a generator.

**Where.**

- `crates/quarry-core/src/` — new `properties.rs` (does not exist today;
  `quarry-core` already has `proptest = "1"` in dev-deps at `Cargo.toml:22-23`).
- `crates/quarry-runtime/src/dns_guard.rs:104-136` — `resolve_public_url` and
  `PinnedDnsResolver::pin` need property coverage.
- `crates/quarry-transform/src/fingerprint.rs:14-28` — `content_fingerprint` and
  `text_fingerprint` need a stability property.

**Work.**

1. `crates/quarry-core/src/properties.rs`:
   - URL-normalization proptest: generator over `url::Url` with random scheme,
     host, port, path, query, fragment; assert `quarry_security::heur` accepts
     public URLs and rejects the crafted private-range set, and that
     `url::Url::parse(parse(url).as_str())` round-trips.
   - Cache-key derivation: generator over `(url, vary_headers, js_required)`
     tuples calling `cache::fingerprint` from `quarry-edge`; assert
     `key(a) == key(a)` for any `a` (stability), `key(a) != key(b)` for any
     pair differing in url, any header, or the js flag.
   - Fingerprint stability: generator over `Vec<u8>`; assert
     `content_fingerprint(x) == content_fingerprint(x)`,
     `text_fingerprint(x.to_uppercase().with_whitespace_collapsed()) ==
      text_fingerprint(x)` (semantic equivalence), and two distinct byte
     strings of length ≥ 16 never collide (use a counterexample-search
     strategy with `proptest!` shrinking).
2. `crates/quarry-runtime/src/dns_guard.rs`:
   - SSRF property test: generator producing (a) hostnames that resolve to
     public IPs in a test fixture, (b) literal IPv4/IPv6 addresses in
     every reserved range from `quarry_security::heur::resolve_guard`;
     assert `pin` returns `SecurityBlocked` for every reserved range and
     `Ok` for vetted public addresses. Note: `lookup_host` is async; use
     `proptest-async` or wrap in `tokio::runtime::Runtime::block_on`.
   - Pinned resolver property: for any `host`, `pin(host, vetted_addrs)`
     followed by `resolve(host)` returns exactly `vetted_addrs`; and
     `resolve(other_host)` errors.
3. Re-export `mod properties` from `crates/quarry-core/src/lib.rs` and add
   `pub use properties::*` only if external callers need it (otherwise
   keep `#[cfg(test)]`-only).
4. Wire all of the above into the existing proptest CI lane (the WS4
   proptest files at `crates/quarry-edge/tests/change_webhook_proptest.rs:1`
   and `crates/quarry-runtime/tests/dom_summary_proptest.rs:1` are
   `cargo test --workspace` targets today).

**Validation.**

- `cargo test --workspace -p quarry-core -p quarry-runtime --features
   proptest-internal` (or whatever feature gate is used by the existing WS4
   proptest lane — copy the same pattern).
- `cargo clippy -p quarry-core -p quarry-runtime --tests -- -D warnings`.
- New test counts added: at least 4 proptest blocks (URL norm, cache key,
  fingerprint, DNS-guard SSRF), each with ≥ 64 cases.

**Out of scope.** Changing `quarry_security::heur::resolve_guard` behavior;
adding a nightly fuzz scaffold (left optional per plan).

---

## F2 — `ApiError::rate_limited` on `extract_routes.rs`

**Why.** Plan §"Stream 1.5" requires the structured 429 envelope on map /
extract / scrape paths. `extract_routes.rs` has zero `rate_limited` or 429
calls today (verified). Map (`map_routes.rs:178-225`), search
(`search_routes.rs:455, :669`), and scrape (`routes.rs:500-506`) all
already emit `ApiError::rate_limited`. Extract is the only holdout.

**Where.** `crates/quarry-edge/src/extract_routes.rs:111-137` —
`fetch_markdown` returns `Err(String)` where the underlying `driver.fetch_conditional`
call at `:125` can surface a `QuarryError` with `ErrorCode::RateLimited`
but currently the typed error gets flattened to a string
(`"fetch failed: {e}"` at `:135`). The route is also used for structured
extraction at `:213-232` where the same flattening happens via
`AiFormatRunner::json_for_org`.

**Work.**

1. Change `fetch_markdown`'s return type to `Result<String, QuarryError>`
   (or to a new `FetchOutcome` enum) so the typed `ErrorCode::RateLimited`
   survives the await.
2. In the route loop at `extract_routes.rs:192-243`, branch on the typed
   error: if `ErrorCode::RateLimited`, return
   `(StatusCode::TOO_MANY_REQUESTS, Json(ApiError::rate_limited(e.message, 60)))`
   and short-circuit the rest of the batch (same fail-closed shape as
   `map_routes.rs:176-183`).
3. Apply the same typed-error branch to the structured extraction error
   path at `:213-232` so a Model Plane rate-limit surfaces as 429, not as
   a per-item `"error"` field.
4. Keep the existing `ExtractItem { status: "error", error: Some(...) }`
   shape for non-rate-limit failures so callers can still iterate the
   per-source result envelope — only the 429 path short-circuits.

**Validation.**

- Add a test at `extract_routes.rs` (alongside the existing
  `no_valid_urls_returns_structured_envelope` at `:302-329`) that drives
  a `RateLimited` `QuarryError` through the driver and asserts the
  response is `429` with the `ApiError` envelope shape (keys
  `code,error,hint,next_actions,retry_after_seconds,window`).
- `cargo test -p quarry-edge` + `cargo clippy -p quarry-edge --tests
   -- -D warnings`.

**Out of scope.** Changing the per-item error shape; rate-limiting
extraction at the route level (no global bucket — let the driver's
existing per-host throttles govern).

---

## F3 — Go control handlers: real `MountBenchmarks` + wire off the `trigger`/`backfill` stubs

**Why.** Plan §"Stream 2.1" calls for the forwarded list families to be
real, not stubbed. Two stubs remain:

- `MountBenchmarks` at `services/quarry-control/internal/resources/cycle23.go:239-243`
  always returns an empty page. Comment says "cycle 28 owner, still empty
  page." That owner has not landed.
- `scheduleTriggerStub` at `cycle23.go:278-300` and
  `scheduleBackfillStub` at `cycle23.go:308-338` return `202 Accepted` with
  `note: "Temporal SDK not yet wired"` even when the schedule is enabled
  and the orchestrator is healthy.

Both routes reach real clients today. Empty benchmarks and accepted-but-
not-executed triggers both silently under-deliver. The plan scopes them as
in-scope, so this item is the natural close.

**Where.** `services/quarry-control/internal/resources/cycle23.go:239-338`.

**Work.**

1. `MountBenchmarks`: replace the empty-page body with a real read
   against the same `db` parameter the rest of `MountSources` already
   takes (today the handler takes no store). Add a `benchmarks` table
   in the existing migration directory
   (`services/quarry-control/internal/store/migrations/`) keyed by
   `(org_id, kind)` with `created_at, label, value, units`. Project
   rows to `quarry_core::resources::Benchmark` via a new wire
   projection in `cycle24.go` (or a new `benchmarks.go`). Until the
   table is in place, **delete the route entirely** rather than ship
   an empty page — the plan's "honest contract" rule says no 200 with
   fake data, and there is no source for the empty list to be honest
   about. Either real rows or 404; the edge `forward_list` will
   tolerate 404 by returning an empty `Page<>`.
2. `scheduleTriggerStub` → `scheduleTrigger`:
   - Validate the schedule is enabled (`db.Schedules().Get`).
   - Look up the orchestrator runtime via the same pattern the rest of
     `quarry-orchestrator` uses (read the existing client wiring in
     `services/quarry-orchestrator/internal/...` and mirror it in
     `quarry-control` — if no client exists, copy the gRPC dial from
     `services/quarry-orchestrator/cmd/orchestrator/main.go`).
   - Call `temporalClient.ScheduleClient().Trigger(ctx, ...)` and
     return `200 OK` with `{schedule_id, run_id?, status: "triggered"}`.
     On Temporal failure return `502` with the typed `quarrycontracts`
     error envelope.
3. `scheduleBackfillStub` → `scheduleBackfill`: same pattern, calling
   `ScheduleClient().Backfill(ctx, BackfillRequest{...})` with the
   parsed `start_at` / `end_at` / `overlap_policy`. Return
   `200 OK {schedule_id, backfill_id, status: "queued"}`.
4. The plan calls this gap "blocked on the Temporal SDK swap" — do
   NOT swap the SDK in this item. The existing
   `quarry-orchestrator` already imports `go.temporal.io/sdk`, so
   wire the client in `quarry-control` against the same module
   version pinned at the workspace root. If a version-conflict
   surfaces, file a one-line `go.mod` bump and stop, do not chase
   the cascade.

**Validation.**

- `go test ./services/quarry-control/... -race`.
- New tests:
  - `cycle23_test.go::benchmarks_returns_404_when_table_empty` (or
    `returns_real_rows` after the migration lands).
  - `cycle23_test.go::trigger_returns_502_when_temporal_unreachable`.
  - `cycle23_test.go::backfill_validates_window` (preserved from
    stub at `:325-327`).

**Out of scope.** The `MountBenchmarks` schema design is intentionally
small — no percentile buckets, no time windows, no federation. If the
benchmark corpus is owned by another service (the lab Python harness
in `Quarry-v2/lab/`), this route is just a thin forwarder — implement
that and stop.

---

## F4 — Document the `/v1/team/*` wire-contract decision (not "fix" it)

**Why.** Plan §"Stream 2.2" says "project-envelope `{data,meta,error}`"
for the org-scoped handlers. The actual state in
`services/quarry-control/internal/resources/cycle24.go:16-25` is the
opposite, and it is **deliberate**:

```text
// Wire-shape rules (pinned by pkg/quarrycontracts roundtrip tests):
//   - The four single-object team endpoints are decoded by the edge's
//     forward_one::<T>, which parses the response body as a BARE JSON object.
//     They must NOT go through httpx.WriteJSON's {data,...} envelope.
//   - List endpoints go through writePage's raw `{items,next_cursor?,
//     total_estimated?}` Page<T> shape; forward_list unwraps a `data` wrapper
//     when present, but the bare Page is what listSourcesHandler already
//     serves and what the Rust tests pin against.
```

The Rust `forward_one` decoder at
`crates/quarry-edge/src/resource_routes.rs` is the authority — if we
change Go to emit envelopes, we break every edge roundtrip test
(`cycle24_test.go`, `sources_test.go`, `quarrycontracts` golden tests).
The plan's wording is wrong; the code is right. The right action is
to make the wire contract explicit at the plan level so this never
re-appears as a "gap."

**Where.** Two places.

- `docs/CHANGE_TRACKING.md` — append a one-paragraph note under
  "Wiring status" explaining the bare-object-for-single / page-for-list
  contract, citing the cycle24.go doc comment + edge `forward_one`.
- A new file at
  `apps/Ingestion Plane/Quarry-v2/docs/CROSS_LANGUAGE_WIRE_CONTRACT.md`
  capturing:
  - Single-object endpoints (`/v1/team/{credit,token,concurrency,queue}-usage`,
    `/v1/sources` POST/DELETE, `/v1/snapshots/:id`) emit a bare JSON
    object. `forward_one::<T>` decodes it as `T`.
  - List endpoints (`/v1/sources`, `/v1/snapshots`, `/v1/request-queues`,
    `/v1/{kind}/jobs`, `/v1/team/activity`) emit `Page<T>` =
    `{items: [...], next_cursor?: ..., total_estimated?: ...}`.
  - The `quarrycontracts.RESTEnvelope {data,meta,error}` shape exists
    in `pkg/quarrycontracts/envelope.go` and is used by `httpx.WriteJSON`
    for **handler-emitted error responses** only. It is not the success
    shape for these routes.
  - Why: the Rust edge's deserializers are typed (`forward_one::<T>`,
    `forward_list::<T>`) and do not currently branch on envelope
    presence. Adding a `data` wrapper for single-object responses
    would force a parallel decoder on every Rust call site, with no
    client-facing benefit.

**Work.**

1. Write the file `CROSS_LANGUAGE_WIRE_CONTRACT.md` (≤ 80 lines, no
   code blocks wider than 30 lines, no examples beyond the two shapes).
2. Append a 5-line "Update 2026-08-28" note to
   `docs/CHANGE_TRACKING.md` referencing the new doc.
3. Update the top-level plan doc (this is the "Start: Firecrawl-parity
   finishers" plan) to mark the envelope-shaped-as-`{data,meta,error}`
   bullet as RESOLVED-AS-DELIBERATE, with a link to the new file.

**Validation.**

- `git grep` the new contract file for the four route families and
  confirm each is cited.
- The plan's §"Stream 2.2" wording is removed/updated.

**Out of scope.** Changing any wire shape. Adding the `data` wrapper
to `forward_one`. Renaming `forward_one` to something neutral.

---

## Sequencing

- F1 and F2 are independent — they touch different files in
  `quarry-edge` and `quarry-core` respectively. Run on parallel
  branches off `main`.
- F3 is independent of F1/F2 — it lives entirely in
  `services/quarry-control`. One branch.
- F4 is doc-only and does not need a branch. Land as a direct commit
  to the plan + the new file.

Lead merges in order: F1 → F2 → F3 → F4 (F4 last so the doc references
the final state of the three code changes). Each merge must run
`cargo test --workspace` (Rust) and `go test ./... -race` (Go).

---

# Part 2 — Web-agent capabilities (Grok-style "dedicated cloud computer" per agent)

Source: `docs/WEB_AGENT_ROADMAP_EXECUTION.md` + `docs/QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026_OWNERSHIP_RECONCILED.md`
+ `docs/Features.md` (Cloud Providers Cluster) + `docs/SOVEREIGN_SCRAPING_RELIABILITY.md`,
verified against live Rust/Go source on 2026-08-28.

Target: each agent run gets a **dedicated cloud browser** (own lease, own
profile, own proxy affinity, own live view). The agent loop drives the
browser; receipts, observations, and SSE/WS streams let a human watch the
agent in real time and intervene. Scraping, crawling, and search stay first-
party Quarry work — the agent is the autonomous planner, not the fetcher.

**Rule of thumb:** the per-page hot path (scrape, crawl, fetch, transform,
fingerprint) must keep working perfectly before any fleet/agent autonomy
work is added. Items W1–W5 are listed in that dependency order: W1 makes
the per-agent browser actually isolated, W2 finishes the live-view
contract, W3 wires SSE/WS for real-time observation, W4 closes the cost
meter so the planner can budget per action, W5 layers the fleet/task
abstraction on top.

---

## W1 — Per-agent "dedicated cloud computer" isolation

**Why.** Today each `POST /v1/agent/runs` mints a `BrowserLease` with a
fresh `lease_id` and a unique `session_affinity_key = run_id`
(`crates/quarry-edge/src/agent_routes.rs:1385-1399`), so a run does not
share a live browser with another run. But the lease carries
`ProxyAffinity { pool: "", sticky_key: None }` (empty pool, no sticky
key) and the pool itself has **no TTL eviction** — `ttl_s` is stored
but never inspected by `RuntimeLeasePool::try_acquire`
(`crates/quarry-runtime/src/lease_pool.rs:34-38, 96-108`). Net effect:
two parallel agents over the same host land on random egress IPs and
their sessions live until the pool's semaphore releases them. This
is the opposite of "dedicated cloud computer with stable identity."

The Grok/Manus/Hyperbrowserx target is: one agent run = one process
tree, one outbound IP, one profile, one cookie jar, one proxy, with
explicit TTL and back-pressure. The "fleet" framing (W5) builds on
this; without W1, fleet is just shared-pool with extra labels.

**Where.**

- `crates/quarry-edge/src/agent_routes.rs:1385-1406` — `start_run`
  populates the lease.
- `crates/quarry-runtime/src/lease_pool.rs:34-108` — pool, no TTL.
- `crates/quarry-runtime/src/proxy_affinity.rs` (if it exists; the
  proxy module is implied by the lease shape but no implementation was
  found in the audit). Likely needs to be created.

**Work.**

1. **Per-agent sticky proxy.** Add a `ProxyAffinity` resolver that
   takes `(org_id, run_id, host)` and returns `(pool, sticky_key)`:
   - `pool` is the per-agent pool (one pool per active run, sized to
     lease constraints, max `max_parallelism`).
   - `sticky_key` is a deterministic hash of `(org_id, run_id, host)`,
     hex-encoded, persisted on lease acquisition.
   - On lease release, the pool entry is removed and the proxy
     identity is retired (Apify-style `mark_retired`).
2. **TTL eviction.** Add a tokio task to `RuntimeLeasePool` that
   sweeps leases older than `ttl_s` every 10s, calls
   `BrowserDriver::release` on each, and bumps the `BrowserSession`
   to `Evicted`. Add a pressure metric `agent_leases_evicted_total`.
3. **Pressure-aware back-pressure.** When the pool is at
   `max_concurrent`, the next `try_acquire` returns
   `RateLimited` with `retry_after_seconds = lease_avg_lifetime_s / 4`
   (clamped to `[5, 60]`). The agent loop already handles
   `RateLimited`; expose it as a typed error in the start-run response
   so Model Plane can replan with a smaller fleet batch.
4. **Per-agent proxy tier.** If the org has `privacy.allow_third_party_processing=false`,
   the resolver MUST pin to `egress.first_party` and reject any
   non-first-party pool. The pool type flows into `DriverInfo` and
   the run events so the App Shell can render the egress class.
5. **Tests.**
   - `lease_pool::ttl_evicts_after_window` (synthetic clock, advance
     past `ttl_s`, assert pool size shrinks).
   - `lease_pool::pressure_returns_rate_limited_with_retry_after`
   - `proxy_affinity::sticky_key_stable_for_same_run_host`
   - `proxy_affinity::first_party_only_when_third_party_disallowed`
   - `agent_routes::start_run_emits_proxy_pool_in_driver_info`

**Validation.**

- `cargo test -p quarry-runtime -p quarry-edge --features
   browser-agent --tests`.
- `cargo clippy -p quarry-runtime -p quarry-edge --tests -- -D warnings`.

**Out of scope.** Spinning up a new Chromium process per run (today
each acquire launches one anyway). Cross-host proxy geo-routing.
A/B-testing proxy providers.

---

## W2 — Live-view URL on `DriverInfo` for every cloud provider

**Why.** The `/v1/agent/runs/:id/frames/stream` SSE endpoint
(`crates/quarry-edge/src/agent_routes.rs:2490-2580`) streams
base64-encoded images and nothing else. The `/frames/ws` WebSocket
(`agent_routes.rs:2589-2644`) is similar. The App Shell can render a
picture, but the operator cannot click through to the provider's own
replay/live-view (HLS stream, Browserbase session URL, Browserless
playback). `BrowserbaseDriver` has the session metadata
(`session_id`, `live_view_url`) in its config but does not surface it
on `DriverInfo`; same gap for Kernel. This breaks the "watch the
agent on its own cloud computer" UX.

**Where.**

- `crates/quary-browser/src/browserbase.rs` — confirm `live_view_url`
  and `recording_id` are populated and exposed.
- `crates/quary-browser/src/kernel.rs` — same.
- `crates/quary-browser/src/browserless.rs` — same.
- `crates/quary-browser/src/driver.rs` — `BrowserDriver::acquire`
  returns `BrowserSession`; add a `live_view: Option<LiveViewRef>`
  field.
- `crates/quary-core/src/contracts.rs` — add `LiveViewRef { url,
  kind: "hls"|"iframe"|"browserbase"|"browserless"|"kernel", expires_at,
  recording_id? }`.
- `crates/quary-edge/src/agent_routes.rs:2490-2644` — emit
  `LiveViewRef` as part of every `frame` SSE event and every
  `live_view` WS message.

**Work.**

1. Extend `BrowserSession` (in `crates/quary-browser/src/driver.rs`)
   with `live_view: Option<LiveViewRef>`. Set it from each driver's
   acquire path:
   - **Browserbase**: `format!("https://www.browserbase.com/sessions/{}",
     session_id)` + the existing `live_view_url` config field.
   - **Browserless**: `format!("https://live.browserless.io/{}?token=...",
     session_id)` (or the current documented replay URL).
   - **Kernel**: `format!("https://app.onkernel.com/sessions/{}",
     session_id)`.
   - **Chromiumoxide**: `None` (no provider live view; the
     `/frames/stream` is the only view).
2. Add `LiveViewRef` to `BrowserObservation` and to every
   `BrowserActionResult`. The downstream consumer (App Shell live
   view) reads it from the SSE event and renders the URL in a
   side panel.
3. When `live_view.expires_at` is past, the next SSE event
   automatically refreshes the URL by calling the driver's
   `refresh_live_view` (new trait method, default `Err(Unsupported)`;
   each cloud provider implements the rotation call).
4. Recording: surface `recording_id` once the run closes (not before,
   so the provider doesn't burn the URL). Add a `recording_ready`
   event to the SSE stream.

**Validation.**

- Wiremock tests for each provider: assert `BrowserSession.live_view`
  is set after `acquire` for Browserbase/Browserless/Kernel and is
  `None` for Chromiumoxide.
- SSE integration test: drive a fake `BrowserbaseDriver`, subscribe
  to `/v1/agent/runs/:id/frames/stream`, assert the first frame
  event carries a `live_view` field with the expected URL shape.
- `cargo test -p quarry-browser --all-features --tests`.

**Out of scope.** Implementing our own HLS re-streamer. Exposing
Browserless' BQL. Adding new providers beyond the existing four.

---

## W3 — Unified agent SSE event stream (not just frames)

**Why.** `/v1/agent/runs/:id/frames/stream` is **frames-only** — the
operator sees pixels, not the typed `Event` stream the agent loop
already emits (`action.started`, `action.completed`, `action.failed`,
`observation.ready`, `agent.completed`, `agent.failed`, plus the 7
event types in `crates/quary-core/src/event.rs`). The agent loop
publishes these to `EventSink` and `NatsEventBus`, but **no SSE
endpoint bridges them** to the App Shell. So a human watching the
stream cannot see "Action click('Submit') completed" or "Grant
rejected by BrowserBroker" — only screenshots between actions.

The Grok-style target is: one SSE connection = one agent run, with
typed events interleaved with frames, so the App Shell can render
the action timeline next to the live browser.

**Where.**

- `crates/quary-edge/src/agent_routes.rs:2490-2580` (frames SSE).
- `crates/quary-runtime/src/agent_loop.rs:42-54` (event emission).
- `crates/quary-core/src/event.rs` (event types).
- `crates/quary-edge/src/agent_routes.rs` — add a new
  `/v1/agent/runs/:id/events/stream` route.

**Work.**

1. Add `GET /v1/agent/runs/:id/events/stream` (SSE) that subscribes
   to the run's `EventBus` subscription for the run's lifetime and
   re-emits every event as an SSE `data:` line. The implementation
   pattern is identical to the existing `/frames/stream` but the
   payload is the typed `Event` JSON, not base64 PNG.
2. Add a single `text/event-stream` keep-alive (15s) so reverse
   proxies don't kill the connection.
3. Filter events: drop `internal.*` and `telemetry.*` from the SSE
   stream. The `EventBus` already classifies, so wire the filter
   here.
4. Add a `?from_seq=N` query param for reconnection; replay events
   from `PostgresEventHistory` first, then live-stream the rest.
   Use the same `JobHistoryEvent` envelope from cycle 24
   (`quarry_core::job_history`) — no new schema needed.
5. Add a `GET /v1/agent/runs/:id/summary` (REST, not SSE) that
   returns the latest 50 events as JSON for the App Shell
   "open in new tab" UX. Use `quarrycontracts.Envelope` shape (see
   F4 — list endpoints get `Page<T>`).
6. Update the `/v1/agent/runs/:id/frames/stream` route to set a
   `X-Agent-Events-Stream: /v1/agent/runs/:id/events/stream` header
   on its 200 response, so the App Shell knows the second URL is
   the typed event companion.

**Validation.**

- `agent_routes::events_stream_emits_typed_events` (synthetic
  AgentLoop, capture SSE bytes, assert event names + payload).
- `agent_routes::events_stream_replays_from_seq` (start a run,
  close, reconnect with `?from_seq=N`, assert replayed events
  + live tail).
- `agent_routes::events_stream_filters_internal_and_telemetry`.
- `cargo test -p quarry-edge --features browser-agent --tests`.

**Out of scope.** A WebSocket variant of the events stream (WS
upgrade adds complexity for marginal latency; SSE is the
de-facto browser EventSource and what App Shell SolidJS uses
today).

---

## W4 — Action-cost meter so `max_cost_usd` becomes real

**Why.** `AgentLoop` has the full budget enforcement code
(`crates/quary-runtime/src/agent_loop.rs:224-260`,
`over_budget` at `:248-260`, `MaxCostExceeded` at
`:399-445`) and the tests pass. But `start_run` **rejects every
request that sets `max_cost_usd`** with the typed error
"max_cost_usd is not supported by the browser edge until a metered
action cost is available" (`crates/quary-edge/src/agent_routes.rs:1272-1277`).
The honest engineering reason is that we cannot meter a Model Plane
planner call's USD cost from inside Quarry without either
(a) a per-token cost published by Model Plane or (b) a flat
per-action cost table. Without W4, the budget is unreachable; without
budgets, autonomous agents have no enforced cost ceiling.

The Grok-style target: each action carries a real `cost_usd` stamp
(per planner call, per browser action, per byte egress) and the
loop aborts on the budget, not on a config-level blanket rejection.

**Where.**

- `crates/quary-edge/src/agent_routes.rs:1272-1277` — the reject.
- `crates/quary-runtime/src/agent_loop.rs:141-152` — `record_cost`.
- `crates/quary-runtime/src/mp_client.rs:313-464` — `ModelPlanePlanner`
  invocation (no cost returned).
- `crates/quary-core/src/contracts.rs` — add `ActionCost { model_usd,
  browser_usd, egress_usd, total_usd, source }` to
  `BrowserActionResult`.
- New file: `crates/quary-runtime/src/action_cost.rs`.

**Work.**

1. **Flat per-action cost table.** Define a cost table in
   `action_cost.rs`:
   - `click/type/press/scroll/select/wait` — flat $0.0001
     (browser driver time, no remote cost).
   - `navigate` — flat $0.001 + $0.0001 per KB transferred (best-effort
     body length from `BrowserSession`).
   - `screenshot` — flat $0.005 (CDP roundtrip + PNG encode).
   - `evaluate` — flat $0.001.
   - **Planner call** — call
     `model_plane_client.estimated_cost_usd(input_tokens, model_id)`;
     the gateway returns a per-token cost (Model Plane already
     computes this for `/v1/invoke`; surface it on the response).
   - If the call returns no `usage.cost_usd`, fall back to
     `$0.005 * (input_tokens + output_tokens) / 1000` (an explicit
     conservative estimate), and stamp `source: "estimated"`.
2. **Disable the blanket reject.** Remove the `if
   req.constraints.max_cost_usd.is_some() { return Err(...) }` in
   `agent_routes.rs:1272-1277`. Replace with a `max_estimated_cost_usd`
   cap (default $1.00, configurable) that the edge enforces in
   addition to the agent's own budget.
3. **Surface cost in receipts.** Every `StepReceipt` gets a
   `cost: ActionCost` field (idempotent, so the existing store
   contract holds). The receipt stream can then be summed for
   `GET /v1/agent/runs/:id/summary?include=cost`.
4. **Re-evaluate `max_cost_usd` rejection comment in
   `WEB_AGENT_ROADMAP_EXECUTION.md:51`** — once this lands, the
   bullet "production forces the validator URL and rejects
   `max_cost_usd` until the edge has a real action-cost meter" is
   obsolete. Update the doc.

**Validation.**

- `action_cost::flat_table_matches_known_actions`.
- `action_cost::planner_call_returns_estimated_cost_when_model_plane_omits`.
- `agent_loop::budget_aborts_at_max_cost_usd` (existing test, must
  still pass; was rejected at the edge before, so the test never
  exercised the in-loop path — wire it in).
- `agent_routes::start_run_no_longer_rejects_max_cost_usd`.
- `step_receipts::cost_field_roundtrips_through_postgres`.

**Out of scope.** True provider-billed reconciliation (the cloud
provider's own meter would close the loop; out of band from the
real-time agent budget).

---

## W5 — Fleet / task orchestration (post-W1–W4)

**Why.** With W1–W4 done, one agent run is a fully isolated, fully
metered, fully observable unit. The natural next layer is a "fleet":
a coordinated batch of agent runs that share budget, share
domain intelligence, and report to a single fleet-level live view
— the Grok/Manus product shape. Today no `fleet`, `task_pool`, or
`agent_pool` exists in `crates/quary-runtime/src/` or
`services/quary-orchestrator/`. Adding fleet without the W1–W4
foundation would just be a label on the shared pool.

**This is intentionally the last item.** W5 should not start until
W1, W2, W3, W4 each have green tests. The plan explicitly says
"core capabilities for quarry needs to work perfectly first then
when they are done we can fine tune the agents." W5 is the tuning.

**Where.** (TBD by lead, after W1–W4 ship.)

- New `crates/quary-runtime/src/fleet.rs` for the in-process pool.
- New `services/quary-orchestrator/internal/fleet/` Go service for
  durable fleet state (budget, fan-out, fan-in, retry policy).
- New event subjects: `quarry.fleet.<fleet_id>.<event>` alongside
  the existing `quarry.run.<run_id>.<event>`.

**Work.** (Sketch only; refine after W1–W4 merge.)

1. **Fleet envelope.** `FleetTask {
     fleet_id, org_id, budget_usd, max_parallel_runs,
     shared_profile_id?, shared_domain_intel_id?,
     member_run_ids: Vec<RunId>, status: FleetStatus }`.
2. **Per-fleet budget.** A new `FleetBudgetTracker` in
   `fleet.rs` aggregates `StepReceipt.cost` across the member runs
   and aborts the fleet when the sum exceeds the cap. Mirrors
   `AgentLoop::over_budget`.
3. **Fan-out / fan-in.** `quarry-orchestrator` Temporal workflow
   `FleetOrchestrator` that starts N child workflows
   (`AgentRunWF`), each with the fleet's shared constraints
   (allowed domains, shared profile, shared proxy affinity pool
   per W1), and joins on completion or budget exhaustion.
4. **Fleet-level SSE.** Reuse the W3 event stream with
   `?fleet_id=<id>` to subscribe to all member runs from one
   connection. The `X-Agent-Fleet-Stream` response header
   advertises the new shape.
5. **App Shell UX.** New `FleetRunConsole` SolidJS component
   that renders a grid of N live views, a shared budget bar, and
   a fleet-level "intervention" panel that pauses the whole fleet
   when one member encounters a typed failure (the existing
   `ActionCascadePolicy` from `agent_routes.rs:2289-2295` covers
   per-run cascades; fleet-level is a new policy).

**Validation.**

- New `fleet.rs` unit tests for budget aggregation and back-pressure.
- Temporal workflow test using `test_workflow_env` to drive a
  3-member fleet to completion and assert combined cost + emitted
  events.
- App Shell: `FleetRunConsole.test.tsx` renders 3 members with
  distinct live views, asserts budget bar updates on receipt cost.

**Out of scope.** Per-org fleet auto-scaling (operator-scoped
limits only). Fleet templates / recipes (those live in Control
Plane per `SOVEREIGN_SCRAPING_RELIABILITY.md:328-330`).

---

## Sequencing (W1–W5)

Strict dependency order. W1 must land first; W5 cannot start until
W1–W4 are green.

- **W1 (per-agent isolation)** — independent branch, merge first.
  Unblocks the W4 cost meter's ability to charge per-action.
- **W2 (live-view URL on DriverInfo)** — independent branch, parallel
  to W1. Needs `BrowserDriver` trait change, so coordinate the merge
  with W1's pool/lease changes.
- **W3 (unified SSE event stream)** — depends on W1's
  `EventBus` per-run subscription being stable; depends on
  Postgres replay (`0003_job_history.sql`) being in production
  (it is per the cycle 24 audit).
- **W4 (action-cost meter)** — depends on W1 (real per-run proxy
  identity lets us bill egress), depends on W3 (receipts carry
  cost; the events stream already serializes the receipt).
- **W5 (fleet)** — only after W1–W4 each have `cargo test
  --workspace` green and a merged main.

Lead merges in order: F1, F2 (parallel) → F3 → F4 → W1, W2 (parallel)
→ W3, W4 (parallel, sequential to W1) → W5.

**No W-stream branch is allowed to merge to main with the
WEB_AGENT_ROADMAP_EXECUTION.md "deliberate gates" list
(`docs/WEB_AGENT_ROADMAP_EXECUTION.md:68-91`) showing an open gate
in its area.** Each merge updates that doc.
