# T2 / Category 9: AI-browser control (cross-source comparison)

Inputs: 205 reference records (Aside 78, Codex in-app browser 120, Firecrawl 7) and 38 Verevon baseline records. Result: 51 clusters; 203 records assigned, 2 excluded (AB092 upstream Chromium strings, CI078 desktop pipe transport). Apify has no records in this category. Firecrawl is a web-only benchmark, so a capability found in both Aside and Codex counts as table-stakes. Full detail: `09-ai-browser-control.clusters.json`.

## Shared baseline (table-stakes)
- Agent loop outside the browser, driving it over an authenticated, routed channel (C001)
- Accessibility-tree observation with stable refs and diffs (C009); stale-ref protection (C010)
- Actionability checks, hit-testing and trusted CDP input (C011); post-action settle (C013)
- Semantic locators across frames (C015); fallback to set-of-marks screenshots and coordinates (C014)
- Agent tab ownership and lifecycle (C017); co-browsing on the user's tab (C018); user takeover (C019)
- Durable, fail-closed human approvals (C023); risk-tier confirmation (C024); session permission modes (C026)
- Gated uploads and downloads (C033); secure credential handoff where the model never sees secrets (C034)
- Prompt-injection envelopes (C036); code-mode REPL (C003)

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Authenticated control channel | Aside (AA005/AB050, AA007) | Signed-challenge CDP tokens; routed, cancellable bridge |
| AX snapshot + refs + diffs | Aside (AB054) / Codex (CA053) | Frame-aware refs, shadow DOM, revision-keyed diffs |
| Hit-tested trusted clicks | Codex (CA056, CI115) | Probes through OOPIFs; typed "covered by X" errors |
| Settle logic | Aside (AA024, AA085) | Mutation quiet-window plus request tracking; ambient event notes |
| Turn-scoped tabs | Codex (CI088) | Temporary vs deliverable/handoff tab marks |
| Agent cursor | Codex (CI085) | Cursor is animated to the target before the click lands |
| Run control (steer/queue) | Aside (AA036) | Idempotent commands, seq replay, steer with expectedTurnId |
| Policy choke point + denials | Codex (CA001-003) | One choke point; 16 typed denial reasons with decisionSource and a retryable flag |
| Per-origin consent | Codex (CA004/005) | Turn/conversation/global scopes; any deny beats any approval |
| Raw CDP / evaluate gating | Codex (CA018, CI114) | Three gates plus allowlist; read-only isolated-world evaluate |
| Credential handoff | Codex (CA036-039, CI103) | Isolated-world fill; page observation blocked after fill; OTP/QR |
| Final-confirm drafts | Aside (AB058) | Typed draft artifact; confirmation is the only call in that turn |
| Hosted prompt "interact" loop | Firecrawl (FE098) | One tool, capped interactive snapshot, guardrails |

## Differentiators and uniques worth noting
Codex alone has these: guardian reviewer model (C025), per-origin consent (C028), site reputation gate (C031), WebMCP site tools (C043), connector-first surface selection (C008) and the denial taxonomy (C027). Aside alone has these: run steering/queueing (C021), crash-safe rerun-safe tool classes (C022) and subagents (C039). Codex and Firecrawl share org origin policy (C029), the only differentiator.

## Verevon's biggest gaps (priority tier)
1. **C001 Control channel not wired** (partial, sig 5). BrowserBroker keeps grants in memory and has no production AcquireGrant caller. The gateway omits the browser and inference auth headers, so AI-step and AI-loop calls likely fail (INF high).
2. **C051 Planner loop disabled** (partial, sig 5). The LLM planner only runs when `QUARRY_BROWSER_AGENT_LLM` is set, and compose does not set it.
3. **C018 Co-browsing absent** (sig 5). An AI run takes its own Quarry lease and cannot act on the tab the user is looking at.
4. **C019 Takeover not enforced** (partial, sig 5). `/step` keeps accepting agent steps during human_takeover, and receipts do not record who started each step.
5. **C034 Credential handoff absent** (sig 5). There are no secret references, and redaction is a stub.
6. **C011 Clicks are synthetic** (partial, sig 4). Ref clicks use `element.click()` via `Runtime.callFunctionOn`, with no visibility, stability or hit-test checks (chromiumoxide.rs:1232-1300, read in this audit).
7. **C032 Evaluate is ungated** (partial, sig 4). Quarry `Evaluate` and raw CSS run on normal runs without approval, which bypasses the risk gates.

Recommended next tier: per-origin consent via child grants (C028), agent cursor overlay (C020), steering (C021), set-of-marks screenshots from the existing `ScreenshotAnnotated` artifact kind (C014), adaptive settle (C013), typed denials (C027), connector-first routing (C008), grant action classes and modes (C026), org origin policy in the Control Plane (C029), enabling uploads/downloads (C033), code-mode REPL plus hardening (C003/C004), capability-conditional docs (C006) and a browsing procedure prompt (C007).

Skipped as not fitting Verevon (server-side, cloud browser): desktop computer use (C048/C049), user bookmark/tab management (C002), cookie-replay site APIs (C038, which conflicts with Integration Plane OAuth), web-push triggers (C045), user history access (C047), payments (C035; checkout stays always-confirm) and MCP Apps (C050).

## Verevon strengths (equal or ahead)
- Native-AX refs bound to CDP backend nodes, fail-closed TARGET_REPAIR_REQUIRED and OOPIF frame actions (C009, C010, C015)
- Fail-closed request-level network guard: the CDP Fetch interceptor defaults to deny-all and allows http/https only, matching Codex (C030)
- Dialogs never auto-resolved, with one-shot dialog grants; stricter than Aside (C016)
- Risk gates enforced in code, plus fail-closed approvals (C023, C024)
- Immutable step receipts, timeline, checkpoints/resume and proof bundles; ahead on evidence (C022, C041)
- Procedure compile and replay checks, ahead of every reference, though still unwired (C042)
- Challenge signals with no bypass (C044); typed contracts with capability gating (C005)
- ZDR propagation to browser runs (v-browser-app-091, v-quarry-145) has no reference equivalent; keep it

## Caveats
- Reference records are mostly SRC from REA analysis. Some Codex policy records are DOC only (CA033/CA034/CA041/CA070/CA071...). C042 rests on INF only, and AA058 enforcement was not observed.
- Verevon status comes from the baseline plus targeted greps in this audit. Clusters marked "unknown" (C006, C007, C012, C037, C038, C045-C047, C050) still need verification in code. "Absent" for C025, C031 and C043 rests on grep results only.
- Wiring failures (401/403) are baseline inferences, not runtime-tested; this audit ran nothing.
- Firecrawl has only 7 records here, so its prevalence counts understate it as a hosted-agent benchmark.
