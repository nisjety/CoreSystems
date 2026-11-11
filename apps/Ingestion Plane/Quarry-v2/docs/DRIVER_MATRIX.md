# Quarry v2 — Driver Selection Matrix

Quarry's `DriverPlan` selects between four driver families per request, based on `DriverSignals` extracted from the request URL, ZDR/cost constraints, and explicit hints.

## Driver families

| Family | Implementation | Strengths | Weaknesses |
|---|---|---|---|
| **Static** | `quarry-runtime/src/fetch.rs` (reqwest+rustls) | Cheapest, fastest, best for static HTML | No JS rendering; many sites cloak it |
| **TLS** | `quarry-tls` + `quarry-runtime/src/tls_driver.rs` (wreq+BoringSSL) | Real Chrome/Firefox/Safari TLS+H2 fingerprints | No JS, but bypasses many fingerprint walls |
| **Browser local** | `quarry-browser/src/chromiumoxide.rs` (CDP) | Full JS, screenshots, PDF, actions | Single-process; needs Chromium |
| **Browser cloud** | Browserless / Browserbase / Kernel | Scale, persistent sessions, replay, live-view | $$, network round-trip per page |

## Selection rules

The default chain from `plan_from_signals(signals)` (`crates/quarry-runtime/src/driver_plan.rs`), built into a driver by `DriverRegistry::build_driver`:

```
1. Try Static (skipped if signals.requires_js or robots fingerprint blocked static)
2. Try TLS (skipped if Static would be blocked; preferred when blocked by UA/JA3)
3. Try Browser local (skipped if no chromiumoxide feature compiled)
4. Try Browser cloud: Browserbase only (Kernel and Browserless are not registered; see ADR 10 memo)
```

Signals that influence selection:

| Signal | Source | Effect |
|---|---|---|
| `requires_js: true` | URL pattern + content-type detection | Skip Static; jump to Browser |
| `block_signal: cloudflare` | Detected via 403/503 + headers | Skip Static; try TLS first, then Browser |
| `requires_screenshot` | Format `screenshot` requested | Skip Static + TLS |
| `requires_pdf` | Format `pdf` requested | Skip Static; TLS or Browser |
| `requires_persistent_session` | Profile restore needed | Browserbase or Browserless with keepalive |
| `cost_ceiling_usd <= 0.001` | Request constraint | Skip Browser cloud |
| `zdr: on` | Run policy | Skip artifact-persistent paths only |

## Provider selection within Browser cloud

```
Browserbase (preferred)
  └── if context_id present → use existing context (cookies + auth restored)
  └── if recording requested → enable session recording
  └── if live_view requested → return live_view URL on session

Kernel (when Browserbase unavailable)
  └── if profile_id present → use VM-isolated session with profile
  └── if replay/live_view requested → return URLs alongside CDP

Browserless (REST fallback)
  └── one-shot REST POST per page; cheapest cloud option
```

Verified 2026-11-11: only the Browserbase branch can run in `quarry-edge`. The Kernel and Browserless branches describe the provider, not a route Quarry serves (see the note under the capabilities matrix).

## Capabilities matrix

| Capability | Static | TLS | Local | Browserless | Browserbase | Kernel |
|---|:---:|:---:|:---:|:---:|:---:|:---:|
| Markdown extraction | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| TLS fingerprint impersonation | ❌ | ✅ | ✅ (default Chrome) | ✅ | ✅ | ✅ |
| JavaScript rendering | ❌ | ❌ | ✅ | ✅ | ✅ | ✅ |
| Screenshots | ❌ | ❌ | ✅ | ✅ | ✅ | ✅ |
| PDF export | ❌ | ❌ | ✅ | ✅ | ✅ | ✅ |
| Click / type / scroll actions | ❌ | ❌ | ✅ | ⚠️ limited | ✅ | ✅ |
| Persistent sessions (cookies) | ❌ | ❌ | ⚠️ in-process | ✅ keepalive | ✅ contexts | ✅ profiles |
| Live view URL | ❌ | ❌ | ❌ | ❌ | ✅ | ✅ |
| Session recording / replay | ❌ | ❌ | ❌ | ❌ | ✅ | ✅ |
| VM isolation | ❌ | ❌ | ❌ | shared pool | shared pool | ✅ per-session |
| Per-page cost (typical) | $0 | $0 | $0 | ~$0.001 | ~$0.005 | ~$0.01 |

