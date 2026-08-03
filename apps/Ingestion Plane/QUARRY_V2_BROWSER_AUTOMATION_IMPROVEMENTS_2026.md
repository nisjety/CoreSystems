# Quarry V2 Browser Automation Improvements 2026

**Status:** Architecture, standards, research, and implementation proposal  
**Scope:** `apps/Ingestion Plane/Quarry-v2`, Model Plane browser planning, BrowserBroker, browser runtime adapters, and App Shell replay/approval UX  
**Research date:** 2026-08-03  
**Verified against the running stack:** 2026-08-03 (same day) — every major proposed component cross-checked against actual Rust/Go source with file:line citations, via 3 verification passes (one from a separate down-the-stack security audit, two targeted at this document specifically). See §1a for the full ledger. **Read this before anything else in the document**: three CRITICAL/HIGH security gaps already exist in the currently-running browser/fetch drivers — a headless-browser SSRF bypass, a redirect-based SSRF bypass, and a DNS-rebinding TOCTOU gap — and they rank above every new-contract item in §31's Phase 0, because they are exploitable-in-shape today, not proposed work.  
**Primary rule:** Model Plane decides. Quarry executes and records evidence. BrowserBroker grants. Browser runtimes provide isolated sessions.

---

## 1. Purpose

Quarry V2 already provides much more than a browser wrapper. It is a self-hosted web-intelligence execution plane with:

- static and impersonated HTTP fetching;
- browser leases and persistent profiles;
- Browserbase support;
- Chromium/CDP execution;
- TLS and browser fingerprint controls;
- scraping, crawling, mapping, search, extraction, answers, and deep research;
- Tantivy scratch indexes;
- Stract, SearXNG, Brave, and Serper routing;
- artifacts, screenshots, PDF output, and event history;
- Temporal workflows, NATS events, Postgres control state, and MinIO/S3 artifacts;
- deterministic source capture and SSRF/tenant policy enforcement.

This document therefore does not propose replacing Quarry with Firecrawl, Stagehand, browser-use, Skyvern, or another browser platform.

## 1a. Verification pass against the running stack — 2026-08-03

This document was written as a research proposal. This section is the
difference between that and reality — every major component checked
against the actual current Rust/Go source, so the rest of the document can
be read as verified-and-prioritized. Re-run this check before trusting
anything below without a citation next to it.

**Read this first — the headline finding changes the whole phasing.**

This document's entire security philosophy (§2's ownership split, §19
credential safety, §20 prompt-injection containment: "actual security comes
from domain/action allowlists, browser grants, capability policy...") rests
on the assumption that Quarry's browser execution layer actually enforces
the SSRF/domain boundary it's supposed to. **It currently does not, in two
places, and a third weakens it:**

1. **Quarry-v2's headless-browser driver (`chromiumoxide.rs`) performs
   zero SSRF enforcement of its own.** `goto()`/`open_tab_page()` call the
   CDP browser directly — no call into `quarry_security`/`dns_guard`
   anywhere in the file. The crate's own `tests/ssrf.rs` asserts a security
   contract (`goto()` must reject metadata/loopback/private URLs) the
   implementation does not satisfy, and every test in that file is
   permanently `#[ignore]`d, so this has never been caught by CI. This
   means sub-resource requests the *rendered page itself* issues (JS
   fetch/XHR/iframe/img — the classic browser-SSRF vector) are checked by
   nothing, on any browser-driven action this whole document proposes to
   build more of.
2. **The static fetch driver auto-follows redirects with no SSRF
   re-check.** `reqwest`'s `Policy::limited(5)` follows up to 5 redirects
   with no callback; the one preflight+DNS-guard check runs once, on the
   original URL, before the fetch. A 301/302 to a private/metadata address
   is followed transparently.
3. **`dns_guard.rs` is not actually TOCTOU-resistant, despite its own doc
   comment claiming so.** It resolves the host once, checks the IPs, then
   discards them — the actual driver re-resolves the same hostname
   independently at connect time. An attacker controlling DNS for the
   target host (short/zero TTL) can serve a public IP to the guard and a
   private one to the real connection. The identical pattern is duplicated
   in imports-core's Python guard (`network_policy.py`) — a systemic
   mistake, not a one-off. Related, smaller: neither the URL-literal nor
   post-DNS IPv4 check tests `is_unspecified()`, so `0.0.0.0` (which Linux
   treats as `127.0.0.1` on connect) bypasses SSRF checks entirely — the
   IPv6 branch correctly checks this, the IPv4 branch does not.

**This means: before building any of the new Browser Action IR / observation
/ verification contracts below, the ground they'd stand on needs fixing.**
None of §7-§15's proposed work makes the SSRF gate more correct — it all
assumes the gate exists and works. Promote the 3 items above to the very
top of §31 Phase 0.

**Second most important correction: several proposed components already
exist, in a real but rougher form than the doc implies — extend, don't
rebuild:**

- **§7's Browser Action IR already exists.** `quarry-core/src/contracts.rs`
  defines `AgentAction` (Navigate/Click/Type/Press/Scroll/Select/Wait/
  Screenshot/Evaluate/etc) and `quarry-browser/src/actions.rs` a parallel
  `Action` enum, dispatched through `ObservationRunner::execute()`. The gap
  is not "no IR" — it's that targeting is a raw CSS/XPath `selector: String`
  everywhere (no ref-based targeting, no `SelectorEnsemble`), and
  `Evaluate { script: String }` lets a caller run **arbitrary JavaScript
  directly** — exactly the "models must not emit raw JS" escape hatch this
  section says should not exist by default. Closing that escape hatch
  (gate `Evaluate` behind an explicit high-risk policy, per §7's own
  closing line) is a small, concrete win available now, before the larger
  ref-based-targeting rework.
- **§21's SmartSearchRouter/AnswerPipeline claim is confirmed real, not
  aspirational.** `quarry-runtime/src/smart_router.rs` genuinely routes
  Tantivy → Stract → SearXNG → Brave (+Serper) with intent classification,
  per-provider circuit breakers, and a tenant-isolated TTL result cache;
  `answer.rs`'s `AnswerPipeline` is a real search→scrape→synthesize
  orchestrator. Build agentic-browsing integration on this — it is solid.
  §22's result fusion is correctly scoped as new work, though: today's
  fusion is simple URL dedup + provider-priority ranking, no RRF, no
  source-class weighting, no evidence-sufficiency check.
- **§3.2's driver waterfall is thinner than described.** The real
  `DriverPlan` (`quarry-runtime/src/driver_plan.rs`) has only 3 `DriverKind`
  variants (Static/Browser/Tls) — no separate Cache/Index/Parser/Specialty
  stages. The local Tantivy index is a post-fetch write-behind search
  index, not a pre-fetch cache-check stage in the waterfall. Preserve the
  fallback-chain *mechanism*; don't assume the 6-stage waterfall itself
  already exists.

