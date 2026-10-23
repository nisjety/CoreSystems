# wf-W2-U44 hand-over (WS12.S2)

Done: see W2-U44-progress.md. Left (owner decisions or later phases): the wave test phase, Flow A/B/C on a rebuilt quarry-edge image, 3 to 5 days of `shadow` on staging, WS12.S3 to S5.

Traps found:
1. rustfmt on a test file reformats its `mod`/`#[path]` children (other units' support files). Revert with `git apply -R` of the captured diff, not with git checkout.
2. A `qtest` command string must not contain backticks (the container shell runs them as command substitution).
3. RUNBOOKS.md carries one giant uncommitted hunk from another unit, so an append cannot be filtered out of `git diff`; build the patch from the HEAD blob plus the new section (see scratchpad W2-U44/mkpatch.js `appendPatch`).
4. The quarry-edge bin test target needs `mod scrape_actions;` in main.rs (other unit) before it compiles.
