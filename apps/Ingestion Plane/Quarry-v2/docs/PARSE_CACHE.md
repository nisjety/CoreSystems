# Parse cache (E3.6-9)

Module: `crates/quarry-transform/src/parse_cache.rs`. Status: built, not wired.
Nothing in the pipeline reads or writes it yet; the parse stage that calls it is
a later slice (E3.6-2 routing and E3.6-4 redaction ordering).

## What it is

A bounded, in-process map from `(org_id, sha256(source bytes))` to a parsed
value. The value type is generic, so the caller decides what a parse produces.

- `ParseCacheKey::new(org_id, bytes)` hashes the bytes with SHA-256;
  `ParseCacheKey::from_digest` takes a digest the caller already has.
- `ParseCache::get(zdr, &key)` and `ParseCache::put(zdr, key, value)` take the
  request's `ZdrMode` on every call.
- Memory is bounded by entry count (`max_entries`, at least one). The oldest
  insertion is evicted first.

## Rules

| Rule | Where it is enforced |
|---|---|
| The org is part of the key. The same bytes under another org miss. | `ParseCacheKey` equality |
| An empty or whitespace-only org id never addresses the cache. | `ParseCacheKey::new` and `from_digest` return `None` |
| Under ZDR nothing is read from or written to the cache. | `guard(zdr, WriteKind::Cache)` in `get` and `put` |
| A ZDR read never returns an entry stored by a non-ZDR request. | `get` returns `None` under ZDR, the entry is kept for non-ZDR callers |

## Caller checklist (for the wiring slice)

1. Pass the request's real `ZdrMode`. Do not default it to `Off`.
2. Build the key with the caller's org id. Do not use a URL or a fingerprint as
   the org.
3. Include the parser identity and options in the cache value's validity check
   before reusing a value. The key is the source bytes only; the same bytes
   parsed by a different parser version are a different output.
4. Do not treat a `false` from `put` as an error. It means ZDR is on or the key
   is invalid, and nothing was stored.

## Not done

- No TTL, no byte budget, no per-org purge. The bound is entry count only.
- No Redis or Postgres backing. The cache lives in one process and is lost on
  restart.
- No runtime wiring and no metrics.
