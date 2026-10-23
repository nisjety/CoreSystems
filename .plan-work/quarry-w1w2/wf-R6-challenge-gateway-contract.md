# R6-challenge: the Quarry scrape contract for the gateway owner (WS3.J)

For whoever owns `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs`
(`classify_scrape_failure`, the WS5.G hunks). Not touched by this unit.

## What Quarry now sends

`POST /v1/scrape` answers a fetched bot wall with:

```
HTTP 403
{ "error": { "code": "CHALLENGE_DETECTED", "message": "...", "retryable": false,
             "details": { "reason": "waf_interstitial",
                          "block_reason": "waf_interstitial",
                          "recovery": "not_applicable",
                          "next": "other_source" } } }
```

- `details.block_reason` is the field the plan names ("only a Quarry-supplied strong `block_reason` yields
  `site_blocked`"). Values: `captcha_widget`, `waf_interstitial`, `unexpected_bot_error`. Never `challenge_loop` on
  this path.
- `details.reason` carries the same value, because the agent path's `CHALLENGE_DETECTED` already uses `reason`
  (there it is `challenge_loop`, with `host` and `recovery: "exhausted"`). Read `block_reason` for the scrape path.
- No host, URL or page text anywhere in the body, ZDR or not.
- Documented in `docs/openapi.yaml` (`/v1/scrape` 403) and `docs/CONTRACTS.md` section 9.2; the field names are pinned in
  `crates/quarry-core/tests/contracts.rs` (`a_scrape_challenge_is_a_403_with_the_block_reason_and_no_recovery`).

## What Quarry does NOT send

- A bare 403, a 429 or a 5xx page is not turned into an error by this change. If the origin answered 403 with an
  ordinary page, `/v1/scrape` still returns 200 with `output.status: 403`, exactly as before. So a 403 *response from
  Quarry* is now one of: `CHALLENGE_DETECTED` (look at `details.block_reason`), `SECURITY_BLOCKED` (SSRF, blocklist,
  policy), `FORBIDDEN` (for example `body_too_large`). Only the first means "the site blocked automation".
- Proxy suggestions: none. Quarry has no proxy offer.

## What to change on the gateway side

- `classify_scrape_failure(status, elapsed)` takes only the HTTP status. It must also read
  `error.code == "CHALLENGE_DETECTED"` and `error.details.block_reason` from the upstream body and map a present,
  known value to `site_blocked`; a 403 without it is `scrape_refused` (plan), a 429 is `scrape_rate_limited`.
- Keep the SPA accepting the old gateway codes for one release (plan, WS3.J rollout).

## Switches

`QUARRY_CHALLENGE_CLASSIFIER=legacy` makes Quarry stop classifying scrapes. `QUARRY_CHALLENGE_ENFORCE` now defaults to
`log` (round 3, review finding; the plan says `log` while the corpus validates): Quarry classifies and logs but returns the
page, so the gateway sees the old behaviour (a 200 wall page) UNTIL an environment sets `QUARRY_CHALLENGE_ENFORCE=enforce`.
The 403 CHALLENGE_DETECTED with `details.block_reason` described above exists only on such an environment. See
`wf-R6-challenge-decision.md`.

Evidence: unit tests only (`scrape_challenge_tests` in `crates/quarry-runtime/src/pipeline.rs`). No live run against a
Turnstile or Cloudflare page, and the gateway was not run against it.
