## 5. EFFORT ROLL-UP

### 5.1 Assumptions

- Days are the designs' own engineer-days (section 4 of each design, section 10 where the design restates them). Where a design gives a range the midpoint is used (WS2.C 5.5, WS2.D 6.5, WS9.J 9, WS5.I 13.5, WS16.J 9). T-shirt sizes use S = 1-2 d, M = 3-5 d, L = 6-12 d as the brief says; the days, not the letters, drive every total.
- Waves follow the fixed numbering: W1 = WS1 to WS5, W2 = WS6 to WS17, with two moves that the dependency graph forces: WS6.A (crawl bound, 1.5 d) is pulled into W1 as a hotfix, and WS3.K (actionability, 8 d, F-t2-015) is counted in W2 because it needs WS3.C, D and F first.
- Columns: *core* = scheduled, unconditional; *gated* = blocked by a named ADR or decision (counted because the plan schedules it once the decision lands); *opt* = optional slices; *unsched* = design-level only, never included in a total. Sibling-owned days that sit outside the Quarry figure (WS6.B +1.5 Model Plane, WS6.E +2 Model Plane and +2 execution-core) are not in the totals; they add about 5.5 days to W2.
- Team and focus: engineer-weeks = engineer-days / 5. Calendar weeks assume 80% focus (review, CI waits, rebases). W1 team 6.5 engineers (L1 x2, L2, L3 x1.5, L4, L5), W2 sensitivity at 8, 12 and 16 engineers.
- Risk adjustment: the audit raised 33 efforts after verification, and several designs say the audit understated (D2 S3 from S to M, D11 from M to L, D14a step 1 from S to 10 d, D15 from M to L, D10 from L to XL by repo scale). W1 slices are small, file-level and mostly verified (x1.15 to x1.5). W2 adds cross-plane review, ADR waits, uncommitted-diff rebases and migration renumbering (x1.3 to x1.8).

### 5.2 Per wave

| Wave | core d | gated d | opt d | unsched d (excluded) | core+gated d | eng-weeks | risk-adjusted eng-weeks | calendar weeks at team size |
|---|---|---|---|---|---|---|---|---|
| W1 | 113.75 | 12 | 10 | 13.5 | 125.75 | 25.1 | 29 to 38 | 6.5 engineers: 5.6 to 7.3 weeks |
| W2 | 447 | 63 | 19.5 | 9 | 510 | 102.0 | 133 to 184 | 8 eng: 21 to 29 wk; 12 eng: 14 to 19 wk; 16 eng: 10 to 14 wk |
| W1+W2 | 560.75 | 75 | 29.5 | 22.5 | 635.75 | 127.2 | 162 to 221 | |

The critical path (section 4.4, 50 engineer-days, 13 to 18 weeks risk-adjusted) is a floor, not the estimate: W1 is throughput-bound at 6.5 engineers (about 6 to 8 weeks), W2 at 12 engineers is about 14 to 19 weeks and is where the critical path and the throughput estimate meet; beyond about 14 engineers the critical path binds and extra people do not shorten W2. End to end, W1 plus W2 at the stated team sizes is roughly 5 to 7 months, with W2 foundation slices starting during W1 as soon as their lane is free. Epics of W3 to W5 are outside every number here.

### 5.3 Per lane

Days are core+gated unless the column says otherwise; the risk range applies the wave multipliers to each wave's days and converts to engineer-weeks.

| Lane | W1 core d | W1 gated d | W2 core d | W2 gated d | opt d | unsched d | total eng-weeks (core+gated) | risk-adjusted eng-weeks | engineers it needs (target: W2 done in about 16 weeks) |
|---|---|---|---|---|---|---|---|---|---|
| L1 GRANTS-BROKER | 26 | 0 | 5 | 15 | 4 | 0 | 9.2 | 11 to 15 | 1.5 (Go broker + model-gateway/registry) |
| L2 ADMISSION | 16 | 0 | 14 | 0 | 1 | 0 | 6.0 | 7 to 10 | 1 |
| L3 DRIVER | 22 | 3 | 34 | 0 | 5 | 0 | 11.8 | 15 to 20 | 1.5 |
| L4 ZDR-TENANT | 17.75 | 9 | 0 | 0 | 0 | 0 | 5.3 | 6 to 8 | 1 (W1 only; tail gated) |
| L5 SCRAPE | 24 | 0 | 44 | 0 | 4 | 22.5 | 13.6 | 17 to 23 | 1.5 |
| L6 SPA-GATEWAY | 6.5 | 0 | 60 | 0 | 2 | 0 | 13.3 | 17 to 24 | 2 |
| L7 CRAWL-JOBS | 1.5 | 0 | 72.5 | 15 | 3.5 | 0 | 17.8 | 23 to 32 | 2 |
| L8 PLANNER-MP | 0 | 0 | 69 | 0 | 0 | 0 | 13.8 | 18 to 25 | 2 |
| L9 CONTROL-PLANE | 0 | 0 | 0 | 25 | 0 | 0 | 5.0 | 6 to 9 | 1 (shares a Control Plane engineer with L15 billing-core) |
| L10 SHARED-BROWSER-QUARRY | 0 | 0 | 30.5 | 0 | 5 | 0 | 6.1 | 8 to 11 | 1 |
| L11 PROFILES | 0 | 0 | 37.5 | 0 | 3 | 0 | 7.5 | 10 to 14 | 1 |
| L12 TRACE | 0 | 0 | 33 | 4.5 | 0 | 0 | 7.5 | 10 to 14 | 1 |
| L13 WEB-POLICY-QUARRY | 0 | 0 | 17 | 3.5 | 0 | 0 | 4.1 | 5 to 7 | 0.5 (pairs with L14) |
| L14 REDACTION | 0 | 0 | 13 | 0 | 2 | 0 | 2.6 | 3 to 5 | 0.5 (pairs with L13) |
| L15 CAPACITY-METERING | 0 | 0 | 17.5 | 0 | 0 | 0 | 3.5 | 5 to 6 | 1 |

