# W3 estimate (E3.1 to E3.9), 2026-10-23

Estimate and breakdown only: nothing of W3 is built yet. Made by Haiku-high agents from the plan epics and the code; every number has a basis in the per-epic notes. Expected 278 days (low 202, high 372); buildable now in this branch: 21.5 days (batch B1).

## Epic table and totals

Totals: low 202 / expected 278 / high 372 days (input sum of stated lows was 203; E3.4 and E3.6 lows corrected, see corrections). Ours: yes 141.5 (51%), partly 105.5 (38%), no 31 (11%). Buildable now: B1 21.5 d (7.7%); B1 to B2 35.5 d (12.8%). Versus W1 148.25 d: W3 is 1.88x (range 1.36x to 2.51x). Versus W2 493.5 d scheduled: 0.56x (range 0.41x to 0.75x).

E3.1 Credential handoff, OTP first | F-024 | 10 slices | low 19 / exp 28 / high 39 | yes 14, partly 12, no 2 | deps: WS12 S1-S5, WS3 K (built), WS1 1a/1b, WS2 S3, WS8.F only if takeover needed, R-12 sibling browser.act entry | gates: ADR-2a/2b ratification; Control R-13 row | buildable B1-B2: 2.0 (f memo).

E3.2 Authority, consent, approval loop | F-032/033/036 | 12 slices | low 32.5 / exp 46 / high 63.5 | yes 15, partly 22, no 9 | deps: WS1 1b/1c, WS7 S1-S3, WS7 org half (WS7.H/I, Control), WS8.C, sibling execution-core | gates: ADR-1 org sign-off, OD13, D9 card placement | buildable B1-B2: 5.0 (b).

E3.3 Presence and observation | F-028/034/038 | 14 slices (b built) | low 25 / exp 36 / high 48.5 | yes 15.5, partly 13, no 7.5 | deps: WS9.A/B/C/H/I, WS3 K, WS3 S3 deadline, sibling WS11.S0 | gates: D9, ADR-3, sibling edit order | buildable B1-B2: 3.0 (c).

E3.4 Durable sessions, downloads | F-035/039 | 8 slices | low 17.5 / exp 24 / high 32 | yes 15, partly 8, no 1 | deps: WS2.D (built), WS8.F (U67), WS1.A2 and X-1 for production downloads, WS12.S3 | gates: ADR-3 detail, X-1, sibling (4) | buildable B1-B2: 0.

E3.5 Monitors and webhooks | F-030/031 | 8 slices | low 16.5 / exp 23.5 / high 31.5 | yes 15, partly 8.5, no 0 | deps: WS5 (built), WS15.L1/S1, WS4.H, WS14.S6 (only 3) | gates: migration 021 tag decision; X-9 not approved (observe half only); DR-15.1 phase 2 excluded | buildable B1-B2: 4.0.

E3.6 Document parsing and PII | F-037/050 | 10 slices | low 25.5 / exp 34 / high 44.5 | yes 24, partly 7.5, no 2.5 | deps: WS5 B/C/D (built), WS4 matrix, WS16 S1/S2 (sibling+Control), WS7 channel | gates: Office hybrid decision, fail-closed PII typed-failure decision, DR-15.1 | buildable B1-B2: 11.0.

E3.7 Bulk acquisition, agent tools | F-040/041/042 | 11 slices | low 35 / exp 45 / high 57 | yes 24, partly 13, no 8 | deps: WS15.L1/L2, WS14.S2/S3/S5/S6, WS6 S0, WS7 B-H, WS1.A2 (R2), sibling execution-core | gates: crawl-approval decision (not among 13), R-13 registry, X-1 | buildable B1-B2: 4.5.

E3.8 Fetch reliability, cache, reputation | F-047/048/049 | 9 slices | low 20.5 / exp 27.5 / high 36.5 | yes 13, partly 13.5, no 1 | deps: WS3 deadline wrapper, WS4 K (built), WS4 S-1, WS7.B/C/G, R-31 queue, Data Plane contract (3.8.8) | gates: Data Plane artifacts contract; reviewer-model owner (F-049 layers 2 and 3, about 15 d, excluded) | buildable B1-B2: 0 (reputation 3.5 at B3).

