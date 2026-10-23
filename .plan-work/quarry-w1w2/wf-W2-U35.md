# W2-U35 hand-over (WS7.J observe part, claim intake)

Built: the token's `web_policy` claim is read (`auth.rs`), applied as the org layer (`policy_baseline.rs`, `web_policy_edge.rs`) on the scrape routes and agent run start, with the required-claim flag (default off). Commits c5cb15995 (code) and b97c3e240 (tests). Details: W2-U35-progress.md and depth/W2-U35.md.

Left: HTTP-level claim tests on real tokens; auth-core goldens and minting (WS7.I); crawl, batch and profile routes (WS7.E, U33); the claim on the agent fallback path.

Traps found:
- The HEAD blobs are LF; the working tree is CRLF (core.autocrlf=true). `qcommit-blobs.sh` stores bytes as given, so its sources must be LF. Build the copies with a node script, never with `grep -c` counts (the grep line-ending counts I first read were wrong; node counts were right).
- `qcommit-blobs.sh` cannot add a NEW file (`update-index --cacheinfo` without `--add`): commit new files with `qcommit.sh` in their own commit.
- `cargo check -p quarry-edge --lib --test X` does not compile the lib's cfg(test) code or the other test targets; `--lib --tests` does, and it fails while another unit's `resource_routes.rs` is mid-edit (`crate::job_stream`). Re-run after that unit lands.
- Changing `web_policy_edge::decide` broke U13's `tests/policy_baseline.rs` (seven calls); fixed by moving them to `decide_for_principal`. Any future signature change must grep `tests/` too.
- `auth.rs` and `routes.rs` carried another unit's WS17.P0 hunks; committed from the HEAD copy plus this unit's edits. Keep this pattern for shared hot files.
