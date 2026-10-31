# R6-challenge: open decision, the default of QUARRY_CHALLENGE_ENFORCE

Status (round 3, 2026-10-19): the default is now `log` (option B). A reviewer read the plan's WS3.H contract text
("`QUARRY_CHALLENGE_ENFORCE=log` while the corpus validates") as explicit, and the corpus has not validated; the scrape-path
refusal (WS3.J) makes a false strong signal lose a real page, which raised the cost of the old default. Changed: `ChallengeConfig::default`
and `from_values` (only the literal `enforce` turns refusals on; a typo is `log`), tests (they use `ChallengeConfig::enforcing()`),
SOVEREIGN doc, RUNBOOKS, CONTRACTS 9.2, openapi, regenerated SDK page. Still open for the human owner: who declares the corpus validated
and the bar; whether the scrape path should enforce earlier or later than the agent path (option C, needs a second flag); and every
environment that wants the DoD evidence (403 CHALLENGE_DETECTED) must set `QUARRY_CHALLENGE_ENFORCE=enforce` explicitly.
The text below is the original (round 2) analysis, where "the code default" meant `enforce`.

## What the plan says

`docs/plans/quarry-browser-build-plan-2026-10-14.md`, WS3.H "Contracts, flags":
"`QUARRY_CHALLENGE_CLASSIFIER=v2|legacy` (default `v2`, `legacy` restores the substring classifier for one release),
`QUARRY_CHALLENGE_ENFORCE=log` while the corpus validates."

Flag table (line ~3572): `QUARRY_CHALLENGE_CLASSIFIER` (`v2|legacy`), `QUARRY_CHALLENGE_ENFORCE`, `QUARRY_BLOCK_ESCALATION`
| default `v2`, enforce (`log` while validating), `legacy` until ADR-5 then `honest`.

Risk list of the same slice names the `log` phase as the mitigation against "false strong signals end runs early".
The DoD and the deployment evidence need refusals ("the next same-host `Navigate` returns 403 `CHALLENGE_DETECTED`"),
which only happen in enforce mode. No ADR covers this flag (ADR-5 is the anti-bot posture and governs
`QUARRY_BLOCK_ESCALATION`, not this one).

So the plan states two things at once: the steady-state default is enforce, and `log` is the setting "while the corpus
validates". It does not say when validation ends, who ends it, or which setting a first deploy ships with. It leaves the
default open, so the code was not touched.

## What exists today

- The classifier has been checked against about 60 hand-written fixture rows and the runner tests. There is no captured
  page with provenance and no live Turnstile, hCaptcha or login-page run. The SOVEREIGN doc says so.
- Default `enforce` has two consequences now (the second is new in this unit, WS3.J):
  1. Agent path: after one recovery, a second strong signal on a host makes every further navigation to it and
     interaction with its page a 403 `CHALLENGE_DETECTED`. A false strong signal (a page that is not a wall) therefore
     locks a host for the rest of that run (it clears when a later observation is not strong).
  2. Scrape path: `/v1/scrape` and every crawl/batch page now fails with 403 `CHALLENGE_DETECTED` when the fetched page
     is classified strong. Before, such a page (a Cloudflare interstitial, a Turnstile page) came back as a 200 whose
     markdown was the wall's text and was stored and ingested. A false strong signal now loses a real page, not just a step.

## Options

A. Keep `enforce` as the default (current).
   - Consequence: the wall pages stop being ingested the day this ships, and the DoD evidence needs no env change.
     False strong signals are visible only as failures (403 with `block_reason`), after the fact. Rollback is one env
     var and a restart (`log`), as the RUNBOOKS entry says.
B. Flip the default to `log` until a captured corpus has passed, enable `enforce` per environment.
   - Consequence: matches the plan's "log while the corpus validates" literally. Nothing is refused by default: the
     agent loop can still hammer a wall (the `would_reject` line is the only trace), and the scrape path keeps
     ingesting wall pages as before. The rollout needs an explicit second step and an owner for "the corpus has
     validated". The WS3.H deployment evidence must set `enforce` on the evidence stack and name it.
C. Split the two paths: `enforce` for the agent path (a refusal costs one step and clears), `log` for the scrape path
   (a refusal loses a page and stops a crawl page) until the corpus validates. Needs a second variable (for example
   `QUARRY_CHALLENGE_SCRAPE=enforce|log`) or a rule that `log` is the scrape default.
   - Consequence: the cheapest false positive is enforced, the costly one is not; one more flag to document and retire.

## Recommendation (not applied)

Option C if a second flag is acceptable, otherwise B. Reason: the only evidence for the classifier is synthetic, and
the scrape-path refusal is the first place a false strong signal destroys data rather than a step. Either way, whoever
flips it also changes `ChallengeConfig::default`, the `config_values_parse_with_safe_defaults` test, the `Default`
doc comment, the SOVEREIGN doc ("default `enforce`"), the RUNBOOKS entry ("Default" column and the "Limits" paragraph) and
the openapi 403 description of `/v1/scrape`.

## What to decide

1. Which default ships (A, B or C)?
2. Who declares the corpus validated, and what is the bar (number of captured pages per vendor, zero false strong on
   the non-signal rows)?
3. For C: the name of the scrape-path switch.
