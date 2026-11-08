# W23 W2-CP-E2b status: Quarry org_core key provider and KeyRouter (WS17.E2b)

Lane a, branch w23/lane-a. **Built (code only, not compiled, no tests run).**

## Gap check
None of the scope existed before this unit. `profile_keys.rs` had only `EnvKeyProvider`, `profile_encryption.rs` refuses `org_core`, and compose had no org-core key lines or billing-core lines. Everything below is new.

## Commits
- aaac6dda5 feat(quarry-runtime): org-core key provider and KeyRouter (W2-CP-E2b)
- eb4644c6c feat(quarry-edge): org_core profile key provider selection and boot check (W2-CP-E2b)

## What was built
- `quarry-runtime/src/profile_keys.rs`
  - `OrgCoreKeyProvider` against the W2-CP-E2a routes on w23/integ (`POST /internal/orgs/:orgId/keys/data-keys`, `.../data-keys/unwrap`, `.../data-keys/rewrap`). It sends the headers `X-Service-Id: quarry-edge` and `X-Service-Token`, because that is what org-core's service auth reads (not Bearer). The body is `{"purpose":"quarry.profile","context":{"profile_id":..}}`, with `key_id` and `wrapped_dek_b64` added for unwrap and rewrap.
  - Built on reqwest. Each call has a 3 s timeout (connect and request) and is retried once on a 5xx or a transport error. Redirects are off. Responses are capped at 16 KiB and read into a zeroized buffer reserved up front. Wrapped DEKs are capped at 1 KiB.
  - Status map: 503, any other 5xx and transport errors give Unavailable. 410 gives Destroyed. 403 gives Denied, and so do 404 and 422 (org-core answers 422 for a wrong org, a wrong profile or a tampered wrap). 401, 408 and 429 give Unavailable, so credential drift never reads as "absent". Other 4xx give Invalid.
  - `key_id` is `oc:<kek_version>` and is checked against `kek_version` in every answer.
  - The token is read from `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN` by name (`from_env`) and held as a sensitive `HeaderValue`. The URL comes from `EdgeConfig.org_core_keys_url`, or from `QUARRY_EDGE__ORG_CORE_KEYS_URL` when that is unset.
  - Bodies, DEKs, the token and the org id are never logged. Each failure logs one `warn` on the caller thread, with only `op`, `status` and the `key_error` class.
  - Unwrapped DEKs are cached as in the env provider (300 s, 4096 entries, evictable by org). The provider also remembers the newest KEK version per org from generate, unwrap and rewrap answers.
  - The trait is synchronous, so the HTTP runs on a 1-worker runtime that the provider owns. The caller waits with `block_in_place` on a multi-thread runtime, and with a plain blocking recv otherwise. Drop uses `shutdown_background`, so dropping the provider in an async context never panics.
  - `wrap()` (wrap a caller's DEK) returns Invalid, because org-core has no such route. `generate` and `rewrap` are overridden instead.
  - New trait method `is_current_key(org, key_id)`. Its default is `key_id == current_key_id()`.
  - `KeyRouter { env: Option<..>, oc }` unwraps by the `env:` or `oc:` prefix. An `env:` row with no env provider gives Destroyed, and any other prefix gives Invalid. Generate and current go to org-core. `env:` rows are never current, so they reseal lazily with a fresh org-core DEK. Rewrap of an `env:` row is refused (Invalid).
  - Tests (wiremock, current-thread): `org_core_provider_maps_statuses`, `provider_never_logs_bodies`, `router_unwraps_env_and_oc_rows`, plus `org_core_config_is_refused_by_class`.
- `quarry-edge/src/config.rs`: new field `org_core_keys_url: Option<String>`. There is no token field.
- `quarry-edge/src/main.rs`: `profile_encryption_plan(&cfg)` replaces the direct `plan_from_settings` call.
  - Provider `env` (the default) is unchanged.
  - Provider `org_core` keeps the same mode and REQUIRED rules. The boot check refuses a missing or blank URL or token in any mode. After that it builds a `KeyRouter`, which keeps the env master when `QUARRY_PROFILE_MASTER_KEYS` is set (validated by the env rules).
- `apps/Ingestion Plane/docker-compose.yml`, quarry-edge env:
  - `QUARRY_EDGE__ORG_CORE_KEYS_URL: ${QUARRY_EDGE__ORG_CORE_KEYS_URL:-http://org-core:8080}`
  - `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN: ${QUARRY_ORG_CORE_SERVICE_TOKEN:-}`
  - `QUARRY_EDGE__BILLING_CORE_URL: ${QUARRY_EDGE__BILLING_CORE_URL:-}` (for W2-CP-L2)
  - `QUARRY_EDGE__BILLING_CORE_TOKEN: ${QUARRY_BILLING_CORE_SERVICE_TOKEN:-}` (for W2-CP-L2)

## Flag and default
- `QUARRY_EDGE__PROFILE_KEY_PROVIDER` defaults to `env`, unchanged. `org_core` is opt-in.
- `QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED` stays 0/false.
- The org-core side stays dark (`ORG_CORE_KEYS_ENABLED=0`).
- Mirror required: `QUARRY_ORG_CORE_SERVICE_TOKEN` and `QUARRY_BILLING_CORE_SERVICE_TOKEN` must be copied from Control's ledger into Ingestion's.

## Assumed decisions
- The token defaults to empty (`:-`), not `:?` as the plan wrote. A `:?` would stop the whole Ingestion stack today, while the provider is still `env`. The boot check refuses `org_core` without the token instead.
- The auth headers are `X-Service-Id` and `X-Service-Token`, which is what org-core's middleware reads, not the plan's `authorization: Bearer` example.
- 404 and 422 map to Denied, and 401, 408 and 429 to Unavailable. The plan named only 503, 410 and 403.
- Transport errors are retried once, like 5xx. The worst case is about 6 s per call.
- There is no network probe at boot, so the edge does not depend on org-core to start. Only the config is checked.
- Provider selection lives in main.rs because profile_encryption.rs is not in this unit's files. `plan_from_settings` still refuses `org_core`, and its test still expects that.
- The provider's `current_key_id()` is the bare `oc:` sentinel, because the KEK version is per org.

## MP asks
None.

## Not verified / follow-ups (also in W23-depth-lane-a.md)
- Not compiled, and no tests run (owner order: code only).
- This BLOCKS the org_core flip. `envelope_profile_store.rs` (not in this unit) still compares `env.key_id` with `self.keys.current_key_id()` at two sites: DEK reuse in `seal_snapshot` and the reseal check on load. Under org_core that never matches. Every save would mint a new DEK, and in mode `on` every load would reseal. Both sites must call `self.keys.is_current_key(org_id, &env.key_id)`.
- The `profiles_backfill` bin still builds only `EnvKeyProvider`.
- Erasure crypto-shred (DELETE keys) belongs to W2-Q-U41b, not this unit. The runbook text is not updated.