**Third: `BrowserBroker` (Model Plane) is real but weaker than this
document's ownership model assumes.** It's a genuine gRPC service with
real, enforced domain-allowlist checks (not just documentation) and real
revocation — but its own package doc comment says outright: "in-memory
storage for browser grant lifecycle." Grant state does not survive a
restart. Action-class restriction, credential-reference binding, cost
metadata, and Quarry-lease binding — all four things §2/§29 assign to
BrowserBroker — do not exist in the proto or the Go code at all (only
domain scoping does). §2's ownership split is the right target state; today
BrowserBroker delivers about a quarter of it.

**Confirmed 100% greenfield, no correction needed to the doc's own framing**:
ARIA/accessibility snapshots and Set-of-Marks (§8, §15); the
SelectorEnsemble fallback chain (§9); deterministic post-action
verification (§12 — today, an action that doesn't error is treated as
succeeded, with no separate check); compiled workflows, site profiles, and
leveled self-healing (§13, §14, §25 — zero grep hits for any of these
concepts anywhere in Quarry-v2). One terminology note: §3.3's existing
"profile" (browser session/reconnect/proxy-affinity metadata) is a
*different concept* from §25's proposed per-host "site profile"
(login/challenge signals, stable selectors, success stats) — they do not
overlap, keep the names distinct when implementing.

**Resolved since a prior audit, confirmed not a live gap:** the
`artifact_store.rs unimplemented!()` paths a previous pass flagged are now
fully implemented (fs/S3/in-memory backends, with enforced org-tenant
isolation) — no action needed. Two narrower, real findings from the same
pass remain open and are folded into §31 Phase 0: an unrecognized
`artifact_backend` config value silently downgrades to a non-durable
in-memory store with only an info-level log, and the filesystem artifact
index is an unbounded, linearly-scanned flat file (a real scaling cliff at
production crawl volume, not urgent).

---

The objective is to make Quarry's agentic browser execution **top class** by improving:

- planner/executor contracts;
- observation quality;
- action reliability;
- verification;
- model routing;
- standards compatibility;
- session and credential handling;
- replay and operator intervention;
- workflow compilation and self-healing;
- browser-agent security;
- evaluation and continuous learning;
- Firecrawl-level API and SDK ergonomics.

---

## 2. Target ownership split

```text
Model Plane
  - interprets the task
  - creates browser/research plans
  - selects a planner model
  - chooses from Quarry observations
  - decides whether to continue, replan, or stop
  - owns risk and approval policy

BrowserBroker
  - issues tenant-bound grants
  - restricts domains and actions
  - binds credential references
  - validates and revokes browser authority

Quarry V2
  - chooses the execution driver
  - creates observations
  - executes typed browser actions
  - enforces browser/runtime security
  - verifies deterministic postconditions
  - stores evidence and artifacts
  - emits complete action history

Browser runtime
  - local Chromium/chromiumoxide
  - Browserbase
  - Browserless
  - Kernel or other CDP-compatible backend
  - future WebDriver BiDi backend

Application Plane / SolidJS App Shell
  - live view
  - replay
  - approval cards
  - intervention and takeover
  - task timeline
```

Model Plane must not directly control Browserbase, Chromium, Playwright, or provider credentials. All browser execution goes through Quarry's trusted action boundary.

**Verified 2026-08-03 — BrowserBroker delivers about a quarter of this
today, see §1a for detail.** It's a real gRPC service with real, enforced
domain-allowlist checks and real revocation — but its own package doc
comment says "in-memory storage for browser grant lifecycle" (grants do
not survive a restart), and action-class restriction, credential-reference
binding, cost metadata, and Quarry-lease binding — all four things this
split assigns to BrowserBroker — do not exist in the proto or the Go code
at all. This is the correct target ownership model; treat it as real
remaining work, not a description of what's already running.

---

## 3. Existing Quarry strengths to preserve

### 3.1 Execution/control split

Keep:

- Rust hot path for fetch, browser, action, transform, artifacts, and events;
- Go control for durable jobs, profiles, history, schedules, and Temporal orchestration;
- Python for benchmarks, experiments, and model adapters only.

### 3.2 Driver waterfall

**Verified 2026-08-03** — see §1a. The real `DriverPlan` has 3 `DriverKind`
variants (Static/Browser/Tls), not the 6-stage waterfall below; preserve
the fallback-chain mechanism, treat the extra stages as new work.

The existing typed `DriverPlan` direction is correct:

```text
index/cache
  -> static impersonated HTTP
  -> browser CDP
  -> browser CDP + stealth/session/proxy
  -> document parser
  -> specialty handler
```

Agentic browsing should be another controlled capability within this waterfall, not a separate ungoverned browser stack.

### 3.3 Lease/profile/session model

Keep the distinction:

- lease: runtime handle with TTL and capabilities;
- profile: durable resource and snapshot metadata;
- session affinity: reconnect and state continuity;
- proxy affinity: stable session/IP pairing.

### 3.4 Evidence-first execution

Every action must create inspectable evidence:

- before/after observations;
- screenshot or visual region when needed;
- action resolution details;
- network/download receipts;
- verification result;
- timing and cost;
- errors and recovery;
- source/artifact references.

---

## 4. Competitor and ecosystem audit

| System | Strongest pattern | Quarry decision |
|---|---|---|
| Firecrawl | Unified search/scrape/interact/agent/crawl/map/batch API, simple SDKs, scrape IDs, live view | Product and API parity reference; do not depend on it |
| Firecrawl open agent | Search/fetch/extract primitives behind an open web research loop | Benchmark and research donor |
| Stagehand | `observe`, `act`, `extract`, `agent`, action preview, caching, self-healing, DOM/vision/hybrid modes | Strong architectural donor; optional lab adapter |
| Browserbase | Persistent contexts, Agent Identity, live view, replay, CDP sessions, action caching | Continue as optional managed runtime backend |
| Playwright | Auto-waits, locators, traces, accessibility snapshots, multi-browser tooling | Use formats and behavior as reference; optional adapter/lab |
| Playwright MCP/CLI | Accessibility snapshot with stable element refs for LLM actions | Adopt compatible observation concepts |
| BrowserGym | Standard action/observation environment and broad benchmark set | Adopt in Python evaluation lab |
| AgentLab | Run agents, collect traces, compare benchmarks | Adopt in evaluation lab |
| WebArena Verified | Realistic, reproducible browser tasks | Adopt for regression tests |
| WorkArena | Enterprise application tasks | Adopt for business-agent tests |
| VisualWebArena | Visual and multimodal browser tasks | Adopt for vision routing tests |
| AssistantBench | Long open-web tasks | Adopt for research-agent tests |
| browser-use | Open agent loop, observation reduction, model adapters, benchmark tasks | Benchmark and pattern donor |
| Skyvern | Resilient business workflows, task blocks, browser automation product patterns | Study; do not add its platform |
| Crawlee | Session pool health, adaptive concurrency, request queues, proxy/session retirement | Continue borrowing runtime patterns |
| Browserless | Persistent remote browser and BQL/CDP patterns | Runtime backend/reference |
| Kernel | VM/browser isolation and long-lived session substrate | Runtime backend/reference |
| WebDriver BiDi | Emerging W3C bidirectional cross-browser control protocol | Add adapter after CDP parity; do not replace CDP prematurely |
| W3C WebDriver | Stable cross-browser automation standard | Compatibility reference |
| WASP / WAInjectBench / PIArena | Browser prompt-injection and defense evaluation | Add to security evaluation lab |

