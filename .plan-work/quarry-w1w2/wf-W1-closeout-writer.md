# W1-closeout-writer hand-over (2026-10-23)

Wrote, from git log 473d0a78..HEAD and 39386dc5..HEAD, the baseline, the breakdown, the unit notes, the harvested open items and the wave test results:
- W1-closeout.md: 39 slice rows, the honest numbers (38 of 39 built, 144.25 of 148.25 days, 0 days at L2, 6.0 days fully at image level on old images, 8.5 with WS4.F), SC1 to SC7 (all NOT-RUN), the wave test phase.
- DEPTH-FINDINGS-W1.md: 87 rows (16 high, 64 medium, 7 low themes), a reconciled list of harvested items HEAD shows closed, and a recommended order.
- The plan's build ledger got a "W1 build wave: closeout" section (8 lines), committed as docs.

How it was generated: node generators in the scratch directory (scratchpad/W1-closeout-writer: depth-data.js, slice-data.js, gen-all.js). Edit the data files and re-run `node gen-all.js <head>` to regenerate both documents; ids are assigned by severity and kind, so a new row renumbers the rest.

Things I verified myself at HEAD: WS4.G plan tests exist; branch compose has DATABASE_URL, MAX_RUN_DOMAINS and the gateway enforcement line; attempt_record still emits proxy_uri; answer_stream emits no tombstone; tabs.rs select and close read the initiator hint; close.rs carries the tombstone call; B-T2 and B-T3 commits exist. Everything else is the unit notes' word.
