# Preset Bundles & Output Profiles

Cycle 25 / clusters #8 + #10.

## Why this exists

Pre-cycle 25, every caller had to encode 20 flags per scrape request:
which format chain to compute, which TTL on the cache, whether to
section the markdown for LLMs, what retention to apply. Three
problems: drift between callers, no central place to bump policy,
copy-paste errors at scale.

`Preset` + `OutputProfile` collapse the bundle to a short name. A
caller saying `preset: "docs-site"` gets a tested configuration with
documented trade-offs.

## Shipped presets

| Name                         | Use case                                   | Refresh    | Change-track? |
| ---------------------------- | ------------------------------------------ | ---------- | ------------- |
| `docs-site`                  | Technical docs (markdown-heavy)             | nightly    | No            |
| `help-center`                | Customer-facing help articles               | nightly    | No            |
| `pricing-monitor`            | Pricing pages — fast refresh + screenshot   | every 30m  | Yes           |
| `knowledge-base-sync`        | Internal KB → durable archive               | nightly    | Yes           |
| `ecommerce-catalog`          | Product catalogs (schema.org / JSON-LD)     | every 6h   | Yes           |
| `policy-and-legal-tracker`   | T&Cs / privacy / legal — high-fidelity diff | daily      | Yes           |

Every preset pins `crawl` (depth, pages, patterns, refresh cron),
`output` (format chain, LLM sections, cache, retention), `retry`
(attempts, backoff, transient codes), and `change_tracking` bool.

## API

```rust
use quarry_core::presets::{builtin_presets, resolve_preset,
    merge_preset_with_overrides, validate_preset_compatibility};

let p = resolve_preset("docs-site").expect("known preset");
let custom = merge_preset_with_overrides(
    p,
    &serde_json::json!({ "crawl": { "max_depth": 10 } })
);
validate_preset_compatibility(&custom, &serde_json::Value::Null)
    .expect("compatible");
```

`merge_preset_with_overrides` uses recursive JSON merge: objects
merge field-by-field; arrays + primitives are wholesale-replaced.

## Output profiles (cluster #8)

`Preset.output` is a full `OutputProfile`:
- `format_chain { markdown, html, extract, screenshot, pdf, source_trace }`
- `llm_sections { enabled, max_section_chars, emit_toc }`
- `cache: CachePolicy { mode, max_age_s, vary_on, stale_while_revalidate_s }`
- `retention { primary_days, archive_days }`

`OutputProfileRegistry` trait abstracts the lookup so a future
cluster (cycle 27+) can add a Postgres-backed registry editable via
REST. Today the registry is `builtin_presets()`-only.

## Stale-while-revalidate

`CachePolicy.stale_while_revalidate_s` is honoured by the cache
layer. When a request finds a cache entry past `max_age_s` but within
the SWR window, the runtime:
1. Returns the stale cached entry **immediately** (no extra latency).
2. Spawns a background fetch to refresh the cache.
3. The next request gets the fresh value.

This drops p95 latency on docs sites by a large margin — the warm-path
caller never waits on the network even when content is "expired".

## Validation

`validate_preset_compatibility` catches obvious bad combinations:
- `llm_sections.enabled=true` but `format_chain.markdown=false` → reject
- `format_chain.extract=true` but `format_chain.html=false` → reject (extract needs HTML)
- `crawl.max_pages=0` → reject
- `crawl.max_depth>50` → reject (runaway-crawl protection)

Operators can extend with custom rules in cycle 27+.

## Tests

- `crates/quarry-core/src/output_profile.rs` — 7 tests: roundtrip
  serde, deep-merge semantics, compat validation, default retention
- `crates/quarry-core/src/presets.rs` — 9 tests: every builtin resolves,
  unique IDs, pricing-monitor specifics, retention forever for policy
  tracker, override merge, validation rejects bad limits, default
  validation passes for every preset

## Pending wiring

The presets module is shape-complete; cycle 27+ wires:
- `/v1/presets` REST endpoint enumerating + resolving (cluster #10
  acceptance).
- Edge handler reads `request.preset: Option<String>` and substitutes
  the resolved bundle into the scrape pipeline.
- Postgres-backed registry for org-custom presets.

## Removed (E4.1-7)

The Go job presets are gone. `GET /v1/presets` and `GET /v1/presets/{name}` are no longer mounted on quarry-control, the deprecated `stealth` alias went with them, and `POST /v1/jobs` refuses a `preset` field with `400 BAD_REQUEST` (`preset was removed; send policy`) instead of resolving it. Job creation never checked the `requires` keys the presets declared. The `fast` preset sets `RobotsIgnore` (`robots.max_mode=ignore`), so the presets were deleted rather than proxied.

Send the run policy as `policy` on job create. With neither field, a job gets the baseline `DefaultRunPolicy`.

Two other uses of the word "preset" are unchanged: the change-monitor cadence on `POST /v1/sources` and `POST /v1/schedules` (`hourly`, `daily` or `weekly`), and the Rust bundles in `quarry_core::presets` described above.

A named, reusable set of job options is a saved job definition, not a preset. See `docs/CONTRACTS.md`, section "Saved job definitions (E4.1-6)".