---

## 5. New browser standards posture

### 5.1 Keep CDP as the primary Chromium hot path

CDP remains the practical choice for:

- Chrome/Chromium internals;
- network inspection;
- console/runtime events;
- DOM and accessibility access;
- screenshots and tracing;
- Browserbase/Browserless compatibility;
- stealth and browser profile control.

### 5.2 Add WebDriver BiDi as a second protocol adapter

WebDriver BiDi is a W3C working draft for bidirectional browser control and event streaming. Quarry should prepare for it because it offers a standards-based route to Firefox, Chromium, and future cross-browser parity.

Recommended posture:

```text
2026:
  CDP = primary production backend
  WebDriver BiDi = experimental adapter + standards tests

Later:
  choose backend by browser/runtime capability
```

Do not lower Quarry's capabilities to the least common denominator. Define a Quarry browser trait and report backend capability flags.

```ts
interface BrowserBackendCapabilities {
  cdp: boolean;
  webdriverBidi: boolean;
  accessibilityTree: boolean;
  networkInterception: boolean;
  tracing: boolean;
  videoReplay: boolean;
  persistentContext: boolean;
  downloads: boolean;
  uploads: boolean;
  extensions: boolean;
}
```

### 5.3 Adopt accessibility snapshots as a canonical structured observation

Playwright's ARIA snapshot and MCP snapshot format demonstrate a useful standard shape:

```yaml
- heading "Invoices" [level=1]
- textbox "Search invoices" [ref=e5]
- button "Download" [ref=e12]
- checkbox "Paid" [checked] [ref=e19]
```

Quarry should expose a compatible concept, not necessarily byte-for-byte Playwright output.

Benefits:

- lower tokens than raw DOM;
- stable semantic references;
- natural mapping to roles and accessible names;
- deterministic verification;
- works well for enterprise applications and forms.

---

## 6. Canonical Model Plane <-> Quarry browser contract

The most important improvement is a stable action/observation protocol.

### 6.1 Start browser task

```ts
interface StartBrowserTaskRequest {
  tenantId: string;
  runId: string;
  graphNodeId: string;
  browserGrantId: string;

  objective: string;
  startUrl?: string;
  allowedDomains: string[];
  forbiddenDomains: string[];

  allowedActionClasses: BrowserActionKind[];
  sideEffectsPermitted: boolean;

  successCriteria: BrowserSuccessCriterion[];
  budget: BrowserBudget;
  observationPolicy: ObservationPolicy;
}
```

### 6.2 Observation response

```ts
interface BrowserObservation {
  observationId: string;
  leaseId: string;
  sequence: number;

  url: string;
  title: string;
  pageId: string;

  structured: StructuredObservation;
  visual?: VisualObservation;
  runtime: RuntimeObservation;
  security: SecurityObservation;

  pageStateHash: string;
  previousStateDiff?: BrowserStateDiff;
  artifactRefs: string[];
}
```

### 6.3 Execute action

```ts
interface ExecuteBrowserActionRequest {
  leaseId: string;
  observationId: string;
  action: BrowserAction;

  expectedEffects: ExpectedEffect[];
  approvalAttestation?: string;
  idempotencyKey: string;
}
```

### 6.4 Action result

```ts
interface BrowserActionResult {
  actionId: string;
  status: "executed" | "blocked" | "failed" | "unknown";

  resolvedTarget?: ResolvedTarget;
  executionReceipt: ExecutionReceipt;
  nextObservation: BrowserObservation;
  verification: VerificationResult;

  recoveryHints: RecoveryHint[];
}
```

---

## 7. Browser Action IR

**Verified 2026-08-03 — this already exists; extend it, don't rebuild it.**
See §1a. `AgentAction`/`Action` enums are real (`quarry-core/src/contracts.rs`,
`quarry-browser/src/actions.rs`), dispatched through
`ObservationRunner::execute()`. The real gaps: targeting is a raw
CSS/XPath string (no ref-based `SelectorEnsemble`, §9), and `Evaluate
{ script }` lets arbitrary JavaScript run directly today — close that
escape hatch first, it's small and concrete.

Models must not emit Playwright code, JavaScript, XPath, or raw CDP commands by default.

Create a versioned browser action intermediate representation.

```ts
type BrowserAction =
  | NavigateAction
  | ClickAction
  | TypeAction
  | FillSecretAction
  | SelectAction
  | CheckAction
  | ScrollAction
  | HoverAction
  | UploadAction
  | DownloadAction
  | ExtractAction
  | WaitAction
  | SwitchTabAction
  | SwitchFrameAction
  | GoBackAction
  | SubmitAction
  | FinishAction;
```

Example:

```json
{
  "version": 1,
  "kind": "click",
  "target": {
    "elementRef": "el_137",
    "semanticFallback": {
      "role": "button",
      "accessibleName": "Continue"
    }
  },
  "expectedEffects": [
    { "kind": "url_matches", "value": "/checkout/review" }
  ],
  "riskClass": "low"
}
```

Raw script execution remains an explicit high-risk action with separate policy and sandboxing.

---

## 8. ObservationBundle

**Verified 2026-08-03 — confirmed greenfield, see §1a.** Today's structure
is a much cruder `DomSummary`/`InteractiveElement` (tag/selector/text/role)
built from raw HTML, plus separate screenshot/visual-diff artifacts — no
ARIA-snapshot tree, no Set-of-Marks. This section's proposal is real,
unbuilt work.

Quarry should produce multiple observation representations and select the cheapest sufficient mode.

```ts
interface ObservationBundle {
  url: string;
  title: string;

  ariaSnapshot?: string;
  interactiveElements?: InteractiveElement[];
  visibleText?: string;
  domDigest?: DomDigest;

  screenshotRef?: string;
  visualRegions?: VisualRegion[];
  setOfMarksRef?: string;

  forms?: FormModel[];
  tables?: TableModel[];
  dialogs?: DialogModel[];
  frames?: FrameModel[];

  networkSummary?: NetworkSummary;
  downloads?: DownloadRecord[];
  consoleSummary?: ConsoleSummary;

  injectionSignals?: InjectionSignal[];
  challengeSignals?: ChallengeSignal[];

  stateHash: string;
  diff?: ObservationDiff;
}
```

### 8.1 Observation modes

#### `structured_fast`

- ARIA snapshot;
- element refs;
- visible text summary;
- forms/dialogs;
- no screenshot unless requested.

Best for dashboards, SaaS interfaces, and forms.

#### `hybrid`

- structured observation;
- screenshot;
- selected visual regions;
- DOM/AX target linkage.

Best default for unfamiliar pages.

#### `visual`

- screenshot;
- Set-of-Marks;
- reduced semantic tree;
- bounding boxes.

