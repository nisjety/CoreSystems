### 4.5 Decisions that gate slices

Gated days are the designs' own. "Proceeds without" says what a lane can still do before the decision; "plan default" is the answer the plan assumes so that dependent slices can be written and tested.

| Decision | Slices gated (days) | Proceeds without it | Plan default if the owner does not rule |
|---|---|---|---|
| ADR topic 1: policy owner and claim carrier (fail mode of the entitlement read, minter, override owner) | WS7.A (0.5), WS7.H (6.5), WS7.I (7.5), WS7.J (3), WS7.K (6.5), WS14.H (6), WS7.M optional (4) | WS7.B to WS7.G (21.5 d, owner-neutral), WS6.D in `static` mode, WS11.C interim quota-row carrier | outcome C (claim in the token plus broker snapshot) with org-core as policy store, as D7 recommends |
| ADR topic 3: human plus agent sessions, renewal and idle semantics, kinds beyond `session`/`run` | WS8.C (8.5); WS9.K and WS9.L unscheduled; WS8.K optional (5) | WS8.A, WS8.B, WS8.E, WS8.F core on wiremock, WS9 omnibox, dock and evidence slices | D8-1 to D8-3 as written (absolute 4 h, idle semantics from D2's `Interactive` class) |
| ADR topic 4: ZDR per feature | WS4.H (2.5, observe-mode first), WS4.J (5, also legal review and QB-T17 commit); WS4.I (1.5) waits for the Model Plane Z-05a/Z-05b commit instead | WS4.A to WS4.G | D4 3.3 defaults: refuse durable resources, inline-only scrape, metadata-only tombstone, no hash-only |
| ADR 5: anti-bot live triggers | WS3.I (3) | WS3.E, WS3.H, WS3.J | default stays `legacy` until ruled |
| ADR 8 part 2: Application projection and Space reads of the trace | WS10.G (4.5) | WS10.A to WS10.F, WS10.H, WS10.I | no Application projection; owner-only trace read |
| ADR 10: execution substrate | WS3.G isolation claims (optional); epics F-t2-059/060 | WS3.F with K=1 | single shared Chromium, loopback-only egress proxy unchanged |
| DR-15.1: extraction grant route (adjacent to topics 1 and 4) | WS16.C (3), then WS16.G's E2 tier | WS16.B, WS16.D to WS16.F, WS16.I | option O4 with O1 as the single accountable owner (D15's recommendation: service lane in model-gateway, owner sibling b, Control co-signs the registry) |
| Key-management ownership and master-key custody (audit 9.3 item 11) | WS17.H (8), and `REQUIRED=1` of WS17.D | WS17.A to WS17.G, WS17.I with the interim env key provider | org-core owns keys (D16-3), env provider in dev only |
| Owner decision OD13 (planner route and default) and the stage-3 prerequisites | WS11.F stage 3 | WS11.A to WS11.F stages 1 and 2 | default intent route, no subscription pin, stage 3 not enabled |
| D14b decisions: D9 results source, D14 command transport, D15 ZDR for schedules, D17 notification principal | WS15.K (3.5, optional), WS15.H (4), WS15.I (5, partly) | WS15.A to WS15.G | D14b's own recommendations per decision; for D15, refuse creation for ZDR callers (the shared call site with WS4.H) plus the fire-time posture re-check, which needs the Control Plane to grant `org:retention:read:any` to a quarry-edge principal |
| Crawl hard ceiling: 5000 (D6) versus 10,000 (D14a) pages | WS6.A value only (not a gate on the PR) | everything | 5000, env-overridable (section 7.2 item 1) |
| Receipt text policy default (`sensitive` versus `all`) and legacy-row rewrite | WS12.D default, WS12.H (2, optional) | WS12.A to WS12.C | `sensitive` for non-human steps, D8's `metadata` for human steps |

Gated totals by wave: W1 12 days (WS3.I, WS4.H, WS4.I, WS4.J); W2 63 days (WS7 24, WS8.C 8.5, WS10.G 4.5, WS14.H 6, WS15.H 4, WS15.I 5, WS16.C 3, WS17.H 8).

<!-- part end -->
