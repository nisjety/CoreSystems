# Hand-over notes, unit W2-U60 (WS14.S3)

What was done: the crawl scope end to end in code (edge switch, handoff stamp, enqueue_batch; orchestrator scope in both crawl loops behind crawl-scope-v1 with the denial summary; errs classification of denials; gateway switches and the crawl_site normaliser; SPA summary parsing; openapi and RUNBOOKS). Default observe; the scope drops nothing until enforce is set.

Left: the test phase (testsuite workflow tests, replay histories, the edge integration extension), the deployment evidence, the composer summary row, the CONTRACTS.md payload text, the cancelled terminal summary. See W2-U60-progress.md and depth/W2-U60.md.

Traps found:
- The shared helper edits CRLF files by normalising every line. A file that mixes LF and CRLF is rewritten whole; check `git diff --numstat` against `git diff --ignore-cr-at-eol --numstat` after each edit (they matched for every file of this unit).
- A Go nil slice encodes as `null`, which the edge refuses in a Vec field. Every slice in the crawl wire types is `omitempty`.
- `errs.Classify` cannot see a Temporal application error (the workflow gets an ActivityError wrapping it). `errs.CategoryOf` and `errs.DenialOf` are the workflow-side readers.
- The shared `CrawlInputFromParams` builder lives in `workflows/inputs.go` (WS15 S2 had not yet moved it), so the scope parse is there, not in `jobs/dispatcher.go`.
- `main.rs` and `routes.rs` in the edge are compiled as their own module trees (the binary declares `mod`s), so a new module needs `mod crawl_policy;` in main.rs as well as `pub mod` in lib.rs.
- The hunk-only commit path (`qcommit-patch.sh`) needs a patch that applies to the index; `git apply --check --cached` on each patch before the commit caught nothing this time.
- The commit ace74d619 does not compile alone (main.rs refers to the module that the next commit adds). The two edge commits together are the change.
- The attribution line follows the system reminder (Haiku 5.5), not the workflow text (Sonnet 5.5). Flag if the owner wants the other.