Best for canvas, maps, icon-only controls, and visually rich interfaces.

#### `diagnostic`

- full structured observation;
- screenshot;
- frame/shadow DOM detail;
- network and console events;
- previous-state diff.

Use only after failure or for debugging.

#### `extract`

- rendered HTML;
- JSON-LD;
- tables;
- network/API payload candidates;
- no autonomous action unless needed.

Use for structured extraction.

---

## 9. Element references and selector ensembles

**Verified 2026-08-03 — confirmed greenfield, see §1a.** Every action
variant carries exactly one `selector: String` (CSS/XPath) today, no
ordered fallback chain. This is the concrete fix for §7's targeting gap.

A model should act on a stable Quarry element reference. Quarry resolves that reference using a ranked selector ensemble.

```ts
interface SelectorEnsemble {
  backendNodeId?: number;
  frameId?: string;
  axPath?: string[];

  role?: string;
  accessibleName?: string;
  testId?: string;
  stableAttributes?: Record<string, string>;

  textAnchor?: string;
  relationAnchor?: ElementRelation;
  css?: string;
  xpath?: string;
  visualBounds?: BoundingBox;
}
```

Resolution order:

```text
live backend node
  -> stable test ID
  -> role + accessible name
  -> stable attributes
  -> relation/text anchor
  -> CSS
  -> XPath
  -> visual grounding
```

Record which resolver succeeded. This becomes site/workflow reliability data.

---

## 10. Stagehand-style observe, preview, act, extract

Stagehand's strongest contribution is the separation of four primitives:

- observe what can be done;
- act on one selected target;
- extract typed information;
- use an agent for longer workflows.

Quarry should implement equivalent first-party concepts.

### 10.1 Observe candidates

```ts
interface ActionCandidate {
  candidateId: string;
  description: string;
  actionKind: BrowserActionKind;
  elementRef?: string;
  confidence: number;
  predictedEffect?: string;
  riskClass: string;
  requiresApproval: boolean;
}
```

### 10.2 Preview action

Before a consequential action:

```text
Action: click "Confirm purchase"
Target: button role=button name="Confirm purchase"
Predicted effect: submit order
Risk: high
Reversible: no
Verification: order confirmation + provider receipt
Approval: required
```

### 10.3 Act

Quarry executes the selected candidate through the action IR.

### 10.4 Extract

Quarry supports deterministic and model-assisted extraction with schema validation and source references.

---

## 11. Hybrid browser model routing

The proposed Fara/open-weight plus frontier-model combination should be part of a broader router rather than a fixed two-model pipeline.

The user-provided Fara and frontier benchmark figures should be reproduced in Velion's own harness before they are treated as routing truth.

### 11.1 Planner tiers

```text
B0 deterministic compiled workflow
B1 low-cost structured DOM/ARIA planner
B2 open-weight visual browser model
B3 frontier browser/reasoning model
B4 independent frontier verifier/recovery model
B5 human takeover
```

### 11.2 Routing inputs

```ts
interface BrowserRoutingContext {
  taskClass: string;
  siteProfile?: string;
  observationMode: string;
  visualComplexity: number;
  semanticTargetConfidence: number;

  authenticationRequired: boolean;
  sideEffectRisk: string;

  previousFailures: BrowserFailure[];
  remainingModelBudget: number;
  remainingBrowserSeconds: number;

  dataResidencyPolicy: string;
  availableModels: BrowserModelProfile[];
}
```

### 11.3 Example policy

```text
Known compiled workflow?
  yes -> deterministic replay
  no -> continue

High-confidence ARIA/DOM targets?
  yes -> low-cost structured planner
  no -> continue

Visual but bounded task?
  yes -> open-weight visual planner
  no -> frontier planner

Failure or uncertainty after execution?
  -> independent verifier/recovery planner

Unsafe or unresolved high-risk action?
  -> human takeover
```

### 11.4 Frontier planner + open executor

A useful hybrid mode:

```text
frontier model
  - decomposes task
  - identifies success criteria
  - identifies risky steps

open visual model
  - handles bounded visual navigation steps

Quarry
  - resolves and executes actions
  - verifies state
  - records evidence

frontier verifier
  - used only when deterministic checks are insufficient
```

Benchmark this against end-to-end open, end-to-end frontier, and deterministic routes.

---

## 12. Deterministic verification

**Verified 2026-08-03 — confirmed greenfield, see §1a.** Today, if the
driver call for an action doesn't error, `ObservationRunner::execute()`
proceeds straight to building the next observation — there is no distinct
re-check of URL/element/network state, and no `VERIFIED`/`FAILED`/`UNKNOWN`
machinery anywhere in the browser crates. This section describes a real,
currently-missing safety property, not a refinement of something existing.

The same model must not be the only judge of success.

### 12.1 State verification

- URL pattern;
- element present/absent;
- field value;
- button disabled;
- dialog state;
- ARIA snapshot partial match;
- page-state hash transition.

### 12.2 Network verification

- expected request observed;
- response status and body class;
- provider request ID;
- idempotency key returned;
- no unexpected effectful request.

### 12.3 Artifact verification

- download exists;
- MIME type is expected;
- file is valid/nonempty;
- screenshot/PDF artifact persisted;
- extraction validates against schema.

### 12.4 Provider verification

When an API capability exists, reread the provider state after a browser action.

```text
browser clicked submit
  -> capture browser confirmation
  -> call provider read API
  -> compare intended and actual state
```

### 12.5 Semantic verifier

Use a model only when deterministic checks cannot determine success. Return:

```text
VERIFIED
FAILED
UNKNOWN
```

Never convert `UNKNOWN` to success.

---

## 13. Action caching and workflow compilation

**Verified 2026-08-03 — confirmed 100% greenfield, see §1a.** A repo-wide
grep for `compiled_workflow`/`workflow_cache` and equivalents returns zero
matches anywhere in Quarry-v2. Note: §3.3's existing "profile" concept
(browser session/reconnect metadata) is unrelated to this section's
proposed workflow-recording concept — don't conflate the two when naming
new types.

Repeated successful agent behavior should become a deterministic program.

### 13.1 Candidate workflow

```yaml
workflow_id: vendor.download_latest_invoice
version: 5
site_profile: vendor_portal

steps:
  - navigate:
      url_template: "https://portal.vendor.no/invoices"

  - click:
      target:
        role: link
        accessible_name: "Invoices"
      fallbacks:
        - text: "Fakturaer"
        - test_id: "nav-invoices"

  - click:
      target:
        role: button
        accessible_name: "Download latest"

verification:
  - download_created
  - mime_type: application/pdf
```

### 13.2 Promotion process

```text
successful agent trajectory
  -> normalize actions
  -> remove volatile selectors
  -> attach semantic targets
  -> add verification
  -> replay in test environment
  -> security review
  -> candidate workflow
  -> approved workflow
```

### 13.3 Cache keys

Action caching should include:

- site profile/version;
- page-state signature;
- semantic action intent;
- browser/runtime class;
- language/locale;
- user role where relevant;
- workflow version.

