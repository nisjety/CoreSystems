
## 4. Slices (PR-sized, in build order)

Two independent tracks. **Track A (security and storage):** P0, P1, E1, P2, E2, E3. **Track B (snapshot completeness):** S0, C1, C2. The tracks share no files except `QB/session.rs` (schema additions land in C1; the encryption layer serializes whatever `SessionSnapshot` serializes to, so it needs no change). Global build order: P0, S0, P1, E1, C1, P2, C2, E2, E3, (S7). Commands for Rust are run in the `rust:1` container recipe (Windows host cannot build Quarry; `bash -c`, cache volumes `quarry-cargo`, `quarry-go`, `quarry-target`; a full `-p quarry-runtime -p quarry-edge` run exceeds 10 minutes, run it in the background).

Common repo commands used below:
- Quarry Rust: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime -p quarry-edge -p quarry-browser`; browser live lane `cargo test -p quarry-browser --features chromiumoxide -- --test-threads=1` with `CHROMIUMOXIDE_TEST=1` (D3 S1 adds the CI job and the stub egress helper `live_driver()`).
- Postgres-backed store tests: `DATABASE_URL=postgres://... cargo test -p quarry-runtime --features postgres-queue profile`. CI today sets no `DATABASE_URL` and has no Postgres service, so these never run in CI (S9). P1 adds a Postgres 16 service to a Quarry CI job, copying `.github/workflows/org-service.yml` [C] 37-50.
- Redis-backed cache tests: `REDIS_URL=redis://... cargo test -p quarry-runtime cached_profile_store` (skip when unset today).
- Edge tests: `cargo test -p quarry-edge --test profile_roundtrip --test zdr_behavior --test profile_guard`.
- Go: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`; `cd "apps/Control Plane/org-core" && go test -v -p 1 ./...` with `TEST_DATABASE_URL` (CI: `org-service.yml` 95-103).
- Stack evidence: Ingestion Plane via the env-file chain recipe (`docker compose -p ingestion-plane <env files> -f "apps/Ingestion Plane/docker-compose.yml" up -d --no-deps --build quarry-edge quarry-control`); Control Plane via `bash scripts/run-control-plane.sh build org-core` then `up -d --no-deps org-core` (never bare `docker compose`).

---

### Slice P0: profile route guard, redacted reads, ZDR and service-principal refusal (Track A, no schema change)

**Goal.** Close the unprotected surface now: no raw cookie reads, no service-principal access, ZDR refusal, import behind a scope. No dependency on encryption.

**File-level change list.**
- `QE/profile_guard.rs` [NEW]: `enum ProfileOp { List, Create, Import, Read, Update, Probe, Delete, Attach }`; `fn precheck(claims: &Claims, op: ProfileOp, cfg: &ProfileGuardCfg) -> Result<(), QuarryError>` implementing: service principal (`claims.is_service()`) -> `Forbidden` for every op ("profiles are not available to service principals; they never read or write browser state"); ZDR (`claims.zdr_floor()`) -> refuse `Create|Import|Read|Update|Probe|Attach`, allow `List|Delete`, message contains "ZDR" (the existing tests assert it), `details.zdr_refusal = {code:"zdr_durable_mutation_forbidden", action}` when D4's `zdr_refusal.rs` exists, observe-only (log `zdr.would_refuse`) while the flag is false; import scope check `claims.has_scope("quarry:profiles:import")`. Owner logic arrives in P2 (function reserved).
- `QE/profile_routes.rs` [C] (all handlers 102-453): call `precheck` first in every handler; `load_profile` (268-299) returns `ProfileView` (D16-8) built by a new `fn redact(&SessionSnapshot) -> ProfileView` instead of `SessionSnapshot`; `restore_probe` (361-430) adds `earliest_expiry`, `cookies_expired`, `origins_count` (from existing fields until C1 populates `origins`) and computes `restorable` as "has any unexpired cookie or any storage"; `save_profile` (102-126) requires the import scope, a 4 MiB body cap and 413 on overflow; `update_profile`'s existence check (202-207) switches from `load` to a new `ProfileStore::exists` (added in P1; until then keep `load`).
- `QE/config.rs` [M]: append `zdr_refuse_durable_resources: bool` (`QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`, default false; shared name with D4 S-8). Boot warning when `ENVIRONMENT` is prod and the flag is false.
- `QE/lib.rs` [M]: `pub mod profile_guard;` (one line).
- `QE/tests/profile_guard.rs` [NEW] (new file, duplicating the small token-minting helper from `zdr_claim_floor.rs` [M] 144-170 rather than editing that file).
- `QE/tests/profile_roundtrip.rs` [C] (lines 107, 209, 414, 456 read the raw snapshot): update the assertions to the redacted shape; keep the save path under the import scope.
- `docs/RUNBOOKS.md` [M] 117-128: replace `curl /v1/profiles/<id>` with the redacted summary and the `restore_probe` fields; remove the nonexistent `expires_at` step. `docs/openapi.yaml` [M]: add the `/v1/profiles*` paths and the redacted schema (none exist today, 2.3-8).
- `GW/domains/ingestions/profiles.rs` [C] 51-61: accept object entries (`v.get("profile_id")`) as well as strings (finding 2.3-5). Frontend Plane file, one hunk, owner: browser-UI workstream; included here because it is the same profile list contract.

**API / contract changes.** `GET /v1/profiles/:id` response becomes `{data:{profile_id, name, scope, saved_at, rev, snapshot_version, cookies:[{name, domain, path, secure, http_only, same_site, session, expires, value_len, value:null}], origins:[...], locale, timezone, has_user_agent, has_viewport, truncated}}` (breaking for raw readers; none exist in repo besides tests and the runbook). New refusal statuses: 403 for service principals and ZDR callers, 413 on import over cap. No migration.

**Flags / rollout / rollback.** ZDR enforcement behind `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` (observe first, D16-9). Service-principal refusal and redaction ship on (security fix; no live caller). Rollback: revert the PR; the flag turns ZDR enforcement off without a revert.

**Tests.** Unit (`profile_guard.rs`): `service_principal_is_refused_for_every_op`, `zdr_user_is_refused_except_list_and_delete_when_flag_on`, `zdr_flag_off_only_logs`, `import_requires_scope`, `redact_never_emits_a_cookie_value`, `restorable_is_false_when_all_cookies_expired`. Integration (`profile_guard.rs`): `get_profile_returns_value_null_and_value_len`, `canary_cookie_value_never_appears_in_any_profile_response`, `service_token_gets_403_on_all_profile_routes`, `zdr_org_user_cannot_create_or_probe_but_can_delete`, `import_over_cap_is_413`. Existing that must stay green: `zdr_claim_floor.rs::a_zdr_org_users_agent_run_cannot_take_a_persistent_profile`, `zdr_behavior.rs` (4 tests, 67/95/132/179). Commands: `cargo test -p quarry-edge --test profile_roundtrip --test zdr_behavior --test profile_guard`; `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test` for the list-shape fix (or `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` from `verevonv3`).

**Definition of done.** No `/v1/profiles` response contains a cookie value; service tokens cannot reach the routes; ZDR-org users are refused on durable operations with the flag on; the gateway profile list shows real profiles.

**Deployment EVIDENCE.** Rebuild the `quarry-edge` image (and the gateway image per the gateway recipe for the list fix) on the running stack. Flow: (1) with a standard-org user token create a profile via `POST /v1/profiles/create`, run a persistent agent run that sets a cookie whose value is the canary `CANARY-d16-p0`, close the run; `curl -s .../v1/profiles/<id> | grep -c CANARY-d16-p0` returns 0 and the body shows `"value":null,"value_len":<n>`. (2) `docker logs quarry-edge 2>&1 | grep -c CANARY-d16-p0` returns 0. (3) Same call with the execution-core service token returns 403. (4) With the flag on, a ZDR-org user token (org ZDR set through org-core, claim stamped by auth-core) gets 403 on `POST /v1/profiles/create` and 200/204 on `DELETE`. (5) `GET /api/ingestions/profiles` through the gateway lists the profile (before the fix it returns an empty list).

**Effort.** S, 3 engineer-days (0.5 of it the gateway fix and runbook).

**Risks and mitigations.** The redaction breaks any undiscovered raw reader: mitigate by a one-release `Deprecation` log on the old shape is not possible (shape changes at once), so grep the three repos (done: only tests and runbook) and announce in the changelog. ZDR enforcement could surprise a ZDR customer holding legacy profiles: observe-first flag, delete stays open.

---

### Slice P1: monotonic saves, deletion tombstones, `exists` (Track A; migration 0014)

**Goal.** Ordered saves per D16-6 on the existing plaintext store, before encryption changes the storage shape.

**File-level change list.**
- `QR/migrations/0014_profile_ordering.sql` [NEW]: `ALTER TABLE quarry_profiles ADD COLUMN IF NOT EXISTS saved_at TIMESTAMPTZ NULL, ADD COLUMN IF NOT EXISTS rev BIGINT NOT NULL DEFAULT 0, ADD COLUMN IF NOT EXISTS owner_user_id TEXT NULL; ALTER TABLE quarry_profiles ALTER COLUMN snapshot DROP NOT NULL; CREATE TABLE IF NOT EXISTS quarry_profile_tombstones (org_id TEXT NOT NULL, profile_id TEXT NOT NULL, deleted_at TIMESTAMPTZ NOT NULL, expires_at TIMESTAMPTZ NOT NULL, PRIMARY KEY (org_id, profile_id)); CREATE INDEX IF NOT EXISTS quarry_profile_tombstones_expiry_idx ON quarry_profile_tombstones (expires_at);` (`owner_user_id` is added here so P2 needs no further migration; `snapshot` nullable prepares E1). Numbering: provisional. quarry-runtime migrations 0010 (D2), 0011 (D6 and D8 both claim it), 0012 and 0013 (D12) are claimed by other designs; take the next free numbers at merge (sqlx requires unique versions; applied files are immutable).
- `QB/session.rs` [C]: `ProfileStore::save` gains `SaveMeta { saved_at: DateTime<Utc>, owner: Option<String> }` and returns `QuarryResult<SaveOutcome>` (`Stored{rev}`, `IgnoredStale`, `IgnoredByTombstone`); new `async fn exists(&self, org, id) -> QuarryResult<bool>`; `InMemoryProfileStore` implements both with the same semantics (tombstone map, saved_at compare) so unit tests run without Postgres.
- `QR/postgres_profile_store.rs` [C] 72-131: conditional upsert exactly as D16-6 (single statement with `WHERE NOT EXISTS (tombstone with deleted_at >= $saved_at)` and `DO UPDATE .. WHERE saved_at IS NULL OR saved_at < EXCLUDED.saved_at`, `saved_at` clamped with `LEAST($saved_at, NOW() + interval '5 seconds')`, `rev = quarry_profiles.rev + 1`, owner set on insert only); `delete` becomes one transaction (tombstone upsert with `GREATEST`, then `DELETE`); `exists`; a sweep method `purge_expired_tombstones()` called from a 1-hour task started in `main.rs`.
- `QR/cached_profile_store.rs` [C] 80-143: invalidate-on-write (D16-1), only after `Stored`; keep for now until E1 replaces it with the blob-level cache.
- `QB/chromiumoxide.rs` [C] 675-697: `persist_current_page` passes `SaveMeta { saved_at: capture start time, owner: lease owner }` and logs the outcome; `QB/kernel.rs` [C] 575-585 same.
- `QE/profile_routes.rs` [C]: `save_profile` and `create_profile` pass `SaveMeta` with `owner = claims.actor_id()`; `update_profile` uses `exists`.
- `.github/workflows/quarry-v2-ci.yml` [C]: new job `quarry-postgres-store` (Postgres 16 service, `DATABASE_URL`, `cargo test -p quarry-runtime --features postgres-queue profile`), first `continue-on-error: true`, promoted after two green weeks (the file's own stance on new gates).

**API / data-model changes.** Migration above; `SaveOutcome` internal. No wire change.

**Flags / rollout / rollback.** None needed: the conditional upsert is a strict tightening and `saved_at NULL` rows behave as "older than anything". Rollback: revert code; the new columns are harmless.

**Tests.** `save_with_older_saved_at_is_ignored`, `save_after_delete_with_older_saved_at_is_ignored_by_tombstone`, `save_after_delete_with_newer_saved_at_creates_a_new_row`, `saved_at_is_clamped_to_now_plus_5s`, `delete_writes_tombstone_in_same_transaction`, `expired_tombstones_are_swept`, `rev_increments_only_on_stored`, `cache_is_invalidated_not_populated_on_ignored_save` (Redis-gated), the existing four Postgres tests unchanged. In-memory parity tests in `session.rs`. Command: Postgres lane above plus `cargo test -p quarry-browser`.

**Definition of done.** A zombie session cannot resurrect a deleted profile; an older capture never replaces a newer one; the Postgres tests run in CI.

**Deployment EVIDENCE.** Rebuild the edge image; the stack applies `0014` at boot (`docker logs quarry-edge | grep -i migrat`). Flow: (1) start run A with `persist_profile` and profile id P (keep it open); `DELETE /v1/profiles/P`; close run A. `psql -d quarry_edge -c "select count(*) from quarry_profiles where profile_id='P'"` returns 0 and `quarry_profile_tombstones` has a row for P (before the slice the row reappears). (2) Two runs on profile Q: start A, start B, close B (writes), then close A. The row's `saved_at` equals the later capture and `rev` counts only stored saves (assert through `select saved_at, rev`). (3) Edge log shows `profile save ignored reason=stale` for the losing write.

**Effort.** M, 4 engineer-days.

**Risks and mitigations.** Clock skew across pods (clamp, and capture time not commit time); `sqlx::migrate!` checksum immutability (never edit applied files); old pods during a rolling deploy ignore the new columns and keep writing plaintext without `saved_at` (rows with NULL `saved_at` accept the next ordered save).

---

### Slice P2: owner and ACL enforcement (Track A; builds on P1's `owner_user_id`)

**Goal.** Enforce D16-7 inside Quarry: `user_private` means owner only, at every `/v1/profiles*` route and at run attach.

**File-level change list.**
- `QE/profile_guard.rs` [NEW from P0]: `async fn authorize(store: &dyn ProfileStore, claims: &Claims, profile: &ProfileKind, op: ProfileOp, mode: OwnerEnforce) -> Result<ProfileMeta, QuarryError>`; reads `ProfileMetadata` extended with `owner: Option<String>`; decision table in D16-7; `observe` mode logs `profile.owner.would_deny{reason=not_owner|legacy_ownerless, op}` (no ids beyond the org, no names) and allows.
- `QB/session.rs` [C]: `ProfileMetadata` gains `owner: Option<String>` (serde default); `owner_is_caller` on the wire is computed in the route, not stored. `list` takes the caller to filter hidden rows: new `list_visible(org, viewer: &str)` (SQL `WHERE org_id = $1 AND (scope = 'org_shared' OR owner_user_id = $2)`; legacy `NULL` owners hidden for `user_private`).
- `QR/postgres_profile_store.rs` [C]: select/insert `owner_user_id`; owner never updated (`ON CONFLICT` list unchanged).
- `QE/profile_routes.rs` [C]: call `authorize` in `update_profile`, `load_profile`, `restore_probe`, `delete_profile`; `list_profiles` uses `list_visible`; `create_profile` stamps the owner; responses add `owner_is_caller: bool` (additive).
- `QE/agent_routes.rs` [C] 1415-1432: when `body.profile_id` is present and the run is not a resume, call `authorize(.., ProfileOp::Attach)` before the lease is built; resume keeps the checkpoint's profile (already authorized at first start). This hunk is the main conflict point with D2's split of `agent_routes.rs` into `agent_routes/*` and D1/D7 additions (section 5).
- `QE/config.rs` [M]: `profile_owner_enforce` (`QUARRY_EDGE__PROFILE_OWNER_ENFORCE`, default `observe`).
- Gateway: no change needed for enforcement (Quarry rejects), but `GW/domains/browser.rs` [C] 2492 swallows 404 on delete; keep (idempotent delete) and note.

**API / contract changes.** `GET /v1/profiles` filters by viewer; non-owner access answers 404 `profile not found`. Additive `owner_is_caller` field. No migration (P1 added the column).

**Flags / rollout / rollback.** `off|observe|enforce`, default `observe`. Move to `enforce` after a week of `would_deny` counts shows only expected denials; rollback is the flag.

**Tests.** `owner_can_use_user_private`, `non_owner_gets_404_on_every_route`, `non_owner_list_hides_user_private_but_shows_org_shared`, `org_shared_member_can_attach_but_not_rename_or_delete`, `legacy_ownerless_user_private_is_hidden_in_enforce_and_allowed_in_observe`, `attach_of_unknown_profile_id_creates_it_owned_by_caller`, `second_user_cannot_claim_existing_profile_id_by_saving`, `model_driven_run_naming_another_users_profile_is_rejected` (service token with a delegated user bearer, D1). Commands as P0 plus `cargo test -p quarry-edge --test zdr_claim_floor` (must stay green).

**Definition of done.** No cross-user profile read, attach, rename or delete inside an org with the flag on; a legacy-row rescue runbook exists.

**Deployment EVIDENCE.** Rebuild the edge image. With two users A and B in one org (tokens through auth-core): A creates a `user_private` profile P; `GET /v1/profiles` as B does not list P; `POST /v1/agent/runs {profile_id:P, persist_profile:true}` as B returns 404 in `enforce` and logs `would_deny` in `observe`; as A it succeeds. Flip A's profile to `org_shared` through the gateway PATCH: B can attach, B's rename returns 404. Evidence includes `docker logs quarry-edge | grep profile.owner`.

**Effort.** M, 4 engineer-days.

**Risks and mitigations.** Legacy `user_private` rows become invisible to their real owners at `enforce` (the 0008 precedent chose this fail-closed behaviour; observe-first plus a rescue runbook and a gateway banner are the mitigation; decision Q2). Org admins have no override (adding one needs a scope the registry does not issue); Spaces cannot widen the ACL (D16-7).
