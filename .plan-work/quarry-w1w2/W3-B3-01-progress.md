# W3-B3-01 progress (E3.8.6 reputation hot reload): built and committed, NOT run

Committed on quarryv2-next (local only, no push):
- 949b40e79 feat(quarry-security): reputation snapshot hot reload and dot-suffix matching (E3.8.6). preflight.rs: DefaultEngine derives Clone; reload_snapshot (both lists swapped under two write guards, failure keeps previous lists); list_counts; with_subdomain_matching (off by default); spawn_snapshot_watch (mtime poll, clamped to >= 1 s, first check after one interval). tests/reputation_reload.rs: 5 tests written.
- c8f610f94 feat(quarry-edge): wire the reputation watch behind env flags, dark (E3.8.6). main.rs: one block after the security snapshot load; env QUARRY_EDGE__SECURITY_SNAPSHOT_WATCH_S (default off) and QUARRY_EDGE__SECURITY_SNAPSHOT_SUBDOMAINS (default off); entries gauge quarry_security_reputation_entries. docker-compose.yml: three env lines on quarry-edge, default 0.
- 4273ad148 docs(quarry-v2): RUNBOOKS section for the reputation hot reload (E3.8.6). Append-only; hunk-only patch against HEAD.
- cfaeb12b9 style(quarry-security): rustfmt of tests/reputation_reload.rs.

Verification (compile only, nothing run):
- qtest: cargo check --tests -p quarry-security: Finished, rc 0.
- qtest: cargo check -p quarry-edge --all-targets: Finished, rc 0 (warnings only in other files).
- qtest: rustfmt --check on preflight.rs and tests/reputation_reload.rs: clean after cfaeb12b9.

Not done in this unit (see depth/W3-B3-01.md):
- Tests not run (the 5 in tests/reputation_reload.rs, and the existing security tests).
- Per-org off|warn|block modes, HostReputation hook in BrowserEgressPolicy, decision_source in receipts (browser file is a W2 writer's).
- Allowlist suffix matching (decision not approved).
- The feed sync as a compose service.
- The estimate row names quarry-runtime preflight.rs; the file is in quarry-security.

Uncommitted, not ours: the RUNBOOKS.md working copy has other units' hunks.
