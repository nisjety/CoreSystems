## 10. Risks and open questions

### 10.1 Main risks

1. **Grant redesign blast radius (authority).** [F-t2-001](#F-t2-001), [F-t2-011](#F-t2-011), [F-t2-013](#F-t2-013) and [F-t2-032](#F-t2-032) all change what a grant means; single-action child grants and the per-step domain-equality check (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:314-339`) do not support the per-origin extension the items assume.
2. **Cross-tenant exposure (security).** The gateway search cache is keyed without org and feeds org-scoped private-corpus providers ([F-t2-003](#F-t2-003)); webhooks have no `org_id` and dial without address vetting ([F-t2-031](#F-t2-031)); all sessions share one Chromium and scrape sessions are keyed by host only ([F-t2-060](#F-t2-060), INF); the fingerprint-keyed PageCache is not org-scoped ([F-t2-048](#F-t2-048), to verify).
3. **ZDR regressions (data governance).** Every new cache, diff, inline payload, span or trace projection is a new leak path: [F-t2-003](#F-t2-003), [F-t2-004](#F-t2-004), [F-t2-014](#F-t2-014), [F-t2-023](#F-t2-023), [F-t2-030](#F-t2-030), [F-t2-048](#F-t2-048). Telemetry already logs a source URL inside a ZDR branch.
4. **Evidence captures secrets (security).** AX snapshots, receipts, screenshots and the planner prompt carry field values verbatim until [F-t2-021](#F-t2-021) lands; [F-t2-024](#F-t2-024) and [F-t2-010](#F-t2-010) widen the exposure if sequenced first.
5. **Plane-ownership drift (architecture).** Policy ([F-t2-022](#F-t2-022)), capacity ([F-t2-025](#F-t2-025)), automation ([F-t2-031](#F-t2-031), [F-t2-061](#F-t2-061)), trace projection ([F-t2-014](#F-t2-014), [F-t2-072](#F-t2-072)) and isolation ([F-t2-060](#F-t2-060)) each had a proposed option that would add cross-plane DB access or let Spaces widen access; verification dropped them, but they will resurface.
6. **Unbounded cost (commercial).** Only `max_steps` is enforced ([F-t2-008](#F-t2-008)); planner-on ([F-t2-010](#F-t2-010)), per-page extraction ([F-t2-019](#F-t2-019)), hydration ([F-t2-042](#F-t2-042)) and profile escalation ([F-t2-065](#F-t2-065)) multiply spend before enforcement exists.
7. **Compliance and posture (data governance).** Crawl follows external links with no robots check ([F-t2-016](#F-t2-016)); the dead consent script clicks the affirmative accept button, granting tracking consent on the user's behalf ([F-t2-066](#F-t2-066)); proxy rotation on bare 403 sits in tension with the no-bypass rule ([F-t2-027](#F-t2-027), [F-t2-058](#F-t2-058)).
8. **Human-browsing regressions (delivery).** Enforcing runtime and cost budgets in `/step` would cap manual sessions, which the gateway creates with `max_steps 12` and `max_runtime_s 120` ([F-t2-008](#F-t2-008), [F-t2-011](#F-t2-011)); budgets must key on the initiator from [F-t2-013](#F-t2-013) phase 1.
9. **Stale docs steer planning (delivery).** Docs mark grant issuance COMPLETE, formats done, budgets enforced, DOCX shipped and an adblock task fixed; none holds in code ([F-t2-001](#F-t2-001), [F-t2-005](#F-t2-005), [F-t2-008](#F-t2-008), [F-t2-037](#F-t2-037), [F-t2-066](#F-t2-066)).
10. **Effort growth (delivery).** 33 efforts were revised (29 up, 4 down), six items were demoted, and the XL items [F-t2-032](#F-t2-032), [F-t2-036](#F-t2-036), [F-t2-058](#F-t2-058), [F-t2-059](#F-t2-059) and [F-t2-060](#F-t2-060) each depend on an ADR; without the ADRs they stall.

### 10.2 Open questions

1. Which plane owns web and browser capability policy, and how is a policy claim revoked mid-run when grants live 15 minutes?
2. For a human browsing session, is navigation governed by a broker policy kind, a renewed grant, or both, and what idle timeout replaces the 120 s run TTL?
3. Can smart-router search results include org-private corpus hits? The verification says yes for Quarry's `/v1/search` providers, which makes the gateway cache a cross-tenant incident rather than only a ZDR gap; confirm the hit rate and whether any such result was ever served across orgs.
4. What does ZDR mean for monitors, schedules, durable crawls and URL caches: refused, inline-only or hash-only?
5. Is the honest anti-bot posture official? If so, delete `stealth.rs`, retire the fingerprint-rotation backlog item, and narrow `block_rotation` triggers.
6. Will Verevon expose Quarry and connectors over MCP to third-party agents, and under which customer authentication model?
7. Who owns secrets used inside pages, and must every email-OTP read be approved and shown with origin and connection?
8. Do enterprise tenants need a container or VM per browser session, and is a sidecar proxy an acceptable relaxation of the loopback-only egress type?
9. Are code-mode control and generated extraction profiles acceptable when Quarry validates every call, and should Python-over-RPC be the first engine?
10. Which claims need runtime confirmation first: the AI-path 401/403 failures, how often the viewport relaunch fires in production, whether Chromium already masks password values in the accessibility tree, and whether same-host scrape sessions from different orgs share a session key?
11. Should manual browsing be exempt from robots.txt, and who may hold the audited robots-override entitlement?
12. Does a chat thread keep one long-lived Quarry run across turns, or one run per Model Plane run as today? This decides [F-t2-054](#F-t2-054) and [F-t2-035](#F-t2-035).
13. Should `GET /v1/profiles/:id` ever return raw cookie values, and who may call it once an owner check exists?
14. Who takes ownership of the org-bound metered Model Plane invoke path (DEF-QB-T17-PART2) that four items wait on?
15. Should stale docs be retired wave by wave and added to `apps/STALE_DOC_DELETION_REGISTER.md`?

### 10.3 Evidence limits that matter for decisions

- All Apify evidence is public documentation [DOC]. Browser-pool lifecycle, per-run containers, charge-within-cap budgets and hosted MCP are described, not inspected, so [F-t2-007](#F-t2-007), [F-t2-060](#F-t2-060), [F-t2-008](#F-t2-008) and [F-t2-064](#F-t2-064) borrow shapes, not implementations.
- Firecrawl's hosted parts (fire-engine, Hangar sessions, the run-trace producer) are seen only through client contracts in the open-source repo [SRC for contracts, INF for behaviour]. [F-t2-001](#F-t2-001), [F-t2-014](#F-t2-014), [F-t2-020](#F-t2-020) and [F-t2-035](#F-t2-035) cite contracts observed, implementation unknown.
- Codex credential handoff is wired to the hosted Codex Cloud Browser, not the shipped desktop in-app browser, and the connector-side OTP fetch is inferred from elicitation text [SRC, INF]. [F-t2-024](#F-t2-024) rests on a cloud-only variant plus Aside's vault fill.
- Several Codex and Aside behaviours are plugin or prompt text, not code: connector-first routing and fail-closed tab-mention resolution [DOC]; Aside takeover handling is an absence claim from static analysis [INF]. [F-t2-010](#F-t2-010), [F-t2-013](#F-t2-013) and [F-t2-028](#F-t2-028) must not cite these as enforced mechanisms.
- Nothing in Verevon was run. Every "fails in production" statement, including the AI-path 401/403, the relaunch frequency and same-host session sharing, is inference from code reading [INF] and should be reproduced before fixes are scoped.
- Prevalence counts were wrong for 14 items before correction; where a correction gave no new count, this document states none.

