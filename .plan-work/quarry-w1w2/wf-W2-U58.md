# W2-U58 hand-over (WS17.C2 restore)

Built, compile-checked, NOT run. No flag of its own (the restore replaces the old path, rollback is a revert); the one tunable is `QUARRY_BROWSER_PROFILE_SESSION_COOKIE_MAX_AGE_S`.

Commits: 8f47e6f2d (the feature), 9706d03ec (OpenAPI schema and RUNBOOKS section, hunk-only), plus the notes commit.

Where things are:
- Pure half (policy, cookie plan, seeding script, report, `ProfileLoadError`): `crates/quarry-browser/src/profile_restore.rs`. CDP half in its `cdp` module (feature `chromiumoxide`).
- Driver wiring: `chromiumoxide.rs` `open_tab_page` (first-tab load, emulation after hygiene, restore after the network guard and before the goto, commit sets `profile_restored`), `load_snapshot`, `persist_current_page` (skips when `lease_restore_failed`).
- Contract: `ProfileRestoreSummary` and `BrowserTelemetry.profile_restore` in quarry-core; `SessionSnapshot.captured_at` in session.rs; `profileRestore` on the live browser-session projection.
- Live tests: `profile_restore_live.rs` (include!d like U55's).

Traps found:
- The first tab's restore must not set `profile_restored` before commit: a first tab that fails takes its context with it, and the next attempt creates a new context that still needs the restore. It is set where `browser_context_id` is committed.
- `ErrorCode::RuntimeNotReady` is also what the envelope store returns for a key it cannot use; `classify` treats any other load error as `Corrupt` (safe: start fresh, never save).
- Capture v2 replaces the whole snapshot on save, so an origin the session never visits would vanish; `OriginJournal::carry` keeps what the loaded profile held (not in the plan).
- `Op::TabAttach` (15 s) bounds the restore; `best_effort` wraps the emulation. The CDP-bound lint scans only chromiumoxide.rs: the new CDP calls live in profile_restore.rs behind those wrappers. Do not move them into chromiumoxide.rs unwrapped.
- `SessionSnapshot` literals without `..Default::default()` break (as with U55): `captured_at` is new.
- Working tree: `docs/RUNBOOKS.md` and `docs/openapi.yaml` carry other units' hunks; commit them hunk-only (qcommit-patch).
