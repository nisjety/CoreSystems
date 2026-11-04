# W2-U58 status (profile restore: load failure signal, setCookies acceptance)

Unit W2-U58 of batch W2-B6, lane b (branch w23/lane-b). D2-028, D2-117.

## Built

BUILT in `crates/quarry-browser/src/profile_restore.rs` (the one scope file).

Already existing (not rebuilt): the corrupt-load guard. `chromiumoxide.rs` `load_first_tab_snapshot` already sets `lease_restore_failed` on a corrupt load and `release` skips the save (`profile_save_skipped_restore_failed`). The gap was the missing typed signal and the unchecked cookie acceptance.

Built (the gap):
- D2-028: `ProfileLoadError::blocks_save()` (corrupt and key-unavailable block the save, key-destroyed does not) and `ProfileLoadError::restore_report()`, a typed `RestoreReport` with `failed` set and the closed label `load_failure: Some("corrupt" | "key_unavailable" | "key_destroyed")`. Class only, never the store's error text.
- D2-117: the pure `apply_cookies(&dyn CookieSink, plans, &mut RestoreReport)` with the `CookieSink` trait (`set`, `read_back`). Chunks of 100; a refused chunk is counted (`cookie_chunks_refused`) and retried per cookie; 2 or more planned cookies with none applied is a systematic refusal and sets `failed`; the jar is read back (`Storage.getCookies { browserContextId }`) and an accepted cookie that the jar does not hold moves from applied to rejected and is counted in `cookies_dropped_after_accept`; an unreadable jar sets `cookie_readback_unavailable` and changes no count. `CookieKey` (name, host, path) normalises the host (lower case, no leading dot, no IPv6 brackets) and prints nothing.
- The CDP half (`restore_into_context`) now drives `apply_cookies` through `ContextJar` (the old inline loop is gone).
- New RestoreReport fields (not part of the wire `ProfileRestoreSummary`): `load_failure`, `cookie_chunks_refused`, `cookies_dropped_after_accept`, `cookie_readback_unavailable`. Existing struct literals use `..Default`, so nothing else changed.

## Tests

`cargo test -p quarry-browser --lib profile_restore` via qtest-lane b: 23 passed, 0 failed, 0 ignored (14 new unit tests with a mock jar, tokio::test, node-free; plus the existing ones). `cargo check -p quarry-browser --features chromiumoxide --lib`: clean. Full quarry-browser lib: 219 passed, 0 failed (default features); 326 passed, 0 failed, 37 ignored (`--features chromiumoxide`).

## Flag and default

None (bug fix). Persistence default unchanged; no behaviour change when every cookie is accepted and read back (one extra `Storage.getCookies` call per restore that applied at least one cookie).

## Assumed decisions

- Systematic refusal threshold is 2 planned cookies with none applied (a lone refused cookie cannot be told from a systematic refusal). A systematic refusal sets `failed`, so the session does not save a cookie-less jar over the stored profile.
- A read-back miss is folded into `cookies_rejected` (so it reaches the existing wire summary) and is also counted separately.
- Load-failure signalling is added as a pure API; wiring it into the attach/telemetry path is left (see not done).

## Model Plane asks

None.

## Not done / not verified

- Wiring: `chromiumoxide.rs` `load_first_tab_snapshot` still only logs and sets `lease_restore_failed`; it does not store `ProfileLoadError::restore_report()` on the session, and `ProfileRestoreSummary` (quarry-core contracts) has no `load_failure` field yet. Out of the unit's file scope; logged as depth row W23-b-U58-1.
- Live legs: the seven live restore tests (D2-117 evidence) were not run (they need Chromium). The `Storage.getCookies` read-back, and the exact domain spelling Chromium reports (host-only bare, IPv6 with or without brackets), are verified only by compile and the mock.
