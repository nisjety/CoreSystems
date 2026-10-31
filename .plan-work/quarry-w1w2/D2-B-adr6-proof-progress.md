# D2-B-adr6-proof: progress (decision 3.8, ADR-6 proof, list B item 13)

Status: the code and the offline tests are COMMITTED (local, branch `quarryv2-next`, not pushed). The live lane has NOT run: it needs the evidence stack, which comes after SC1 to SC7. No flag changed: the production compose keeps `QUARRY_EDGE__SCRAPE_ACTIONS:-off`. Open findings: `depth/D2-B-adr6-proof.md`. Continuation pointer: `wf-D2-B-adr6-proof-progress.md`.

## Commits (local)

- `9c548a823` fix(quarry-runtime): an actions-only run counts its blocked requests (WS13.5, ADR-6). Root-cause fix found while writing check 8: the count was taken only for a capture plan, so an actions-only run always reported `blocked_requests` 0.
- `3253eb010` test(quarry-edge): real-token scrape actions tests beside zdr_claim_floor (WS13.5, ADR-6). Decision 3.8 step 4. Also one word in `tests/support/zdr_harness.rs` (`token_with_scopes` is `pub`).
- `33b38087c` test(quarry-edge): scrape_actions_live, the env-gated evidence lane (WS13.5, ADR-6). Decision 3.8 step 3. Also the header of `tests/scrape_actions.rs`, which named a runtime live run that does not exist.
- `c383153b9` test(quarry-edge): the ADR-6 real-token tests are evidence-gated and skipped by default (WS13.5, ADR-6). The brief asks for both proof tests to be skipped by default; the real-token ones ran in every `cargo test` until this commit.
- the notes commit that adds this file, its pointer and `depth/D2-B-adr6-proof.md`.

Files of this unit: `crates/quarry-runtime/src/browser_driver.rs`; `crates/quarry-edge/tests/scrape_actions_live.rs` (new); `crates/quarry-edge/tests/scrape_actions_real_token.rs` (new); `crates/quarry-edge/tests/scrape_actions_real_token_user_mode.rs` (new); `crates/quarry-edge/tests/support/actions_real_auth.rs` (new); `crates/quarry-edge/tests/support/zdr_harness.rs` (one word); `crates/quarry-edge/tests/scrape_actions.rs` (header only).

Not this unit's, left alone: `tests/support/fixture_site.rs` (modified by another unit, not committed here).

## What runs where

- `tests/scrape_actions_real_token.rs` (switch `on`) and `tests/scrape_actions_real_token_user_mode.rs` (switch `user`): real RS256 tokens through the real `require_auth`, golden claim sets, browser double. `#[ignore]`d and gated by `QUARRY_EVIDENCE_STACK=1` like the live lane (`evidence_lane()`), so a plain `cargo test` reports them as ignored. They need no stack and no Chromium: `QUARRY_EVIDENCE_STACK=1 cargo test -p quarry-edge --test scrape_actions_real_token --test scrape_actions_real_token_user_mode -- --ignored`.
- `tests/scrape_actions_live.rs`: `#[ignore]`; returns at once unless `QUARRY_EVIDENCE_STACK=1`; with the flag, a missing variable fails the test. It drives a running edge over HTTP with a real Chromium. NOT RUN.

## Verified (qtest, rust:1 container, this session)

- `cargo test -p quarry-edge --test scrape_actions_live --test scrape_actions_real_token --test scrape_actions_real_token_user_mode --test scrape_actions --test scrape_capture`: `scrape_actions_live` 1 ignored; `scrape_actions_real_token` 5 passed; `scrape_actions_real_token_user_mode` 2 passed; `scrape_actions` 9 passed; `scrape_capture` 4 passed.
- Gate, flag off: `cargo test -p quarry-edge --test scrape_actions_live -- --ignored --nocapture` prints `skipping: set QUARRY_EVIDENCE_STACK=1 ...` and passes.
- Gate, flag on, no `E2E_*` variables: panics `QUARRY_EVIDENCE_STACK=1 needs E2E_EDGE set ...` (rc 101, as intended).
- `cargo test -p quarry-edge --lib scrape_actions`: 12 passed.
- `cargo test -p quarry-runtime --lib browser_driver`: 27 passed (after the blocked-count fix).
- `cargo check --workspace --all-targets`: Finished, rc 0 (run before the two last test-only edits, which were then compiled and run as listed here). The warnings are in other units' files; none point at the new test files.
- Gate, real-token binaries (`c383153b9`): default `cargo test`: 7 ignored. `-- --ignored` with no flag: 7 skip lines, 5 and 2 passed. `QUARRY_EVIDENCE_STACK=1 ... -- --ignored`: 5 passed and 2 passed.
- rustfmt 1.9.0 `--check`: `scrape_actions_live.rs` and `support/actions_real_auth.rs` are clean; the two real-token binaries have no diff of their own. `--check` reports diffs only in their child modules (`support/zdr_harness.rs`, `support/fixture_site.rs`), which were already unformatted and were not reformatted here.
- NOT RUN: `make test-scrape-capture` (`Quarry-v2/scripts/e2e-scrape-capture.sh`), and `scrape_actions_live`. Both need the stack.