Do not replay actions across incompatible tenant/user states.

---

## 14. Controlled self-healing

**Verified 2026-08-03 — confirmed 100% greenfield, see §1a.** Zero grep
hits for `repair_level`/`self_healing` anywhere in Quarry-v2 — there is
currently no repair/retry concept at all beyond whatever the driver call
itself does.

Self-healing must preserve semantics.

### Repair levels

```text
0 exact replay
1 alternate selector for same element identity
2 local re-observation and target repair
3 current-step replanning
4 remaining-workflow replanning
5 human intervention
```

For high-risk actions, only levels 0-2 may execute automatically. Levels 3-4 must re-run policy and approval when the action meaning changes.

Unsafe repair example:

```text
Expected "Approve invoice" disappeared.
Do not substitute "Send payment" merely because it looks similar.
```

---

## 15. Set-of-Marks visual grounding

Quarry should support a Set-of-Marks transform for pages where semantic accessibility is weak.

```text
[1] Search field
[2] Menu icon
[3] Product card
[4] Checkout button
```

The planner returns `targetMark: 4`, and Quarry maps it to a live element reference and bounds.

Use SoM only when:

- ARIA/DOM target confidence is low;
- canvas or custom-rendered controls are detected;
- visual layout carries essential meaning;
- the selected model is optimized for visual grounding.

Avoid it on dense pages when it adds clutter without improving accuracy.

---

## 16. Browser runtime router

Quarry should select the runtime backend separately from the planner model.

### 16.1 Runtime profiles

| Runtime | Best use |
|---|---|
| Local chromiumoxide/Chromium | Low latency, self-hosted, controlled sites |
| Browserbase | Scale, anti-bot, persistent contexts, replay, live view |
| Browserless | Remote CDP/BQL and existing infrastructure compatibility |
| Kernel | VM-level browser isolation and long-lived execution |
| WebDriver BiDi backend | Cross-browser standards and Firefox/WebKit-oriented testing |

### 16.2 Selection inputs

- site block history;
- browser feature requirements;
- profile and login requirements;
- proxy geography;
- replay/live-view requirements;
- cost;
- concurrency;
- data residency;
- tenant policy;
- runtime health;
- required browser engine.

### 16.3 Backend independence

The Browser Action IR and ObservationBundle must remain backend-neutral. Backend-specific data may be attached under `runtimeExtensions`.

---

## 17. Browserbase improvements

Quarry already supports Browserbase. Improve the integration by using its strongest runtime capabilities while keeping Velion's own control plane.

### Recommended use

- persistent contexts for login state;
- keep-alive and reconnect;
- live-view takeover;
- HLS session replay embedded in Velion;
- browser/session metadata bound to tenant/run;
- multitab recording;
- Agent Identity/verified fingerprints when policy permits;
- geolocated proxy affinity;
- download/upload APIs.

### Do not adopt

- Browserbase Model Gateway as Model Plane's model authority;
- Browserbase Functions as canonical orchestration;
- Browserbase dashboard as Velion's customer administration UI.

Velion should show Browserbase replay/live-view through its own SolidJS task UI where licensing and API terms allow.

---

## 18. Human-in-the-loop browser control

The browser task UI should support:

- watch live;
- take control;
- return control to agent;
- approve an exact action;
- edit safe action arguments;
- provide one-time credentials without exposing them to the model;
- resolve CAPTCHA/2FA;
- upload a file;
- mark a task failed or complete.

### Control transfer event

```ts
interface BrowserControlTransfer {
  leaseId: string;
  from: "agent" | "human";
  to: "agent" | "human";
  actorId: string;
  reason: string;
  timestamp: string;
}
```

When a human takes control, Model Plane must not continue issuing actions until control is explicitly returned.

---

## 19. Credential and identity safety

### 19.1 Secret references

The model emits:

```json
{
  "kind": "fill_secret",
  "target": { "elementRef": "password_1" },
  "secretRef": "vault://connection/123/password"
}
```

Quarry resolves the secret at the trusted executor boundary.

The next observation says only:

```text
password field populated: true
value: [REDACTED]
```

### 19.2 Context isolation

Use one saved browser context per:

```text
tenant + provider/site + identity + environment
```

Avoid simultaneous sessions mutating the same context unless the site and context implementation explicitly support it.

### 19.3 Identity changes

Detect login/session changes and invalidate the site profile or workflow cache when:

- account changes;
- user role changes;
- context is refreshed;
- geolocation changes;
- session is logged out;
- site forces reauthentication.

---

## 20. Prompt-injection containment

Browser content is untrusted data.

### 20.1 Observation labeling

Represent page text as:

```xml
<untrusted_web_content origin="https://example.com" observation_id="obs_123">
  ...
</untrusted_web_content>
```

### 20.2 Injection signals

Detect and report:

- instructions to ignore prior/system instructions;
- fake tool-call syntax;
- requests for secrets or credentials;
- hidden text that differs from visible text;
- text targeting the agent rather than the human user;
- suspicious instructions embedded in images;
- attempts to navigate outside allowed domains;
- requests to disable safety or audit.

### 20.3 Security policy

Injection detection is advisory. Actual security comes from:

- domain/action allowlists;
- browser grants;
- capability policy;
- credential isolation;
- approval gates;
- deterministic action IR;
- no raw arbitrary tool access;
- verification.

### 20.4 Evaluation

Add browser security suites based on:

- WASP;
- WAInjectBench;
- PIArena;
- BrowserGym security environments/DoomArena where available;
- Velion-specific malicious pages.

---

## 21. Unified search and browser intelligence

**Verified 2026-08-03 — confirmed real, not aspirational, see §1a.**
`smart_router.rs` genuinely routes Tantivy → Stract → SearXNG → Brave
(+Serper) with intent classification, per-provider circuit breakers, and a
tenant-isolated TTL cache; `answer.rs`'s `AnswerPipeline` is a real
search→scrape→synthesize orchestrator. Build on this directly.

Quarry already has SmartSearchRouter and AnswerPipeline. Agentic browsing should integrate with them rather than create a separate research stack.

```text
user task
  -> Model Plane research/query plan
  -> SmartSearchRouter discovery
  -> result fusion and source scoring
  -> Quarry acquisition route per URL
       cached corpus
       static fetch
       impersonated fetch
       browser render
       browser interaction
  -> evidence sufficiency check
  -> additional search or browsing if needed
  -> cited synthesis in Model Plane
```

### 21.1 Search-to-browser escalation

Escalate when:

- static fetch is blocked;
- content is JS-rendered;
- the result requires in-page search/filtering;
- authentication is required;
- a download or action is required;
- evidence is hidden behind tabs, menus, or pagination.

### 21.2 Browser-to-search recovery

Search again when:

- the site has moved content;
- official documentation reveals a direct URL;
- the browser workflow encounters an unknown product term;
- additional independent evidence is required;
- the task should use an API rather than the UI.

### 21.3 Evidence distinction

A search result is discovery evidence. A fetched passage, screenshot, network response, or downloaded artifact is factual evidence.

