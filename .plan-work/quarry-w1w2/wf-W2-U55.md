# W2-U55 hand-over (WS17.C1 capture v2)

Built, compile-checked, NOT run. Flag `QUARRY_BROWSER_PROFILE_CAPTURE_V2` defaults to 0.

Commits: 45d817ab4 (profile_roundtrip literals, prerequisite), 8ecaf112b (the feature), aa09d2d7b (RUNBOOKS section), plus the notes commit.

Where things are:
- Pure types, caps, journal, tagged encoding, flags: `crates/quarry-browser/src/profile_snapshot.rs`.
- CDP side (cookies, the two embedded scripts, flush binding, capture orchestration): `profile_capture.rs`.
- Driver wiring: `chromiumoxide.rs` (`SessionPages.journal`, `persist_profile_v2`, flush install in `open_tab_page`, `update_tab_metadata` records visits).
- Live tests: `profile_capture_live.rs` (include!d in `mod tests`).

Traps found:
- The `cdp_calls_are_bounded` lint scans only chromiumoxide.rs; the new CDP calls are in profile_capture.rs and the one call site is wrapped in `with_deadline(Op::ProfileCapture, ..)`. Do not move CDP calls into chromiumoxide.rs without wrapping them.
- Struct literals of `Cookie` and `SessionSnapshot` need `..Default::default()` now. U30's uncommitted files (`profile_routes.rs` tests, `tests/profile_guard.rs`) already carry the fix in the working tree.
- Uncommitted by this unit, left for U30's committer (they depend on U30's uncommitted code): the `profile_routes.rs` redact and restore_summary changes, the `docs/openapi.yaml` additions (`same_site`, origin storage counts, `truncated`) and the `..Default::default()` fixes in `profile_routes.rs` tests and `tests/profile_guard.rs`.
- WS17.C2 (U58) reads the tagged IndexedDB encoding documented on `IdbValue`; its seeding script is the decoder. `profile_loaded_nonempty` is on `SessionPages`; C2 adds `profile_restored` there.
