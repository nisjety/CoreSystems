# W2-U21 hand-over (WS6.B remainder, agent-run budgets)

What was done: see W2-U21-progress.md. In short: the agent.budget_exhausted event (announced once per segment, checkpointed), meta.budget on REST step responses, receipt cost_source (migration 0012), failure receipts carry the flat cost, DELETE returns usage, gate 9 no longer applies the request's step cap to a person's session when enforcement is on or shadow.

Traps found (for whoever continues):

- The working tree is shared. Other units' hunks sat in files this unit touched: quarry-core/tests/contracts.rs (WS7.F), docs/RUNBOOKS.md (a CDP-spike section replaced by a request-id section, plus a new section by another unit), quarry-runtime/src/browser_driver.rs, quarry-edge main.rs, routes.rs, state.rs, profile_routes.rs, config.rs, lib.rs. Commits were made hunk-only with the hunk filter (scratchpad/W2-U21/hunkfilter.js, which keeps the hunks matching a regex) and qcommit-patch.sh. Check `git diff -U0 <file> | grep ^@@` before a whole-file qcommit.
- A cargo check on quarry-edge failed once on another unit's profile_routes.rs (ProfileMetadata missing `owner`). It passed later without any change from this unit. Re-run rather than debug someone else's file.
- The Edit tool on a CRLF file can leave LF lines behind. After any Edit on a CRLF file, normalise with the lib-edit.js approach (the node helper keeps EOL) or check `file <path>`. budget.rs was normalised once for this reason.
- The workflow text asked for "Co-Authored-By: Claude Sonnet 5.5" and said to use it whatever the model. The session's attribution instruction said Claude Haiku 5.5. The commits carry Haiku 5.5 (the session's instruction); change the trailer if the owner wants the workflow's line.
- Shadow default: in shadow, an agent run keeps the legacy 429 from its request cap (intended, so shadow changes nothing for agents). A person's session is no longer capped by its own request step count in shadow or on. Owner decision needed before `on` (depth note, severity high).
- The integration test for the budget (tests/agent_run_budget.rs, named in the WS6.B detail) is not written. It needs the test_support insert_live_run harness.

Next steps for the next agent:
1. Test phase: run `cargo test -p quarry-edge budget` and `cargo test -p quarry-runtime step_receipts` through qtest.sh, then `cargo test -p quarry-core envelope` and `cargo test -p quarry-core --test contracts`. Expect all the new unit tests listed in W2-U21-progress.md to pass; the Postgres migration test only with DATABASE_URL.
2. Sibling (b): map 402 BUDGET_EXHAUSTED and the event in execution-core and model-gateway (depth notes).
3. WS10: add the trace projection row for agent_budget_exhausted if the run trace should show it.
4. Owner: decide the human step-cap change before ENFORCEMENT=on (depth note, high).