Do not cite only a search snippet when full evidence can be captured.

---

## 22. Result fusion and evidence quality

**Verified 2026-08-03** — today's fusion is simple URL-based dedup +
provider-priority rank renumbering (`smart_router.rs:merge_dedupe`); no
RRF, source-class weighting, or evidence-sufficiency logic exists. This
section is correctly scoped as new work.

Improve SmartSearchRouter with:

- canonical URL normalization;
- duplicate and syndication clustering;
- Reciprocal Rank Fusion;
- source authority classification;
- freshness scoring;
- language/country scoring;
- semantic reranking;
- domain diversity/MMR;
- provider quality learned from accepted evidence.

### 22.1 Source class

```ts
type SourceClass =
  | "primary"
  | "official_documentation"
  | "research_paper"
  | "government"
  | "company_claim"
  | "independent_reporting"
  | "community"
  | "aggregator"
  | "unknown";
```

### 22.2 Evidence sufficiency

```ts
interface EvidenceSufficiency {
  minimumSourcesMet: boolean;
  primarySourcePresent: boolean;
  independentSources: number;
  freshnessSatisfied: boolean;
  contradictionDetected: boolean;
  citationCoverage: number;
  continueResearch: boolean;
}
```

---

## 23. Firecrawl comparison and parity improvements

Firecrawl's current product surface emphasizes:

- search with full content;
- scrape to Markdown, HTML, screenshot, and JSON;
- interact against a persistent scrape/session ID;
- autonomous agent endpoint;
- crawl;
- map;
- batch scrape;
- actions before extraction;
- media/document parsing;
- live-view URLs;
- simple SDK and CLI ergonomics;
- MCP/agent skill exposure.

Quarry already covers much of this. The highest-value parity improvements are below.

### 23.1 Stateful interact API

Add a clear public contract:

```text
POST /v1/browser/sessions
POST /v1/browser/sessions/{id}/observe
POST /v1/browser/sessions/{id}/actions
POST /v1/browser/sessions/{id}/extract
GET  /v1/browser/sessions/{id}
POST /v1/browser/sessions/{id}/close
```

This should map to Quarry leases and profiles rather than introduce a new session authority.

### 23.2 Agent endpoint

Expose an optional high-level endpoint through Quarry edge, but execution remains split:

```text
POST /v1/agent
  -> Model Plane plan
  -> Quarry search/fetch/browser tools
  -> artifacts and citations
```

The endpoint is a product facade, not a second agent runtime.

### 23.3 Map performance

Improve URL discovery through:

- sitemap indexes;
- robots references;
- HTML links;
- known framework route manifests;
- RSS/Atom feeds;
- canonical/hreflang links;
- browser network route discovery;
- optional Common Crawl hints;
- bounded site search.

### 23.4 Batch and async ergonomics

- stable job IDs;
- SSE event streams;
- webhook delivery;
- partial result pagination;
- predictable cancellation;
- explicit per-item failure reason;
- SDK polling helpers;
- idempotent batch requests.

### 23.5 Format contracts

Keep format output independent from driver choice:

- markdown;
- cleaned HTML;
- raw HTML;
- links;
- images;
- screenshot;
- PDF;
- JSON schema extraction;
- summary;
- query answer;
- branding;
- audio when supported;
- change tracking;
- browser trace;
- network evidence.

### 23.6 SDK quality

Firecrawl's simplicity is a competitive advantage. Quarry SDKs should provide:

```ts
quarry.search()
quarry.scrape()
quarry.extract()
quarry.crawl()
quarry.map()
quarry.answer()
quarry.agent()
quarry.browser.createSession()
quarry.browser.observe()
quarry.browser.act()
quarry.browser.extract()
quarry.jobs.watch()
```

The SDK should hide REST/gRPC/NATS implementation details.

---

## 24. Browser tracing and replay

### 24.1 Step trace

```ts
interface BrowserStepTrace {
  runId: string;
  graphNodeId: string;
  stepId: string;

  plannerModel: string;
  routeDecisionId: string;

  observationMode: string;
  observationTokens: number;
  screenshotRef?: string;

  proposedAction: BrowserAction;
  resolvedTarget?: ResolvedTarget;
  backend: string;

  executionDurationMs: number;
  stateBefore: string;
  stateAfter: string;

  verification: VerificationResult;
  failureClass?: string;
  recoveryLevel?: number;

  modelCost: number;
  browserCost: number;
}
```

### 24.2 Replay sources

- Quarry action/observation events;
- screenshots and artifacts;
- Playwright-compatible traces where an adapter is used;
- Browserbase HLS replay;
- network and console summaries;
- optional native Quarry video/frames for local sessions.

### 24.3 App Shell

The SolidJS UI should display:

- live browser;
- current action and predicted effect;
- planner/model route;
- evidence timeline;
- approval request;
- verification result;
- replay after completion;
- downloadable artifacts.

---

## 25. Site profiles and learned reliability

**Verified 2026-08-03 — confirmed 100% greenfield, see §1a.** Distinct
from §3.3's existing browser-session "profile" (reconnect/proxy-affinity
metadata) — that concept does not overlap with or partially satisfy this
one. Zero grep hits for `site_profile`/`SiteProfile` anywhere in Quarry-v2.

```ts
interface SiteProfile {
  profileId: string;
  hostPattern: string;
  version: number;

  preferredBackends: string[];
  requiredObservationModes: string[];

  loginSignals: string[];
  challengeSignals: string[];
  logoutSignals: string[];

  stableTargets: Record<string, SelectorEnsemble>;
  compiledWorkflows: string[];

  waitStrategy: WaitStrategy;
  proxyPolicy?: ProxyPolicy;

  knownFailures: FailurePattern[];
  successStatistics: SiteSuccessStatistics;
}
```

Promote site knowledge only from verified runs. Treat it as a reviewed Quarry resource, not uncontrolled model memory.

---

## 26. Adaptive browser concurrency

Continue Crawlee-inspired session health and add runtime-aware scheduling.

### Inputs

- host rate limits;
- robots/crawl-delay;
- block/challenge rate;
- CPU/memory per browser;
- backend concurrency quota;
- model latency;
- proxy capacity;
- session health;
- tenant budget.

### Policy

```text
healthy host + low block rate
  -> increase concurrency gradually

rising 429/403/challenges
  -> lower concurrency
  -> increase delay
  -> rotate or retire session
  -> escalate runtime only if policy allows
```

Keep browser-agent action sequences serial within one tab unless the workflow explicitly supports parallel pages.

---

## 27. Browser benchmark program

### 27.1 Public benchmarks

Use BrowserGym/AgentLab adapters for:

- WebArena Verified;
- WebArena Lite where useful;
- VisualWebArena;
- WorkArena;
- AssistantBench;
- WebLINX;
- MiniWoB for primitive actions;
- OpenApps and TimeWarp as appropriate.

### 27.2 Velion BrowserBench

Create business-relevant suites:

#### Knowledge acquisition

- find primary sources;
- compare multiple vendors;
- extract cited structured facts;
- recover from moved pages.

