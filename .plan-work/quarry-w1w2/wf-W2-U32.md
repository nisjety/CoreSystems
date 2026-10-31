# Hand-over: W2-U32 (WS6.H job admission, wave 2 batch 6)

## What was done

- The W1 commit 40cb38df4 had built most of WS6.H (migration 019, claim endpoint, bounded backlog, TTL, dispatcher claim, edge and gateway 429). U32 closes the row-locking gap: `ClaimAccepted` takes a transaction-scoped advisory lock first, so two dispatchers cannot lease past a cap together (commit cc75bdf33).
- Team concurrency counts leased accepted jobs (same commit).
- Edge ZDR crawl refusal test (0018ac497). RUNBOOKS and openapi 429 (3b8e841f2, 054d4b8c7), both committed hunk-only because each file carries other units' uncommitted hunks. Those hunks remain uncommitted in the working tree.

## What is left

- WS6.D-s plan-dependent cap stamp in `handoff.rs` (not built; needs the plan values).
- Tests run and the deployment evidence (test phase and stack, not this wave's build).
- See `depth/W2-U32.md` for the list.

## Traps found

- The shared tree's `quarry-edge` bin does not compile while U31 has an uncommitted `resource_routes.rs` import of `crate::job_stream`. Use `cargo check -p quarry-edge --lib --test job_admission` to compile-check a test target that does not depend on the bin.
- `docs/RUNBOOKS.md` and `docs/openapi.yaml` in the working tree carry other units' hunks. A plain `qcommit.sh` would commit them. Use a patch built against `git show HEAD:<path>` and `qcommit-patch.sh`. Edit the working-tree copy too, or the next whole-file commit by another unit deletes the section.
- The working-tree RUNBOOKS.md once showed a whole-file diff on CRLF. Use `git diff --ignore-cr-at-eol` to see real changes.
- The background form of qtest with `&` and a shell redirect produced an empty log. Run it in the foreground with a timeout, or use the Bash tool's `run_in_background` parameter.
