# P1-ws3f design note: WS3.F (slot book, reaper, crash, probe, /ready) + WS3.D leftovers

## Data model

`crates/quarry-browser/src/slots.rs` (new, pure, no chromiumoxide types, every method takes `now: Instant`):

- `BrowserPoolConfig::from_env()`: `QUARRY_BROWSER_MAX_SESSIONS` (40, 0 = no cap, clamp <= 4096), `MAX_PAGES_PER_SLOT` (24, 0 = no cap, clamp <= 4096), `SESSION_IDLE_S` (3600, clamp 30..=7 d), `SESSION_MAX_AGE_S` (21600, clamp 60..=30 d, and never below idle), `REAPER_INTERVAL_S` (30, 1..=3600), `PROBE_INTERVAL_S` (15, 1..=3600), `QUARRY_BROWSER_REAPER` = `0|log|1` (default `log`). Garbage = default + one warn.
- `SlotState { id, status: Starting|Ready|Retiring|Dead, generation, pages, probe_failures, healthy }`. Generation 0 = never launched. `launched(slot)` bumps the generation and sets Ready.
- Session entries keyed by the lease's `session_affinity_key`: `{ slot, generation, admitted_at, last_touch, pages, idle_limit = max(2 x ttl, idle), age_limit = max(2 x ttl, max_age), reap_reported }`.
- Tombstones (10 min): key -> `LossReason::{Reaped(Idle|MaxAge), ProcessLost}`. `admit` of the same key clears it.
- `admit(key, ttl, now)`: an already admitted key is re-admitted (no double count, `fresh=false`); otherwise `Busy {limit, active, retry_after_s: 15}` at the cap, else the least loaded non-retiring slot (ties: Ready before Starting before Dead, then lowest id) with `needs_launch` when the slot is not Ready.
- `page_opened` (reserve, fails `PageCap {limit, active}` or `NotAdmitted(loss)`), `page_closed`, `release` (idempotent, returns `Option`), `touch`, `reap_idle(now, enforce)` (log mode reports each candidate once and removes nothing), `retire_candidates`, `mark_dead(slot, generation)` (stale generation is a no-op; returns the lost keys and tombstones them), `record_probe(slot, generation, ok)` (two consecutive failures = unhealthy), `health()`.

`lib.rs`: public `BrowserHealth { state: NotApplicable|Idle|Healthy|Degraded|Unhealthy, max_sessions, sessions_open, slots: Vec<SlotHealth> }`, default trait method `health()` = NotApplicable. State: no launched slot = Idle (ready); every launched slot dead or unhealthy = Unhealthy; some = Degraded.

## Driver (chromiumoxide.rs), K = 1 (slot 0)

- `acquire`: admit (Busy -> 429 RATE_LIMITED `details {reason: browser_session_cap, scope: driver, limit, active, retry_after_seconds: 15}`), then `ensure_browser`; a failed launch rolls a fresh admission back. Background reaper + prober start once at the first acquire, hold a non-owning view of the driver (all fields are `Arc`s), and are aborted by the owner's `Drop`.
- `ensure_browser`: relaunches when the book says the slot is Dead (old handle taken, killed and waited 5 s, its launch dirs removed). First launch sweeps stale `edge-<pid>-*` dirs whose pid is not ours and not alive (`/proc/<pid>` absent; Linux only).
- Crash detection: the CDP handler task marks the slot dead on a websocket error (`CdpError::Ws`) or stream end; the prober also checks `try_wait()`. Lost sessions lose their page handles at once; their next call gets `browser_process_lost` (DriverFailed, retryable false).
- `current_tab_page` / `goto` / `new_tab`: a tombstoned key answers `session_reaped` or `browser_process_lost`; a live session without a page answers `no_current_page`; all `retryable: false`.
- Live-frame cache: per session, with a per-session epoch taken from a global counter; capture happens outside the lock and is stored only if the epoch is unchanged and the entry still exists.
- Reaper (`QUARRY_BROWSER_REAPER=1`): runs the release teardown without `persist_current_page`; logs `browser session reaped reason=idle|max_age` (no key).
- Prober: `probe:<slot>` context via the extracted `create_session_context`; with a pinned proxy it gets an endpoint and a deny-all policy first (pinned-proxy invariant), opens about:blank under `Op::HealthProbe`, closes page and context, releases the provider session. Never touches `pages`, policies, receipts. A Dead slot is relaunched by the prober (otherwise a 503 /ready would never see traffic to recover). An unhealthy slot with sessions is never killed; without sessions it is recycled.

## Locking order

`attach lock (per session, tokio)` -> `browser (tokio)` -> `pages` and the other maps (tokio) -> `book (std::sync::Mutex, innermost, never held across .await)`. The browser lock in `open_tab_page` is held only around context creation (plus download behaviour) and `new_page`; hygiene, collectors, guard and navigation run without it. Attaches of one session are serialised by the per-session lock so two tabs cannot each create a context.

`AttachGuard`: owns the page reservation, the page and the context this call created. Disarmed on commit; on drop (error, timeout or cancellation) it returns the page reservation synchronously and spawns a task that closes the page and disposes a context only this call created.

## Failure cases

Launch failure (slot stays Dead/Starting, admission rolled back); crash mid-step (typed loss); release twice (book no-op, maps already gone); reaped session later used (typed `session_reaped`, never a fresh tab); attach cancelled after context creation (guard disposes it); probe timeout (counts as failure, cleans up best effort); stale crash notice for an older generation (ignored).

## What I will not do

No pool (K > 1 is WS3.G); no R-26 boot check in main.rs (the plan says main.rs is not touched; noted as not done); no openapi.yaml edit (outside owned paths); no gateway mapping; no log-baseline regeneration (R2 count in chromiumoxide.rs kept at 13).
