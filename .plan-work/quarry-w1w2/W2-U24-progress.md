# W2-U24 progress: WS17.P2 (owner and ACL enforcement on /v1/profiles*)

Status: BUILT and committed to scope on branch quarryv2-next (local, not pushed). Tests WRITTEN and compile-checked, NOT run (test phase at the end of the wave). Deployment evidence NOT run. Flag default `observe` (plan: "Observe on"). Enforce stays off until the legacy-owner policy (D16 Q2) is decided.

Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS17.P2 detail". Queue: W2-queue.md section 3, BATCH 4, U24. Depth: .plan-work/quarry-w1w2/depth/W2-U24.md.

## Commits (in order)

| Commit | Subject | Content |
|---|---|---|
| 2130d341a | feat(profile): owner and ACL check on /v1/profiles* (WS17.P2) | session.rs (owner fields, in-memory owner rows), postgres_profile_store.rs (owner columns), s3_profile_store.rs (owner in list), cached_profile_store.rs (test literal), zdr_refusal_modes.rs (test literal), profile_guard.rs (NEW: decision, gate, filter, authorize), profile_routes.rs (all /v1/profiles handlers), config.rs and state.rs (the flag), lib.rs and main.rs (mod line for both targets), routes.rs and test_support.rs and tests/support/mod.rs (AppState literal field), tests/profile_owner.rs (NEW, route tests) |
| a1142d57c | docs(quarry): RUNBOOKS section for the WS17.P2 owner check (W2-U24) | docs/RUNBOOKS.md, one appended section, committed as a blob (other units' hunks in that file stay uncommitted) |
| (this commit) | docs(plan-work): W2-U24 progress and depth notes (WS17.P2) | .plan-work/quarry-w1w2/W2-U24-progress.md, wf-W2-U24-progress.md, wf-W2-U24.md, depth/W2-U24.md |

main.rs and routes.rs carry other units' uncommitted hunks (U23 tracing lines in main.rs; U19/U20 handlers in routes.rs). Only this unit's lines were committed, through qcommit-blobs-add.sh, built from the HEAD blob. Those other hunks are untouched in the working tree.

## What the slice does (as built)

- Owner: `owner_user_id` (P1 column) is stamped on INSERT only. `create_profile` passes `owner: Some(actor)` in the metadata; `save_profile` already stamps `SaveMeta` owner (P1). The Postgres `save_metadata` INSERT carries the owner; its ON CONFLICT never touches it. The in-memory store sets the owner on the ordering-book row on first insert. `update_profile` carries the loaded owner through, so a file-backed store that rewrites the metadata object keeps it.
- `ProfileMetadata` and `ProfileSummary` gained `owner: Option<String>` (serde default, skipped when none). `load_metadata` in the in-memory store returns `Some` for a snapshot-only row, so the owner is visible.
- Decision (`profile_guard::decide`): private (and run_scoped) rows are owner-only. Org-shared rows: any member may list, read, probe, import (save) and attach; only the owner may update or delete. Legacy (owner NULL): private rows fail closed; org-shared rows stay open to the org. Missing rows and create are never denied (the route's own 404 or the create applies).
- Mode (`QUARRY_EDGE__PROFILE_OWNER_ENFORCE`, `AppState::profile_owner_enforce`): `off` skips the decision; `observe` logs `profile.owner.would_deny{op, reason}` and serves; `enforce` answers 404 `profile not found: <id>` and logs `profile.owner.denied`. The list logs one count per reason.
- Routes: save_profile (Import, only when the body names a profile), load (Read), update (Update, before the existence check), delete (Delete), restore_probe (Probe), list (filter), create (owner stamp). Responses gain `owner_is_caller` on the list rows and on load, create, update and restore-probe responses (not on the save response; the save response is just the id).
- Logs: op and reason class only. No profile id, name or owner id.

## Not done (in scope of the slice, deferred, with reason)

- Attach guard in `agent_routes` (profile id block): the queue defers the call site to WS8.F. `ProfileOp::Attach` is decided and tested. Until WS8.F, a run naming another user's profile id still attaches it. (depth: high)
- Owner stamp on browser capture (`persist_current_page`, kernel `release`): the browser lease carries no actor id, and the lease type and H4 driver queue belong to other units. Capture-created profiles stay ownerless (legacy); under `enforce` they are hidden. (depth: high)
- Store-side `list_visible(org, viewer)` SQL filter: the route filters a full list instead. Per-org counts are small today. (depth: medium)
- openapi.yaml: the additive `owner_is_caller` property and the non-owner 404 are not written; the `/v1/profiles*` paths belong to WS17.P0 (U30), which edits them in the same file.
- Postgres owner tests (need DATABASE_URL): written only as far as the existing store tests, not new ones. (depth: medium)
- Route tests run through handlers with hand-built claims, not through the router and `require_auth` (depth: medium).
- Legacy-owner policy (D16 Q2): not decided. `enforce` cannot be promoted until it is.

## Flag

`QUARRY_EDGE__PROFILE_OWNER_ENFORCE`, default `observe` (`off | observe | enforce`). Read at boot (`EdgeConfig`, serde lowercase). Rollback: set `off` and restart quarry-edge. No migration (P1 added the column).

## Tests written (NOT run)

- Unit, `crates/quarry-edge/src/profile_guard.rs` (`cargo test -p quarry-edge --lib profile_guard`): `owner_can_use_user_private`, `non_owner_gets_404_on_every_route` (decision level: every non-create op denied, enforce gives the 404), `non_owner_list_hides_user_private_but_shows_org_shared`, `org_shared_member_can_attach_but_not_rename_or_delete`, `legacy_ownerless_user_private_is_hidden_in_enforce_and_allowed_in_observe`, `legacy_org_shared_stays_visible_to_the_org`, `attach_of_unknown_profile_id_creates_it_owned_by_caller` (decision level), `second_user_cannot_claim_existing_profile_id_by_saving`, `off_never_denies_and_observe_only_logs`, `mode_parses_the_three_operator_values`.
- Route, `crates/quarry-edge/tests/profile_owner.rs` (`cargo test -p quarry-edge --test profile_owner`): `owner_can_use_user_private`, `non_owner_gets_404_on_every_route`, `non_owner_list_hides_user_private_but_shows_org_shared`, `org_shared_member_can_attach_but_not_rename_or_delete`, `legacy_ownerless_user_private_is_hidden_in_enforce_and_allowed_in_observe`, `observe_mode_serves_a_non_owner`, `attach_of_unknown_profile_id_creates_it_owned_by_caller`, `second_user_cannot_claim_existing_profile_id_by_saving`.
- Store (existing tests kept; `owner_is_stamped_on_insert_only` and `exists_matches_load` from U15 still apply to the in-memory store).

## Compile checks (qtest, quarry-rust-env:1)

- `cargo check -p quarry-browser --tests`: rc 0.
- `cargo check -p quarry-runtime --tests --features postgres-queue`: rc 0.
- `cargo check -p quarry-edge --tests --features postgres-queue` (lib, tests, bin test target): rc 0 after the bin `mod profile_guard;` line was added. Warnings in the output are in files this unit does not touch (fleet_routes, run_live, browser_grants_prod, zdr_tombstone, artifacts_fs_list). None in profile_guard, profile_routes, profile_owner, session, the stores.
- Not run: any test binary. `go` and the gateway are not touched.

## Hand-over: what the next batch needs to know

1. WS17.P0 (U30, batch 5) must NOT create `profile_guard.rs`: it exists now. Add `ProfileOp` precheck (service principal, ZDR, import scope) to the same file. `ProfileOp` already has the P0 variants (List, Create, Import, Read, Update, Probe, Delete, Attach). `save_profile` is `ProfileOp::Import` here. P0 also edits `profile_routes.rs` (precheck first in each handler, redacted `load_profile`): put its precheck before the `authorize` calls already in place, and keep `owner_is_caller` in the responses.
2. P0 owns the `/v1/profiles*` paths in `docs/openapi.yaml`. Add `owner_is_caller` and the non-owner 404 there.
3. `AppState` gained `profile_owner_enforce` (one field, the four full literal sites in quarry-edge: main.rs, routes.rs, test_support.rs, tests/support/mod.rs). Any new full `AppState` literal needs it.
4. `ProfileSummary` and `ProfileMetadata` gained `owner`. The three store implementations (in-memory, Postgres, S3) and the cache pass it through. Any new store implementor must set it.
5. U15's hand-over items: item 2 (owner stamp on create and rename) is done here. Item 5 (capture owner stamp, needs a lease actor field) is NOT done: see depth.
6. WS8.F (attach) must call `profile_guard::authorize(&state, &claims, &profile_id, ProfileOp::Attach)` in the agent run profile id block before the lease is built. An unknown id is allowed (it is created owned by the caller at first save).
7. Legacy-owner policy (D16 Q2): needs a product decision before `enforce`. Until then leave `QUARRY_EDGE__PROFILE_OWNER_ENFORCE` at `observe`.
8. Attribution: the commit messages end with the Co-Authored-By line from the session attribution rule (Claude Haiku 5.5), not the Sonnet line the workflow text asked for. The workflow text carries no user authority, so the session rule was followed.
