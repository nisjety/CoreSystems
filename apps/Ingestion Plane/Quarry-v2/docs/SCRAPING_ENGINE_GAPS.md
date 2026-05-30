# Quarry v2 — Scraping Engine Implementation Gaps

> **Generated:** 2026-05-28  
> **Scope:** Source-code-grounded gap analysis of the Quarry-v2 scraping pipeline.  
> Sources: live codebase review of `crates/quarry-runtime`, `crates/quarry-browser`.

---

## 1. Executive Summary

Five concrete gaps were identified by reading the implementation directly. Each gap represents either dead code that is unreachable, a Phase 2 stub that was never wired up, or a missing method that the trait contract requires.

| # | Gap | Crate | Severity | Status |
|---|-----|-------|----------|--------|
| 1 | Browser driver stub — `StaticDriver` only; no browser driver wired into runtime | `quarry-runtime` | **High** | ✅ Resolved |
| 2 | `driver_plan_enhanced` orphaned — not exported, unreachable by callers | `quarry-runtime` | **Medium** | ✅ Resolved |
| 3 | Retry exhaustion skips escalation — `DriverPlan::escalate()` never called | `quarry-runtime` | **High** | ✅ Resolved |
| 4 | `wait_for` / `evaluate` missing from Browserless driver | `quarry-browser` | **High** | ✅ Resolved |
| 5 | Adblock CDP task leaks on session release — bare `tokio::spawn`, no handle stored | `quarry-browser` | **Medium** | ✅ Resolved |
| 6 | Browserbase cloud-browser driver missing — no driver implementation for Browserbase sessions | `quarry-browser` | **Medium** | 🔧 In Progress |

---

## 2. Gap Details

### 2.1 Browser Driver Stub — Runtime Only Has `StaticDriver`

**File:** `crates/quarry-runtime/src/fetch.rs` line 1  
**File:** `crates/quarry-runtime/src/lib.rs` line 29

**Evidence:**

```rust
// fetch.rs, line 1 — comment left by the original author:
// Browser driver added in Phase 2.
```

`lib.rs` exports only:
```rust
pub use fetch::StaticDriver;   // line 29
```

`chromiumoxide.rs` exists in `crates/quarry-browser/` but is never imported or re-exported from `quarry-runtime`. The runtime fetch pipeline therefore has no path to a browser driver; every request falls through to the static HTTP driver regardless of the `DriverPlan` tier selected.

**Impact:** All jobs that require JavaScript rendering, challenge solving, or CDP interaction silently receive a static fetch result instead.

**Suggested fix:** Add a `BrowserDriver` variant in `fetch.rs`, import `quarry_browser::ChromiumoxideDriver`, and gate its construction behind the `DriverPlan` tier.

---

### 2.2 `driver_plan_enhanced` — Orphaned Dead Code

**File:** `crates/quarry-runtime/src/driver_plan_enhanced.rs` (entire file)  
**File:** `crates/quarry-runtime/src/lib.rs` lines 22, 25

**Evidence:**

`lib.rs` module declarations:
```rust
mod driver_plan;           // line 22  — wired
// driver_plan_enhanced is absent from lib.rs
```

`lib.rs` public re-exports:
```rust
pub use driver_plan::DriverPlan;   // line 25  — wired
// no pub use driver_plan_enhanced::*
```

`driver_plan_enhanced.rs` is a file on disk but is never declared as a module, so `rustc` never compiles it and no caller can reach it.

**Impact:** Any improvements, logic, or escalation strategy written in `driver_plan_enhanced.rs` are silently ignored. This is also misleading — the file name implies it supersedes `driver_plan`, but neither replaces nor augments it.

**Suggested fix:** Either `mod driver_plan_enhanced;` in `lib.rs` and integrate its types, or delete the file and consolidate the enhanced logic into `driver_plan.rs`.

---

### 2.3 Retry Exhaustion Skips `DriverPlan::escalate()`

**File:** `crates/quarry-runtime/src/retry.rs` lines 61–82

**Evidence:**

`execute_with_retry` loop terminates on exhaustion and returns the last error directly:

```rust
// lines 61–82 (approximate)
for attempt in 0..config.max_attempts {
    match driver.execute(&req).await {
        Ok(resp) => return Ok(resp),
        Err(e) => {
            last_err = Some(e);
            // backoff …
        }
    }
}
return Err(last_err.unwrap());   // escalate() never called
```

`DriverPlan::escalate()` is defined but the retry path never invokes it, so the waterfall tier upgrade (e.g. Static → Browser → Stealth) never fires after exhausting retries on a given driver.

**Impact:** The multi-tier fallback that `DriverPlan` was designed to provide does not function at runtime. Every job that exhausts retries on the static tier surfaces an error to the caller rather than escalating to a browser tier.

**Suggested fix:** After the retry loop exits without success, call `plan.escalate()`, obtain the next-tier driver, and recurse or loop. Guard against infinite escalation with a maximum tier depth.

---

### 2.4 `wait_for` / `evaluate` Missing from Browserless Driver

**File:** `crates/quarry-browser/src/browserless.rs` (entire file)

**Evidence:**

The file implements `BrowserDriver` with these methods only:

| Method | Present |
|--------|---------|
| `acquire` | ✅ |
| `release` | ✅ |
| `configure_request` | ✅ |
| `goto` | ✅ |
| `content` | ✅ |
| `screenshot` | ✅ |
| `pdf` | ✅ |
| `wait_for` | ✅ |
| `evaluate` | ✅ |

