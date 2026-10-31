# U2-live-ci hand-over
Commits: 992e3b95 (live_gate: live tests FAIL not skip when CHROMIUMOXIDE_TEST=1 and no Chromium; 6 pure-fn tests), bf2e7882 (CI job required, pinned Chrome for Testing 155.0.8059.84 via setup-chrome@v2, three test sets, RUNBOOKS paragraph).
Evidence: 3 green loops + 2 negative runs (CHROME=/nonexistent; chromium off PATH) in quarry-rust-env:1; actionlint clean. Not run on GitHub Actions.
Traps:
- chromiumoxide ignores a nonexistent CHROME and falls back to PATH; live_gate is stricter on purpose.
- zdr_behavior needs no Chromium and already ran in the workspace job; browser_egress_boundary is the one that was compiled out (#![cfg(feature="chromiumoxide")]).
- apt `chromium` on ubuntu runners: claim from knowledge (snap), not verified.
- Open: add "Live browser tests (Chromium)" to branch protection; workflow path filter means a required check stays pending on PRs outside Quarry-v2.
- Open: setup-chrome@v2 inputs checked from its action.yml via WebFetch, not run.
- Left in place: browser-broker-ci.yml has another agent's uncommitted diff (not mine).
