# W3-B1-05 progress (E3.5-2 quarry-control egress guard)

Build-first rule: built, tests written, compile-checked (go vet) only. Nothing run.

Committed (branch quarryv2-next, local, not pushed):
- 9af43e7cf feat(quarry-control): webhook and catalog egress guard, copied from integration-core (E3.5-2). Files: internal/egress/{guard.go, client.go, guard_test.go, client_test.go, conformance_test.go} (new), internal/dispatcher/dispatcher.go (import plus the nil fallback line), internal/catalogwatch/catalogwatch.go (import plus the nil fallback line).
- 578c327bf docs(quarry-v2): runbook section for the webhook egress guard (E3.5-2). Built from a HEAD blob read before cc05191fd landed, so it dropped the W3-B1-01 eval section. Repaired by 6e6ea62c3.
- 7c250ea01 docs(quarry-v2): restore eval CI runbook section; add webhook egress guard section (E3.5-2). Its message is wrong: it did not restore the eval section, and it carried a duplicate egress heading. Later commit 61c4e8d6e (WS8.E unit) left a single egress heading. Do not rely on this commit message.
- 6e6ea62c3 docs(quarry-v2): restore the eval CI runbook section dropped by 578c327bf. Appends the cc05191fd eval section back. Net effect on RUNBOOKS.md from 578c327bf to HEAD: 0 removed lines.

Verified (commands run, results):
- Diff of the copied guard.go against integration-corev2's guard.go: only the package comment differs (header lines 2 to 13).
- gofmt -l on internal/egress/: clean. gofmt -l also lists dispatcher.go, catalogwatch.go and their untouched _test files, because they are CRLF (pre-existing).
- GOWORK=off GOPROXY=off go vet ./internal/egress/ ./internal/dispatcher/ ./internal/catalogwatch/: exit 0.
- GOWORK=off GOPROXY=off go vet ./... (quarry-control module): exit 0.
- git diff of the two anchor files: 5 insertions, 2 deletions, CRLF kept (lib-edit.js).
- qcommit.sh pre-commit gitleaks: no leaks found (both code and runbook commits).

Behaviour change, live not dark: cmd/control/main.go calls dispatcher.Run with a nil client, so the dispatcher fallback is the production webhook path. Webhook targets that resolve to loopback, private, link-local, CGNAT, reserved or metadata addresses now fail delivery. Redirects are not followed. The old client's proxy-from-environment is not kept.

Not done:
- go test ./internal/egress/ ./internal/dispatcher/ ./internal/catalogwatch/ not run (test phase at the end of the wave).
- Org scoping of webhook registrations, the fail-closed migration 021_webhook_org_scope and the write-only secret (E3.5-1). Not in this unit.
- Registration-time URL check (E3.5-1).
- Sync check between the two egress copies (decision for the owner, see depth file).
- Startup log of the effective EGRESS_ALLOWLIST_* values (depth).
- Working copy of docs/RUNBOOKS.md is untouched. It holds another unit's uncommitted reordering and does not have this unit's section. A whole-file commit from the working copy would drop committed sections. Owner must commit from HEAD.

Attribution: commit messages end with "Co-Authored-By: Claude Haiku 5.5" as the harness reminder asks. The unit text asked for a Sonnet 5.5 line; this was not followed. Flag for the owner.

Uncommitted, not mine: none of mine; the .plan-work files are untracked notes.

Depth bullets: .plan-work/quarry-w1w2/depth/W3-B1-05.md
Hand-over notes: .plan-work/quarry-w1w2/wf-W3-B1-05.md
