# W2-U45 hand-over (WS13.4b: screenshot and pdf on POST /v1/scrape)

What was done: the first shippable half of the WS13.4b slice, behind QUARRY_EDGE__SCRAPE_CAPTURE (committed `off`).
Four code commits and one docs commit (see W2-U45-progress.md). The browser takes the screenshot or PDF after the page
settles, before release; a failed capture is a warning; ZDR returns the bytes inline under a 6 MiB cap; the stream
route refuses capture formats; the page cache is bypassed for capture requests.

What is left: the tests named by the plan (run in the test phase), the live Chromium evidence (flag on, 501 path, ZDR
inline, artifact GET of PNG and PDF), openapi.yaml (blocked on another unit's uncommitted hunks), the usage meter's
capture counts (WS6.C-e owner), the deploy compose decision, and the SINGLE_CAPTURE_MAX_DEVICE_PX spike.

Traps found:
- 11c22dae reverted WS6.D-s in routes.rs. The blob helper (qcommit-blobs.sh) builds from the HEAD at the moment it runs;
  cfdc8a07a landed between my blob build and my commit. 52cd874c8 restored it. Rebuild blobs right before committing.
- qcommit-blobs.sh needs qcommit-blobs-add.sh's variant for new files (update-index --add). Both are in the scratch tools dir.
- Working-tree copies of lib.rs, routes.rs and docker-compose.yml carried other units' hunks (redaction_boot, WS12.S2 env
  lines, WS6.D-s handlers). Commit only your hunks; the blob helper does that when the blob is built from HEAD.
- The Edit tool and the node helper both preserve CRLF; the repo blobs are LF. Do not commit CRLF blobs (strip \r).
- A duplicate module line appeared when a failed edit script was re-run: scripts that sub() on an anchor they already
  changed double-apply. Check the file after any re-run.
- The quarry-edge lib test target does not build while budget.rs has inaccessible test helpers (another unit's).
- Commit attribution: the harness reminder asks for Co-Authored-By Claude Haiku 5.5; the batch text asked for Sonnet 5.5.
  The commits carry the Haiku line (the later harness instruction), as U09 did.