Lanes whose serial sum exceeds the roughly 65 working days of a 13-week W2 need a second engineer or a split: L7 (crawl branch versus jobs branch), L8 (planner versus trace persistence), L3 (WS12.B/F, WS13.C/G, WS8.E share the driver with WS3), L5 (WS5/WS13 versus WS16), L6 (SPA versus gateway halves). Staffing totals of the right-hand column are about 17.5 engineers for the W2 lanes if every lane ran flat out for the whole wave; at 12 engineers L13/L14 share one person, L9 and L15 share one, and L4 and L2 hand their W1 people to L10 and L12 once W1 closes.

### 5.4 Per workstream (cross-check against the designs)

| WS | Title | core d | gated d | opt d | unsched d | Design stated | Note |
|---|---|---|---|---|---|---|---|
| WS1 | Grants and delegated bearer | 26 | 0 | 0 | 0 | about 26 (16-18 working days with two engineers) |  |
| WS2 | Quarry run control | 16.5 | 0 | 1 | 0 | 16-18 |  |
| WS3 | Browser driver stability and actionability | 36 | 3 | 5 | 0 | about 44 | includes WS3.K (W2-timed) and sibling days of WS3.J |
| WS4 | ZDR and tenant boundary | 17.75 | 9 | 0 | 0 | about 27 |  |
| WS5 | Scrape contract and file routing | 24 | 0 | 4 | 13.5 | 24 (28 with the optional sandbox) | WS5.I (13.5 d) is unscheduled |
| WS6 | Budgets, capacity, metering | 42.5 | 0 | 0 | 0 | about 47 (includes sibling days) | plan total excludes about 5.5 sibling days |
| WS7 | Web policy / ResolvedPolicy | 24.5 | 24 | 4 | 0 | about 52 (21.5 owner-neutral + 31) |  |
| WS8 | Shared browser backend | 54.5 | 8.5 | 5 | 0 | 63 to 68 |  |
| WS9 | Shared browser frontend | 47 | 0 | 0 | 0 | about 47 scheduled |  |
| WS10 | Run trace envelope | 43 | 4.5 | 0 | 0 | about 47.5 |  |
| WS11 | LLM planner default-on and memory guard | 29.5 | 0 | 0 | 0 | F-t2-010 about 26 + F-t2-045 3.5 |  |
| WS12 | Secret redaction | 27.5 | 0 | 2 | 0 | about 20.5 text + 7 screenshots + 2 optional |  |
| WS13 | Scrape-time actions, screenshot and PDF | 33 | 0 | 2 | 0 | about 33 (+2 optional) |  |
| WS14 | Crawl scope and robots | 30.5 | 6 | 0 | 0 | about 34.5 | plan figure includes the +2 Control Plane days of WS14.H |
| WS15 | Job lifecycle and schedules | 33 | 9 | 3.5 | 0 | about 46 |  |
| WS16 | Structured extraction | 38 | 3 | 0 | 9 | 41 without S9 | WS16.J unscheduled |
| WS17 | Profile hardening | 37.5 | 8 | 3 | 0 | about 46 |  |

The epics of W3, W4 and W5 (48 items, one parked) carry their own estimates in the epic documents another agent is writing; nothing here includes them. Unscheduled design-level work listed in section 2.1 adds about 22.5 days to W1/W2 if it is pulled in, plus the D6 L-series (about 26 days) and F-t2-024 E1 to E4 (20 to 30 days).

<!-- part end -->