E3.9 Evals in CI | F-044 | 7 slices | low 10.5 / exp 14 / high 19.5 | yes 6, partly 8, no 0 | deps: WS14.S1 (one shared fixture), WS11.S0 sibling, WS11 provenance seam, WS6.C-e meters | gates: score thresholds ratification; Tier C runner and capped credential | buildable B1-B2: 6.0.

## Batch plan

Batches are W3 units of at most 6, disjoint files within a batch (one writer per hot file per batch; additive one-line anchors allowed). Each batch starts after the W2 batch named.

B1 (now, 21.5 d): W3-B1-01..06 as in batch1.

B2 (after W2 batch 2, 14.0 d): E3.7.8 pricing fn (2.0, quarry-runtime new module + lib line); E3.3-c @tab mention token in DashboardComposer.tsx (3.0; file clean in git status); E3.5-8 ZDR gate on monitors (1.0; after W2 U01, which edits change_routes.rs); E3.1-f decision memo (2.0; the R-13 JSON row rides a Control PR); E3.9.2 Tier B corpus in lab/evals (3.0; after W2 U10, reads test_site.rs read-only); E3.6-9 parse cache module (3.0).

B3 (after W2 batch 3, 9.0 d): E3.8.6 reputation hot reload (3.5; preflight.rs, one spawn anchor in quarry-edge main.rs, compose append); E3.7.4 sitemap.rs gzip and Sitemap-line parse (2.5; NOT robots.rs, owned by W2 U63; the map_routes hunk waits for U33 and U63); E3.6-10 fixture corpus (3.0).

B4 (after W2 batches 4-5, incl. U01, U08, U12, U18, U19, 12.5 d): E3.5-4 fingerprint switch (3.5; change_routes.rs, workflows.go ChangeMonitorWF; pipeline.rs not needed since text_fp is already emitted); E3.2-g Settings hub, contract-first in action-registry.ts (6.0; H13 one id group with handlers.rs and dispatchers.rs); E3.4-8 Data Plane hand-off via ingest_client.rs (3.0).

B5 (after W2 batch 6, U31-U33, 14.0 d): E3.5-1 webhook org scope plus E3.5-7 notifyOnChange filter (6.0; store.go, store/pg, migration 021, resources.go regions 662-667 and 286-300; after U11, U28, U32); E3.7.7 search scrape block (5.0; search_routes.rs after U33; answer.rs prepare); E3.5-5 previous-content baseline (3.0; change_routes.rs after B4 fingerprint; confirm WS4.H observe status).

B6 (after W2 batch 8, U34-U47, 11.5 d): E3.1-a FillOtpRef contract (2.0; contracts.rs enum append plus pkg mirror; needs the R-12 sibling entry first); E3.3-e1 action.started rect (3.0; event.rs, contracts.rs, observation.rs additive, after U27 and U44); E3.6-5 per-request option (2.5); E3.9.6 cost scoring (1.0; after U37); E3.1-d OtpSource and fixture module (3.0; new module, test_site.rs hunk after U52).

B7 (after W2 batch 10, U48-U54, 18.0 d): E3.6-4 fail-closed redaction wiring (5.0; pipeline.rs and artifact sinks; after U34, U45, U49); E3.3-j settle_ms (3.0; observation.rs and browser_driver.rs; after U52, U53); E3.4-1 checkpoint status and migration (3.0; step_receipts.rs after U49; migration number by arbiter, W2 U67 proposes 0018); E3.8.1 + E3.8.2 typed sub-codes and one deadline budget (5.0; fallback_driver.rs, policy.rs, error.rs mirror; after U23, U26); E3.8.3 scrape-path retries (2.0; retry.rs and routes.rs scrape region; after U45, U53).

B8 (after W2 batch 10, 8.0 d): E3.1-b broker grant kind (3.0; grant.go single editor, sensitive_approval.rs; after W1 closeout reconciled); E3.8.4 URL cache freshness plus ZDR matrix row (5.0; cache.rs, routes.rs scrape region after E3.8.3, zdr matrix rows).

Chromium lane (one chromiumoxide.rs writer per batch, serial, after W2 U58 in batch 12 since W2 batches 9-12 each hold one writer: U50, U52, U55, U58): C1 E3.1-c OTP preconditions (5.0); C2 E3.3-h + E3.3-k hit-test and AX deadlines (7.0, also routes.rs route table); C3 E3.8.7 browser reputation (4.5); C4 E3.4-6 downloads proof (4.0). Total 20.5 d serial.

