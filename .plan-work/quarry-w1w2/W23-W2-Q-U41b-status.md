# W2-Q-U41b status: cryptographic erasure hop (destroy the org KEK after org erasure)

Branch w23/lane-a. Date 2026-11-03. Plane: Ingestion (quarry-runtime, quarry-edge). **Built to scope. Code only, not compiled, no tests run** (owner order).

## Gap check
Nothing of the scope existed. W2-CP-E2b built the org-core key provider (generate, unwrap, rewrap) but no destroy call, and W2-U41a's erasure families never reach org-core. Everything below is new.

## Commits (local, w23/lane-a)
- 03dfdf47c feat(quarry-runtime): crypto-shred hop destroys the org's KEKs after the profile purge (W2-Q-U41b)
- 5e3fa9bd3 feat(quarry-edge): erasure route runs every family and reports the key shred receipt (W2-Q-U41b)
- docs commit with this file and the lane-a depth rows D-U41b-1..6

## What was built
- `quarry-runtime/src/org_erasure.rs`
  - `OrgKeyShredder`: an async reqwest client for org-core `DELETE /internal/orgs/:org/keys`. It uses the same principal and headers as the key provider (`X-Service-Id: quarry-edge`, `X-Service-Token`) and the same URL and token env (`QUARRY_EDGE__ORG_CORE_KEYS_URL`, `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN`, read by name and never logged). Each call has a 5 s timeout, redirects are off, and it retries once on a 5xx or a transport error. The response body is never read. The org id is validated the way org-core checks it (max 128, trimmed, no control characters).
  - `key_shred_status` maps the answer: 2xx gives `Destroyed`. 404 (the org is gone, so the cascade took its keys) and 410 (already destroyed) give `AlreadyGone`, which counts as done. 401 and 403 give Denied, 408, 429 and 5xx give Unavailable, and any other status gives Invalid. All three errors leave the erasure retryable (`RuntimeNotReady`, 503, with the class only and no org id).
  - `KeyShredSetup` is one of `Off`, `Ready` or `Misconfigured(class)`, and `from_env()` reads the flag. With the flag on but the URL or token unusable, every erasure that needs a shred fails. It never silently skips the shred.
  - `ProfileErasure` decides before the purge whether a shred is needed:
    - Always when the key provider is `org_core`, meaning `current_key_id()` starts with `oc:` (the `KeyRouter` or the org-core provider).
    - Otherwise when the org has an `oc:`-sealed row, or when an earlier failed shred is still pending.
    - A failed probe counts as needed.

    The shred runs last, after the rows, tombstones, cache keys and DEK eviction. A failure returns an error but does not undo those steps. A process-local pending set keeps the retry shredding after the rows are gone.
  - `PurgeCounts.key_shred: Option<KeyShredOutcome>` is the receipt (`destroyed`, `already_gone` or `not_needed`), and the hop also logs one info line with the outcome only.
  - `ProfileErasure::connect` (the one call main.rs already makes) now ends with `.with_key_shred(KeyShredSetup::from_env())`. `with_key_shred` is public for tests and for later main.rs wiring.
- `quarry-runtime/src/profile_backfill.rs`: `org_has_oc_rows(blobs, org)` lists the org's rows, then gets each one until it finds a `key_id` starting with `oc:`.
- `quarry-edge/src/internal_routes.rs`:
  - `run_families` makes sure one family's failure never stops the others. Every selected family runs, then the first error is returned, still non-2xx, so quarry-control NAKs and retries the idempotent purge.
  - `FamilyReport.key_shred` is left out of the response when `None`, so the default response is byte-identical.
- D2-093 is closed in code. Erasure is cryptographic for `oc:`-sealed rows once the provider is `org_core` and the flag is on. Rows sealed under the `env` provider are still erased only by deleting them.

## Tests written (not run)
- org_erasure.rs `key_shred_tests` (wiremock):
  - `key_shred_status_maps_done_and_retryable_answers`
  - `shred_is_off_by_default`: no call and no receipt.
  - `shred_is_called_once_per_purge_and_is_idempotent`: org-core provider. The first purge is 200 Destroyed, the second is 410 AlreadyGone, and each purge makes exactly one call.
  - `shred_follows_oc_rows_when_the_provider_is_env`: an oc org is shredded. An env-only org gets not_needed and no call.
  - `a_failed_shred_keeps_the_erasure_retryable`: 503 twice (the call and its retry) fails the purge after the rows went. The retry still shreds through the pending set and gets 200.
  - `a_misconfigured_shred_fails_loudly`
- profile_backfill.rs: `org_has_oc_rows_sees_only_org_core_sealed_rows`.
- internal_routes.rs:
  - `a_failed_family_never_blocks_the_others`
  - `the_key_shred_receipt_is_reported_when_present`
  - The existing shape test also asserts that `key_shred` is absent.

## Flag and default
- `QUARRY_EDGE__ERASE_SHRED_KEYS` is off by default (1/true/on/yes turns it on). When it is off, nothing changes: no probe, no call and no response field.
- Prerequisites for turning it on:
  - org-core has `ORG_CORE_KEYS_ENABLED=1`.
  - The quarry-edge principal holds `org:keys:destroy:any`. It is not yet in org-core's allowlist (D-U41b-1, high).
  - The org-core URL and token are mirrored into Ingestion (D-E2b-5).

## Assumed decisions
- The hop is part of the profile family (in `ProfileErasure`), not a new registered family. The decision needs the rows before they are deleted, and registration lives in main.rs, which is not in this unit's files.
- The flag is read inside `ProfileErasure::connect`, and the URL comes only from env (not `EdgeConfig`), because main.rs is not in the files (D-U41b-3).
- "For orgs with oc:-sealed rows" means always under the `org_core` provider (idempotent, and it survives retries), and otherwise by a probe before the purge plus an in-process pending set.
- "Records a receipt" means the `key_shred` field in the family report plus an info log line with the outcome only. There is no durable receipt table, because that needs a migration outside the files (D-U41b-6).
- 401 and 403 are errors, never "done", so credential drift or a missing scope can never read as shredded.
- "Never blocks the other hops" was applied at two levels. Inside the profile family the shred runs last. In the route, every family runs before the error is returned (a behaviour change, D-U41b-5).
- The error code is `RuntimeNotReady` (503) with the class only.

## MP asks
None (no Model Plane change).

## Control Plane follow-up (not an MP ask)
- Add `org:keys:destroy:any` to org-core's `orgServiceScopeAllowlist` and grant it to the quarry-edge service principal before the flag is turned on (D-U41b-1).

## Not done / not verified
- Not compiled, and no tests run. Risky spots for the compile guard:
  - The `match &self.shred` arm with a `_ if !needed` guard before the variant arms.
  - `key_shred_err(class)` with a `&&'static str`, which relies on deref coercion.
  - In the edge test, the `#[async_trait::async_trait]` fake.
  - wiremock mock ordering with `up_to_n_times`.
- Not done (outside the files): compose, openapi-internal and RUNBOOKS entries (D-U41b-4), main.rs wiring with the cfg URL (D-U41b-3), a durable pending marker (D-U41b-2), and the D2-093 row in DEPTH-FINDINGS-W2.md is not edited.
- No live proof: needs org-core keys on, the scope granted and a real erasure.
