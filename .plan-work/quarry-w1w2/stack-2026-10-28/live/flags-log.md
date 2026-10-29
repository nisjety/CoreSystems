# Flag log, live dev stack session, 2026-10-28

Baseline: an empty live-flags.env (no overrides) = the committed compose defaults with the live env chain; values in README.md "Baseline switches".

One line per change: time UTC | leg | KEY old -> new. One line per revert.

2026-10-28T19:50:14Z | P0-live | QUARRY_EDGE__METRICS_ENABLED 0 (default) -> 1 (internal :9092 listener, no token in dev; needed to read counters)
2026-10-28T19:54:03Z | P0-live | QUARRY_EDGE__METRICS_ENABLED 1 -> 0 (default; revert, live-flags.env empty, edge recreated)
2026-10-28T19:56:58Z | L-X10-robots | QUARRY_EDGE__METRICS_ENABLED (unset) -> 1
2026-10-28T19:56:58Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS (unset) -> off
2026-10-28T19:57:54Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS off -> observe
2026-10-28T19:57:54Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE (unset) -> observe
2026-10-28T19:58:43Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS observe -> enforce
2026-10-28T19:58:44Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE observe -> enforce
2026-10-28T20:00:27Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS enforce -> observe
2026-10-28T20:00:27Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE enforce -> observe
2026-10-28T20:00:54Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS observe -> enforce
2026-10-28T20:00:55Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE observe -> enforce
2026-10-28T20:01:38Z | L-X10-robots | QUARRY_EDGE__CRAWL_ROBOTS enforce -> (unset)
2026-10-28T20:01:39Z | L-X10-robots | QUARRY_EDGE__CRAWL_SCOPE_MODE enforce -> (unset)
2026-10-28T20:01:40Z | L-X10-robots | QUARRY_EDGE__METRICS_ENABLED 1 -> (unset)
2026-10-28T20:01:40Z | L-X10-robots | note: '(unset)' means the compose default (CRAWL_ROBOTS observe, CRAWL_SCOPE_MODE code default observe, METRICS_ENABLED 0); live-flags.env is empty again and the edge was recreated at 20:01:41Z
2026-10-28T20:07:29Z | L-SC7-billing | RUST_LOG info (default) -> info,quarry_runtime::usage=debug (only to see the 'usage metered' debug line, which carries subject, metric and quantity; no secrets in it)
2026-10-28T20:09:17Z | L-SC7-billing | RUST_LOG info,quarry_runtime::usage=debug -> info (default; revert, live-flags.env empty, edge recreated)
2026-10-28T20:12:10Z | L-SC2a-authority | QUARRY_EDGE__TAKEOVER_ENFORCEMENT (unset) -> 1
2026-10-28T20:12:10Z | L-SC2a-authority | QUARRY_EDGE__METRICS_ENABLED (unset) -> 1 (internal :9092, to read the takeover counters)
2026-10-28T20:13:04Z | L-SC2a-authority | QUARRY_EDGE__TAKEOVER_ENFORCEMENT 1 -> (unset) (observe; METRICS_ENABLED stays 1 for now)
2026-10-28T20:14:11Z | L-SC2a-authority | QUARRY_EDGE__METRICS_ENABLED 1 -> (unset) (revert; live-flags.env empty, edge recreated)
2026-10-28T20:24:27Z | L-SC3-SC4-runs | QUARRY_EDGE__METRICS_ENABLED (unset) -> 1 (internal :9092, to read the run and sweeper counters)
2026-10-28T20:24:27Z | L-SC3-SC4-runs | QUARRY_EDGE__RUN_SWEEP_INTERVAL_S (unset, 15) -> 5
2026-10-28T20:24:27Z | L-SC3-SC4-runs | QUARRY_EDGE__AGENT_RUN_IDLE_TTL_S (unset, 300) -> 40 (RUN_SWEEPER_DRY_RUN stays 1 in this step)
2026-10-28T20:24:27Z | L-SC3-SC4-runs | QUARRY_EDGE__MAX_CONCURRENT_RUNS (unset, 32) -> 3 (global cap check)
2026-10-28T20:24:27Z | L-SC3-SC4-runs | QUARRY_BROWSER_DEADLINE_SCALE (unset, 1) -> 0.5 (navigate deadline 15 s, so the typed TIMEOUT beats the library's fixed 30 s)
2026-10-28T20:30:36Z | L-SC3-SC4-runs | QUARRY_EDGE__MAX_CONCURRENT_RUNS 3 -> (unset, 32) (revert)
2026-10-28T20:30:36Z | L-SC3-SC4-runs | QUARRY_BROWSER_DEADLINE_SCALE 0.5 -> (unset, 1) (revert)
2026-10-28T20:30:36Z | L-SC3-SC4-runs | QUARRY_EDGE__RUN_SWEEPER_DRY_RUN 1 -> 0 (the sweeper now really releases runs; sessions_open was 0 before the change, so no one else's run is on the edge)
2026-10-28T20:30:36Z | L-SC3-SC4-runs | QUARRY_EDGE__INTERACTIVE_RUN_IDLE_TTL_S (unset, 900) -> 40
2026-10-28T20:30:36Z | L-SC3-SC4-runs | QUARRY_EDGE__INTERACTIVE_RUN_MAX_TTL_S (unset, 14400) -> 150 (the 4 h backstop, shortened to 150 s)
2026-10-28T20:30:36Z | L-SC3-SC4-runs | QUARRY_EDGE__METRICS_ENABLED 1 -> 1, RUN_SWEEP_INTERVAL_S 5 -> 5, AGENT_RUN_IDLE_TTL_S 40 -> 40 (unchanged)
2026-10-28T20:34:29Z | L-SC3-SC4-runs | QUARRY_EDGE__RUN_SWEEPER_DRY_RUN 0 -> (unset, compose default 1) (revert)
2026-10-28T20:34:29Z | L-SC3-SC4-runs | QUARRY_EDGE__INTERACTIVE_RUN_IDLE_TTL_S 40 -> (unset) (revert); INTERACTIVE_RUN_MAX_TTL_S 150 -> (unset) (revert); AGENT_RUN_IDLE_TTL_S 40 -> (unset) (revert); RUN_SWEEP_INTERVAL_S 5 -> (unset) (revert); METRICS_ENABLED 1 -> (unset) (revert)
2026-10-28T20:34:29Z | L-SC3-SC4-runs | QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_ORG (unset, 8) -> 16 (RSS at 16 contexts needs more than the per-org cap of 8)
2026-10-28T20:35:49Z | L-SC3-SC4-runs | QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_ORG 16 -> (unset, 8) (revert; live-flags.env empty, edge recreated)
2026-10-28T20:40:18Z | L-SC5-SC6-docs-zdr | QUARRY_EDGE__DOC_ROUTING (unset, code default shadow) -> on
2026-10-28T20:41:40Z | L-SC5-SC6-docs-zdr | QUARRY_EDGE__DOC_ROUTING on -> (unset, shadow) (revert)
2026-10-28T20:41:40Z | L-SC5-SC6-docs-zdr | RUST_LOG (info default) -> debug (worst-case log level for the ZDR log scan)
2026-10-28T20:43:01Z | L-SC5-SC6-docs-zdr | RUST_LOG debug -> info (default; revert, live-flags.env empty, edge recreated)
2026-10-28T20:46:23Z | L-SC5-SC6-docs-zdr | QUARRY_EDGE__RECEIPT_TEXT_MODE (unset, code default sensitive) -> all
2026-10-28T20:47:08Z | L-SC5-SC6-docs-zdr | QUARRY_EDGE__RECEIPT_TEXT_MODE all -> (unset, sensitive) (revert; live-flags.env empty, edge recreated)
2026-10-28T20:49:56Z | L-X12-D16 | QUARRY_EDGE__METRICS_ENABLED (unset) -> 1 (internal :9092, to read the budget and profile counters)
2026-10-28T20:51:53Z | L-X12-D16 | QUARRY_EDGE__BUDGET_ENFORCEMENT shadow -> on
2026-10-28T20:51:53Z | L-X12-D16 | QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS (unset, 60) -> 5
2026-10-28T20:51:53Z | L-X12-D16 | QUARRY_EDGE__RUN_SWEEPER_DRY_RUN 1 -> 0 (so an idle agent run is really released; sessions_open was 0 before the change, so no one else's run is on the edge)
2026-10-28T20:51:53Z | L-X12-D16 | QUARRY_EDGE__RUN_SWEEP_INTERVAL_S (unset, 15) -> 5; QUARRY_EDGE__AGENT_RUN_IDLE_TTL_S (unset, 900) -> 30 (idle-expiry resume check)
2026-10-28T20:55:46Z | L-X12-D16 | QUARRY_EDGE__BUDGET_ENFORCEMENT on -> (unset, compose default shadow) (revert)
2026-10-28T20:55:46Z | L-X12-D16 | QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS 5 -> (unset, 60) (revert)
2026-10-28T20:55:46Z | L-X12-D16 | QUARRY_EDGE__RUN_SWEEPER_DRY_RUN 0 -> (unset, compose default 1) (revert); RUN_SWEEP_INTERVAL_S 5 -> (unset) (revert); AGENT_RUN_IDLE_TTL_S 30 -> (unset) (revert); METRICS_ENABLED stays 1 for the D16 half
2026-10-28T20:58:56Z | L-X12-D16 | QUARRY_EDGE__METRICS_ENABLED 1 -> (unset) (revert; live-flags.env empty, edge recreated)

## Re-run 2026-10-29

2026-10-28T23:18:02Z | R-SC4 | QUARRY_BROWSER_DEADLINE_SCALE (unset, 1) -> 0.5 (T2; same non-QUARRY_EDGE__ name as the original leg; navigate deadline 12.5 s)
2026-10-28T23:20:33Z | R-SC4 | QUARRY_BROWSER_DEADLINE_SCALE 0.5 -> (unset, 1) (revert; live-flags.env empty, edge recreated)
2026-10-28T23:25:35Z | R-X12 | QUARRY_EDGE__METRICS_ENABLED (unset, 0) -> 1 (internal :9092, to read quarry_agent_budget_exhausted_total)
2026-10-28T23:25:35Z | R-X12 | QUARRY_EDGE__BUDGET_ENFORCEMENT shadow -> on
2026-10-28T23:25:35Z | R-X12 | QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS (unset, 60) -> 5
2026-10-28T23:27:37Z | R-X12 | QUARRY_EDGE__BUDGET_ENFORCEMENT on -> (unset, compose default shadow) (revert)
2026-10-28T23:27:37Z | R-X12 | QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS 5 -> (unset, 60) (revert)
2026-10-28T23:27:37Z | R-X12 | QUARRY_EDGE__METRICS_ENABLED 1 -> (unset, 0) (revert; live-flags.env empty, edge recreated)
2026-10-29T01:10:33Z | R2-SC4 | QUARRY_BROWSER_DEADLINE_SCALE (unset, 1) -> 0.5 (T2 incl. a first-step case; navigate deadline 12.5 s; sessions_open was 0)
2026-10-29T01:12:13Z | R2-SC4 | QUARRY_BROWSER_DEADLINE_SCALE 0.5 -> (unset, 1) (revert; live-flags.env empty, edge recreated)
2026-10-29T02:44:10Z | R3-SC4 | QUARRY_EDGE__RUN_SWEEPER_DRY_RUN 1 -> 0 (RS: an idle agent run is really released; sessions_open was 0 before the change)
2026-10-29T02:44:10Z | R3-SC4 | QUARRY_EDGE__RUN_SWEEP_INTERVAL_S (unset, 15) -> 5; QUARRY_EDGE__AGENT_RUN_IDLE_TTL_S (unset, 300) -> 30 (RS idle-expiry resume)
2026-10-29T02:46:37Z | R3-SC4 | QUARRY_EDGE__RUN_SWEEPER_DRY_RUN 0 -> (unset, compose default 1) (revert); RUN_SWEEP_INTERVAL_S 5 -> (unset, 15) (revert); AGENT_RUN_IDLE_TTL_S 30 -> (unset, 300) (revert; live-flags.env empty, edge recreated)
