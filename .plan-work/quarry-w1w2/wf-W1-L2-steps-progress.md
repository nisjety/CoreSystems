# W1-L2-steps hand-over
Done: L2-signed-in-steps.md (legs A, B, C, restart-survival R, host-side scan, results table). No code changed, nothing committed, nothing run against a stack; all expectations are marked [CODE] or [PLAN] and cite file:line. The 21 JS snippets were syntax-checked only (async-function constructor).
Traps found (also in depth/W1-L2-steps.md): the spec's B/C use the wrong session-id and ai-run field shapes; the page cannot reach A1-A3, A5a-c, A6; restart check needs TTL >= 300 s so it does not fit the 45 s gate stack; grantlog.go is another unit's uncommitted file.
