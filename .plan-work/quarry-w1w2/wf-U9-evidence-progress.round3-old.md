# U9-evidence progress
- 2026-10-20 start. HEAD 31441f12. Finding: U3 tab-initiator Quarry code and U5 WS6.E are UNCOMMITTED in the tree (docker was down), so HEAD's NewTabBody still lacks the initiated_by alias.
- Plan: HEAD tree = git archive in scratchpad/U9/head-tree (touched). Image qv2v/quarry-edge:u9head. Then optionally working-tree image :u9wt for check 1. Live lane via qtest QROOT=head-tree.
- HEAD image u9head built (rc 0); check1 on HEAD saved out/check1-head.txt: snake_case body initiated_by IGNORED on HEAD (200 during takeover, recorded human), no 422 for both spellings, no openedBy/agent -> U3 not in HEAD. Started wt build (u9wt) log build-wt.log
- wt image u9wt built rc 0 (working tree compiles). live lane running via live-lane.sh -> U9/live-lane.log (HEAD tree). Next: replace edge with u9wt, run check1.sh again (out/check1-wt.txt)
- DONE. committed 2b7ac61b (RUNBOOKS section only, private index). Notes wf-U9-evidence.md. Cleanup verified (no qv2e/u9 containers, network, volume, images).
