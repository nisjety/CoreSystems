# W3-B1-01 progress (E3.9.1 and E3.9.5): status DONE for scope, not run

Built and committed (local branch quarryv2-next, no push):
- 185a3f802 feat(quarry-evals): release score file keyed by commit (E3.9.5). release_matrix.rs writes release_score.json (commit from QUARRY_RELEASE_COMMIT or GITHUB_SHA, else "unknown"; gate counts; fixture pass rate; bakeoff net wins; captured providers). build_score is pure; three unit tests written.
- c98d262d6 ci(quarry-v2): eval gates job (E3.9.1). New .github/workflows/quarry-v2-eval.yml: build the three gate bins, delete stale lab/evals outputs, run quarry-eval, quarry-bakeoff (solo), quarry-release-matrix; job summary and artifact upload.
- 14e4babff docs(quarry-v2): eval CI rows in SCOREBOARD (E3.9.5). 12 added lines, all _pending_.
- cc05191fd docs(quarry-v2): RUNBOOKS section for the eval CI gates (E3.9.1). Hunk-only patch against HEAD (10 lines). Other units' RUNBOOKS hunks left uncommitted.

Verification (compile only, nothing run):
- `cargo check -p quarry-evals --bin quarry-eval --bin quarry-bakeoff --bin quarry-release-matrix` (qtest, quarry-rust-env:2): Finished, exit 0, no warnings.
- `cargo test -p quarry-evals --bin quarry-release-matrix --no-run`: Executable built, exit 0.
- `cargo check --tests -p quarry-evals` (all bins): FAILED on lab/evals/src/browser_benchmark.rs:221 (missing tab_pin, in-flight WS8.E). Not my file; excluded from the job by building only the three gate bins.
- git apply --check --cached on the RUNBOOKS hunk: ok. qcommit secret scan: no leaks found on each commit.
- YAML: not parsed by a tool (no YAML module locally); checked by reading.

Not done (boundary, see depth/W3-B1-01.md):
- No CI run on GitHub; first run on a pushed commit fills the SCOREBOARD row.
- Test suites not run (per wave rule); the three build_score tests are compile-checked only.
- browser_benchmark.rs tab_pin fix is WS8.E's to land with its field (one line).
- Gate thresholds (D11) remain placeholder, ratify.

Traps found: lib-edit.js rewrites the whole file and normalises EOL, which churned 43 lines of another unit's uncommitted block in RUNBOOKS.md; restored from the pre-edit backup and appended without normalising. HEAD's RUNBOOKS.md is LF; the working tree is CRLF.

Uncommitted files of mine: none. Scratch: C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/W3-B1-01/
