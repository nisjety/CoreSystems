# W2-U55 status: profile capture keeps IndexedDB of closed-tab origins (D2-070)

Branch w23/lane-c. Date 2026-11-02. Status: BUILT (unit tests green; live test written, compile-checked, not run).

## Built
Nothing of the scope existed (the header of `profile_capture.rs` said it was not built), so the whole phase is new, in `crates/quarry-browser/src/profile_capture.rs` only:
- Phase 4b of `capture_profile`: when `QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS` is on (and the IndexedDB switch `QUARRY_BROWSER_PROFILE_IDB` is on), the origins the journal has visited that no live tab covers are read through the frame-less `IndexedDB` CDP domain (`requestDatabaseNames`, `requestDatabase`, `requestData`) sent through a live tab of the same browser context, with the closed origin as `securityOrigin`. Keys and values are turned into the snapshot's tagged encoding: primitives in Rust (`primitive_to_tagged`), objects by one `Runtime.callFunctionOn` per object running `ENCODE_REMOTE_JS` (the same encoding as the in-page serializer).
- Source seam: `ClosedIdbSource` trait; `CdpClosedIdb` is the real source, a mock in the tests. `capture_closed_origins` is the bounded orchestration, `merge_closed_idb` folds the result into the states the journal holds for those origins, `closed_origin_candidates` picks the origins.
- Bounds: own budget 2 s (`BUDGET_CLOSED_MS`), clamped to the overall 10 s minus a 500 ms margin so the caller's `Op::ProfileCapture` deadline cannot fire and lose the cookies; 16 origins per capture; 16 databases per origin; 2 MiB and 10 000 records per origin; 8 MiB over all closed origins; every limit also capped by the snapshot caps (`ClosedLimits::from_caps`). The merged result still goes through `merge_and_cap`.
- Safety rules: a failure or deadline inside an origin discards that origin (a half-read database never replaces a complete one); a cap keeps what was read and flags `truncated` (a store cut before its first record keeps its definition with `omitted`); a read that finds no database never erases IndexedDB the journal or loaded profile held; a record whose key cannot be carried is skipped and counted in `dropped_values`; no live tab means the phase reports `no_page` and does nothing.
- Report: `ProfileCapture.closed: ClosedReport` (counts only) and a `profile_capture_closed_origins` tracing event with the counts (no origin, URL or value); `CaptureReport.truncated` is set when the phase truncates.

## Commits
- 24cdcca75 feat(quarry-browser): profile capture reads IndexedDB of closed-tab origins behind QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS (W2-U55, D2-070)
- plus the docs commit with this file and the depth rows.

## Tests (Rust, wrapper lane c, `cargo test -p quarry-browser --features chromiumoxide --lib`)
- `profile_capture` tests: 22 passed, 0 failed, 1 ignored (the live test). 19 of the 22 are new: flag default and parsing, limits never above the caps, candidates, paged read, per-origin record cap, per-origin byte cap, omitted store, all-origins cap, database cap, one failing origin, failure in a later page, deadline (paused time), empty origin, merge replace/add, empty read never erases, merged result through `merge_and_cap`, primitive encoding table, decoder round trip, remote encoder shape.
- Whole quarry-browser lib with the feature: 332 passed, 0 failed, 38 ignored (the same 370 tests as before plus the 20 new ones, no other test changed).
- Live (`live_closed_tab_origin_idb_is_captured_through_another_tab`, `#[ignore]`): opens two tabs in one context, seeds IndexedDB (Date, BigInt, Uint8Array) on a loopback origin, closes that tab, reads it back through the other tab. Compiled, NOT run (needs the pinned Chromium: `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 cargo test -p quarry-browser --features chromiumoxide --lib live_closed_tab -- --ignored`).

## Flag and default
`QUARRY_BROWSER_CAPTURE_CLOSED_ORIGINS`, default OFF (same `1/true/on/yes` parsing as the other profile flags). Read inside `capture_profile` because `chromiumoxide.rs` (the caller) is outside this unit's file; with it off, capture is byte-for-byte today's behaviour.

## Assumed decisions
- Enumerate = the journal's visited origins minus live ones (not `Storage.getUsageAndQuota` or a browser-wide scan): only origins this session visited can have changed, and the journal already bounds them at 64.
- Reading goes through a live tab; with no tab open the phase is skipped (reported `no_page`), the previous state is kept.
- An empty or failed read keeps the older IndexedDB (journal/carried) rather than erasing it.
- Per-origin and total caps as listed above (2 MiB, 10k records, 16 databases, 16 origins, 8 MiB).
- Cost accepted: one `callFunctionOn` round trip per object-valued key/value; the 2 s budget bounds it.

## Model Plane asks
None.

## Not verified
- Anything against a real Chromium: that `IndexedDB.requestDatabase*` with `securityOrigin` works for a closed origin on the pinned build and in a non-default browser context (the S0 probe P3 recorded this for `example.com` in a context, but its table was not re-run for this unit), the shape of `requestData` RemoteObjects for Blob/File values, and the real cost of 10k records at one round trip per object.
- The wiring in `persist_profile_v2` (it calls `capture_profile` unchanged and does not read `ProfileCapture.closed`; the field has `#[allow(dead_code)]`).
- Restore of a captured closed-origin IndexedDB (the restore path is unchanged and already handles any origin in the snapshot).
