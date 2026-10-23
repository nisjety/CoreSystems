# wf-W2-U36 hand-over (WS17.E1)

Read first: .plan-work/quarry-w1w2/W2-U36-progress.md (what is committed, tests, not done) and depth/W2-U36.md.

Traps found while building:
- HEAD moves under a running unit (other commits landed during the build). Rebuild any blob-based commit source from the current HEAD right before committing, or the blob reverts a newer commit.
- lib.rs and main.rs carry other units' uncommitted hunks: commit them through the blob helper with HEAD plus only this unit's edits.
- The RUNBOOKS working copy has a large line-ending change from another unit. Append through HEAD, not the working copy.
- `SessionSnapshot` has no PartialEq; tests compare the JSON form.
- `ProfileKind` ids need a real ULID (use `ProfileKind::new()` in tests); `"prof_x"` strings do not parse.
- The source-scan test needles are built with concat! so the test file does not match itself.
- Cargo check results can look empty when the docker run was started with `&` inside a background job: run qtest in the foreground or wait for its QTEST_DONE marker.

Next steps for whoever continues: run the test phase (cargo test for quarry-runtime profile, the Postgres and Redis lanes, cargo test -p quarry-edge profile_encryption), write the pinned known-answer vector, add the Postgres-lane tests listed in the progress file, then the deployment evidence in RUNBOOKS WS17.E1.
