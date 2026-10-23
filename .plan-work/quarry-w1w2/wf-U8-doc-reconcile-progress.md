# U8-doc-reconcile progress (second pass, 2026-10-20 after Docker came back)

Earlier pass (70f2c932, ed7f722e, 674e9e26, 31441f12, 2b7ac61b) described most of round 4 as uncommitted. All of it is committed now (HEAD c7429f2e). This pass corrects the docs to that.

- DONE 64356ca4 RUNBOOKS + ERROR_REASONS + PROGRESS (numbers measured at HEAD c7429f2e: workspace 2305/0/7, live lane 138/4 + ssrf 4/1 + egress 5 + zdr_behavior 4, pg store 23, node 23/19/65, zdr-sync 50, bootstrap 33, gateway 751).
- DONE: 64356ca4, fe07ec5c, acbd04b7, bcd78023. Hand-over: wf-U8-doc-reconcile.md.
Scratch: scratchpad/U8-doc-reconcile/ (edit-*.js, lane.sh, pg-run.sh, mut-bootstrap.sh, logs).
Traps: gateway export needs nginx.conf + two compose files of verevonv3 AND the Quarry zdr-sensitive-fields golden, else 3 compile errors / 1 failing test; with-lock + nohup in run_in_background returns at once, wait with qwait on the log.

- DONE ddbd4b2c (review round): range counts fixed to 56 for a60722f9..c7429f2e (55 Sonnet, 1 Haiku); erratum for 64356ca4 SPA-copy claim (23 of 33 gateway codes without SPA copy) in plan + register.