Gated, 149 d (need W2 G1-G2 units U60-U67, sibling (b), Control, or a decision): E3.7.1 (after U11, U32, U64), E3.7.2 (after U60, U28), E3.7.3 (after U64, U65), E3.7.4 map_routes hunk (after U33, U63), E3.7.5 (after U60, U63, U20), E3.7.9 (after U65, B4 hub, W3-B1-06; needs approval decision), E3.7.10 (R2, sibling, Control; 8.0 d), E3.7.11 (end); E3.4-2 and E3.4-3 (after U67, run_sweeper, run_live, lifecycle), E3.4-5 (after U67, live.rs, gateway browser.rs), E3.4-7 (after U62 and U66); E3.3-a (after U62: browser-session.ts, browser.rs, BrowserChrome.tsx); E3.3-d (after U66, gateway H10 route block), E3.3-g and E3.2-b swap (after U62, H12), E3.3-f, i, l (sibling); E3.2-a, c, d (sibling/Application), E3.2-e1 (after WS1 and grant.go, OD13), E3.2-e2 (after U67 tabs.rs), E3.2-e3 and E3.2-f (Control, WS7.H/I); E3.1-g (IC action after ADR-2a), E3.1-h (after W3-B1-06), E3.1-i, E3.1-e, E3.1-j (end); E3.5-3 (after U65); E3.5-6 (after B5 migration tag decision); E3.6-6 (Office decision), E3.6-7 (WS16 grant; build on fake), E3.6-8 (sibling); E3.8.8 (Data Plane contract), E3.8.5 (Frontend Plane); E3.9.3, E3.9.4 (sibling WS11.S0 and provenance), E3.9.7 (runner and credential).

## Batch 1 (start now)

- W3-B1-01: E3.9.1 + E3.9.5: eval CI job (new .github/workflows/quarry-v2-eval.yml), score file in lab/evals release_matrix.rs, SCOREBOARD.md rows. No W2 file. (3.0 d)
- W3-B1-02: E3.6-1: quarry-transform/src/pii.rs NEW (validators, golden corpus), one lib.rs mod line. Nothing wired. (4.0 d)
- W3-B1-03: E3.6-2 + E3.6-3: parser bomb fixtures and tests; per-page text-vs-scanned classifier in quarry-transform pdf.rs (classifier only, no pipeline routing yet). (4.0 d)
- W3-B1-04: E3.7.6: canonical URL helper, pure, new quarry-transform module with reconciliation tests. Enqueue wiring waits for W2 U60. (2.5 d)
- W3-B1-05: E3.5-2: quarry-control internal/egress NEW (copy of IC guard plus conformance tests); one-line anchors in quarry-control internal/dispatcher/dispatcher.go:41 and internal/catalogwatch/catalogwatch.go:64. Go tests run natively. (3.0 d)
- W3-B1-06: E3.2-b: keyboard-first approval card in NEW features/browser/approval/*, use-chat-shortcuts.ts, orchestration-client.ts decideApproval, Vitest. BrowserChrome.tsx and ChatMessages.tsx swap deferred until W2 U62 (H12). (5.0 d)

## Open questions

- Ratify the ADR-2 split (2a OTP-only handoff, 2b secrets) that E3.1-f recommends; it is not among the 13 approved defaults. Blocks the E3.1-f close-out.
- Should knowledge.crawl_site (E3.7.9, R1) require approval above a page count? Not among the 13 approved defaults; blocks 6.0 d.
- Approve the Office hybrid path (E3.6-6, 5.0 d)? Recommended, not approved.
- Approve fail-closed PII semantics with typed per-page failure (E3.6-4, 5.0 d)? Recommended, not approved.
- Where does the shared approval card and the @tab token render when no dock is open (D9)? Blocks the swap in W3-B1-06 and the render in W3-B2 E3.3-c.
- Approve OD13 consent pattern language for E3.2-e1 (5.0 d)?
- Migration 021: E3.5-6 tag column must be in 021 (same batch as E3.5-1) or use 025 (021 and 022 are free on disk, 024 is taken)? Choose one.
- WS1.A2 status conflict: W1 closeout marks it built (14 commits); the W2 queue still gates broker work on it. Confirm before E3.1-b, E3.2-e1, E3.7.10 start.
- Plan 3.3 sequencing line omits WS2 S3 and WS8.F, which E3.1 names as dependencies. Owner to fix the plan line.
