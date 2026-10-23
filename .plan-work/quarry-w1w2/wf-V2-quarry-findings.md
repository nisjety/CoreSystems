# V2-quarry-findings hand-over (round 6, 2026-10-20)

Commits (branch quarryv2-next, local only): aa54623a (6), 721fd400 (8), 17360e6a (3 py), 2daa3f0c (3 CI), c9d61e31 (4),
a1cb37c4 (5c), d49dfd48 (1), 1932afdf + 55a3831a (2; the second corrects the CRLF blobs of the first), cba698aa (5a, 5b, 7),
36977011 (RUNBOOKS/PROGRESS).

Per finding
1. CONTRACTS fixed (400 in select/close query, 422 in a body, agentLabel/agent.label); new 422 body assertion in agent_run_control.rs.
   RUNBOOKS 'ignores the agent headers at HEAD' text was already gone (line 385 says Quarry reads them since dec93d1e): no change.
2. tabs.rs already pruned closed tabs first; added TabOrigin.unattributed (class-only entry past 256 full records), openapi
   TabOpener.unattributed, TabOpener.md regenerated, CONTRACTS, unit test (258 entries) + HTTP test on a ZDR run.
3. Python test uses AgentAction.from_dict: 28 passed vs a regeneration of the working-tree spec. CI: verevonv3-ci.yml split the
   generate step and added a `git status --porcelain -- sdks` check (zero extra generation). actionlint rc 0.
4. postgres_test_support.rs, one schema per test, REQUIRE_CONTRACT honoured by all five stores; CI --test-threads=1 dropped.
   New tree 5/5 runs green (23 tests); old tree 5/5 runs red (5-6 failures each).
5. a/b: mutation runs on a copy (V2/mutation-copy/mut.sh): M1 ssrf red, M2 interception red incl. the 6cffdc2e assertion, M3 grant red.
   c: bootstrap test fixed; head test + without CREATE ROLE still passes (flaw), new test without it fails.
6. gateway absent-retry-key test + mutation red/green.
7. 10-space run and 'run everywhere' header fixed.  8. PROGRESS.md stale text removed.

Full runs on the working tree before the commits: quarry-edge --no-fail-fast 972 passed / 0 failed / 27 binaries (log lint included);
quarry-runtime 757 passed / 0 failed / 1 ignored; cargo check --workspace --all-targets rc 0; node --test scripts/tests 23 pass.

TRAPS
- docker volume quarry-target is ~228 GB. Never copy/seed from it. A seed attempt was killed and its partial volume
  quarry-mut-target removed; the mutation run used a cold build in its own volume (4m43s), removed afterwards.
- qcommit-blobs.sh needs LF content; working-tree copies of tabs.rs/types.rs/agent_run_control.rs are CRLF (grep -c $'\r' lies in Git Bash).
  1932afdf committed CRLF by mistake, 55a3831a restores LF; read them together.
- Docker lock contention: other agents' builds held the lock for 10-30 minutes at a time.
- postgres tests leave one qt_<secs>_<uuid> schema per test; schemas older than 6 h are dropped by the next call.

## Round 4 addendum
- dbc27a55: the SDK drift CI step could never fail (job default working-directory + repo-root pathspec); fixed and proved by negative controls. Python SDK tests now run in CI (spa-verify). Neither step has run on a real GitHub runner.
- Trap: in a job with defaults.run.working-directory, git pathspecs in run steps are cwd-relative; a bad one only warns and exits 0.
