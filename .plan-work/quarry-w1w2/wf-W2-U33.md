# W2-U33 hand-over (WS7.E, remaining routes)

Built and committed: the route gates and the paid-provider downgrade (search, search/similar, extract, map, crawl, batch, profile create), helpers in web_policy_edge.rs, the openapi property, the RUNBOOKS section, tests/web_policy_routes.rs. Details: W2-U33-progress.md and depth/W2-U33.md.

Left: route-level tests (orchestrator double, router-level downgrade, 403 bodies, counter), the optional ZDR consolidation PR, the WS7.G answer filter, save and update profile gates, deployment evidence.

Traps found:
- A HEAD copy taken for a commit goes stale when another unit commits first: two fix-up commits (0f3dde7f, 629c1dd90) had to put back other units' lines. Take the copy and commit in one command, then check `git diff --stat <base> HEAD` for deletions.
- tests/policy_baseline.rs (WS7.C) does not compile after the WS7.J decide signature change (c5cb15995); its owner must update the 7 call sites to pass `&claims`.
- Background qtest with a bare `&` writes an empty log; use run_in_background with a redirect.
- A bare `python -` in the Bash tool read stdin and hung a command; do not use it.
