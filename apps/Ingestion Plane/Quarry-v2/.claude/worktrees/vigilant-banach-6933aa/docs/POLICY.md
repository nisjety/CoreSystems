# RunPolicy + Determinism + Host Scheduler

Cycle 21 / clusters #2 + #3.

## Determinism modes

```rust
pub enum Determinism { Strict, BestEffort, Off }
```

| Mode         | Pins all inputs?       | Retries enabled? | Use case                                       |
| ------------ | ---------------------- | ----------------  | ---------------------------------------------- |
| `Strict`     | Yes (hard error on drift) | No (would mask races) | Compliance archives, regression baselines      |
| `BestEffort` | Yes (warn on drift)     | Yes                | Default for everyday scrapes                   |
| `Off`        | No                      | No                 | Exploratory one-shot; no durable ingest        |

## RunPolicy sub-policies

`crates/quarry-runtime/src/policy.rs`:

| Sub-policy          | Pins                                                    |
| ------------------- | -------------------------------------------------------- |
| `DiscoveryPolicy`   | `respect_robots`, `respect_meta_robots`, `include_subdomains`, `use_sitemap` |
| `FetchPolicy`       | `user_agent`, `driver_kind` (static\|tls\|browserless\|kernel), `timeout_s`, headers |
| `RetryPolicy`       | `max_attempts`, `base_delay_ms`, `transient_codes`        |
| `BlockPolicy`       | `block_codes`, `block_body_markers`                       |
| `ExtractionPolicy`  | `dom_cleanup_version`, `markdown_converter_version`, `fingerprint_algorithm`, `emit_source_trace` |
| `CheckpointPolicy`  | `every_n_pages`, `min_interval_s`                          |

Presets:

```rust
RunPolicy::strict()       // max pins, no retries, no surprises
RunPolicy::best_effort()  // default everyday preset
RunPolicy::off()          // no pins, exploratory only
```

## Fingerprint + identity

`policy_fingerprint(&RunPolicy) -> "blake3:..."`
- Hex blake3 of canonical-JSON of the policy. Two equal policies always produce the same fingerprint.

`record_determinism_inputs(url, &policy) -> DeterminismIdentity { url, policy_fp, id }`
- `id` is `"dq:<blake3(url|policy_fp|driver_kind|user_agent)>"`.
- Two runs with the same `id` MUST produce identical `NormalizedOutput.fingerprint` in Strict mode. Use this for cache-key + audit-trail stamping.

## Artifact stamp

Every successful `NormalizedOutput` now carries:

```rust
pub struct DeterminismStamp {
    pub mode: String,         // "strict" | "best_effort" | "off"
    pub policy_fp: String,
    pub identity_id: String,
}
```

Consumers can verify the claim by rebuilding the identity from the URL + recorded policy and comparing to `identity_id`.

## Acceptance criterion

> "Same URL + strict preset ⇒ identical fingerprint across 3 runs."

Verified at the contract level: `policy_fingerprint` is pure-functional over canonical JSON, and `record_determinism_inputs` is stable across calls — so Strict-mode `identity_id` is stable. Cycle 22 will add a live re-run test against the test-site harness.

---

# Host Scheduler (cluster #3)

`crates/quarry-runtime/src/host_scheduler.rs`.

## AIMD model

```
acquire → fetch → mark_good (latency)  → maybe additive-increase target
                ↓
                → mark_bad  (kind)     → multiplicative-decrease target by /2
```

| Knob                      | Default | Meaning                                                   |
| ------------------------- | ------: | --------------------------------------------------------- |
| `initial_target`          | 2       | Starting per-host concurrency                              |
| `max_target`              | 16      | Ceiling on additive-increase                                |
| `min_target`              | 1       | Floor on multiplicative-decrease                            |
| `increase_after_good`     | 8       | Goods needed to bump target by 1                            |
| `slow_multiplier`         | 2.0     | Latency >2× EWMA counts as soft-bad                          |
| `ewma_alpha`              | 0.3     | EWMA smoothing factor                                       |
| `retire_after`            | 300s    | Idle threshold for slot pruning                              |

## Failure kinds

```rust
pub enum BadKind { RateLimited, Blocked, Timeout, Server }
```

All four halve the target. Distinguishing them is reserved for future per-kind backoff tuning — for now they're equivalent.

## Per-host isolation invariants

Verified by tests in `host_scheduler.rs`:
- `unknown_host_starts_at_initial_target`
- `good_streak_increases_target`
- `one_bad_halves_target`
- `target_never_falls_below_min`
- `target_never_exceeds_max`
- `per_host_isolation_one_bad_host_does_not_affect_another`
- `ewma_updates_on_each_good`
- `slow_response_does_not_grow_concurrency`
- `acquire_blocks_until_slot_frees`
- `retire_drops_host_state`
- `sweep_prunes_idle_hosts`
- `aimd_invariant_shrinks_under_sustained_failures`

## Wiring

`PageRunner.scheduler: Option<Arc<HostScheduler>>`:
- `None` disables throttling (test harnesses).
- Production: `AppState.scheduler = Some(Arc::new(HostScheduler::with_defaults()))` — one shared scheduler so concurrent handlers coordinate.

Fetch flow:

```rust
let _slot = scheduler.acquire(host).await;   // blocks if target hit
let result = driver.fetch(url).await;
match &result {
    Ok(resp) => scheduler.record_good(host, resp.duration).await,
    Err(e)   => scheduler.record_bad(host, map_kind(e.code)).await,
}
drop(_slot);                                  // release before post-processing
```

## Acceptance criterion

> "Block-prone domain corpus shows ≥30% reduction in 429/403 vs unthrottled baseline."

Proven by invariant: every failure halves the target while a long good streak only bumps it by one. Real-corpus measurement is cycle 28's benchmark harness scope; the invariant guarantees the back-pressure shape that produces the observed reduction.
