# D2-B-x19-receipts progress (list B, X-19; written 2026-10-28)

Unit spec: decision 3.6 X-19 of DECISIONS-ROUND-2026-10-27.md. Prepare only: receipts stay off for real users; no flag was added or touched, no script was run against a stack.

## State: complete for the unit's spec, except what is listed as not done

Commits (local, branch quarryv2-next):
- `b40ee90b5` fix(quarry): key-by-key password masking (D2-065) and the event follows the receipt text flag (D2-210). Code and tests.
- docs commit: SECRET_REDACTION.md and RUNBOOKS.md (append only).
- notes commit: this file, the depth file `depth/D2-B-x19-receipts.md`, the design note `D2-x19-retention-design.md`.

What is built:
1. Default `sensitive` already was the default (`RecordPolicy::default`, `redaction_boot::decide_record_policy(None)`); nothing to change. Tests that pin it: `redaction_boot` tests and `redaction_wire` (`decide_record_policy(None) == SensitiveOnly`).
2. Event follows the flag: `action_for_event(action, typed, policy, mode, tally)`; the runner passes `quarry_core::redaction::action::record_policy()`.
3. D2-065: `press_produces_text`, `redacted_for_record` Press branch, `ObservationContext.key_focus`, `receipt_field_kind`, `key_focus_after`; the three edge call sites; read-time mask of older rows' key presses.
4. Retention design: `D2-x19-retention-design.md` (options A to E, recommendation C in two steps, Control Plane triggers and Quarry executes, open questions for the owner).

Uncommitted files that are not mine: the `policy_mode` field-order hunk in `quarry-runtime/src/step_receipts.rs` (legacy_checkpoint_without_budget_fields_deserialises) was left in the tree on purpose; I committed only my hunks of that file with qcommit-patch.sh. The other modified files in the tree belong to other units.

## Test commands and results (qtest, Linux container, quarry-rust-env)

- `cargo test -p quarry-core redaction`: lib 76 passed; redaction_golden 15 passed, 1 ignored (release-only perf).
- `cargo test -p quarry-runtime --lib -- observation step_receipts browser_procedure receipt`: 117 passed, 0 failed (run after the focus-tracker test was added; an earlier run of 116 passed before it).
- `cargo test -p quarry-edge --lib`: 634 passed.
- `cargo test -p quarry-edge --features browser-agent --test redaction_wire --test zdr_feature_matrix --test log_field_discipline`: 8, 13 and 22 passed.
- `cargo check --workspace --all-targets` and `cargo check -p quarry-edge --features browser-agent --all-targets`: finished, no errors (existing warnings only).

## Traps found

- The Bash heredoc limit bit twice (a JS script with apostrophes inside a heredoc failed to parse). Write long scripts with the Write tool.
- `git diff` with `-c core.autocrlf=false` shows whole-file CRLF diffs for some files (HEAD blobs are CRLF); build patches for qcommit-patch.sh with the default git config.
- qtest mounts only the Quarry workspace, so a multi-line shell script has to live inside it; I used a temporary `.qrun-x19.sh` and deleted it.
- `quarry_core::redaction::record_policy` does not exist; the function is `quarry_core::redaction::action::record_policy`.

## Not done

See `depth/D2-B-x19-receipts.md` and the design note. In short: retention or erasure (design only), the stack evidence, the `RULE_VERSION` bump, the real-focus probe, the dropped-observation counter, in-place fixes of stale RUNBOOKS lines.