#### Enterprise workflows

- Microsoft 365;
- SuperOffice;
- CRM/ERP portals;
- shipping portals;
- support systems.

#### Authentication

- reuse context;
- expired login;
- 2FA handoff;
- role changes;
- file upload/download.

#### High-risk actions

- draft but do not send;
- stop at final confirmation;
- require approval;
- verify provider state.

#### Recovery

- stale selector;
- popup;
- new tab;
- iframe;
- shadow DOM;
- changed language;
- rate limit;
- browser disconnect;
- partial action success.

#### Security

- visible prompt injection;
- hidden prompt injection;
- malicious image instructions;
- fake system/tool messages;
- credential requests;
- cross-domain exfiltration attempts.

### 27.3 Benchmark matrix

```text
planner model
x observation mode
x execution backend
x verification strategy
x task class
```

Track:

- verified completion rate;
- false-success rate;
- intervention rate;
- action count;
- recovery count;
- model tokens;
- browser seconds;
- total cost;
- p50/p95 duration;
- security violations.

---

## 28. Outcome-driven routing and learning

Every browser run creates a routing example:

```text
site/task state
+ selected observation mode
+ selected planner model
+ selected runtime backend
+ action sequence
+ failures/recovery
+ verified outcome
+ cost/latency
= browser routing record
```

Use this to learn:

- which model works best for a site/task class;
- which sites need screenshots;
- which workflows can become deterministic;
- when Browserbase improves success enough to justify cost;
- which selectors remain stable;
- which proxy/session profiles work;
- which action types produce unknown outcomes.

Start with rule-based routing and a LightGBM/ranker-style offline model before using another LLM as the router.

---

## 29. Service and crate improvements

### `quarry-core`

Add:

- Browser Action IR;
- ObservationBundle;
- verification contracts;
- browser backend capabilities;
- browser trace event schemas;
- versioned failure/recovery reason codes.

### `quarry-browser`

**Verified 2026-08-03 — the two most urgent items in this whole document
live here (see §1a/§31 Phase 0)**: wire SSRF/DNS-guard request
interception into `chromiumoxide.rs`'s `goto()`/`open_tab_page()` (today,
zero enforcement of its own), and gate the existing `Evaluate { script }`
action behind explicit high-risk policy instead of allowing raw JS
execution as an ordinary action.

Add:

- **SSRF/DNS-guard interception in the CDP driver (do first)**;
- **gate the raw-JS `Evaluate` action (do first)**;
- ARIA snapshot builder;
- stable element refs (extend the existing `Action`/`AgentAction` enums
  with ref-based targeting rather than a parallel IR);
- selector ensemble resolver;
- Set-of-Marks transform;
- WebDriver BiDi experimental backend;
- deterministic verifiers;
- control takeover state;
- multitab/frame model;
- credential injection action.

### `quarry-runtime`

