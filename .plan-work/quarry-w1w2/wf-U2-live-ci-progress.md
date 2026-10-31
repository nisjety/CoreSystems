# U2-live-ci progress
- Findings: apt `chromium` does not exist on ubuntu-latest (24.04: snap shim) -> the job was likely never clean. chromiumoxide ignores a nonexistent CHROME and falls back to PATH (so CHROME=/nonexistent alone would not fail). zdr_behavior needs NO chromium and already runs in `cargo test --workspace`; only browser_egress_boundary is feature-gated (chromiumoxide) and never ran in CI.
- Plan: setup-chrome@v2 pinned 155.0.8059.84 (CfT stable; image has 155.0.8059.79).
- Step 1 (uncommitted, mine): crates/quarry-browser/src/live_gate.rs (new), lib.rs (+mod), chromiumoxide.rs (live_enabled + 6 inline guards -> gate), quarry-runtime/tests/browser_egress_boundary.rs (gate).
- Next: qtest unit tests of live_gate; commit; then workflow yml; 3x loop + CHROME=/nonexistent run; YAML validate; RUNBOOKS paragraph.

## Round 4 (review fixes) - NOT COMMITTED: Docker Desktop down (qtest "DOCKER IS NOT ANSWERING"; docker info hangs; pre-commit gitleaks hook runs via docker and reports "found a secret" when the engine returns 500 - that is the hook failing, not a secret)
All edits are in the working tree, rustfmt-clean, verified as in the messages:
- crates/quarry-browser/src/live_gate.rs (enforce() + 3 tests; mutation verified natively), crates/quarry-browser/tests/ssrf.rs (4 tests un-ignored + gated) -> msg file wf-U2-live-ci-round4-msg1.txt. These two paths are STAGED in the index (qcommit's git add ran); the commit was blocked by the hook.
- crates/quarry-runtime/tests/browser_egress_boundary.rs (restructured, NOT compiled), .github/workflows/quarry-v2-ci.yml (comments only), docs/RUNBOOKS.md (appended paragraph) -> msg file wf-U2-live-ci-round4-msg2.txt.
To finish when Docker is back: 
  1) qtest: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-runtime --features chromiumoxide --test browser_egress_boundary -- --test-threads=1   (expect 6 tests; the first compile of this file happens here)
  2) qcommit.sh msg1 live_gate.rs ssrf.rs ; qcommit.sh msg2 browser_egress_boundary.rs quarry-v2-ci.yml RUNBOOKS.md   (edit msg2's "Not done" if step 1 passed)
  3) optional mutation: comment out the resolve_public_url SSRF check in quarry-runtime browser_egress_proxy.rs and see the *_ssrf_* tests fail.
Host trick that worked: native cargo for quarry-browser only, CARGO_TARGET_DIR=C:/Users/IMAFER~1/AppData/Local/Temp/u2t (a long target path breaks blake3's MASM build); host Chrome at C:/Program Files/Google/Chrome/Application/chrome.exe runs the live quarry-browser tests. quarry-runtime cannot build on the host (no libclang).

## Round 4, second agent (Docker still down: qtest DOCKER IS NOT ANSWERING, docker version = 500, pre-commit hook fails with "gitleaks found a secret" = hook cannot reach docker)
- Still UNCOMMITTED (same two commits as above, msgs updated): msg1 = live_gate.rs + ssrf.rs (staged); msg2 = browser_egress_boundary.rs + quarry-v2-ci.yml + docs/RUNBOOKS.md.
- Changes this agent: deadline of the page-request receipt wait 20 s -> 30 s (CI slack); msg files corrected; finding 2 re-run with MSYS_NO_PATHCONV=1 CHROME=/nonexistent: 4 ssrf tests fail with the message naming "/nonexistent" exactly.
- Review of browser_egress_boundary.rs against the real APIs (read, not compiled): every method/field/import exists (evaluate, egress_receipts, content, receipts_after, endpoint_for_session, configure_policy, as_str, BrowserEgressReceipt{sequence,url,decision,policy}, redact_egress_url keeps host:port, policy labels domain_grant_blocked / url_or_dns_blocked / dns_or_ssrf_blocked match the source). Still never compiled: quarry-runtime has no libclang on the host.
- RUNBOOKS.md working copy also carries ANOTHER unit's uncommitted appended section (WS6.E "A grant carries a budget claim"). Committing the file as a whole commits it too; to commit only the U2 paragraph build a patch and use git apply --cached, or accept and say so in the message.
- Non-comment lines of quarry-v2-ci.yml are identical to HEAD (diff of comment-stripped text), so the structure actionlint accepted is unchanged. pyyaml/js-yaml not installed; did not download one.
