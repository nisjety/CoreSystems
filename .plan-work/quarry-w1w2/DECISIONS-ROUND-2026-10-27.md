# Decision round 2026-10-27: Quarry v2 and the in-app browser

Date: 2026-10-27 (written 2026-10-28). Branch `quarryv2-next`. Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md`.

On 2026-10-27 the owner asked for a recommendation on each open decision: X-10, X-12, D16 Q2, X-19, X-9 and ADR-6, plus the new decisions from the depth round (STATUS-2026-10-24.md section 7.5 item 2). Each item was handled in two steps. First, one agent wrote a brief and checked it against the code, not only the notes. Then an independent challenger tried to prove the brief wrong. The recommendation below is the final one. Where the challenger amended or reversed the brief, the challenger's version is used, and each section says plainly where the brief was wrong. **Nothing changes in code, flags or compose until the owner says yes.** All of this is still dark: Wave 1 and most of Wave 2 are built, sit mostly behind flags that default off, and nothing has been proven on a live stack.

## 1. Summary

| Id | The question in one line | Recommendation (final) | Confidence | Reversible | Who else must agree |
|---|---|---|---|---|---|
| X-10 | Should Quarry obey robots.txt (observe to enforce)? | Yes, after an observe soak. Unreachable robots.txt counts as disallow. Go criterion is a per-decision ratio, and the soak ends on volume, not after 7 days. Fix the cache-before-check gap (D2-161) and test /v1/map on the enforce leg before the flip. Switch it on through the production compose env. | High | Yes (env + restart) | Nobody to switch on. Counsel only before any "ignore robots" override (WS14.S7). Ops reads the soak. |
| ROBOTS-RFC | Does a blank line between User-agent lines split a robots group? | Yes, accept e5e14bb6c (no split, RFC 9309). Before the soak, fix three parser gaps: leading BOM, Crawl-delay ending the agent list, empty Allow. Keep percent-encoding (D2-184) separate. | High | Yes | Nobody |
| X-12 | Turn on agent-run budget enforcement, with which numbers? | Yes to the run-budget numbers (60 steps / 900 s / 1 USD; ceilings 300 / 3600 s / 20 USD; 0.00003 USD/s as an estimate only), to people being uncapped, and to takeover steps being exempt. Flip to `on` only after the edge guard, the integration test and one scripted stack proof exist. The Model Plane 402 mapping follows but does not block. | Medium | Yes (env + restart) | Model Plane (402 mapping, follow-up, not a blocker) |
| TAKEOVER-BUDGET | Do a person's steps during a takeover spend the agent's budget? | No, keep the exemption as built (7d78ab2e8). Add a counter of exempt takeover steps and seconds. Record that it relies on agents using service tokens (ties to MP-GATEWAY-AUTH / D2-012). | High | Yes | Nobody |
| D16-Q2 | Who owns a saved browser profile, and when can owner checks be enforced? | Yes to option C: stamp the owner at run start, **human users only**. Model-driven runs never create an owned profile. The stamp honours deletion tombstones. Default scope is user_private everywhere. Legacy private rows stay hidden, not deleted. Count legacy rows on the live stack before enforce. | Medium | Yes (flag) | Nobody. Model Plane is informed. |
| X-19 | What should a stored step receipt keep of typed text by default? | `sensitive` (keep ordinary text, mask secrets). Four things must be true before receipts go on for real users: the event follows the same flag, the key-by-key password gap (D2-065) is fixed, receipts have a retention or erasure path, and the stack evidence passes. | Medium | Forward only: rows already written keep their text | Nobody. Counsel only for customer wording. |
| X-9 | What happens to schedules and monitors when an org turns ZDR on later? | Yes to the event-driven route: quarry-control consumes Control Plane's ZDR-enabled event. No cross-tenant read scope for Quarry. Control Plane must also emit the event when a plan change turns ZDR on. Build the monitor half and the follow=true refusal (D2-057) first, after the stack session. The executable-schedule half and S3 to S5 wait. | Medium | Yes (own flag, starts in observe) | Control Plane (consumer row, plan-change event). Counsel before enforce. |
| ADR-6 | Ratify declarative scrape actions, and where may the flag go on? | Ratify ADR-6: closed, capped action lists and no Evaluate. Rewrite the X-20 gates in the same ruling. `user` mode only on the dev/evidence stack, against fixture or owned hosts, after the core proof. Production stays off. | High | Yes | Nobody now. Billing, Control Plane and counsel before any production use. |
| MP-GATEWAY-AUTH | Should the Model Plane get its own credential to the verevonv3 gateway (D2-012)? | No. Quarry decides who the agent is, from the verified token. Before D2-012 is closed, Quarry must also refuse a reload from an agent. | High | Yes | Nobody |
| OPENAPI-NULL | How do we unblock SDK regeneration? | Change the one bare `type: 'null'` (openapi.yaml:3198) to `type: [string, 'null']`. Keep generator 7.22.0. Commit the spec and the regenerated docs together. Do this before any other spec change. | High | Yes | Nobody. Frontend is informed. |
| BILLING-IDEMPOTENCY | Do client retries double-bill, and where is it fixed? | Fix crawl/batch only: key by Control's job id, do not bill a replay (Control answers 200), stop billing failed handoffs. Leave billing-core alone. Sync-route retries stay billable until real idempotency (stored response replay) exists. Do not build the Redis scheme. | High | Yes | Nobody. The billing owner is informed. |
| DATAPLANE-OPTION-B | Land Data Plane rerank "option B" here, change compose defaults, or both? | Commit a real flags-off compose now: the four flags set to 0, and MODEL_PLANE_URL opt-in (empty by default). Keep the QB-T17 test pending for the four ungated features. Option B lands later, from main, and is not enabled until the search_suggest ZDR gap and the retention of the data-plane token have been reviewed. | Medium | Yes | Nobody now. Data Plane, Control Plane and counsel before option B is enabled. |
| OPENAPI-INTERNAL | Do /v1/internal/* routes belong in the public openapi.yaml? | No. Now: move run_page and the 8 edge internal routes to `docs/openapi-internal.yaml`, delete the internal files already shipped in the SDKs, and add a test that keeps /v1/internal out of the public spec. The 8 Go control routes follow with the D2-050 secret split. | High | Yes | Nobody. The D2-050 owner reviews the second half. |

Every brief was amended by its challenger except OPENAPI-NULL, which was upheld.

## 2. Suggested order

The dependencies decide most of the order.

- OPENAPI-NULL must land before OPENAPI-INTERNAL and DATAPLANE-OPTION-B, because both touch openapi.yaml or the generated SDKs. CI (`spa-verify`) will fail on the first PR to main until it lands.
- ROBOTS-RFC (parser fixes) must land before the X-10 soak starts, so the soak measures the final parser.
- TAKEOVER-BUDGET (the counter) and the X-12 edge guard should land before the X-12 shadow review, so the review can see takeover use.
- MP-GATEWAY-AUTH and TAKEOVER-BUDGET share one residual: an agent on a person's session token. Settle them together.
- X-10 and X-9 touch each other: scheduled crawls with no stored scope bypass the robots gate (D2-318). That only matters once scheduled jobs go live, which X-9 keeps off.

**A. Yes today. Code only, no stack needed. Each item is small.**
1. OPENAPI-NULL: a one-line spec change, then regenerate and commit (0.5 to 1 day). This unblocks CI.
2. OPENAPI-INTERNAL, first half (about 1 day, after step 1).
3. ROBOTS-RFC: three parser fixes plus tests (about 0.5 day).
4. MP-GATEWAY-AUTH: Quarry refuses an agent reload, and D2-012 is closed by decision (about 0.5 day).
5. TAKEOVER-BUDGET: ratify, add the counter, correct the notes (about 0.5 day).
6. DATAPLANE-OPTION-B: flags-off compose with MODEL_PLANE_URL opt-in (about 0.5 day). First check which callers expect `/v1/answer` to degrade rather than return 501.
7. BILLING-IDEMPOTENCY: the crawl/batch slice (about 1 to 1.5 days).
8. ADR-6: the ruling, the X-20 gate rewrite in the plan, and a `:-off` line in both compose files (no code).

**B. Yes to the direction today. Build now, flip only after the stack-evidence session (STEP 2, `L2-signed-in-steps.md`).**
9. X-12: edge guard, `agent_run_budget.rs` and a scripted stack proof, then `on` (about 3 to 3.5 days plus the stack).
10. D16-Q2: stamp at run start (humans only, honours tombstones) and the scope default. Then the observe week and a legacy count on the stack, then enforce (about 3 days plus a week).
11. X-10: the D2-161 fix and the composer row "N pages blocked by robots.txt". Then the e2e observe and enforce legs, the map check, and the volume-based soak. Then enforce (about 4 to 6 days plus the soak).
12. X-19: make the event follow the flag and fix D2-065 (about 3.5 days). The retention or erasure path needs its own small design. The receipts stack evidence runs in STEP 2.
13. ADR-6 proof: `scrape_actions_live` and the real-token test, run on the evidence stack after SC1 to SC7.

**C. Waits for another plane, counsel, or go-live.**
14. X-9: the Control Plane ask (consumer row plus the plan-change event). Then build the monitor half after the stack session. Counsel decides prospective versus retroactive ZDR before enforce. The executable-schedule half and S3 to S5 wait until scheduled jobs head for go-live.
15. OPENAPI-INTERNAL, second half: together with the D2-050 internal secret split.
16. DATAPLANE option B itself: after Data Plane commits QB-T05a on main, plus the counsel and ZDR review.
17. The X-12 402 mapping in the Model Plane, and any Model Plane opening for scrape actions (ADR-6 option D): Model Plane session.

## 3. Decisions in detail

### 3.1 X-10: robots.txt enforcement (WS14.S8)

**Question.** Should Quarry obey robots.txt when crawling (turn the gate from observe to enforce)? And how should an unreachable robots.txt be treated?

**Current state.** The gate is built and wired for every crawl and batch page (routes.rs:1704-1710). It is checked again after redirects (pipeline.rs:827-830, 954-958), and robots.txt is fetched through the SSRF-guarded driver chain. The default mode is `observe` (robots_gate.rs:73-80): the edge fetches robots.txt, counts `would_deny` and blocks nothing. Enforce needs only `QUARRY_EDGE__CRAWL_ROBOTS=enforce`, and no compose file sets it. An unreachable robots.txt (5xx, timeout, 429) defaults to disallow and is cached for 5 minutes (robots_gate.rs:104-111, 350-358; robots_cache.rs:36-40). A denied page is not metered (routes.rs:1731-1754). Gaps:
- A cached page is served and metered before the robots check (routes.rs:1717-1728, D2-161).
- Scheduled crawls with no stored scope, single scrape, extract, search and agent browsing are not checked (D2-318, D2-161).
- The composer row "N pages blocked" is not built.
- The override (`ignore_robots_txt`) is not built and always returns 403 (crawl_request.rs:425-433).
- The e2e harness `crawl-scope-robots.sh` has never run.

**Options.**

| Option | What it means | Effort | Main risk |
|---|---|---|---|
| A. Observe forever | No change | 0 d | We say we respect robots.txt but do not |
| B. Soak, then enforce robots only, unreachable = disallow, via env | Planned path | 4 to 6 d plus the soak | Pages blocked by a flaky robots.txt, with no UI explanation |
| C. Enforce now | Flip on the next deploy | 1 d | Never run on a stack; crawls shrink silently |
| D. As B, but unreachable = allow | Firecrawl's posture | As B | Weaker RFC position; a down site gets crawled |

**Recommendation.** Option B, with the challenger's three changes:
1. The go criterion is a per-decision ratio: would_unreachable and would_deny as a share of all robots decisions. Alternatively, first add a small origin-level signal and count origins.
2. The soak ends on volume (enough crawls across enough distinct sites, topped up by an internal crawl of a known site list if needed), not after a fixed 7 days.
3. The D2-161 cache-before-check fix ships before the flip. /v1/map is checked separately on the e2e enforce leg.

Scope mode, the ignore-robots override, and robots checks for scrape and agent browsing are **not** part of this yes. Treating 429 as unreachable is stricter than RFC 9309, which reads every 4xx as "may crawl". It is a deliberate choice, and Google does the same. Why this route: not honouring robots.txt is the largest standing legal and trust exposure in this feature. The mechanism is built and reversible by restart. Only operational data is missing, and only a soak produces it.

**If accepted.**
1. Run the e2e observe and enforce legs, including the 503 and metadata-redirect legs, in the STEP 2 stack session.
2. Fix D2-161 (check robots before the cache read). Build the nb/en composer row. Fix RUNBOOKS (around line 1836).
3. Deploy in observe and soak until the volume target is met. Write the read-out into the PR.
4. If the go criterion holds, add `QUARRY_EDGE__CRAWL_ROBOTS: enforce` and `QUARRY_EDGE__ROBOTS_UNREACHABLE: disallow` to `docker-compose.production.yml` in one revertible commit. Scope mode stays at observe.
5. Watch `page_blocked` for a week. Change the Rust default a release later, if at all.

**Challenger verdict: amend.** Where the brief was wrong:
- Its main go criterion ("under 2 percent of distinct origins") cannot be measured. The counter has only `mode` and `outcome` labels, never a host (metrics.rs:513-533), and the observe log has no origin.
- It put /v1/map in the yes, but map consults the gate only under enforce (map_routes.rs:187-221). The soak therefore gives no map data. Under enforce, one unreachable robots.txt fails the whole map request with 403.
- Calling the policy "RFC 9309 rules" is slightly loose, because 429 is handled more strictly than the RFC.

**Open risks.**
- A 5xx robots.txt stops an origin for 5 minutes at a time.
- Scheduled crawls with no scope bypass the gate (fix before schedules go live, WS14.I before WS15).
- The product token (`quarrybot`, `quarry`) does not match the advertised user agents, and the token question is still open (F-t2-067).
- Map failures under enforce are all-or-nothing.

### 3.2 ROBOTS-RFC: the blank-line rule

**Question.** Does a blank line between two User-agent lines split the group? Should commit e5e14bb6c, which says no, stand?

**Current state.** Since e5e14bb6c, `parse()` skips blank lines, and only a rule line ends the user-agent list (crates/quarry-transform/src/robots.rs:30-51). It reversed 67e3b3d3e, whose own message said it was not checked against the RFC. The old reading failed open: "User-agent: quarrybot / blank / User-agent: * / Disallow: /" allowed quarrybot everything. Tests pin the new rule (robots.rs:349, 358, 395). map_routes.rs:227-229 uses the same parser. The gate is in observe, so nothing live changes.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Keep e5e14bb6c, change nothing else | 0 d | Edge cases under-block later |
| B. Keep it and close the small parser gaps before the soak | 0.5 d | Low |
| C. Revert to "a blank line splits the group" | 0.5 d | Fails open: legal and trust problem |
| D. Wait for counsel | Waiting time | Delay with no new information; this is not a legal question |

**Recommendation.** B, in the challenger's form. Accept the RFC reading. Before the X-10 soak, fix three gaps, each with a test:
- Strip a leading UTF-8 BOM.
- Crawl-delay must not end the user-agent list.
- An empty `Allow:` must end the list.

Leave percent-encoding normalisation (D2-184) as its own item. The RFC 9309 section 2.2 grammar allows empty lines between user-agent lines, so the text decides this. Nothing is enforced today, so the fix costs nothing now.

**If accepted.** Record in the ledger that the W2-U63 note is withdrawn. Edit robots.rs around lines 58-87 and add a strip for the BOM. Add tests, including a small corpus of real file shapes (BOM, CRLF, comments, Crawl-delay first). Run the robots tests in quarry-transform, quarry-runtime and quarry-edge. Add one line to `docs/CRAWL_SCOPE_ROBOTS.md`.

**Challenger verdict: amend.**
- The brief missed a real fail-open defect. No robots code strips a leading BOM (grep found none), and Rust's `trim()` keeps U+FEFF. A file that starts with a BOM therefore loses its first `User-agent: *` line, and "disallow everything" becomes "allow everything". This is more important than the two edge cases the brief listed.
- The brief also folded D2-184 into the same pass, which is scope creep.

**Open risks.** Agent matching is an exact lowercase comparison, so `User-agent: QuarryBot/1.0` does not match. This is minor and belongs in the corpus test.

### 3.3 X-12: agent-run budget enforcement (WS6.B), with D2-014

**Question.** Should `QUARRY_EDGE__BUDGET_ENFORCEMENT` move from `shadow` to `on`, with the placeholder numbers ratified? Should a person's own step cap lift as part of it?

**Current state.** The switch has three modes and defaults to `shadow` (budget.rs:51-95; docker-compose.yml:234). In shadow it refuses nothing and only logs and counts would-exhaust (budget.rs:960-1000).
- D2-014 is resolved in code: shadow keeps every run's request step cap, and only `on` lifts it (budget.rs:1139-1144, commit 03b35e1c0, test tests.rs:513).
- Gate order is already built: budget is gate 9, before the sensitive approval at gate 10 (admission.rs).
- Org allowance (billing-core) is not built, so that dimension cannot fire.
- `crates/quarry-edge/tests/agent_run_budget.rs` does not exist.
- The only live proof is from 2026-10-16, on older code.
- **Hole found in code:** `on` lifts the legacy cap for every run that has a budget, with no input for initiator or class. A model-proposed step (`initiated_by agent`) inside a person-class session therefore loses its only bound, the 12-step cap the gateway sends (gateway browser.rs:4617-4628, 4902-4912). The test at tests.rs:513-545 pins this behaviour.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Stay in shadow | 0 d | A direct API caller that sends max_steps 0 is unbounded |
| B. Flip to `on` now | 0.1 d | Removes the bound on agent steps in a person's session (a regression); no live proof |
| C. Ratify, add the guard, write the test, run the stack proof, then `on` | About 3 to 3.5 d (Quarry plus stack) | Low |
| D. As C, but keep a person's 12-step cap | About 4 d | A 12-action wall for people stays |

**Recommendation.** C, as the challenger amended it. Yes to:
- The run-budget numbers as env-tunable starting values: 60 steps / 900 s / 1 USD; ceilings 300 / 3600 s / 20 USD; 0.00003 USD per browser second as an estimate, not a price.
- People are uncapped on steps.
- Takeover steps are exempt.

Flip to `on` once three things exist: the edge guard (an agent-initiated step on an interactive run keeps a bound), the `agent_run_budget.rs` integration test, and one scripted stack proof (plan WS6.B deployment evidence steps 1 to 5). The Model Plane mapping of the typed 402 follows as soon as it can be scheduled, but it does not block the flip. The job and crawl numbers (1000 pages, depth 5, 3 concurrent jobs, backlog 200) are **not** part of this yes. They belong to D2-082.

**If accepted.**
1. Guard in admission.rs and budget.rs (`legacy_step_cap_applies` gets the initiator and the class). Change the pinned test at tests.rs:513, do not just add to it.
2. Write agent_run_budget.rs.
3. Stack session: a scripted day in shadow, then steps 1 to 5 with `on` and `AGENT_DEFAULT_MAX_STEPS=5`.
4. Set `on` in the Ingestion env. `off` is the documented rollback.
5. Model Plane: map the 402 to a resource-limit terminal with partial results.
6. Keep `GRANT_BUDGET_CLAIM` at `ignore`, which is a separate decision.

**Challenger verdict: amend.** Where the brief was wrong:
- It said "an AI run that sends no limit is unbounded" and left the Model Plane unchecked. The Model Plane does bound its own runs: tool_bridge defaults max_steps to 20, execution-core enforces it (browser_agent.rs:321) and forwards it to Quarry, and plan.rs has MAX_STEPS 100. The real exposure is a direct API caller that sends 0, so flipping is less urgent.
- It made the Model Plane 402 mapping a hard gate. That is unnecessary, because MP runs rarely reach the steps dimension.
- It bundled the job and crawl numbers into the yes.
- A 24-hour shadow review is not meaningful with no real agent traffic. A scripted day is enough.

**Open risks.**
- Runtime or cost 402s show up as failed steps in the Model Plane until it maps them.
- The would-exhaust metric does not count the legacy 429.
- Org allowance stays unconfigured.

### 3.4 TAKEOVER-BUDGET: a person's steps during a takeover

**Question.** When a person takes over an agent-class run, should their steps spend the agent's step budget?

**Current state.** The notes are stale, and the code answers the question. Commit 7d78ab2e8 (2026-10-27 16:02) made a person's step during human takeover never count (human_browse.rs:139-153), and the call sites pass the flag (admission.rs:421-422, 486; step.rs). Takeover steps are not charged (step.rs, `if counts_budget`), and the runtime clock is paused (control.rs:101-135). A person who steps without first taking over an agent-class run still counts. There is no flag. STATUS 7.4/7.5 and `depth/D-D-B1-agent-policy.md` item 1 still describe the old behaviour.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Ratify the exemption as built | 0.25 d (notes) | Unmetered takeover time |
| B. Count takeover steps (revert) | 0.25 d | People are locked out of their own capped run |
| C. Exempt, but meter takeover separately | 3 to 5 d | A new cap with no data to set it |
| D. Exempt every human step whatever the run class | 0.25 d | Reopens the R-8 hole; not recommended |

**Recommendation.** A, with the challenger's two conditions:
1. Add a counter of exempted takeover steps and seconds per run, so the X-12 shadow review can see takeover use.
2. Record that the exemption relies on agents using service tokens, and settle that together with MP-GATEWAY-AUTH (D2-012).

The cap exists to bound the agent, not the person who steps in to supervise it.

**If accepted.**
1. No change to the rule.
2. Add the counter.
3. Correct STATUS 7.4/7.5, D-D-B1-agent-policy.md item 1 and D2-207. Add one sentence to CONTRACTS.md.
4. In the stack session, check that a capped run can be taken over and that the agent's budget is unchanged on hand-back.

**Challenger verdict: amend.** Where the brief was wrong:
- It said "only a person can set takeover, so no security risk". In fact control.rs treats any token that is not a service token as a person, and it ignores the hint. An agent on a person's session token (UserSession basis, run_live.rs:156/178) can switch to takeover and then step without limit. The hole is narrow, because the same token could open an uncapped interactive run anyway, but it is real.
- It said takeover is bounded only by the 1800 s idle TTL. Agent-class runs keep their absolute TTL too (run_live.rs:364-376), which is tighter than the brief claimed.

**Open risks.** The UserSession bypass stays until agents always use service principals. On interactive runs, `viewers > 0` suppresses expiry.

### 3.5 D16-Q2: profile ownership (WS17.P2), with D2-044

**Question.** Who owns a saved browser profile? What happens to old profiles with no owner? How do profiles that a run creates get an owner? Which scope does a profile saved without metadata get? These answers decide whether owner checks can move from observe to enforce.

**Current state.** The owner check is wired on every `/v1/profiles*` route and on run attach (commits 2130d341a, 911d69b95, e46bb66cb). The default is `observe` (config.rs:393; profile_guard.rs), so it only logs, and today any org member can attach another member's private profile.
- `create_profile` stamps an owner (profile_routes.rs:377). RUNBOOKS and D2-044 are stale on this point.
- Profiles a run creates, which is the main persistent-session path, are saved with owner None (chromiumoxide.rs:1842, 1943; kernel.rs:593). BrowserLease has no actor field.
- Under enforce, the creator would lose their own profile.
- Default scope: Postgres gives user_private (migration 0005), while the in-memory store reads back ephemeral (session.rs:577).
- Migration 0015 applies at the next edge boot. Every existing row is then ownerless.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Leave in observe | 0 d | Private is not private |
| B. Enforce now as built | 0.5 d | People lose their own saved logins |
| C. Stamp at run start, user_private default, observe week, then enforce; hide legacy rows | About 3 d plus a week | Low |
| D. Actor id on BrowserLease, stamp at save | 3 to 4 d, cross-crate | Delivery risk; does not fix S3 |

**Recommendation.** C, with the challenger's changes:
- Stamp the owner at run start **only when the caller is a human user**. A model-driven (service-principal) run must not create an owned persistent profile until an acting-user claim exists. Under enforce it is refused, or the row stays ownerless and hidden.
- The insert-only stamp must honour `quarry_profile_tombstones`, as `save()` does.
- A profile saved without metadata is user_private in every store.
- Legacy private rows stay hidden, not deleted.
- Legacy org_shared rows stay open to the org.
- Members may save into org_shared profiles (last writer wins on the cookie jar). Only the owner may rename, rescope or delete.
- Count the legacy ownerless rows on the live stack before enforce. Then choose between a manual, org-confirmed rescue and a backfill from run records (user actors only).

**If accepted.**
1. lifecycle.rs: after the ZDR refusal and the attach guard, stamp insert-only with a tombstone guard, user principals only.
2. session.rs: scope default.
3. cached_profile_store.rs: fix the test.
4. profile_attach.rs: run-path tests, including the service-principal and ZDR cases.
5. Fix RUNBOOKS, D2-044 and DB2P-01.
6. Rebuild so 0015 applies. Run the legacy count, observe for a week, then rescue or backfill, then enforce. Run the two-user evidence.

**Challenger verdict: amend.** Where the brief was wrong:
- C as written stamps `claims.actor_id()`, which for a model-driven run is the shared service id (auth.rs). Every model run in the org would then "own" those cookies, and the human could not see them. That recreates the hole C is meant to close.
- The stamp ignored tombstones, so a deleted profile could come back.
- "Legacy volume is probably small" has no evidence behind it. The gateway's default persistent flow has always created ownerless rows.

**Open risks.**
- A run that fails leaves an empty placeholder row.
- Model-driven runs cannot use a user's profile until WS8.K or an acting-user claim exists.
- Legacy owners lose access until they are rescued.

### 3.6 X-19: receipt text default (WS12.S3)

**Question.** What should a stored step receipt keep of what an agent typed or selected? Ordinary text, such as a search query (`sensitive`, today's default)? Or should every typed text be replaced (`all`)? Secrets are masked in both modes.

**Current state.** Masking is built and on. `QUARRY_EDGE__RECEIPT_TEXT_MODE` defaults to `sensitive` (redaction/action.rs:49-58, set once per process; redaction_boot.rs). No compose file sets it.
- Passwords, OTPs, cards, phones and labelled secrets are always masked. An unknown control kind on a ref-addressed typing action is masked as a secret.
- A person's own steps are metadata-only by default. ZDR runs write no receipts.
- Under `all`, a compiled procedure reports `repair_required` for every typed step (browser_procedure.rs:116-140).
- The rule version is `qr-redact/4`.

Known gaps:
- A password typed one key press at a time is stored raw (D2-065, high).
- The ActionStarted event ignores the flag (observation_redaction.rs:310-334, D2-210).
- Nothing has run on a stack (D2-135, D2-244).

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Leave the default unconfirmed | 0 d | The default becomes "approved" by drift |
| B. Confirm `sensitive`, with conditions | About 2 d, plus D2-065 (about 3 d) | Ordinary typed personal data is kept |
| C. Switch the default to `all` | 2 to 3 d | Recorded procedures can no longer be replayed |
| D. Per-org policy | 8 to 12 d across planes | New config surface; not asked for |

**Recommendation.** B, as the challenger amended it. Choose `sensitive`. Before agent receipts are turned on for real users, all four must be true:
- (a) The ActionStarted event follows the same flag.
- (b) D2-065 (key-by-key password presses) is fixed.
- (c) Receipts get a retention period or an erasure path, so typed personal data is not kept forever.
- (d) The STEP 2 stack evidence passes: psql on `quarry_step_receipts`, a hand-inserted legacy row, and the procedure `redacted_input` check.

With `sensitive`, recorded procedures stay replayable, and secrets are masked either way.

**If accepted.**
1. Record the answer in the plan (X-19 row), W2-closeout and RUNBOOKS.
2. Make `action_for_event` read `record_policy()`.
3. Add a counter for dropped observations.
4. Sync SECRET_REDACTION.md and CONTRACTS.md, and fix the stale `qr-redact/3` mention.
5. Schedule D2-065.
6. Write a short design for retention or erasure, probably through the Control Plane GDPR flow.
7. Run the stack evidence.

**Challenger verdict: amend.**
- The brief rated legal risk as low to medium without checking retention. I found no retention period, TTL, purge or erasure path for `quarry_step_receipts` (migrations 0007 to 0018).
- The brief called the default "reversible", but tightening is one-way for data already written. Read-time masking does not re-mask rows that carry the current stamp. A yes to `sensitive` is therefore permanent for whatever is written before (c) exists, which is why the conditions gate turning receipts on and are not follow-up work.
- The event leak matters more than the receipt.

**Open risks.**
- A raw `type` action on an opaque selector with no snapshot keeps its text.
- Legacy rows with no stamp are over-masked on read.

### 3.7 X-9: schedules of an org that turns ZDR on later (WS15.S3 to S5), with D2-057

**Question.** What happens to schedules and change monitors when an org turns on Zero Data Retention after they were created? Should the live job-event tail get a ZDR floor?

**Current state.**
- **Change monitors** are live. They stamp the ZDR posture at creation and never re-check it (RUNBOOKS.md:539-570, D1-006, D2-192). The only remedy is operator SQL.
- **Executable schedules** are fully dark: `QUARRY_SCHEDULED_JOBS_ENABLED=0` (docker-compose.yml:334). If they were switched on, preflight would always answer allow/unchecked (schedule_preflight.rs, zdr_matrix.rs:535-545). Executable schedules are sent with `zdr: None` (schedule_routes.rs:463, already noted at STATUS line 85).
- Refusal at create time is built but in observe.
- The plan's S4 (a fire-time lookup in org-core) would need `org:retention:read:any` for quarry-edge. Today only auth-core holds that scope.
- Control Plane already publishes a durable false-to-true event, `aqencia.controlplane.org.interactive_retention.enabled` (shared_publisher.go:179-188), and conversation-core consumes it. quarry-control already uses the same pattern for GDPR (gdpr/subscriber.go).
- **D2-057:** follow and the legacy drain have no ZDR floor, while `errors_for` does. Follow is off (no compose setting). The legacy drain is live.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Stay dark, with an operator routine | 0.5 d | Depends on someone remembering it every time |
| B. Plan D15: org-core lookup at each fire | About 4.5 d plus a Control Plane PR | A new cross-tenant read scope; org-core down means fires skipped |
| C. Event-driven: quarry-control consumes the ZDR event, stamps and pauses | About 4 d (Quarry plus Control Plane) | A lost event; the entitlement path |
| D. C now, B later as a safety net | C, then B only if needed | "Later" may be forgotten |

**Recommendation.** C for D15, with no `org:retention:read:any` for Quarry, as the challenger amended it:
- Make one condition part of the Control Plane ask: org-core also emits the event when a plan change turns effective ZDR on while the intent is already true.
- Build the monitor half (consumer, `MarkOrgZdr`, refuse on the stamp) and the follow=true refusal (D2-057) first, after the stack-evidence session.
- Defer the executable-schedule half and S3 to S5 until scheduled jobs actually head for go-live.
- D14 (pull model), D17 (notices) and D9 also wait.
- The legacy drain stays as the matrix documents ("reads pre-ZDR jobs as before"). Changing it belongs to ADR-4 stage B.

**If accepted.**
1. Control Plane: a consumer row in audit-core's provisioner.go and broker permissions for quarry-control. In org-core, emit the event from the plan-change transaction.
2. quarry-control: a consumer next to gdpr/subscriber.go and an idempotent `MarkOrgZdr`, behind the flag `QUARRY_CONTROL__ZDR_RETENTION_EVENT=off|observe|enforce` with default observe.
3. Edge: refuse `follow_job` under ZDR, with the same code as `errors_for`, plus a test.
4. Update matrix texts, the golden and the runbook. Gap rows stay Gap.
5. After a week in observe on a live stack, the owner decides on enforce together with ADR-4 stage B and the customer notice.

**Challenger verdict: amend.**
- The brief treated the entitlement gap as an edge case covered by "B later". In fact effective ZDR is plan AND intent (org-core repository.go:333-366), and a plan change emits no event. An org that set its intent earlier and then upgrades its plan gets ZDR silently, and C fails open exactly there. Closing this at the source is a small change on an existing outbox path.
- Building the executable-schedule half now is speculative, because the feature is dark and every `Zdr` value is NULL.
- The brief overstated how new the "unstamped schedules" finding is.
- The owner's order is green lanes, then the stack proof, then new builds. "Approve" therefore means approve the direction.

**Open risks.**
- Prospective versus retroactive ZDR: pausing a monitor does not remove baselines already stored, URLs in Temporal history, or change_detected events. Counsel must answer this before enforce.
- No handling for ZDR going true to false: paused schedules stay paused. Put this in the runbook.
- A cheap extra: the edge can lazily re-stamp schedules when it sees `zdr_floor()==true` on a request.

### 3.8 ADR-6 and X-20: scrape actions (WS13.5), with D2-023 to D2-025 and D2-069

**Question.** Should we ratify declarative, capped scrape action lists? Can `QUARRY_EDGE__SCRAPE_ACTIONS` go on, and where?

**Current state.** Built end to end, dark and unproven.
- Flag modes are off, user and on. Default is off by absence, since neither compose file has the line (scrape_actions.rs:150-187).
- `user` admits user tokens only. `on` also admits service principals that hold `scrape:interact`, and no principal holds it. A request labelled `initiated_by: agent` is refused in every mode (scrape_actions.rs:196-212).
- The wire, `data.actions[]` and safe mode are built (fc3cf2b19, 736c9ae35, 50c567dd7).
- Limits: 8 action types, 10 denied names (including evaluate), 50 actions, 60 s total wait.
- One heavy permit per process: a second request gets 429 at once.
- No billing meter and no per-org entitlement.
- `scrape_actions_live` does not exist. Only the docs name it.
- There is no consumer: the SPA does not expose it and the Model Plane does not send it.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Dark everywhere | 0 d | Unproven code stays in the tree |
| B. Ratify, `user` in dev and evidence only, write the proof | 2 to 3 d | Low |
| C. `user` in production | 6 to 9 d more (meter, entitlement, SDK, proxy) | Unmetered compute; third-party interaction |
| D. Open to the Model | 15 d or more across 3 planes | High |

**Recommendation.** B, as the challenger amended it.
- Ratify ADR-6: closed, declarative, capped lists. Evaluate stays denied. A future read-only evaluator would be a new action type, never a relaxed Evaluate.
- **In the same ruling, rewrite the X-20 gates.** `on` requires a verified Model caller (D2-012) plus an approval gate. `user` outside dev requires the live proof, a billing meter and a per-org entitlement.
- Allow `user` only on the dev and evidence stack, against the fixture site or owned hosts, with the default "report denied" egress.
- Run the proof after SC1 to SC7.
- Production stays off.

**If accepted.**
1. Ledger entry and gate rewrite in the plan (around line 3740).
2. Add `QUARRY_EDGE__SCRAPE_ACTIONS: "${QUARRY_EDGE_SCRAPE_ACTIONS:-off}"` to both compose files.
3. Write `scrape_actions_live`: post-click DOM, off-host denial, 429 while the permit is held.
4. Write the real-token agent-label test beside zdr_claim_floor.rs.
5. Run `make test-scrape-capture` on the evidence stack.
6. Confirm the cache and CAS are untouched.
7. Only then open option C as a separate decision.

**Challenger verdict: amend.** The brief claimed the plan already prescribes this order. It does not. The plan's written gates are weaker:
- "`on` only after ADR-6" means that a yes to ADR-6 would, on paper, open `on`.
- "`user` in dev until the agent-label test exists" is already met by a unit test that uses the dev auth bypass.

The ruling therefore has to rewrite these gates. The challenger also added three points: the egress choice (report denied), fixture hosts only, and sequencing after the core proof.

**Open risks.**
- The credential-field guard is a soft heuristic.
- Inline ZDR screenshots above the 6 MiB cap are omitted.
- The production front door must allow 125 s (option C only).

### 3.9 MP-GATEWAY-AUTH: Model Plane to verevonv3 gateway (D2-012)

**Question.** Should the Model Plane get its own credential to the verevonv3 gateway, so the gateway can force actor=agent for a service caller?

**Current state.** Commit ff496d2fd added `CallerClass` and `effective_actor()` in the gateway, but nothing produces `ServicePrincipal`. It is dead code (browser.rs:262-296), and the four dispatchers pass None. The gateway authenticates only Better Auth cookie sessions (middleware.rs:169-232), and its browser routes need a user and a cookie. The Model Plane never calls `/api/v1/browser` (a grep over apps/Model Plane found nothing). It goes to Quarry directly with a `browser:execute` service token (execution-core quarry_agent.rs). Quarry's `derive_initiator` forces Agent for any service token (run_live.rs:84-100).

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Leave the item open | 0 d | The item stays red forever |
| B. Close by decision: Quarry decides, no gateway credential | 0.5 d (plus the reload fix) | Low |
| C. Build an MP-to-gateway credential | 8 to 12 d, 3 planes | New attack surface on the public gateway |
| D. Remove the SPA agent-under-user call | 1 to 2 d | Loosens gates; a product change |

**Recommendation.** B, with the challenger's condition. Do not build a Model Plane credential to the gateway. Quarry decides who the agent is, from the verified token. **Before D2-012 is closed, Quarry must refuse a reload from an agent** (refuse `AgentAction::Reload` when `derive_initiator` gives Agent), with a test. That is about half a day. Reopen the item if a Model Plane to gateway browser path is ever planned.

**If accepted.**
1. Quarry: the reload refusal and a test.
2. List each gateway-only "agent may not X" rule and match it to a Quarry check.
3. Reword the doc comment at browser.rs:262-282 and the four D2-012 comments in dispatchers.rs.
4. Close D2-012 as "by decision, residual accepted". Record that the row's own suggested fix ("ignore body.actor, change the SPA") was rejected because it would loosen the gates.
5. Run the gateway cargo tests.

**Challenger verdict: amend.** The brief's reason ("Quarry gates everything that matters") is incomplete. The WS9.B reload refusal, one of the two harms D2-012 names, exists only in the gateway. Quarry accepts Reload from an Agent step. Only the Model Plane's tool schema keeps it out (contracts.rs:961-963; a grep of quarry-edge/src for Reload finds only budget labels).

**Open risks.**
- A person's session can still claim "human". No gateway design can detect that.
- The live-view socket still trusts the client frame actor. That is a person's channel and outside this decision.

### 3.10 OPENAPI-NULL: SDK regeneration is blocked

**Question.** Which spec shape or generator should Quarry use so that `sdks/generate.sh` works again?

**Current state.** Only **one** construct is the bare `type: 'null'`: ProfileCookieView.value at openapi.yaml:3198. The server always returns null there, and tests pin that (profile_guard.rs:213). The other quoted-null hits are type arrays, a form generator 7.22.0 already handled in d707acf60. On the 2026-10-27 attempt, Python aborted on that property, and TypeScript imported a missing `./Null` model. **This blocks CI:** `spa-verify` regenerates the SDKs, checks for drift, and runs the Python tests and the TS build (verevonv3-ci.yml:179-233). It will fail on the first PR to main. No frontend code uses the profile schemas.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Do nothing | 0 d now | spa-verify fails on the first PR |
| B. `type: [string, 'null']` on that one field | 0.5 to 1 d | Low |
| C. Remove `value` from the schema | 0.5 to 1 d | The spec no longer matches the wire |
| D. Downgrade to 3.0 or switch generator | 3 to 6 d | Breaks the SPA typings |

**Recommendation.** B. Keep the description ("Always null. A cookie value is never returned.") and keep the field required. Regenerate with the pinned 7.22.0. Commit the spec and the regenerated SDK docs together (25 changed, 38 new pages), so the drift check passes. If Python trips on the same_site enum at :3191, switch that field to the proven `nullable: true` plus enum-with-null pattern (as at :6123). Do this first among all spec changes.

**If accepted.** Edit :3198. Run `MSYS_NO_PATHCONV=1 sdks/generate.sh` under the docker lock. Then run `tsc -p sdks/typescript` with TypeScript 5.9.3, the 28 Python tests, and `node --test scripts/tests/*.test.mjs`. Commit everything in one commit. Proof comes when spa-verify passes on a real runner.

**Challenger verdict: uphold.** Small corrections:
- The count is 12 type arrays plus line 3198, and an unquoted enum null exists at :3191.
- A 14th null construct at :6123 (BudgetMeta.exhausted) already passed generation, which lowers the risk at :3191.
- The notes' "13 places" (D-D-B3-openapi-sdk.md:20) is wrong.

**Open risks.** `string | null` on a cookie `value` may look like "cookie values can be returned" to an outside reader. The description and the server tests must stay.

### 3.11 BILLING-IDEMPOTENCY: retries double-bill (D2-033)

**Question.** A client retry of a metered call produces a second billing event. Should we fix it now, and where: in quarry-edge, or in billing-core's dedupe contract?

**Current state.** billing-core dedupes on `event_id` and refuses a repeat as a conflict when org, metric, occurred_at or the payload differ (repository.go:399-419). It protects against NATS redelivery, not API retries. The Rust constructors always stamp `occurred_at = now` (usage.rs:66-139). Most emitters mint fresh ids, so a retry is a new event. **The legacy crawl seed and batch meters key by the client-suppliable X-Request-Id** (routes.rs:1127-1140, 1280-1282). A client that reuses one id is billed once, and ids can collide across tenants. That hole is live today. Control already dedupes crawl/batch by (org, Idempotency-Key) and answers 200 for a replay (resources.go:353-368). The new usage metrics flag is off.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Leave as is | 0 d | Double-billing and the suppression hole stay |
| B. Edge-only: job id for crawl/batch, Redis first-seen per key for sync routes | 4 to 6 d | Creates a new exploit (see verdict) |
| C. Change billing-core's dedupe | 6 to 10 d across planes | Changes the money ledger for every producer |
| D. Crawl/batch only | 1 to 1.5 d | Sync retries stay billable |

**Recommendation.** D, done properly, as the challenger amended it:
- Key the crawl seed and batch events by Control's job id, not the request id.
- **Do not bill a replay.** Skip the meter when Control answers 200, or use the job's `created_at` from Control. Do not use the edge clock.
- Stop billing failed handoffs on the legacy path (`else if !queue_full`).
- Leave billing-core alone.
- A sync-route retry re-does the work, so it stays billable until real idempotency exists (a stored response replayed, plus a body fingerprint). Do not build the Redis first-seen scheme.

**If accepted.**
1. routes.rs around 1116-1140 and 1266-1285: key by job id, and skip on replay. The edge needs the HTTP status or Control's `created_at`, because ControlJob in handoff.rs:321-329 drops it today.
2. Stop the meter on failed handoffs.
3. Tests: replay gives no second event; a failed handoff gives no event; two orgs never collide.
4. Fix the BILLING.md event-id text and the D2-033 row (DEPTH-FINDINGS-W2.md:62 still says "key by request id").
5. Tell the billing owner, as information only.

**Challenger verdict: amend.** Where the brief was wrong:
- Step 1 relied on `ack.accepted_at`, which is the edge's own `now()` (handoff.rs:279-286), so it would not dedupe anything.
- Step 2 creates a new exploit: without response replay, a fixed Idempotency-Key gives repeated synchronous work billed once or not at all.
- "Double-bill" is the wrong framing for sync routes, because a retry costs Quarry real work.

**Open risks.** Nobody has established which legacy metrics reach invoices today. If they do, finance may want a look-back at past over- and under-billing from the request-id key.

### 3.12 DATAPLANE-OPTION-B: docs-contract todos and QB-T17 compose defaults

**Question.** The Quarry docs-contract test has 12 todo assertions. 11 wait for Data Plane "option B" (uncommitted in the main checkout only), and 1 pins the QB-T17 compose defaults. What should we do?

**Current state.** The todos fail nothing: the file exits 0 and is not in CI. The committed compose defaults `AUTOPROMPT` and `SEMANTIC_RERANK` to 1 (docker-compose.yml:178, 183), while Rust defaults them to false (config.rs:363, 365). Every Model Plane call is refused today, because quarry-edge has no model-gateway audience. Option B is half there: the Data Plane authz row and the Control Plane `data:rerank` grant are committed, but `/v1/rerank` has no routed handler (QB-T05a), and the Quarry client exists only in the live tree.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Leave as is | 0 d | Compose contradicts "flags stay off" |
| B. Flags-off compose, option B stays pending | 0.5 d | Very low |
| C. Port option B onto this branch now | 3 to 5 d plus QB-T05a | Regression risk; paid Cohere; ZDR |
| D. Owner commits option B on main, then cherry-pick | 2 to 4 d | Slower but safer |

**Recommendation.** B, as the challenger amended it, with three points:
1. Turn off **all** Model Plane traffic: AUTOPROMPT, SEMANTIC_RERANK, LLM_CLASSIFY_INTENT and PAGE_TITLE_ENRICH set to 0, **and** MODEL_PLANE_URL opt-in (empty by default).
2. Keep the QB-T17 assertion pending for the four ungated features until real gate code (model_plane_gate.rs) exists. Do not narrow the test.
3. Option B lands later the D way, and is not enabled until the search_suggest ZDR gap and the persistent retention of the data-plane token have been reviewed (plus counsel on Cohere).

**If accepted.**
1. Edit the compose block (lines 168-183) and its comment.
2. Leave the test pending, with an updated reason.
3. Run `node --test` and `docker compose config`.
4. Recreate quarry-edge and confirm that no Model Plane mints happen.
5. First check who relies on MODEL_PLANE_URL being set by default, and whether `/v1/answer` changing from a degraded answer to 501 matters to any frontend caller.

**Challenger verdict: amend.** Where the brief was wrong:
- It called four keys "dead switches". Answer synthesis, AI extract formats, search suggest and audio are live features with **no** switch: they run whenever MODEL_PLANE_URL is set (main.rs:1460-1495, extract_routes.rs:228, search_routes.rs:1518, audio_routes.rs:92).
- Flipping two flags leaves four Model Plane paths on, and the narrowed test would certify something false.
- search_suggest has a test-pinned ZDR gap (zdr_matrix.rs:163-175). It would go live the moment someone grants quarry-edge a model-gateway audience.

**Open risks.** /v1/answer returns 501 instead of a degraded answer (not checked against frontend callers).

### 3.13 OPENAPI-INTERNAL: internal routes and the public spec

**Question.** Should the `/v1/internal/*` routes be described in the public Quarry openapi.yaml?

**Current state.** The policy is inconsistent. Only `/v1/internal/run_page` is in the public spec (openapi.yaml:425-466, tag internal), and its schema is stale (D2-331). The other 8 edge internal routes are on the allowlist with the reason "never part of the public API" (openapi-route-allowlist.json:75-105). The notes say 3; the code has 8. The Go control service has 8 more internal routes that no spec and no test covers. The public SDKs are generated from the public spec, and **the checked-in SDKs already ship `InternalApi` and `InternalRunPage`**: `sdks/python/quarry_client/api/internal_api.py`, `sdks/typescript/src/apis/InternalApi.ts`, and `test_smoke.py:28-29,191-204`.

**Options.**

| Option | Effort | Risk |
|---|---|---|
| A. Leave as is | 0 d | A contradictory rule and contract drift |
| B. A separate internal spec | 1 d now, Go half later | Low |
| C. Everything in the public spec | 2 d | Publishes a map of the trusted bus |
| D. One file, filtered at generation | 3 to 4 d | A filter bug leaks routes silently |

**Recommendation.** B, as the challenger amended it. The rule is: internal routes never appear in the public spec or the SDKs. Now (about 1 day):
- Move run_page and the 8 edge internal routes into `docs/openapi-internal.yaml` (repo docs only, never served).
- Delete the shipped internal files and smoke-test cases from the SDKs.
- Add a test that fails if any `/v1/internal` path is in `docs/openapi.yaml`.

The 8 Go control routes and their scanner follow as part of the D2-050 internal secret split, not before it.

**If accepted.** Sequence this after OPENAPI-NULL, because regeneration is blocked until then. Otherwise the internal files must be deleted by hand. Update the coverage test and the allowlist, and note the rule in CONTRACTS.md.

**Challenger verdict: amend.** Where the brief was wrong:
- "No consumer expected" is false: the SDKs already ship the internal surface, and the smoke test exercises it.
- The brief bundled the cheap public cleanup with the costlier Go documentation. That half overlaps the D2-050 auth redesign and would document an auth model that is about to change.

**Open risks.** An internal spec protects nothing by itself. Exposure is decided by gateway routing (D2-312, not proven live). The internal spec names callers and secret env var names, so it must stay in repo docs only.

## 4. Owner checklist

Answer each line yes, no, or other.

1. X-10: Obey robots.txt after a volume-based observe soak. Unreachable robots.txt counts as disallow. Fix D2-161 and check map before the flip. Switch on through the production env. Yes / no / other
2. ROBOTS-RFC: Accept e5e14bb6c (a blank line does not split a group). Fix the BOM, Crawl-delay and empty Allow before the soak. Yes / no / other
3. X-12: Ratify the run-budget numbers. People are uncapped. Flip to `on` after the guard, the test and a scripted stack proof. The Model Plane mapping follows. Yes / no / other
4. TAKEOVER-BUDGET: Keep takeover steps exempt (as built). Add a counter. Tie the residual to D2-012. Yes / no / other
5. D16-Q2: Stamp the profile owner at run start for human users only. Model runs never own a profile. Default scope user_private. Legacy private rows hidden. Count legacy rows, then enforce after an observe week. Yes / no / other
6. X-19: `sensitive` is the receipt default. Receipts for real users only after the event follows the flag, D2-065 is fixed, there is a retention or erasure path, and the stack evidence passes. Yes / no / other
7. X-9: Event-driven ZDR for schedules (no cross-tenant scope for Quarry). Ask Control Plane for the plan-change event. Monitor half and the follow refusal first. The rest waits. Yes / no / other
8. ADR-6: Ratify closed, capped action lists with no Evaluate. Rewrite the X-20 gates. `user` only on the dev/evidence stack. Production off. Yes / no / other
9. MP-GATEWAY-AUTH: No Model Plane credential to the gateway. Quarry decides who the agent is. Quarry refuses an agent reload before D2-012 is closed. Yes / no / other
10. OPENAPI-NULL: Change the one bare null to `[string, 'null']`. Keep the generator. Do it first. Yes / no / other
11. BILLING-IDEMPOTENCY: Fix crawl/batch only (job id key, no bill on replay, no bill on a failed handoff). Leave billing-core alone. No Redis scheme. Yes / no / other
12. DATAPLANE-OPTION-B: Flags-off compose with MODEL_PLANE_URL opt-in. QB-T17 stays pending. Option B lands later from main, after the ZDR and retention review. Yes / no / other
13. OPENAPI-INTERNAL: Internal routes leave the public spec and the SDKs now. The Go routes follow with the D2-050 split. Yes / no / other

## 5. Appendix: other owner-decision rows this round did not brief

One line each, with where to find it. Rows already covered above are marked.

**ADR-4 and counsel**
- ADR-4 stage B (D1-008, D1-069): flip the ZDR refusal flag default, mark five matrix rows Enforced, invert `zdr_gap_rows.rs`, send the customer notice. DEPTH-FINDINGS-W1.md lines 31, 134; STATUS 7.5 item 1.
- Counsel on the ZDR run tombstone (D1-014): gates `QUARRY_EDGE__ZDR_TOMBSTONE` and the control receiver. DEPTH-FINDINGS-W1.md line 47; STATUS 7.5 item 1.
- D1-070: the tombstone sweeper retention (30 days flat; `RETENTION_DAYS=0` empties the table) has no owner sign-off. W1 line 135.
- D2-058: external crypto review of the profile encryption code before `PROFILE_ENCRYPTION_REQUIRED` goes on in production. W2 line 93.

**W1 owner rows (DEPTH-PLAN-2026-10-27.md "Left out on purpose", DEPTH-FINDINGS-W1.md)**
- D1-006: later-ZDR-org gap. Covered by X-9 above.
- D1-007: ChangeMonitorWF puts the monitored URL in Temporal args and history before the record refusal (event-contract decision). W1 line 30.
- D1-013: should a 412 on stop under ZDR still close the session? W1 line 46.
- D1-018: the 15-minute session-grant ceiling; grants are never refreshed. W1 line 63.
- D1-041: the per-grant total is unbounded (one grant backs many runs; needs a use-once grant or a per-grant ledger). W1 line 91.
- D1-067: is quarry-browser-live a required CI status? W1 line 132.
- D1-068: challenge enforce, options A, B or C (`QUARRY_CHALLENGE_ENFORCE` defaults to log). W1 line 133.
- D1-071: strict `BROWSER_GRANT_ENFORCEMENT` in production (waits for L2), and whether the Ingestion Postgres may host browser_grants. W1 line 136.
- D1-072: no named owner for `search_suggest` (forward the posture or refuse under ZDR). Linked to DATAPLANE-OPTION-B. W1 line 137.
- D1-073: seccomp profile shape, and no memory limit on quarry-edge (X-15 number). W1 line 138.
- D1-074: the plan's 64 KiB JSON leaf cap is deliberately not built (accept the deviation). W1 line 139.
- D1-075: items waiting on an owner or ADR (PRE-1 baseline commit X-3, ADR-10 for WS3.G beyond K=1, others). W1 line 140.
- D1-084: grouped small decisions (grant-too-short hardening, broker budget ceiling for derived grants, typed `browser_grant_expired`, and others). W1 line 181.
- W1 rows owned by other planes: D1-002 (tab initiator contract and the Model Plane caller, W1 line 20), D1-015 (no org_id in ValidateGrantResponse), D1-016 (Model Plane half of WS3.J, W1 line 54), D1-044 (session id in the Model Plane run read model), D1-019 limit d and process death (Session Core).

**W2 owner rows (DEPTH-FINDINGS-W2.md, DEPTH-PLAN "Left out on purpose")**
- D2-043: migration 017 disables every pre-existing scrape, crawl and batch schedule on first run; confirm this is intended. W2 line 78.
- D2-044: owner stamping needs an actor id on BrowserLease. Covered by D16-Q2 (option C avoids the lease change).
- D2-050: internal route secret split. Finalize can end any org's job; it needs a new deploy secret. W2 line 85; linked to OPENAPI-INTERNAL.
- D2-053: an empty `domains.allow` claim reads as "allow anything"; needs WS7.I confirmation before any minting. W2 line 88.
- D2-054: the denial `policy_rev` format (decimal versus 12-hex). W2 line 89.
- D2-055: the dev-bypass stub carries `quarry:profiles:import`; the owner must decide. W2 line 90.
- D2-056: import now needs that scope; confirm there is no live importer before the edge image ships. W2 line 91.
- D2-057: ZDR floor on the job-event tail. Covered by X-9.
- D2-060: human-browse TTLs (idle 900 s, takeover 1800 s, absolute 4 h) and RenewGrant are not wired. W2 line 95.
- D2-201: the legacy drain now pages up to 200 pages of 500; confirm before the flag is on. W2 line 283.
- D2-202: org-shared members may save; S3 save stamps no owner. Mostly covered by D16-Q2. W2 line 284.
- D2-204: the web-policy claim default is enforce, while the plan says observe first; set observe in the production overlay. W2 line 286.
- D2-205: the redirect flag default is derived from ENVIRONMENT; a staging replay is owed. W2 line 287.
- D2-207: human steps skip budget_charge. Covered by TAKEOVER-BUDGET.
- D2-208: `LIMITS_FAIL_MODE` open is accepted in production although ADR-1 pins floor; batch cap mismatch. W2 line 290.
- D2-209: `dispose()` closes only ZDR sessions while `clearPreview` closes all. W2 line 291.
- D2-210: the event ignores `RECEIPT_TEXT_MODE`. Covered by X-19.
- D2-211: Method B scroll stitch, after the tile spike. W2 line 293.
- D2-212: the user and on scrape modes. Covered by ADR-6.
- D2-213: the ZDR tombstone outcome on drain is `cancelled`; confirm with the consumer. W2 line 295.
- D2-214: an opaque cookie partition key is stored as none. W2 line 296.
- D2-215: plan value 0 and absent encoding for limits. W2 line 297.
- D2-216: `dispatch_crawl_site` SSRF check and the 5000-page clamp; confirm with the Model Plane. W2 line 298.
- D2-217: keep-alive semantics for plain session grants. W2 line 299.
- D2-218: 429 on robots.txt. Covered by X-10. The user-agent product token (F-t2-067) is **not** decided. W2 line 300.
- D2-219: keep-alive interval, 300 s built versus 20 s in the evidence leg. W2 line 301.
- D2-220: rollback text and removal order conflict between CRAWL_SCOPE_ROBOTS.md and the plan. W2 line 302.
- W3-batch decision rows D2-221 to D2-229 (email local parts, same_host, webhook redirects, approval-card focus, pages unit, observe-mode monitor baselines under ZDR, ingestion retention posture, Tier B corpus, parse-cache retention). W2 lines 303-311. W3 is not ordered.

**STATUS-2026-10-24.md 7.4 (left open, not decisions this round)**
- D1-001 and D1-019 limit c: fixed per gateway process only; the durable fix needs a broker-side consumed mark (Model Plane proto) and Session Core.
- D2-018: /v1/audio, /v1/answer(/stream), create-time URLs of /v1/sources and /v1/schedules, and the internal enqueue are not gated by the edge policy; recorded with owners.
- D2-029 Go side: `appliedBy` fails closed on a missing `scope_applied` (version-skew risk).
- D2-017: no entitlement source (WS6.D); the stamp is inert.
- WS16.S0: model_plane_gate.rs is not on this branch; the DR-15.1 ledger entry belongs to the Model Plane session.

**STATUS-2026-10-24.md 7.5 item 3**
- The stack session with the owner's own sign-in (STEP 2, `L2-signed-in-steps.md`): rebuilt images, strict grant enforcement, L2 legs, SC1 to SC7, the live Chromium lane. Most "after the stack" steps above depend on it.