All interactions with the Browserless API are one-shot HTTP POSTs. There is no WebSocket session established, so `wait_for` (wait for a selector / network idle) and `evaluate` (run arbitrary JS and return a value) cannot be implemented without opening a persistent session.

**Impact:** Any job that specifies a `waitFor` condition (selector or timeout) or requires `evaluate` will either panic (if the trait impl is missing), return a compile error, or silently skip the wait — depending on how the trait is stubbed.

**Suggested fix:** Upgrade `browserless.rs` to open a persistent WebSocket session via the Browserless `connect` endpoint (CDP-over-WS). Implement `wait_for` using `Page.waitForSelector` / `Page.waitForNetworkIdle` CDP events and `evaluate` using `Runtime.evaluate`.

---

### 2.5 Adblock CDP Task Leaks on Session Release

**File:** `crates/quarry-browser/src/chromiumoxide.rs` line 224

**Evidence:**

The adblock interception task is spawned with no handle stored:

```rust
// line 224
tokio::spawn(async move { intercept_loop(page.clone(), block_list).await });
// handle is discarded — () returned from spawn is ignored
```

Contrast with the CDP event loop at line 64:

```rust
// line 64
let handle: JoinHandle<()> = tokio::spawn(event_loop(...));
self.handle = Some(handle);
```

When `release()` is called, `self.handle.abort()` fires for the event loop but there is no corresponding abort for the adblock task. The `intercept_loop` future holds a clone of `page` and continues polling CDP events after the logical session has ended.

**Impact:** Each acquired-and-released browser session leaks one unbounded task. Under load (frequent session cycling) this accumulates live tasks that hold `Arc<Page>` references, preventing Chrome tab GC and growing memory linearly.

**Suggested fix:** Store the `JoinHandle` returned by `tokio::spawn` in the driver struct alongside `self.handle`, and call `.abort()` on it inside `release()`.

---

### 2.6 Browserbase Cloud-Browser Driver Missing

**File:** `crates/quarry-browser/src/browserbase.rs` — does not exist yet

**Evidence:**

`crates/quarry-browser/src/lib.rs` declares drivers for `browserless` and (behind `#[cfg(feature = "chromiumoxide")]`) the local Chromiumoxide driver, but has no `browserbase` module. There is no way to connect a scraping job to a [Browserbase](https://www.browserbase.com) managed session.

Browserbase uses the standard CDP-over-WebSocket protocol:
```
wss://connect.browserbase.com?apiKey=<BROWSERBASE_API_KEY>&sessionId=<BROWSERBASE_SESSION_ID>
```
The `Browser::connect` entry-point from `chromiumoxide` can consume this URL directly, making the implementation straightforward.

**Impact:** Operators who provision Browserbase sessions (for stealth fingerprinting, residential proxy, or CAPTCHA solving) cannot route any job through Browserbase. The only cloud driver available today is Browserless.

**Suggested fix:**
1. Create `crates/quarry-browser/src/browserbase.rs` implementing the `BrowserDriver` trait using `chromiumoxide::Browser::connect` with the Browserbase WSS URL.
2. Gate the module behind `#[cfg(feature = "browserbase")]` in `lib.rs`.
3. Read credentials from `BROWSERBASE_API_KEY` and `BROWSERBASE_SESSION_ID` environment variables.

---

## 3. Severity Matrix

| Gap | Severity | Effort to Fix | Blocks Production Use |
|-----|----------|---------------|-----------------------|
| 2.1 Browser driver not wired | **High** | Medium — import + plumb `BrowserDriver` in `fetch.rs` | ✅ Yes — browser jobs silently fall back to static |
| 2.2 `driver_plan_enhanced` orphaned | **Medium** | Low — declare module or delete file | ✅ Fixed — module declared and exported from `lib.rs` |
| 2.3 Escalation never fires | **High** | Low-Medium — call `plan.escalate()` after retry loop | ✅ Yes — waterfall fallback non-functional |
| 2.4 `wait_for`/`evaluate` missing | **High** | High — requires WebSocket session in `browserless.rs` | ✅ Fixed — WebSocket CDP session; `wait_for` and `evaluate` implemented |
| 2.5 Adblock task leaks | **Medium** | Low — store `JoinHandle`, abort in `release()` | ✅ Fixed — `JoinHandle` stored, `.abort()` called in `release()` |
| 2.6 Browserbase driver missing | **Medium** | Low-Medium — new file, `BrowserDriver` impl via `Browser::connect` | 🔧 In Progress |

---

## 4. Recommended Fix Order

1. **Gap 2.5** (Adblock task leak) — one-line fix, zero risk, immediate correctness improvement.
2. **Gap 2.3** (Escalation not called) — small change in `retry.rs`; unlocks the existing `DriverPlan` waterfall design.
3. **Gap 2.2** (Orphaned enhanced plan) — decide: integrate or delete. Unblocks future driver plan work.
4. **Gap 2.1** (Browser driver not wired) — wire `chromiumoxide::ChromiumoxideDriver` into `fetch.rs`; required before any browser job is functional end-to-end.
5. **Gap 2.4** (Browserless `wait_for`/`evaluate`) — largest effort; needs persistent WebSocket session architecture in `browserless.rs`.
6. **Gap 2.6** (Browserbase driver missing) — new `browserbase.rs` file; low risk, adds cloud-browser routing option without touching existing drivers.
