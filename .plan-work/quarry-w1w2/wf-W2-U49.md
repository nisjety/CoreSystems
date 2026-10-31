# W2-U49 hand-over (WS12.S3)

Receipts now mask on write and on read. See `W2-U49-progress.md` for the commits and `depth/W2-U49.md` for what is deferred. Short version:

- One function, `quarry_core::redaction::action::redacted_for_record`, with a human branch (WS8.B metadata) and a typed branch (control kind + `RecordPolicy`). The builder applies it in `complete`, `fail` and `skip`.
- `StepReceipt.redaction_rule_version` (migration 0013); `redact_for_read` masks unstamped rows at read; stored rows are never rewritten.
- `QUARRY_EDGE__RECEIPT_TEXT_MODE=sensitive|all` (default `sensitive`; X-19 still needs the product owner).
- Traps: RUNBOOKS.md working copy is behind HEAD (commit against the HEAD blob); `step.rs` and `step_receipts.rs` carry other units' uncommitted hunks; heredocs over a few lines fail in this Bash tool (use Write); rustfmt was not run on any file (do not run it on `tests/redaction_wire.rs`, it follows `mod support` into other units' files).
