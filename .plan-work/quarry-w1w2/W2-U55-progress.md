# W2-U55 progress (WS17.C1, capture v2): BUILT, compile-checked, NOT run

Committed: 45d817ab4 (test literal prerequisite), 8ecaf112b (feature: profile_snapshot.rs, profile_capture.rs, profile_capture_live.rs, session.rs, chromiumoxide.rs, deadlines.rs, kernel.rs, lib.rs), aa09d2d7b (RUNBOOKS section), and the notes commit with this file.
Not committed (working tree, another unit's base): quarry-edge `profile_routes.rs` / `tests/profile_guard.rs` / `docs/openapi.yaml` hunks (see wf-W2-U55.md).
Verified: `cargo check -p quarry-browser --tests` (default and `--features chromiumoxide`) and `cargo check -p quarry-runtime -p quarry-edge --tests`: no errors; `node --check` on both embedded scripts; node mock of the serializer's storage path.
Not run: all 19 unit tests, the 5 live tests, the S0 spike.
Not built: restore (C2), IndexedDB for closed-tab origins, the checkpoint hook, 413 mapping, a CONTRACTS.md paragraph.
Test commands for the final phase: `cargo test -p quarry-browser`; `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide capture_ -- --test-threads=1 --ignored --nocapture`; plus `cargo test -p quarry-browser --features chromiumoxide bounded_calls` (the lint that scans chromiumoxide.rs).
Depth bullets: depth/W2-U55.md. Hand-over: wf-W2-U55.md.
