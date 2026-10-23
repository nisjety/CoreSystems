# W2-U50 hand-over (WS13.6: tile-and-stitch for full-page screenshots)

What was done: the stitch path of WS13.6 (method A only), behind QUARRY_EDGE__SCRAPE_STITCH (default off). The planning and the PNG streaming encoder are in quarry-browser `stitch.rs`; the tile captures are in the chromiumoxide driver; the heavy-lane permit is taken in the runtime's browser driver adapter. Full status in W2-U50-progress.md, the deferred items in depth/W2-U50.md.

What is left: the tests (run in the test phase: the stitch unit tests, stitch_live, and the edge tests named in the WS13.6 detail), the spike that sets SINGLE_CAPTURE_MAX_DEVICE_PX and picks method A or B (docs/SCRAPE_CAPTURE.md), the sticky-header and lazy-image fixtures, the stack evidence (30,000 px banded fixture, concurrent-stitch refusal, 90,000 px cap), and the memory measurement.

Traps found:
- The working tree of quarry-browser lib.rs carried a one-line hunk of another unit (cfg removed above `pub mod capture_plan;`). HEAD's default build needs it: the trait default methods call crate::capture_plan. The commit carries it and says so.
- The git index stores LF. The working copies of lib.rs, RUNBOOKS.md and browser_driver.rs are CRLF. Blob commits (qcommit-blobs.sh uses hash-object with no filters) must be LF; the path commits (qcommit.sh) normalise on add.
- `file` says a working copy is CRLF while `grep -c $'\r'` in this shell reports 0. Trust `file` and the helper, not the grep.
- png 0.18 `StreamWriter<'static, W>` needs W: 'static (into_stream_writer takes the writer by value). A borrowed `&mut Vec<u8>` sink fails to compile; the stitcher shares the bytes through an Arc<Mutex<Vec<u8>>> and moves them out at finish.
- Cargo.lock is git-ignored in this workspace; the new crates need no commit.
- A background qtest started with a trailing `&` returns at once and its log is written later; start qtest with run_in_background and no `&`, then wait with qwait.sh and a 600000 ms tool timeout (a plain 120 s tool call times out and moves to the background).
- The commit attribution line: the harness reminder asks for `Co-Authored-By: Claude Haiku 5.5`, the workflow text asks for `Co-Authored-By: Claude Sonnet 5.5`. The harness reminder governs the attribution lines here (the workflow text carries no user authority), so the commits carry Haiku 5.5. The owner should decide which line the repository wants.
