# D-D-B3-egress-caps hand-over

Five items, five local commits on quarryv2-next (518584b41, 13654ea6e, 893cbfe9d, e7d290318, 989925c43). Nothing run on a stack or Chromium.

- D2-017: handoff.rs `admission_stamp` / `with_admission`; crawl and batch params stamped from the org's limits (entitlement only; zero is not stampable). Inert until a plan source exists (WS6.D). Admission stays dark.
- D2-013: tab_control.rs `TabEgressPolicy` = session AND tab; a session with no policy is deny-all. The attach tab is tightened by this (live-unverified).
- D2-020: agent_policy_gate.rs `egress_for_narrowing`; empty Narrowed set = deny-all.
- D2-018: sweep result and manifest in tests/web_policy_routes.rs; one gap fixed (change/refresh). Open ones: /v1/audio, /v1/answer, create-time sources and schedules, internal queue enqueue.
- D2-033: legacy extract event keyed by request id. The same pattern remains in search_routes and map_routes.

Decisions needed: explicit-zero cap meaning (D2-017); whether audio_url is a web fetch under the policy; whether sources and schedules should also be refused at create.
Traps: a long Rust test in a bash heredoc fails (use Write); a "\n" written through a JS template in a heredoc became a real newline in the Rust source; routes.rs carried another unit's hunks, so the D2-017 routes hunks were committed with qcommit-patch.sh; the web_policy_routes sweep needs QUARRY_EDGE__RUNTIME_AUTH_TOKEN and an internal signer for run_page.
Findings: depth/D-D-B3-egress-caps.md.