**Verified 2026-08-03**: fix the static fetch driver's redirect-following
SSRF bypass (`fetch.rs`'s `Policy::limited(5)`) and `dns_guard.rs`'s
TOCTOU gap (§1a/§31 Phase 0) before adding new routing logic on top of
this crate. `smart_router.rs`'s SmartSearchRouter is real and solid —
build the browser/search integration on it, not around it.

Add:

- **the two SSRF fixes above (do first)**;
- browser planner/executor adapter boundary;
- runtime backend router;
- observation mode router;
- action cache (confirmed greenfield);
- self-healing levels (confirmed greenfield);
- site-profile scoring (confirmed greenfield, distinct from the existing
  session/reconnect "profile" concept);
- browser security signal extraction;
- outcome-driven statistics.

### `quarry-transform`

Add:

- ARIA/semantic snapshot normalization;
- visual regions and SoM artifacts;
- page-state diff;
- static branding extraction;
- network-derived structured-data candidates;
- improved rich-document extraction benchmarked against Docling/Unstructured/Kreuzberg.

### `quarry-control`

Add:

- browser session resource views;
- durable site profiles;
- compiled workflows;
- action-cache metadata;
- replay/artifact manifests;
- runtime/model benchmark results;
- lifecycle reconciliation.

### `quarry-orchestrator`

Add Temporal workflows for:

- agentic browser supervision;
- approval wait/resume;
- human takeover timeout;
- session reconnect;
- workflow promotion tests;
- benchmark runs;
- profile health checks;
- replay retention/deletion.

### Model Plane `browser-broker`

**Verified 2026-08-03**: this service is real and already does domain
restriction + revocation correctly — the list below is the accurate
remaining gap, not a from-scratch build. Its own package doc comment
states grant storage is in-memory today ("in-memory storage for browser
grant lifecycle") — no SQL/pgx anywhere in the package.

Add:

- **durable grants (confirmed: currently a `sync.Mutex` + in-memory map,
  lost on restart)**;
- **action-class restrictions (confirmed absent — only domain scoping
  exists in the proto/Go code today)**;
- **Quarry lease binding (confirmed absent — no lease/session cross-reference
  to Quarry found)**;
- **exact credential references (confirmed absent from the proto)**;
- grant revocation events (revocation itself is real; there is no
  event/audit log emitted when it happens — add one);
- **browser runtime cost metadata (confirmed absent from the proto)**.

### Model Plane `execution-core`

Add:

- browser GraphNode executor;
- planner model adapter;
- next-action schema;
- semantic verifier fallback;
- bounded recovery policy;
- no direct browser provider calls.

---

## 30. Adoption matrix

### Adopt as embedded/runtime standards

| Technology or pattern | Use |
|---|---|
| CDP | Primary Chromium execution protocol |
| WebDriver BiDi | Experimental cross-browser adapter |
| Accessibility/ARIA snapshots | Canonical structured observation concept |
| axe-core | Accessibility metadata and QA tool |
| Browserbase APIs | Optional managed session backend |
| BrowserGym/AgentLab | Evaluation lab |
| WebArena Verified/WorkArena/VisualWebArena | Regression benchmarks |

### Build natively

- Browser Action IR;
- ObservationBundle;
- element refs and selector ensembles;
- action preview;
- verification;
- model/backend router;
- browser grants;
- action cache;
- workflow promotion;
- site profiles;
- traces and artifacts;
- prompt-injection boundaries;
- Velion BrowserBench.

### Use as donors or lab adapters

- Stagehand;
- browser-use;
- Skyvern;
- Firecrawl agent;
- Crawlee;
- Playwright MCP/CLI;
- Browserless BQL patterns.

### Avoid as production authorities

- Stagehand or browser-use owning sessions and policy;
- Firecrawl as the default execution fallback;
- another visual browser workflow UI;
- raw model-generated Playwright/JavaScript execution;
- browser provider dashboards as the customer control plane;
- unverified model self-report as completion proof.

---

## 31. Implementation phases

### Phase 0: contract and correctness

**Resequenced 2026-08-03 against verified reality (see §1a). The original
7 items are pushed after 5 newly-found, already-live security/reliability
gaps — these are exploitable-in-shape today, not proposed work, and
nothing below is meaningfully safer until they close.**

1. **Fix the headless-browser SSRF bypass** — wire `quarry_security`/
   `dns_guard` request interception (CDP `Fetch.enable`/Network
   interception) into `chromiumoxide.rs` so every navigation and
   sub-resource request is checked, not just a single top-level preflight;
   un-ignore `tests/ssrf.rs` so this can't silently regress again.
2. **Fix the static-driver redirect SSRF bypass** — replace
   `Policy::limited(5)` with a custom redirect policy that re-runs the
   preflight/DNS-guard check against every `Location` before following it.
3. **Fix `dns_guard`'s TOCTOU gap** — pin the checked IP into the actual
   connection (e.g. `reqwest`'s `resolve()`/`resolve_to_addrs()`) instead
   of re-resolving the hostname independently at connect time; apply the
   same fix to imports-core's Python guard, which has the identical gap.
4. **Add the missing `is_unspecified()` check to the IPv4 SSRF heuristics**
   (both the URL-literal and post-DNS branches) — `0.0.0.0` currently
   bypasses both, and Linux treats it as `127.0.0.1` on connect.
5. **Close the raw-JavaScript escape hatch in the existing Browser Action
   IR** — gate `Evaluate { script }` behind an explicit high-risk policy
   rather than allowing it as an ordinary action (§7).
6. Define Browser Action IR v1 *(largely already exists — extend the real
   `AgentAction`/`Action` enums with ref-based targeting, don't replace
   them, §7/§9)*.
7. Define ObservationBundle v1 (confirmed greenfield, §8).
8. Bind BrowserBroker grants to Quarry leases (confirmed absent today,
   along with action-class restriction and credential-reference binding —
   §1a/§2).
9. Add deterministic verification outcomes (confirmed greenfield, §12).
10. Make approval continuation durable — **cross-reference
    `MODEL_PLANE_IMPROVEMENTS_2026.md` §1a before starting this**: the
    equivalent work in Model Plane's own approval path is ~70% done
    already (a durable delivery-outbox exists; only the execution-core
    dispatcher is missing) — check whether this item is really about
    binding browser-specific actions into that same mechanism rather than
    building a second one.
11. Add complete browser step traces.
12. Standardize failure and recovery reason codes.

**Also fold in, lower urgency but real and already found**: an
unrecognized `artifact_backend` config value silently downgrades to a
non-durable in-memory artifact store with only an info-level log (add a
startup failure or explicit opt-in flag instead); the filesystem artifact
index is an unbounded, linearly-scanned flat file (a real scaling cliff at
production volume, not urgent — address alongside Phase 3's reliability
work).

### Phase 1: observation quality

1. ARIA/accessibility snapshots with refs.
2. Interactive element model.
3. Selector ensembles.
4. hybrid screenshot/semantic mode.
5. page-state diff.
6. form/dialog/table/frame models.

### Phase 2: hybrid model routing

1. Open visual-model adapter.
2. Frontier browser-planner adapter.
3. low-cost structured planner.
4. deterministic workflow route.
5. bounded escalation policy.
6. independent semantic verifier.

### Phase 3: reliability and self-healing

1. observe/preview/act primitives.
2. action cache.
3. repair levels.
4. site profiles.
5. workflow compiler and promotion.
6. Browserbase replay/live-view integration in SolidJS.

### Phase 4: standards and breadth

1. WebDriver BiDi experimental adapter.
2. Firefox/WebKit-oriented tests where supported.
3. richer download/upload and extension support.
4. improved map/route discovery.
5. static branding and rich-document parity.
6. agent/interact public API and SDK improvements.

### Phase 5: evaluation and learning

1. BrowserGym adapter.
2. AgentLab execution.
3. WebArena Verified.
4. WorkArena.
5. VisualWebArena.
6. Velion BrowserBench.
7. prompt-injection suites.
8. outcome-based model/runtime routing.

---

## 32. Acceptance criteria

Quarry's browser-agent improvements are successful when:

- Model Plane can switch planner models without changing browser execution code;
- browser actions are typed, inspectable, and replayable;
- most enterprise workflows use structured observations rather than screenshots alone;
- visual mode is selected only when it improves expected success;
- a clicked button is never treated as proof of business success;
- every effectful action has approval, idempotency, and verification where required;
- repeated verified trajectories can be promoted into deterministic workflows;
- self-healing cannot silently substitute a different business action;
- Browserbase, local Chromium, Browserless, and future backends share one Quarry contract;
- browser sessions can be watched, taken over, resumed, and replayed in Velion's own UI;
- benchmark results identify the best planner/observation/backend combination by task class;
- security tests include indirect prompt injection and credential-exfiltration attempts;
- Firecrawl-level endpoint and SDK ergonomics are available without making Firecrawl a dependency.

---

## 33. Recommended final posture

```text
Model Plane
  smart task planning
  step-level model routing
  risk and approval
  semantic verification

Quarry V2
  browser observations
  safe action execution
  deterministic verification
  evidence and artifacts
  self-healing and workflow compilation
  search/fetch/browser acquisition routing

BrowserBroker
  exact grants and revocation

Runtime backends
  local Chromium
  Browserbase
  Browserless
  Kernel
  WebDriver BiDi-compatible browsers

Evaluation
  BrowserGym
  AgentLab
  WebArena Verified
  WorkArena
  VisualWebArena
  Velion BrowserBench
```

Quarry V2 should become the **governed browser and web-intelligence execution engine** underneath Model Plane, not another end-to-end black-box browser agent.

---

## 34. Research references

### Firecrawl

- Firecrawl repository: https://github.com/firecrawl/firecrawl
- Firecrawl open web-agent announcement: https://www.firecrawl.dev/blog/firecrawl-agent-open-source

### Browser agent frameworks and runtimes

- Stagehand: https://github.com/browserbase/stagehand
- Stagehand agent modes: https://docs.stagehand.dev/v3/basics/agent
- Browserbase browser agents: https://docs.browserbase.com/use-cases/agents
- Browserbase live view: https://docs.browserbase.com/platform/browser/observability/session-live-view
- Browserbase session replay: https://docs.browserbase.com/platform/browser/observability/session-replay
- Browserbase contexts: https://docs.browserbase.com/platform/browser/core-features/contexts
- browser-use: https://github.com/browser-use/browser-use
- BrowserGym: https://github.com/ServiceNow/BrowserGym
- WebArena: https://github.com/web-arena-x/webarena

### Standards and structured observations

- W3C WebDriver BiDi: https://www.w3.org/TR/webdriver-bidi/
- W3C WebDriver: https://www.w3.org/TR/webdriver/
- Playwright ARIA snapshots: https://playwright.dev/docs/aria-snapshots
- Playwright MCP snapshots: https://playwright.dev/mcp/snapshots

### Security evaluation

- WASP browser-agent prompt-injection benchmark: https://arxiv.org/abs/2504.18575
- WAInjectBench: https://arxiv.org/abs/2510.01354
- PIArena: https://arxiv.org/abs/2604.08499