Scrape captures and actions (`POST /v1/scrape`, WS13.4b to WS13.6) run only on the Local driver (`chromiumoxide`). The other browser adapters inherit the trait defaults, so a capture or an action sent to them is refused with 501 `browser_capability_unavailable`. The table above describes what each provider offers in general, not what `/v1/scrape` uses today. The switches are `QUARRY_EDGE__SCRAPE_CAPTURE`, `QUARRY_EDGE__SCRAPE_ACTIONS` and `QUARRY_EDGE__SCRAPE_STITCH`, all off by default (CONTRACTS.md section 10).

**Verified 2026-11-11:** Only Browserbase is registered as a remote browser driver in `quarry-edge`, and only when its API key and project ID are set. Kernel (behind the `kernel` cargo feature, on by default in `quarry-browser`) and Browserless are REST-only drivers compiled into `quarry-browser`. They are exercised by unit tests only, and no other crate constructs either one. Since F-t2-074 (commit `f90ed49a3`), their errors carry the status, not the upstream body. The Kernel and Browserless columns above describe what those providers offer, not what Quarry serves. Every other Kernel or Browserless mention in this file, such as the signal table and the profile table, describes the provider rather than a served route. Registering them is blocked until the owner decides [ADR 10](../../../../docs/architecture/adr-quarry-browser-execution-substrate-adr10.md), a proposal that accepts nothing yet.

## Cost guards

When `max_cost_usd` is set on a request:
- Routes prefer cheaper drivers
- AgentLoop tracks accumulated cost via `record_cost()` and aborts with `MaxCostExceeded` when exceeded

## Failover behavior

`FallbackDriver` wraps the chain. On retryable errors (`Timeout`, `RateLimited`, `UpstreamBlocked`, `DriverFailed`), it tries the next driver. Errors are recorded per attempt in `DriverInfo.attempts[]` so observers can see the cascade.

Non-retryable errors (`Forbidden`, `BadRequest`, `SecurityBlocked`) abort immediately without falling through.

## Profile / context handoff

| Provider | Profile binding | Restore latency |
|---|---|---|
| Browserbase | `context_id` + `persist=true` on session create | ~1s |
| Kernel | `profile_id` on `/v1/browsers` create | ~2s |
| Browserless | `keepalive` session ID + cookies on each call | ~500ms |
| Local | In-process state (volatile across restarts) | n/a |

For long-lived authenticated workflows, Browserbase contexts are the most reliable. Kernel adds VM isolation for compliance-sensitive customers. Browserless keepalive is the cheapest for short-lived flows.

## Configuring overrides

Per-request overrides via `DriverSignals.preferred_driver`:

```json
{
  "url": "https://example.com",
  "formats": ["markdown"],
  "driverHints": {
    "preferredDriver": "browserbase",
    "tlsProfile": "firefox",
    "fallbackChain": ["browserbase", "browserless", "static"]
  }
}
```

## Provider acceptance matrix

`lab/evals/src/provider_matrix.rs` is the harness for capturing per-provider TLS/HTTP2 fingerprints against `tls.peet.ws` and `browserleaks.com`. Run it after credential setup:

```bash
PROVIDERS_PATH=./provider_matrix.json cargo run -p quarry-evals --bin quarry-provider-matrix
```

The output JSON is checked into `lab/evals/provider_matrix.report.json` and reviewed at release cuts.
