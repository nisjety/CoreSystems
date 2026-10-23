# W2-U34 hand-over (WS7.G 6a backstops)

What was built: see W2-U34-progress.md (commit list) and the RUNBOOKS section "Web policy backstops". What is left: 6b, running the tests, the evidence listed in depth/W2-U34.md.

Traps found
- `ObservationRunner::execute` reinstalls the allow-list egress policy on every action; anything set once at run start (narrowing, open web) is lost after step one unless it rides on `ObservationContext`.
- The edge bin target (`main.rs`) declares its own module list and lacks `mod scrape_actions;`; it does not compile at HEAD.
- `routes.rs`, `quarry-browser/src/lib.rs` and `RUNBOOKS.md` carry other units' uncommitted hunks: commit with a filtered patch (`qcommit-patch.sh`) or the HEAD blob plus the section (`qcommit-blobs.sh`).
- The commit message of 53df13da1 wrongly says it carries the RUNBOOKS section; f8a24e56e does.
