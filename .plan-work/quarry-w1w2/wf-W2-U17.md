# W2-U17 hand-over (WS12.S1, field-facts probe and AX/target masking)

## What was done

The native page projection masks editable values (see `W2-U17-progress.md` for the file list and commits `1f829687c`, `ea229444a`). The probe is on by default and can be turned off with `QUARRY_BROWSER_FIELD_PROBE=off`, which masks every editable value.

## Traps found while building

- `chromiumoxide.rs` and `RUNBOOKS.md` both carry other units' uncommitted hunks. Commit with `qcommit-patch.sh` and a hunk-only patch (selection by marker: `WS12.S1`, `field_probe`, `field_kind: None`, `redaction: None`). `qcommit.sh` would have swept the other hunks in.
- The shared working tree moved during the build: `SaveMeta` (kernel.rs, chromiumoxide.rs test) and `ParseCacheKey` (quarry-transform) broke the compile for a while because other units were mid-edit. Re-run the check after a few minutes; do not edit their files.
- Backgrounding a `qtest.sh` command with a trailing `&` inside a bash string kills the job when the tool call returns, and the log stays empty. Use `run_in_background` on the bare command with `> log 2>&1`, then `qwait.sh`.
- `qwait.sh` returns after 540 s with TIMEOUT when the container is still queued behind other agents; rerun it.
- The `BrowserNativeProjection` and `BrowserNativeTarget` literals exist in `chromiumoxide.rs` (projection and targets), `trusted_input_live.rs` (one target literal) and nowhere else in the workspace. A new field breaks every literal, so grep before adding one.
- Fail-closed rule as built: an editable control with no facts gets `[redacted:secret]`, which also covers controls past the 64-node cap. A heavy form can therefore show many `[redacted:secret]` values; that is the intent, but measure it.

## Left for the next person

- Run Q10 (`scripts/q10/ax-password-probe.mjs`, Node 22+, pinned Chromium) before the S1 merge, then commit `crates/quarry-browser/tests/fixtures/ax-password-probe.json`. The answer may change the probe's first path (describe versus resolve).
- The test phase runs the 13 pure tests in `field_probe.rs` and `ax_field_probe_live`, and records `probe_ms`.
- Commit attribution: the workflow text asked for `Claude Sonnet 5.5`; the session attribution instruction said `Claude Haiku 5.5`. The commits carry Haiku 5.5 (the session instruction). Decide which the repository owner wants.
