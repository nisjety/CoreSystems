# D-D-B1-agent-policy: findings not fixed (2026-10-27)

What was fixed is in the commits (git log, `fix(quarry-edge): ...`). Open items:

1. Budget subject vs human step exemption. WS8.B's `counts_against_step_budget(initiator)` exempted every
   `Human` initiator; plan R-8 says the budget subject is the run class. Fixed for agent-class runs (a person's step
   on an agent-class run now counts; a person's step on an interactive run is still exempt). Not fixed and a
   product question for the owner: a person who takes over an agent-class run now spends the agent's step budget
   while driving. R-8 reads that way; if the owner wants takeover steps exempt, `counts_against_step_budget`
   needs the control mode as a third input.
2. The delegate lane (`attach.rs delegate_step`) has its own budget accounting; not read in this unit.
3. `drain_runs` (older) and `finish_run(Drain)` now both treat a held lane at the deadline as "in use".
   Before, a step held the run entry only until WS8.H; since WS8.H a running step holds only its lane, so the
   old entry-lock timeout never fired and the drain closed a run an action still ran on (the browser session was
   released under the call). A delegate lane held by an attached agent is also waited for. After a drain
   deadline the busy run stays registered (`finish_run`) or is out of the registry (`drain_runs`, which empties it
   first): the browser dies with the process in both cases.
4. D2-014 is fixed in `budget::legacy_step_cap_applies` (shadow no longer lifts any run's request cap). In practice
   only an agent-initiated step on a person-class run was affected, since a person's own steps never reached the
   legacy cap. Flag defaults unchanged (BUDGET_ENFORCEMENT stays `shadow`). X-12 (moving to `on`) stays open.
5. `tests/web_policy_agent.rs`: upload, download, resume and grant-equality tests (D2-271) still not written
   (the harness driver lacks the capabilities); unchanged here.
6. `drain_closes_streams_so_graceful_shutdown_completes` was flaky (2 of 4 runs): the event stream's select between
   the run channel and streams_closing was random, so `done` could precede `agent.expired`. Fixed (biased select in
   live.rs, 6 of 6 green). ws.rs has the same select shape without a buffered terminal event; left.
7. Tests fixed rather than code (stale expectations): policy_baseline require-claim lookup, domain_denied reason,
   web_policy_agent interact tests driven by a service principal (browser.act.interact is autonomous-only).
