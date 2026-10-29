# ProfileStore — Multi-Tier Persistence

Cycle 24 / cluster #6.

## Tier hierarchy

| Tier | Backend                  | Purpose                                                       |
| ---- | ------------------------ | ------------------------------------------------------------- |
| L1   | `CachedProfileStore` (Redis) | Hot-restore cache; 1h TTL; per-instance               |
| L2   | `PostgresProfileStore`   | Authoritative durable store; multi-instance consistent       |
| L2'  | `S3ProfileStore`         | Alternate durable backend; cycle 19 ship; cheaper at huge N  |
| L3   | `InMemoryProfileStore`   | Dev / tests only                                              |

Read path: `L1.get → on miss L2.load → populate L1`. Write path:
`L2.save → invalidate (delete) L1` — the cache rebuilds on next read.

## Why three tiers

- **L1 alone**: Restart = lost session restore, all 100k profiles
  must be re-fetched.
- **L2 alone**: Every authenticated crawl pays a Postgres round-trip
  → p95 latency hit on the warm path.
- **L1 + L2**: One round-trip per profile per hour. Postgres
  authoritative; Redis is the speed tier.

## Tenant isolation

Composite PK `(org_id, profile_id)` on `quarry_profiles` makes
cross-org access **statically impossible** — every query is a PK
probe. The L1 cache key shape `quarry:profile:{org_id}:{profile_id}`
preserves the isolation in Redis. Admin tools invalidate one org's
cache via `DEL quarry:profile:org_alpha:*`.

## Configuration

`maybe_cache(inner, redis_url)` helper in
`crates/quarry-runtime/src/cached_profile_store.rs`:
- `redis_url` set → returns `CachedProfileStore` wrapping `inner`
- `redis_url` empty → returns `inner` unchanged

`main.rs` calls this once at boot. The trait `ProfileStore` is
identical at both layers so handlers don't know which tier they hit.

## TTL choice (1h default)

- **Longer (e.g. 24h)**: Stale state more likely after a peer
  instance's write. Peers don't bust this instance's Redis on
  remote writes — the cache settles via TTL expiry.
- **Shorter (e.g. 5m)**: Cache mostly cold; the extra Postgres
  round-trips offset the savings.
- **1h**: Longer than typical Quarry session windows, short enough
  that peer-write staleness is bounded.

Override via `CachedProfileStore::with_ttl(duration)` in tests.

## Migration

`crates/quarry-runtime/migrations/0002_profiles.sql` — `quarry_profiles`
table + `quarry_profiles_org_recent_idx` partial index on
`(org_id, updated_at DESC)`.

## Feature flag

`PostgresProfileStore` and the Redis cache live behind the
`postgres-queue` cargo feature (same flag as cycle 20's queue store).
Default builds get `InMemory` + `S3` only. Production image
(`Dockerfile.edge`) enables the feature.

## Tests

- 4 `PostgresProfileStore` tests (save/load, upsert, cross-org
  isolation, list ordering) — skip when `DATABASE_URL` unset
- 4 `CachedProfileStore` tests (key-format, default TTL, cache-hit,
  delete-invalidates) — skip when `REDIS_URL` unset
- The pre-existing `InMemoryProfileStore` tests (cycle 20)
  continue to pass.

## What's pending

- **Wire `PostgresProfileStore` into `main.rs`** — currently the
  edge boots with `InMemoryProfileStore` unconditionally. Cycle 25
  adds the env-var-driven swap (`QUARRY_EDGE__PROFILE_STORE=postgres`).
- **S3 → Postgres migration tool** — for operators with existing
  S3-backed deployments, a one-shot CLI that reads S3 keys and
  upserts into `quarry_profiles`.
