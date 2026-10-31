# W3-B2-05 progress (E3.9.2: Tier B eval corpus in lab/evals)

Status: BUILT and committed. Nothing run against a stack; the tests are written and compile-checked only (build-first rule).

## Commits (local, not pushed)

- `59ea6025b` feat(quarry-evals): Tier B eval corpus, loader and scoring hooks (E3.9.2). Adds `lab/evals/tier_b/corpus.json`, `lab/evals/tier_b/site/{article,injection,external}.html` and `site/private/secret.html`, `lab/evals/src/tier_b.rs` (the `quarry-tier-b` binary: `validate` default, `score --observations <file> [--out <file>]`), and the 4-line `[[bin]]` in `lab/evals/Cargo.toml`.
- `dd4eb3dbd` docs(quarry-v2): RUNBOOKS section for the Tier B corpus (E3.9.2). Patch-only commit (`qcommit-patch.sh`), 11 lines. The working copy's other RUNBOOKS hunks stay uncommitted and untouched.

## Verification run

- `cargo check -p quarry-evals --tests --keep-going` via qtest (after the first run without `--keep-going` stopped at the first error): the only error is `lab/evals/src/browser_benchmark.rs:221`, `BrowserSession` literal missing `tab_pin`. That is another unit's uncommitted `quarry-browser` change in the shared tree; not touched. No errors or warnings reported for `quarry-tier-b`.
- Gitleaks pre-commit hook: no leaks found on both commits.
- Not run: `cargo test -p quarry-evals --bin quarry-tier-b` (per the build-first rule).

## Not done (and why)

- Tier B loop that writes observations: needs WS11 S0 `ScriptedPlanner` / `FakeQuarry` (sibling) and a stub grant validator (WS1 or a dev Quarry configuration).
- CI wiring into `.github/workflows/quarry-v2-eval.yml`: no producer, so no run.
- Threshold ratification: `thresholds` stays `placeholder`, `ratified: false`; the score file reports `gate.status: not_evaluated`.
- Scheduled Tier C run and the 12-case default-on corpus (plan E3.9 P4).

## Uncommitted files of this unit

None. The `.plan-work` notes (this file, `depth/W3-B2-05.md`, `wf-W3-B2-05.md`) are untracked, as the other W2/W3 notes are.
