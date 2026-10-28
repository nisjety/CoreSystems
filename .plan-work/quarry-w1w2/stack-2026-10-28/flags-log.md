# Flag log, qv2e evidence stack, 2026-10-28

Baseline: `qv2e-flags.env` as written by the lead at 16:50 UTC = the committed compose defaults of `apps/Ingestion Plane/docker-compose.yml` at `bd61260ee`, plus `ENVIRONMENT=dev`, `QUARRY_EDGE_AUTH_DEV_BYPASS=1`, `QUARRY_EDGE__ZERO_SAAS_SEARCH=1`, `QUARRY_EDGE__PROFILE_OWNER_ENFORCE=observe`, an empty `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL`. Not set (code default): `QUARRY_EDGE__TAKEOVER_ENFORCEMENT`, `QUARRY_EDGE__RECEIPT_TEXT_MODE`, `QUARRY_EDGE__CRAWL_SCOPE_MODE`, `QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS`, `QUARRY_EDGE__DOC_ROUTING`.

One line per change: time UTC | leg | KEY old -> new. One line per revert.

2026-10-28T17:01:16Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS observe -> off
2026-10-28T17:02:19Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS off -> observe
2026-10-28T17:02:20Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE (unset) -> observe
2026-10-28T17:03:07Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS observe -> enforce
2026-10-28T17:03:07Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE observe -> enforce
2026-10-28T17:05:52Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS enforce -> observe
2026-10-28T17:05:53Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE enforce -> observe
2026-10-28T17:06:05Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS observe -> enforce
2026-10-28T17:06:05Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE observe -> enforce
2026-10-28T17:08:49Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS enforce -> observe
2026-10-28T17:08:50Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE enforce -> observe
2026-10-28T17:09:11Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS observe -> enforce
2026-10-28T17:09:11Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE observe -> enforce
2026-10-28T17:09:43Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS enforce -> observe
2026-10-28T17:09:44Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE enforce -> (unset)
# L-X10-robots note: the two changes at 17:05:52Z and 17:06:05Z were file edits only. The edge restart failed at that time (QV2E_REV was empty because the helper script ran with MSYS_NO_PATHCONV=1), so the running edge stayed in enforce. That D2-161 attempt is void (log kept as x10/L-X10-robots-d2161-attempt1-no-restart.log). The run at 17:08 to 17:09 is the valid one; the restart helper now checks that the running flags match the file.
# L-X10-robots end state: QUARRY_EDGE__CRAWL_ROBOTS observe (baseline), QUARRY_EDGE__CRAWL_SCOPE_MODE unset (baseline). Edge restarted 17:09:44Z, healthy, /version qv2e-bd61260ee. File diff against the leg-start copy: empty.