## Stack session: steps (do them only after SC1 to SC7; nothing below was run)

1. Edge (evidence only, never production): `QUARRY_EDGE__SCRAPE_ACTIONS=user`, `QUARRY_EDGE__SCRAPE_CAPTURE=on`. Leave `QUARRY_SCRAPE_HEAVY_PERMITS` at its default 1. Restart the edge. Production compose stays `off`.
2. Fixture: serve `page.html` (below) from a throwaway nginx container or the fixture host, reachable by the edge container at `E2E_FIXTURE_URL`. Optional: a probe URL for this host, `E2E_FIXTURE_PROBE_URL`. Confirm the stack's egress does NOT allow `offhost.example.net`.
3. Tokens: `E2E_TOKEN` = a user token, minted as `make test-scrape-capture` does. Optional `E2E_SERVICE_TOKEN` = a service token WITHOUT `scrape:interact`.
4. Run and keep the transcript:
   `QUARRY_EVIDENCE_STACK=1 E2E_EDGE=<edge base> E2E_TOKEN=<user token> E2E_FIXTURE_URL=<fixture as the edge reaches it> [E2E_FIXTURE_PROBE_URL=<fixture as this host reaches it>] [E2E_SERVICE_TOKEN=<service token>] cargo test -p quarry-edge --test scrape_actions_live -- --ignored --nocapture --test-threads=1`
   Expect one `ok` line per check and `all N checks passed`: N is 11 (checks 1 to 11 always run; the probe and the service token are optional and each adds one).
5. Logs: `docker logs <edge container> 2>&1 | grep -c SECRET_SENTINEL_123` must print `0` (check 10 cannot read logs). Do the same for the event stream if the stack keeps one readable.
6. Run `make test-scrape-capture` (in `apps/Ingestion Plane`) on the same stack, the shell contract run, and keep its transcript.
7. Cache and CAS untouched (decision 3.8 step 6). Count the org's artifacts (`GET /v1/artifacts`) before and after checks 1 to 4 and 9: under ZDR the count must not move. Give each actions run its own query string on the fixture URL, then check that a plain scrape of that URL afterwards is not served from an entry the actions runs wrote. Record both results.
8. The owner reads the transcripts. A failed check means: do not flip anything, add it to `depth/D2-B-adr6-proof.md`, and fix it in a new unit.
9. Rollback: set `QUARRY_EDGE__SCRAPE_ACTIONS` back to `off` (or unset it) and restart the evidence edge.

Traps found while preparing (check each on the first run):
- Check 11 assumes one heavy permit. With `QUARRY_SCRAPE_HEAVY_PERMITS` above 1 it fails by design.
- Check 1 gets `400 feature_disabled` when the edge is `off`; that is the first thing to look at.
- Check 8 needs the blocked count from commit `9c548a823`. A count of 0 means the egress receipts show no Block for the page; look at the receipts before blaming the page.
- The actions route gets a 125 s router deadline while actions are on; any proxy in front of the edge must allow it. Check 11 holds the permit for 6 s, so the proxy only matters for the 115 s bound.
- The heading text is deliberate: `AFTER` appears only in `data-after`, so a markdown that shows AFTER without the click cannot pass check 2.

## Fixture page (`page.html`, to serve on the fixture host)

```html
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Actions fixture</title></head>
<body>
<main>
<h1 id="heading" data-after="AFTER">BEFORE</h1>
<p>Fixture text for the scrape actions evidence run. It is long enough for the main-content extractor to keep, and it says nothing else.</p>
<button id="swap" type="button">Swap the heading</button>
<button id="offhost" type="button">Load an off-host image</button>
<input id="search" type="text" name="q" aria-label="Search">
</main>
<script>
document.getElementById('swap').addEventListener('click', function () {
  var heading = document.getElementById('heading');
  heading.textContent = heading.getAttribute('data-after');
});
document.getElementById('offhost').addEventListener('click', function () {
  var image = document.createElement('img');
  image.src = 'http://offhost.example.net/pixel.png';
  document.body.appendChild(image);
});
</script>
</body>
</html>
```

## Open items

See `depth/D2-B-adr6-proof.md`. None of them changes a flag. Item 2 (the live run has never happened) is the one that gates the ADR-6 `user` step.
