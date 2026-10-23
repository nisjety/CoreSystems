# Hand-over: R6-challenge (WS3.H open items, WS3.J Quarry half)

Commits (branch quarryv2-next, local):
- 08b0a8be reject_if_exhausted also refuses Evaluate; table test per action (MouseWheel, RespondDialog, Back/Forward stay allowed by the spec: the rule stops further attempts at the wall, not a person's take-over or passive actions).
- 91a64773 ChallengeSignal schema golden (docs/schemas/challenge-signal.schema.json), quarry-core challenge_schema test, scripts/tests/openapi-challenge-parity.test.mjs (8 tests), two CI steps in quarry-v2-ci.yml.
- d8e62fa3 scrape path: challenge::classify_fetched + PageRunner refusal -> 403 CHALLENGE_DETECTED with details.block_reason (additive, no request/response type changed); CONTRACTS 9.2; SOVEREIGN doc.
- 23e1ad7f RUNBOOKS QUARRY_CHALLENGE_* section.
- ab8a5b99 SDK docs ChallengeSignal.md and ScrapeApi.md regenerated from HEAD spec.

Traps / notes:
- openapi.yaml: my 403 and evaluate hunks were swept into a4447aff (WS3.F /ready commit) before I committed; they are in HEAD, just not under my message.
- Regenerating the TS SDK from HEAD also changes OpsApi.md and adds 7 Ready*.md pages: WS3.F's, not committed here.
- QUARRY_CHALLENGE_ENFORCE default is now `log` (round 3, after a reviewer pointed at the plan text); `enforce` is per environment. See wf-R6-challenge-decision.md.
- Gateway owner: wf-R6-challenge-gateway-contract.md.
- Not run: live Chromium, real wall corpus, GitHub Actions for the new CI steps, Python SDK regen.
- Round 3: commits 1b1a9810, 12e37792, d2829c30. The generic "page extracted" info line logs the URL (pre-existing; a wall passing through in log mode appears in it) - not changed.
