# W2-U10 progress (WS14.S1: hermetic fixture site and real-stack harness)

Unit: W2-U10, batch 2. Slice: WS14.S1 (plan `docs/plans/quarry-browser-build-plan-2026-10-14.md`, "Slice WS14.S1 detail"). Worktree `C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b`, branch `quarryv2-next`. Status: BUILT, committed; the stack run and the test run are not done (build-first rule).

## Commits (local, not pushed)

- `ba965aa9b` test(quarry-edge): hermetic in-memory fixture site (tests/support/fixture_site.rs, one line in tests/support/mod.rs, tests/fixture_site_selftest.rs, 12 cases). Compile-checked.
- `b247455d5` test(ingestion): compose overlay `apps/Ingestion Plane/docker-compose.crawl-fixture.yml`, fixture content `Quarry-v2/deploy/compose/crawl-fixture/{a,b}`, harness `Quarry-v2/scripts/e2e/crawl-scope-robots.sh`.
- `922747679` docs(quarry-v2): RUNBOOKS section for WS14.S1 (appended; hunk-only through a private index, because another unit's uncommitted RUNBOOKS hunks are in the working copy).

## What was checked, and the result

- `cargo check -p quarry-edge --test fixture_site_selftest` through qtest: attempts 1 to 3 failed in `quarry-browser` (lib), not in this unit's files (another unit mid-edit on BrowserSession and BrowserDriverCapabilities literals). Attempt 4: finished, rc 0, no warnings in the new files (2 pre-existing warnings in quarry-edge lib: unused import FleetStatus, deprecated fetch_update).
- `bash -n scripts/e2e/crawl-scope-robots.sh`: ok. `--self-test`: 8 of 8 pass. `--guard`: PASS. `--expect bogus`: usage, exit 2.
- `docker compose -f docker-compose.yml -f docker-compose.crawl-fixture.yml config -q`, in a scratch copy with placeholder env values: exit 0. Merged config attaches quarry-edge to crawl-fixture (plus ingestion-net and inter-plane-bus) and resolves the fixture mounts. On the real tree the check cannot run (no `.env` files; required variables missing).
- Not run: the Rust self-test (`cargo test -p quarry-edge --test fixture_site_selftest`), the stack, the harness against a stack.

## Not done (and why)

- Stack run (image rebuild, `up` with the overlay, `--expect legacy`): WS14.S1 definition of done, and the build phase runs no stack evidence. It is the first step for the evidence owner (see wf-W2-U10.md).
- CI grep guard as a CI step: `.github/workflows/quarry-v2-ci.yml` is shared (H25, U12 edits it in this batch). The guard runs as `crawl-scope-robots.sh --guard`.
- observe and enforce assertion sets: planned expectations, for WS14.S2 to S4.

## For the next batch

- Keep `STANDARD_INDEX_A` and `STANDARD_ROBOTS_A` in `fixture_site.rs` in step with `deploy/compose/crawl-fixture/a/index.html` and `robots.txt`.
- Tests use `mod support;` and `support::fixture_site::{FixtureSiteDriver, FixtureReply}`.
- Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U10.md`.
