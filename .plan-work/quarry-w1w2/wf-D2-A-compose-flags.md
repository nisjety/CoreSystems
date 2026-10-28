# wf-D2-A-compose-flags: hand-over (2026-10-28)

Unit D2-A-compose-flags, list A of the 2026-10-28 owner order: decision 3.12 (DATAPLANE-OPTION-B: flags-off compose, MODEL_PLANE_URL opt-in, QB-T17 kept pending) and decision 3.8 (ADR-6: the scrape-actions line in the compose). Local commits only; nothing pushed; no live stack touched.

## Done

- Caller audit for /v1/answer (what a degrade would have looked like, and who would notice a 501): depth/D2-A-compose-flags.md item 1.
- The empty-URL trap fixed at load (config.rs), with four tests: 8a8bf5c1b.
- Both Quarry compose files flags-off, the URL opt-in, the scrape line: 675903067.
- QB-T17 todo reason corrected; assertions unchanged: 88fb4afe3.
- Compose-default statements in the docs: c457cdc51.

## Verified (details in D2-A-compose-flags-progress.md)

- quarry-edge lib tests: 631 passed, 0 failed. cargo check --workspace --all-targets: rc=0.
- Quarry node scripts/tests: 44 tests, 32 pass, 0 fail, 12 todo (unchanged).
- Root node scripts/tests: 143 pass, 0 fail.
- docker compose config -q: standalone rc=0; Ingestion base and production overlay rc=0 (from a scratch copy; see trap 2).

## Traps for the next person

1. An empty MODEL_PLANE_URL reaches the edge as an empty value. The boot gate in main.rs tests is_some(). config.rs now normalises it; do not remove that line without also changing the gate.
2. The worktree has no gitignored env files for the Ingestion compose (Quarry-v2/.env, imports-core/.env, integration-corev2/.env, shipping-core/.env, finspo-core/.env, autocomplete-core/.env). `docker compose config` fails here with "env file not found". Validate from a scratch copy with empty stand-ins; never create those files in the worktree.
3. Other agents' strays are in the tree: main.rs, lib.rs, fixture_site.rs, step_receipts.rs (EOL and mod-order only), robots.rs and the robots unit's progress file. Commit only the paths you own (qcommit.sh does).
4. The gateway's SSE proxy relays a 501 from Quarry as "Model gateway returned 501" (model_error). The SPA ignores the body, so nothing breaks, but the label is wrong.
5. config.rs is CRLF in the working tree while git stores LF. The lib-edit helper keeps the EOL; a grep for CR with a shell quoting trick reported 0 and misled me for a minute. Use node to count.

## Left for the owner or the stack session

- Four Model Plane features (answer synthesis, AI formats, search suggest, audio) have no switch and run whenever MODEL_PLANE_URL is set. QB-T17 stays pending.
- SPA citations from the answer stream are no longer sent when the Model Plane is absent. Feature decision.
- Root docker-compose.yml legacy quarry-edge block uses names the binary never reads.
- Plan text for X-20 (lines 386, 514) still describes the old gates; not done here (outside the spec).
- Stack session: recreate quarry-edge with the new defaults and confirm no Model Plane mints (decision 3.12 step 4). Option B is not landed, by decision.

## Trailer

The unit brief asked for `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` on every commit. The harness attribution rule for commits I create names Haiku 5.5, which is the model I am, so the commits carry `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`. Change it with the owner if the Opus line is wanted; the commits are local, so a rewrite is still possible.
