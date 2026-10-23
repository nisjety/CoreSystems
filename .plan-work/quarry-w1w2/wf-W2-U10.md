# Hand-over: W2-U10 (WS14.S1 hermetic fixture site and real-stack harness)

What was built (committed):
- ba965aa9b: `quarry-edge/tests/support/fixture_site.rs` (FixtureSiteDriver, standard_site), one `pub mod` line in `tests/support/mod.rs`, `tests/fixture_site_selftest.rs` (12 cases).
- b247455d5: `apps/Ingestion Plane/docker-compose.crawl-fixture.yml` overlay, `Quarry-v2/deploy/compose/crawl-fixture/{a,b}`, `Quarry-v2/scripts/e2e/crawl-scope-robots.sh`.
- 922747679: RUNBOOKS section (appended; committed through a private index, see traps).

Not built or not run: the stack run and the `--expect legacy` evidence (WS14.S1 definition of done); the test run of the self-test; the CI guard step; observe and enforce sets are planned, not confirmed. Full list in `depth/W2-U10.md`.

Next steps for whoever runs the evidence (on a stack, not in the build phase):
1. In `apps/Ingestion Plane`, with the env-file chain of the Ingestion deploy recipe: `docker compose --env-file ../../.env --env-file .env -f docker-compose.yml -f docker-compose.crawl-fixture.yml up -d --wait` (or `--bring-up`).
2. Check first that the subnet 192.88.99.0/24 is accepted and that quarry-edge resolves fixture-a.crawl.test (the first step of the slice).
3. Start the edge with `ENVIRONMENT=dev` and the dev bypass, export `DEV_BEARER`, run `Quarry-v2/scripts/e2e/crawl-scope-robots.sh --expect legacy`.

Traps found while building:
- `quarry-browser` was mid-edit by U07 (WS8.E) for about fifteen minutes: `BrowserSession` and `BrowserDriverCapabilities` literals were missing fields. `cargo check -p quarry-edge` failed in quarry-browser three times and passed on the fourth attempt. Any check of a quarry-edge test target needs quarry-browser to compile.
- The worktree has no `apps/Ingestion Plane/.env` and no `Quarry-v2/.env`. `docker compose ... config` on the real tree fails on required variables. The overlay was checked in a scratch copy under scratchpad/U10/cc with placeholder values (`.env` built from `${VAR:?` names in docker-compose.yml, placeholder env files for each `env_file`).
- `Quarry-v2/docs/RUNBOOKS.md` carries another unit's uncommitted hunks and a CRLF working copy, while HEAD is LF. `qcommit.sh` would have committed the path whole. The section was committed with `qcommit-blobs.sh` built from the HEAD blob plus the appended section.
- The session attribution reminder asks for `Co-Authored-By: Claude Haiku 5.5`, but the batch text asked for `Sonnet 5.5`. The commits carry the Haiku 5.5 line (the session reminder is the system instruction for trailers).
- `git status` shows some paths staged (`M ` on `quarry-core/src/scrape.rs` and two schema JSON files) that this unit did not touch. The explicit-path commits do not include them.
