# Quarry V2 Browser Automation Improvements 2026

**Status:** Architecture, standards, research, and implementation proposal  
**Scope:** `apps/Ingestion Plane/Quarry-v2`, Model Plane browser planning, BrowserBroker, browser runtime adapters, and App Shell replay/approval UX  
**Research date:** 2026-08-03  
**Verified against the running stack:** 2026-08-03 (same day) — every major proposed component cross-checked against actual Rust/Go source in `Quarry-v2`, with file:line citations. See §1a for the full ledger. Two findings change how this document should be prioritized: the SSRF/DNS controls in P0 item 1 are not merely "incomplete," they are confirmed broken in the headless-browser path today (a real security gap, not a hardening exercise), and the "in-memory frontier" in P0 item 2 has a durable Postgres replacement **already written** in Rust — feature-flagged off and with zero production callers. Both are smaller, more urgent, and more concrete than they read as proposals. Re-verify before trusting anything below without a citation next to it.  
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

## 1a. Verification pass against the running stack — 2026-08-03

This document was written as a research proposal. This section is the
difference between that and reality: every major component was checked
against the actual current Rust/Go source (file:line cited), including
findings from a separate down-the-stack code-health audit run earlier the
same day. Re-run this check before treating anything below as still
accurate.

**Read this first — three corrections that change how the rest of the
document should be read.**

1. **§48 P0 item 1 ("complete SSRF/DNS/address-authority controls") is not
   hardening work on an incomplete system — it is a confirmed, currently
   exploitable-in-shape gap.** `crates/quarry-browser/src/chromiumoxide.rs`'s
   `goto()`/`open_tab_page()` call the CDP browser directly with **zero**
   call into `quarry_security`/`dns_guard` anywhere in the file — the
   crate's own `tests/ssrf.rs` asserts `goto()` must reject metadata/
   loopback/private URLs, an assertion the implementation cannot satisfy,
   and every test in that file is permanently `#[ignore]`d, so this has
   never been caught by CI. Three more, independently confirmed: the static
   fetch driver (`crates/quarry-runtime/src/fetch.rs`) auto-follows
   redirects (`Policy::limited(5)`) with no re-check against a private/
   metadata destination; `dns_guard.rs` resolves once, discards the IPs,
   and hands the driver a re-resolvable *hostname* — a textbook DNS-rebinding
   TOCTOU, duplicated independently in imports-core's Python guard
   (`network_policy.py`); and `heur.rs`'s IPv4 checks never test
   `is_unspecified()`, so a literal `0.0.0.0` (which Linux treats as a
   loopback connection) passes every guard. This is the single highest-
   priority item in this entire document — it directly undermines CLAUDE.md's
   architecture rule that Quarry-v2 is the trusted arbiter of browser
   actions, and it is exploitable in shape today, not theoretical.
2. **§37.2/§41/§48 P0 item 2 ("finish the durable request frontier") has a
   real, already-written Postgres implementation sitting completely
   unwired — this is a wiring task, not a from-scratch build.** The
   production crawl/batch path is a Go/Temporal workflow
   (`quarry-orchestrator/internal/workflows/workflows.go:371,313`) using a
   **plain in-process Go slice and map** (`frontier :=
   make([]frontierEntry, ...)`, `visited := map[string]struct{}{}`) — its
   "durable checkpoint every 50 pages" only POSTs progress *counts* to
   Quarry Control, never the actual frontier URLs or seen-set, so real
   resumability rides on Temporal's own workflow-history replay, not on any
   queryable durable store. Meanwhile a **complete**, correctly-designed
   Postgres queue already exists in Rust —
   `crates/quarry-runtime/src/{request_queue,postgres_queue,crawl_frontier}.rs`,
   migration `0001_request_queue.sql`, with `SELECT FOR UPDATE SKIP LOCKED`,
   visibility timeouts, and org-scoped isolation — but it is gated behind a
   cargo feature not in the default build, constructed only in its own test
   module, and `quarry-control/internal/resources/cycle23.go:255-263`'s
   `MountRequestQueues` literally comments "Go control reads it without
   owning migrations... cycle 24 will surface it" and always returns an
   empty page today. Wire the existing schema into the Go orchestrator and
   into `cycle23.go`'s read side — do not design a new one.
3. **Most of the ~7 new schemas this document proposes (§7, §8, §25, §38,
   §39, §42) are genuinely greenfield, confirmed by direct source read —
   but two are not, and one existing system is a real strength to build on,
   not rebuild.** See the per-component ledger below.

**Per-component ledger** (EXISTS / PARTIAL / GREENFIELD), cross-referenced
inline at each relevant section:

| Doc section | Proposed component | Verified state |
|---|---|---|
| §5.3 | ARIA/accessibility snapshot observation | GREENFIELD — raw HTML regex-scraping today, no CDP AXTree call anywhere |
| §7 | Browser Action IR | **EXISTS** — a real typed `Action` enum already, see §7 note |
| §8 | ObservationBundle | PARTIAL — a real `BrowserObservation` struct exists, missing ARIA/forms/dialogs fields |
| §9 | Selector ensembles (ranked resolver fallback) | GREENFIELD — single CSS selector today, no fallback, no resolver-success tracking |
| §12 | Deterministic postcondition verification | GREENFIELD — "no exception ⇒ success" is the entire model today |
| §18 | Browser-action approval continuation | GREENFIELD as a Quarry concept — Quarry only validates grants Model Plane issues, never originates its own approval/pause-resume; the only pause/resume today is whole-run-level, not per-action |
| §21 | SmartSearchRouter / AnswerPipeline | **Real and working already — extend, don't rebuild.** See §21 note |
| §25 | Site profiles (preferred backend, success stats per host) | GREENFIELD — a `ProfileStore` exists but for a different concept (auth session cookies/storage), not host reliability |
| §37.2/§41 | Durable crawler frontier | PARTIAL, see correction 2 above — schema exists, unwired |
| §38 | ElementFingerprint / Adaptive Target Memory | GREENFIELD, confirmed |
| §39.1 | Challenge/failure classification | PARTIAL — two real enums exist but lump 401/403/451/999/CDN-challenge into one bucket by explicit design comment |
| §39.4 | Runtime compatibility manifest (real probe, not process health) | GREENFIELD |
| §42 | Compiled/deterministic workflow with rollout state | GREENFIELD — only an unrelated session video-replay concept exists |
| §48 P0.1 | SSRF/DNS/address-authority controls | **BROKEN TODAY**, see correction 1 above |

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

---

## 3. Existing Quarry strengths to preserve

### 3.1 Execution/control split

Keep:

- Rust hot path for fetch, browser, action, transform, artifacts, and events;
- Go control for durable jobs, profiles, history, schedules, and Temporal orchestration;
- Python for benchmarks, experiments, and model adapters only.

### 3.2 Driver waterfall

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

| System                         | Strongest pattern                                                                                    | Quarry decision                                              |
| ------------------------------ | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| Firecrawl                      | Unified search/scrape/interact/agent/crawl/map/batch API, simple SDKs, scrape IDs, live view         | Product and API parity reference; do not depend on it        |
| Firecrawl open agent           | Search/fetch/extract primitives behind an open web research loop                                     | Benchmark and research donor                                 |
| Stagehand                      | `observe`, `act`, `extract`, `agent`, action preview, caching, self-healing, DOM/vision/hybrid modes | Strong architectural donor; optional lab adapter             |
| Browserbase                    | Persistent contexts, Agent Identity, live view, replay, CDP sessions, action caching                 | Continue as optional managed runtime backend                 |
| Playwright                     | Auto-waits, locators, traces, accessibility snapshots, multi-browser tooling                         | Use formats and behavior as reference; optional adapter/lab  |
| Playwright MCP/CLI             | Accessibility snapshot with stable element refs for LLM actions                                      | Adopt compatible observation concepts                        |
| BrowserGym                     | Standard action/observation environment and broad benchmark set                                      | Adopt in Python evaluation lab                               |
| AgentLab                       | Run agents, collect traces, compare benchmarks                                                       | Adopt in evaluation lab                                      |
| WebArena Verified              | Realistic, reproducible browser tasks                                                                | Adopt for regression tests                                   |
| WorkArena                      | Enterprise application tasks                                                                         | Adopt for business-agent tests                               |
| VisualWebArena                 | Visual and multimodal browser tasks                                                                  | Adopt for vision routing tests                               |
| AssistantBench                 | Long open-web tasks                                                                                  | Adopt for research-agent tests                               |
| browser-use                    | Open agent loop, observation reduction, model adapters, benchmark tasks                              | Benchmark and pattern donor                                  |
| Skyvern                        | Resilient business workflows, task blocks, browser automation product patterns                       | Study; do not add its platform                               |
| Crawlee                        | Session pool health, adaptive concurrency, request queues, proxy/session retirement                  | Continue borrowing runtime patterns                          |
| Browserless                    | Persistent remote browser and BQL/CDP patterns                                                       | Runtime backend/reference                                    |
| Kernel                         | VM/browser isolation and long-lived session substrate                                                | Runtime backend/reference                                    |
| WebDriver BiDi                 | Emerging W3C bidirectional cross-browser control protocol                                            | Add adapter after CDP parity; do not replace CDP prematurely |
| W3C WebDriver                  | Stable cross-browser automation standard                                                             | Compatibility reference                                      |
| WASP / WAInjectBench / PIArena | Browser prompt-injection and defense evaluation                                                      | Add to security evaluation lab                               |

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

**Verified 2026-08-03 — GREENFIELD, confirmed.** Today's observation
(`crates/quarry-runtime/src/observation.rs:214-224,610-644`) pulls raw HTML
via `browser.content()` and builds a `DomSummary` with **regex/string
matching** (`html.match_indices("<button")` etc.) over 5 hardcoded tags,
pulling attributes only if literally present in markup. There is no CDP
`Accessibility.getFullAXTree` call anywhere in `quarry-browser`, no computed
accessible-name logic, and no ref-based node identity. This section's
proposal is the correct fix for a real gap, not incremental polish.

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

**Verified 2026-08-03 — EXISTS.** `crates/quarry-browser/src/actions.rs:7`
already defines a real, serde-tagged `Action` enum (Wait/WaitFor/Click/
ClickPoint/Type/Scroll/MouseWheel/Screenshot/Pdf/Evaluate/Navigate/Press/
Select/Back/Forward) plus `ActionScript{actions, on_error:
Abort|Continue|Retry}` — a typed IR planners can already emit instead of
raw CDP/JS. Extend this with the `version` field, `expectedEffects`, and
`riskClass` this section proposes rather than introducing a parallel enum.

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
	"expectedEffects": [{ "kind": "url_matches", "value": "/checkout/review" }],
	"riskClass": "low"
}
```

Raw script execution remains an explicit high-risk action with separate policy and sandboxing.

---

## 8. ObservationBundle

**Verified 2026-08-03 — PARTIAL, extend don't replace.**
`crates/quarry-core/src/contracts.rs:19`'s `BrowserObservation` already
covers `url`, `title`, `dom_summary` (node_count + interactive_elements
with tag/selector/text/role), `screenshot_artifact_id`,
`visual_observation_artifact_id`, `console_summary`, `network_summary`, and
`policy_denials`. Missing: an ARIA-tree field (correctly, since none is
built yet — see §5.3), and `forms`/`dialogs`/`tables`/`frames` fields.
Extend the existing struct.

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

**Verified 2026-08-03 — GREENFIELD, confirmed.**
`crates/quarry-browser/src/actions.rs:15-17,22-25`'s `Action::Click`/
`Action::Type` carry a single `selector: String` field, and
`chromiumoxide.rs:1397-1487` calls `page.find_element(selector)`
(chromiumoxide's single-CSS-selector resolution) exactly once — on
failure it errors immediately. No fallback to backend-node-id, test-id,
role+accessible-name, or XPath exists, and nothing tracks which resolver
succeeded. `DomSummary.InteractiveElement.selector` (what the agent
actually sees) is likewise a synthesized single CSS string, not a ranked
set. This section is real, unbuilt work, not a refinement.

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

**Verified 2026-08-03 — GREENFIELD, confirmed.**
`crates/quarry-runtime/src/action_runtime.rs:266-300`: every action (Click,
Type, etc.) simply calls the driver method and treats the absence of an
`Err` as success — there is no follow-up check of URL change, element
appearance/disappearance, download existence, or expected network request
anywhere (`postcondition`/`verify_action`/`url_changed`/`network_idle`/
`download_exists` all return zero grep hits across `quarry-browser` and
`quarry-runtime`). Today's model is exactly "action sent, no exception ⇒
success" — this section is the fix for a real, currently-live gap, not
hardening of a partial system.

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

| Runtime                      | Best use                                                    |
| ---------------------------- | ----------------------------------------------------------- |
| Local chromiumoxide/Chromium | Low latency, self-hosted, controlled sites                  |
| Browserbase                  | Scale, anti-bot, persistent contexts, replay, live view     |
| Browserless                  | Remote CDP/BQL and existing infrastructure compatibility    |
| Kernel                       | VM-level browser isolation and long-lived execution         |
| WebDriver BiDi backend       | Cross-browser standards and Firefox/WebKit-oriented testing |

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

**Verified 2026-08-03 — GREENFIELD as a Quarry-owned concept.**
`crates/quarry-runtime/src/grant_validator.rs:1-13`'s own doc comment is
explicit: "Model Plane has its own `BrowserBrokerService` that issues
*grants*... Before a lease handoff, Quarry MUST validate the grant via
`ValidateGrant`." Quarry only **validates** externally-issued grants
(`HttpGrantValidator`/`NoopGrantValidator`) — it never originates or
persists an approval/pause-resume record of its own. The only pause/resume
that exists today is `crawl_signals.rs`'s operator pause/resume/cancel of
an **entire crawl run** at the page boundary — a run-level control signal,
not the per-action "approve this exact click before it executes" gate this
section proposes. No Go-side approval code exists in
`quarry-orchestrator`/`quarry-control` either. This is real, unbuilt work
for Quarry specifically, even though Model Plane's own approval system
(separately verified this session) is further along.

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

**Verified 2026-08-03 — confirmed real and working; extend, don't
rebuild.** `SmartSearchRouter`/`SmartSearchRouterBuilder`
(`crates/quarry-runtime/src/smart_router.rs:178,203,214,311`) is a real,
production-wired system: providers are composed via
`with_tantivy`/`with_stract`/`with_searxng`/`with_brave`/`with_serper`,
routing Tantivy → Stract → SearXNG → Brave/Serper as documented, with real
intent classification, caching, and fallback tests (e.g. "failing Brave...
proves it isn't called when Stract returns enough results"). `AnswerPipeline`
(`answer.rs:110,126`) is likewise real and consumed by `quarry-edge`'s
actual answer/search routes, not a parallel or aspirational stack. **The
one gap this section correctly identifies**: neither has any connection to
`quarry-browser`/`action_runtime.rs` today — "should integrate" describes
real, not-yet-done wiring, accurately.

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
quarry.search();
quarry.scrape();
quarry.extract();
quarry.crawl();
quarry.map();
quarry.answer();
quarry.agent();
quarry.browser.createSession();
quarry.browser.observe();
quarry.browser.act();
quarry.browser.extract();
quarry.jobs.watch();
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

**Verified 2026-08-03 — GREENFIELD, and a naming collision to watch.**
`cached_profile_store.rs`/`postgres_profile_store.rs`/`s3_profile_store.rs`
already implement a `ProfileStore` — but for **auth session snapshots**
(cookies/storage keyed by org/profile_id), a completely different concept
from this section's per-host backend-preference/challenge-signal/
success-statistics record. No per-host reliability profile exists anywhere.
Name the new concept distinctly (e.g. `SiteReliabilityProfile`) to avoid
confusing it with the existing, unrelated `ProfileStore` in code reviews
and logs.

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

**Verified 2026-08-03**: `Action` (the Browser Action IR) and
`BrowserObservation` (the ObservationBundle) already exist here
(`actions.rs`, `contracts.rs`) — extend both with the fields noted in §7/§8
rather than introducing parallel types.

Add:

- ~~Browser Action IR~~ — **exists** (`actions.rs::Action`), extend with
  version/expectedEffects/riskClass;
- ~~ObservationBundle~~ — **partially exists** (`contracts.rs::
  BrowserObservation`), extend with ARIA/forms/dialogs fields;
- verification contracts (confirmed greenfield, §12);
- browser backend capabilities;
- browser trace event schemas;
- versioned failure/recovery reason codes (extend the existing `ErrorCode`/
  `CrawlDenialReason` enums, §39).

### `quarry-browser`

**Verified 2026-08-03: fix `chromiumoxide.rs`'s missing SSRF enforcement
first (§1a correction 1, §48 P0.1) — before adding any of the features
below, since they all execute through the same unguarded `goto()`/
`open_tab_page()` path.**

Add:

- **the SSRF fix above (do first)**;
- ARIA snapshot builder (confirmed greenfield, §5.3);
- stable element refs;
- selector ensemble resolver (confirmed greenfield, §9);
- Set-of-Marks transform;
- WebDriver BiDi experimental backend;
- deterministic verifiers (confirmed greenfield, §12);
- control takeover state;
- multitab/frame model;
- credential injection action.

### `quarry-runtime`

**Verified 2026-08-03**: this crate already holds a complete, unwired
Postgres durable-queue implementation (`request_queue.rs`/
`postgres_queue.rs`/`crawl_frontier.rs`) — enable the `postgres-queue`
feature and connect it to `quarry-orchestrator` (§1a correction 2, §41)
before adding new capabilities below. Also fix the static driver's
redirect-following SSRF gap in `fetch.rs` here (§1a correction 1).

Add:

- **wire the existing durable queue above (do first)**;
- **fix `fetch.rs`'s redirect SSRF gap above (do first)**;
- browser planner/executor adapter boundary;
- runtime backend router;
- observation mode router;
- action cache;
- self-healing levels;
- site-profile scoring (confirmed greenfield — the existing `ProfileStore`
  is a different concept, session cookies not host reliability, §25);
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

Add:

- durable grants;
- domain and action class restrictions;
- Quarry lease binding;
- exact credential references;
- grant revocation events;
- browser runtime cost metadata.

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

| Technology or pattern                      | Use                                      |
| ------------------------------------------ | ---------------------------------------- |
| CDP                                        | Primary Chromium execution protocol      |
| WebDriver BiDi                             | Experimental cross-browser adapter       |
| Accessibility/ARIA snapshots               | Canonical structured observation concept |
| axe-core                                   | Accessibility metadata and QA tool       |
| Browserbase APIs                           | Optional managed session backend         |
| BrowserGym/AgentLab                        | Evaluation lab                           |
| WebArena Verified/WorkArena/VisualWebArena | Regression benchmarks                    |

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

1. Define Browser Action IR v1.
2. Define ObservationBundle v1.
3. Bind BrowserBroker grants to Quarry leases.
4. Add deterministic verification outcomes.
5. Make approval continuation durable.
6. Add complete browser step traces.
7. Standardize failure and recovery reason codes.

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

---

## 35. 2026 ecosystem validation expansion

This expansion was added after validating Scrapling and adjacent crawling,
extraction, browser-runtime, recording, and change-monitoring systems against
Quarry V2's actual ownership model.

The validation method deliberately separates:

1. project positioning and public API claims;
2. implementation dependencies and architecture;
3. tests demonstrating the claimed behavior;
4. current maturity and open defects;
5. patterns Quarry should reproduce natively;
6. components worth benchmarking behind adapters;
7. components Quarry should not make authoritative.

The main correction to the earlier document is that browser reliability is not
only a planner, selector, or visual-grounding problem. It is a layered
acquisition and maintenance problem:

```text
source discovery
  -> protocol/runtime selection
  -> access/challenge classification
  -> page-state observation
  -> target identity and relocation
  -> extraction or action
  -> deterministic verification
  -> persisted site knowledge
  -> replay, repair, or escalation
```

Quarry already owns most of this lifecycle. The missing work is to connect the
layers through typed contracts and verified feedback.

### 35.1 Updated principle

```text
Do not adopt another crawler as Quarry's control plane.

Adopt proven algorithms, contracts, test corpora, and runtime adapters where
those improve Quarry's own governed execution boundary.
```

### 35.2 Systems added to the audit

| System             | Primary contribution                                                                                   | Quarry posture                        |
| ------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------- |
| Scrapling          | Persisted element fingerprints, non-LLM adaptive relocation, fetcher escalation, multi-session spiders | Strong donor and benchmark adapter    |
| Crawl4AI           | LLM-ready extraction, Markdown quality, deep-crawl recovery, prefetching, deployment hardening         | Extraction and recovery benchmark     |
| Crawlee            | Durable queues, session pools, adaptive concurrency, proxy and block handling                          | Runtime-pattern donor                 |
| Spider-rs          | Rust HTTP-first streaming crawler with browser escalation                                              | High-priority Rust benchmark/donor    |
| Patchright         | Patched Playwright/Chromium anti-detection runtime                                                     | Isolated lab runtime only             |
| Camoufox           | Firefox-derived anti-detect browser                                                                    | Experimental lab runtime only         |
| nodriver           | Direct asynchronous CDP with frame-aware lookup                                                        | CDP and target-lookup donor           |
| Lightpanda         | Low-resource agent-oriented browser and deterministic script export                                    | Read-heavy experimental runtime       |
| Steel              | Self-hosted session/browser API, quick actions, debugger UI                                            | Runtime/API/observability reference   |
| Maxun              | Human recorder to reusable robot, scheduled extraction                                                 | Workflow-recording UX donor           |
| changedetection.io | Visual selectors, browser steps, semantic change filters, notification semantics                       | Change-intelligence donor             |
| Playwright MCP/CLI | Accessibility snapshots, stable refs, token-efficient skill/CLI direction                              | Observation and agent-interface donor |

---

## 36. Scrapling deep validation

### 36.1 What Scrapling actually is

Scrapling is a Python web-scraping framework that combines:

- an `lxml`-based parser;
- static HTTP fetching through `curl_cffi`;
- Playwright browser fetching;
- Patchright-backed stealth browser execution;
- browser and HTTP sessions;
- an asynchronous spider runtime;
- request scheduling and deduplication;
- blocked-response retry and proxy rotation;
- pause/resume checkpoints;
- MCP and agent-skill exposure;
- persisted adaptive element relocation.

Its packaging currently identifies it as beta. This matters: the algorithms are
valuable, but Quarry should not turn a beta Python package into a production
security or execution dependency.

### 36.2 Adaptive extraction is its most important contribution

Scrapling's adaptive mode stores a fingerprint for a selected element. When the
original CSS or XPath no longer resolves, it compares candidates on the new
page and returns the most similar element.

The persisted properties include:

- tag name;
- normalized text;
- attribute names and values;
- sibling tag names;
- tag-only structural path;
- parent tag, attributes, and text;
- domain and caller-provided logical identifier.

The default storage is SQLite. Matching is deterministic and does not require a
model. Repository tests demonstrate relocation when:

- an `id` becomes a `data-id`;
- classes change;
- additional wrapper nodes are introduced;
- the original path no longer exists;
- a similarity threshold produces no acceptable candidate.

This validates the core concept, but not automatic safety for consequential
browser actions.

### 36.3 Why Scrapling-style similarity is not enough for actions

Similarity can recover an extraction target such as a product title with low
risk. It cannot independently prove that a visually or structurally similar
button has the same business meaning.

Example:

```text
previous target: "Approve invoice"
new high-similarity candidate: "Approve and pay"
```

Both may share:

- tag;
- CSS classes;
- parent structure;
- nearby invoice text;
- position;
- visual style.

The second has a materially different effect.

Therefore Quarry must distinguish:

```text
adaptive extraction repair
  may use similarity with schema/evidence validation

adaptive read-only navigation repair
  may use similarity plus semantic and state verification

adaptive effectful-action repair
  requires preserved semantic identity, unchanged risk, policy recheck,
  and deterministic postcondition verification
```

### 36.4 Fetcher escalation is the second useful pattern

Scrapling exposes separate modes for:

```text
Fetcher
  fast static HTTP

DynamicFetcher
  browser rendering and small interactions

StealthyFetcher
  browser rendering with additional fingerprint and challenge controls
```

Its spider examples also demonstrate changing the session used for a retry:

```text
ordinary HTTP + inexpensive network route
  -> blocked response detected
  -> requeue request
  -> stealth browser + higher-cost route
```

This is aligned with Quarry's `DriverPlan`, but Quarry should make the decision
more precise and governed.

### 36.5 Spider-runtime patterns worth adopting

Scrapling's spider layer includes:

- priority scheduling;
- request fingerprinting;
- URL deduplication;
- global and per-domain concurrency;
- robots policy;
- named multi-session routing;
- customizable blocked-response detection;
- bounded retries;
- periodic checkpoints;
- pending-request and seen-set restoration;
- streaming item output;
- development response replay.

Quarry already has stronger durable-plane primitives available. It should copy
the operational behavior, not the local pickle/SQLite implementation.

### 36.6 Maturity and dependency risks

Scrapling's stealth and browser layer depends on tightly coordinated versions
of Playwright, Patchright, BrowserForge, and fingerprint datasets. A current
open issue against version 0.4.12 reports that browser imports can fail on a
Linux server because a hard-coded browser version is absent from the installed
fingerprint dataset.

The broader lesson for Quarry is more important than the specific defect:

```text
fingerprint profile
+ browser binary
+ protocol driver
+ stealth patch set
+ header dataset
+ operating-system profile

must be validated as one compatibility unit
```

A runtime must not advertise readiness merely because its process starts.

### 36.7 Scrapling verdict

```text
Adopt natively:
  element fingerprint concepts
  domain/profile-scoped target memory
  deterministic similarity fallback
  thresholded no-match behavior
  static -> browser -> specialized-runtime escalation
  multi-session crawler semantics
  block-aware retry records

Benchmark:
  parser and selector throughput
  extraction stability under layout mutations
  static HTTP impersonation
  Patchright-backed browser success
  proxy/session behavior on authorized targets

Do not adopt as authority:
  Scrapling storage
  Scrapling scheduler
  raw page_action callbacks
  automatic challenge solving as a policy decision
  unverified anti-bot success claims
  Python fetcher runtime in Quarry's production hot path
```

---

## 37. Similar-system validation

### 37.1 Crawl4AI

Crawl4AI is the strongest additional extraction-oriented comparison. Its useful
patterns include:

- clean and fit Markdown;
- numbered link references;
- heuristic and BM25 content filtering;
- CSS/schema and model-assisted extraction;
- persistent browser profiles and sessions;
- browser hooks and user scripts;
- deep-crawl recovery state;
- prefetch-oriented URL discovery;
- security-hardening of its exposed API server;
- explicit separation of raw HTML, browser fetch, extraction, and generated
  model context.

Quarry should benchmark Crawl4AI on:

```text
main-content precision
Markdown structural fidelity
link and citation preservation
tables and code blocks
noise removal
schema extraction
infinite-scroll/lazy content
crash recovery
memory and browser utilization
```

It should not replace `quarry-transform` or the Rust/Go execution split.

### 37.2 Crawlee

**Verified 2026-08-03 — see §1a correction 2: this claim is confirmed
correct, and the fix is smaller than it reads.** The production crawl/batch
path (`quarry-orchestrator/internal/workflows/workflows.go:371,313`) really
is an in-process Go slice/map with only progress-count checkpoints. But a
complete, correctly-designed Postgres queue already exists in Rust
(`crates/quarry-runtime/src/{request_queue,postgres_queue,crawl_frontier}.rs`)
— feature-flagged off, zero production callers, and `quarry-control`'s own
`cycle23.go` comments that surfacing it is future work. Wire the existing
schema; do not design a new one.

Crawlee remains the strongest general crawler-runtime donor for:

- persistent request queues;
- session-pool health;
- proxy/session affinity;
- retries and blocked-session retirement;
- automatic concurrency based on available resources;
- unified HTTP and browser routing;
- state persistence and restartability;
- pluggable datasets and key-value artifacts.

Quarry's current in-memory frontier is a higher-priority gap than adding another
browser model. The durable frontier should be completed before large-scale
browser-agent expansion.

### 37.3 Spider-rs

Spider-rs is particularly relevant because Quarry's acquisition hot path is
already Rust.

The project demonstrates:

- concurrency-first streaming;
- HTTP-first crawling;
- browser rendering only when a page requires it;
- common APIs across local and managed execution;
- proxy, retry, rate-limit, and stealth options;
- Markdown, JSON, WARC, and agent-oriented use cases.

Recommended action:

```text
Create a benchmark adapter and conduct a focused source audit.
Compare its frontier, URL normalization, streaming, HTML discovery,
resource use, and smart HTTP/browser escalation with Quarry's implementation.
```

Potential code reuse must be decided only after checking:

- license compatibility;
- dependency graph;
- security posture;
- SSRF and redirect behavior;
- tenant and request isolation;
- observability hooks;
- ability to preserve Quarry's evidence contract.

### 37.4 Patchright

Patchright is a Playwright-compatible Chromium driver that modifies known
automation-detection surfaces and supports closed shadow roots.

Useful Quarry work:

- benchmark it as a browser adapter;
- compare success and failure against stock Chromium/CDP;
- test closed-shadow-root interaction;
- record which protocol features are removed or altered;
- test whether console, tracing, accessibility, and network evidence remain
  sufficient for Quarry verification.

It must not become the default runtime until compatibility, maintenance,
security, and observability are proven. Stealth improvements that disable
protocol surfaces can directly conflict with Quarry's evidence requirements.

### 37.5 Camoufox

Camoufox is an anti-detect browser oriented toward scraping and agents. The
project explicitly describes itself as under development.

Use only as an experimental runtime for:

- Firefox-oriented fingerprint diversity;
- cross-engine acquisition tests;
- authorized anti-bot benchmark scenarios;
- comparison against Chromium fingerprint monoculture.

Do not use it for effectful workflows until its stability and protocol/evidence
coverage meet Quarry's acceptance criteria.

### 37.6 nodriver

nodriver is a direct asynchronous CDP client and successor to
undetected-chromedriver. Relevant patterns include:

- no WebDriver intermediary;
- direct CDP events and commands;
- frame-inclusive element lookup;
- text lookup that ranks candidates rather than returning the first substring;
- persistent cookie/profile support;
- concise high-level helpers with full low-level CDP access.

Quarry already has a direct-CDP direction through chromiumoxide. The value is a
source and behavior comparison, especially for:

- frame traversal;
- event routing;
- target lookup;
- reconnection;
- challenge-page detection;
- protocol flattening.

### 37.7 Lightpanda

Lightpanda is a Zig browser built for automation. Its most interesting concepts
are:

- a much lighter read-oriented runtime than Chromium;
- CDP compatibility;
- direct Markdown dumping;
- per-client MCP session isolation;
- an agent that exports a deterministic script for replay without a model.

The project is beta and does not yet implement the complete browser platform.
Therefore:

```text
Good fit:
  public read-heavy pages
  high-volume rendering experiments
  crawl discovery
  extraction
  deterministic script benchmark

Poor fit today:
  compatibility-critical SaaS applications
  high-risk actions
  browser extensions
  flows relying on obscure Web APIs
  workflows requiring Chrome-identical behavior
```

Vendor performance claims must be reproduced in Quarry's own environment.

### 37.8 Steel

Steel is a self-hostable browser API with:

- sessions and persisted browser state;
- CDP/Puppeteer/Playwright access;
- proxies and fingerprint controls;
- extension support;
- debugging UI and request logging;
- quick scrape, screenshot, and PDF endpoints.

It is a useful reference for BrowserBroker/runtime ergonomics and local
operator tooling. Quarry should not add Steel as another session authority,
but may benchmark it as a runtime backend if self-hosted browser capacity is
needed beyond local Chromium.

### 37.9 Maxun

Maxun's strongest pattern is its recorder:

```text
human performs workflow
  -> actions are recorded
  -> robot is generated
  -> extraction is scheduled and exposed as an API
  -> later layout changes trigger recovery
```

Quarry should borrow the recorder-to-compiled-workflow UX. It should not embed
Maxun's platform because:

- it duplicates Quarry control and product surfaces;
- it is early-stage;
- its AGPL license requires careful legal review;
- Quarry already owns session, evidence, approval, and audit requirements.

### 37.10 changedetection.io

Relevant patterns include:

- visual target selection;
- browser steps before capture;
- structural, textual, JSON, PDF, and visual changes;
- schedules and time windows;
- noise filters;
- user-authored natural-language change conditions;
- concise change summaries;
- screenshots attached to change evidence.

Quarry's `/v1/change` and schedule capabilities should be expanded into
**Change Intelligence**, not only raw text diffs.

### 37.11 Playwright MCP and CLI/skills

Playwright's current direction validates two separate modes:

```text
MCP
  persistent state, rich introspection, exploratory and long-running loops

CLI + skills
  compact, purpose-built, token-efficient commands for known operations
```

Quarry should support both concepts internally:

- rich ObservationBundles for diagnosis and unfamiliar tasks;
- compact procedure/action commands for compiled workflows;
- snapshot deltas instead of full accessibility trees after every action;
- result handles instead of repeating large page payloads.

---

## 38. New Quarry concept: Adaptive Target Memory

**Verified 2026-08-03 — GREENFIELD, confirmed.** No fingerprint/logical-
identity/structural-path/sibling-tags concept exists anywhere in `crates/`.
The one similarly-named hit, `fingerprint_rotation.rs`, is unrelated — it
rotates TLS/browser anti-bot fingerprints to avoid blocking, not per-element
identity tracking. This section is genuinely new work, and per §1a/§9 it
also has no selector-ensemble foundation to extend yet — build §9 first.

The existing selector ensemble should be extended into a persisted,
versioned target-identity system.

### 38.1 Element fingerprint

```ts
interface ElementFingerprint {
	fingerprintId: string;
	siteProfileId: string;
	workflowId?: string;
	semanticTargetId: string;
	version: number;

	tagName: string;
	role?: string;
	accessibleName?: string;
	normalizedText?: string;
	textTokens?: string[];

	stableAttributes: Record<string, string>;
	volatileAttributeNames: string[];

	parent?: ElementContextFingerprint;
	ancestors?: ElementContextFingerprint[];
	siblingTags?: string[];
	structuralPath?: string[];

	nearbyLabels?: string[];
	relationAnchors?: ElementRelation[];
	visualRegion?: BoundingBox;

	locale?: string;
	userRole?: string;
	pageClass?: string;

	capturedFromObservationId: string;
	lastVerifiedRunId: string;
	lastVerifiedAt: string;
}
```

### 38.2 Logical identity must be separate from selectors

```text
semantic target:
  invoice.approve_button

possible observations over time:
  #approveInvoice
  [data-testid="approve"]
  role=button name="Approve invoice"
  text="Godkjenn faktura"
  visual button near invoice summary
```

The logical target remains stable while selectors and presentation change.

### 38.3 Candidate scoring

Do not use one generic string-similarity score. Use a weighted feature model:

```ts
interface TargetCandidateScore {
	candidateElementRef: string;

	roleScore: number;
	accessibleNameScore: number;
	textScore: number;
	attributeScore: number;
	structuralScore: number;
	relationScore: number;
	parentScore: number;
	visualScore?: number;

	semanticRiskPenalty: number;
	ambiguityPenalty: number;
	totalScore: number;
}
```

Initial weights should be deterministic and task-sensitive:

```text
form submission or purchase:
  role/name and exact business labels dominate

read-only extraction:
  text, parent context, and structure may dominate

icon-only control:
  relation, tooltip, visual bounds, and network behavior dominate
```

Later, weights may be learned offline from verified repair outcomes.

### 38.4 Resolver order

```text
1. current backend node
2. stable test ID
3. role + accessible name
4. exact stable-attribute conjunction
5. persisted fingerprint match
6. relation/text anchor
7. visual target match
8. model-assisted target proposal
9. human intervention
```

### 38.5 Repair receipt

```ts
interface TargetRepairReceipt {
	repairId: string;
	semanticTargetId: string;
	previousFingerprintId: string;
	observationId: string;

	candidateScores: TargetCandidateScore[];
	selectedElementRef?: string;
	selectionReason: string[];

	semanticInvariantChecks: VerificationResult[];
	riskBefore: string;
	riskAfter: string;
	approvalReused: boolean;

	outcome: "repaired" | "ambiguous" | "rejected" | "human_required";
	verifiedBy: string[];
}
```

### 38.6 Automatic-repair policy

| Action class                    | Automatic fingerprint repair                                               |
| ------------------------------- | -------------------------------------------------------------------------- |
| Extract visible text            | Allowed above threshold with schema validation                             |
| Open read-only detail page      | Allowed with URL/content postcondition                                     |
| Fill non-secret search/filter   | Allowed with field-value verification                                      |
| Download known artifact         | Allowed with artifact and MIME verification                                |
| Send/publish/delete/pay/approve | Never on similarity alone; exact semantic and effect verification required |

---

## 39. New Quarry concept: Acquisition and Challenge Intelligence

**Verified 2026-08-03 — PARTIAL.** Two real enums already exist:
`crates/quarry-core/src/error.rs:10`'s `ErrorCode` (BadRequest/
Unauthorized/Forbidden/NotFound/RateLimited/Timeout/SecurityBlocked/
DriverFailed/UpstreamBlocked/...) and `crates/quarry-core/src/
crawl_denial.rs:12`'s `CrawlDenialReason` (OutOfScope/RobotsDisallowed/
DepthExceeded/SecurityRejected/...). But `host_scheduler.rs:39`'s own code
comment confirms the exact gap this section targets: "401/403/451/999/
CDN-challenge: the host actively refused" — these are explicitly lumped
into ONE bucket by design, with no `dns_failure`, `captcha_or_human_challenge`,
or `authentication_required` distinction anywhere. Extend the existing
enums with the finer-grained variants below rather than introducing a third.

A `403` is not a sufficient diagnosis. Quarry should classify why acquisition
failed before selecting another driver.

### 39.1 Challenge classes

```ts
type AcquisitionFailureClass =
	| "dns_failure"
	| "connect_timeout"
	| "tls_failure"
	| "http_rate_limit"
	| "server_error"
	| "robots_disallowed"
	| "policy_disallowed"
	| "authentication_required"
	| "authorization_denied"
	| "geo_restricted"
	| "javascript_required"
	| "cookie_or_consent_gate"
	| "waf_javascript_challenge"
	| "captcha_or_human_challenge"
	| "fingerprint_rejected"
	| "content_missing"
	| "unknown";
```

Do not copy a policy that treats all `401`, `403`, `500`, `502`, `503`, and
`504` responses as bot blocks. Some are authentication, application failure,
or transient infrastructure errors and require different recovery.

### 39.2 Escalation plan

```ts
interface AcquisitionEscalationPlan {
	requestId: string;
	currentDriver: string;
	failureClass: AcquisitionFailureClass;

	nextDriver?: string;
	sessionAction?: "reuse" | "refresh" | "rotate" | "retire";
	proxyAction?: "keep" | "rotate" | "change_geo" | "none";

	additionalCostCeiling: number;
	reasonCodes: string[];
	policyDecisionId: string;
	requiresHuman: boolean;
}
```

Recommended waterfall:

```text
owned cache / previous evidence
  -> static direct HTTP
  -> impersonated static HTTP
  -> lightweight JS runtime where compatible
  -> local Chromium/CDP
  -> Chromium with approved profile/session/network route
  -> optional managed runtime
  -> human challenge/credential handoff
  -> stop with explicit denial reason
```

### 39.3 CAPTCHA and access controls

Quarry must not make "solve every challenge" a product invariant.

Policy should distinguish:

- authorized automation where a user may complete a challenge;
- public crawling where retry/runtime changes are permitted;
- explicit legal, robots, authentication, or authorization denials;
- challenge circumvention prohibited by tenant or source policy.

A human takeover or approved first-party API is often the correct route.

### 39.4 Runtime compatibility manifest

**Verified 2026-08-03 — GREENFIELD, confirmed.** No `health_check`/
`HealthCheck`/manifest-style function exists in `driver.rs`/
`driver_registry.rs`/`quarry-browser` — the only "readiness" hit in the
codebase is an unrelated comment about Data Plane retrieval readiness. This
directly connects to the artifact-store finding from this session's
down-the-stack audit: `quarry-edge`'s own startup silently downgrades to a
non-durable in-memory artifact store on any unrecognized/misconfigured
backend config, logged at `info` level only — the exact "readiness ≠
process started" failure mode this section warns against, already
observed in production configuration, not hypothetical.

```ts
interface BrowserRuntimeManifest {
	runtimeId: string;
	runtimeVersion: string;
	browserEngine: string;
	browserVersion: string;
	protocol: "cdp" | "webdriver_bidi" | "webdriver" | "native";

	patchSet?: string;
	fingerprintDatasetVersion?: string;
	operatingSystemProfile: string;
	headerProfileVersion?: string;

	evidenceCapabilities: BrowserBackendCapabilities;
	compatibilitySuiteVersion: string;
	compatibilityStatus: "verified" | "degraded" | "blocked";
	verifiedAt: string;
}
```

Readiness must include a real browser-start, navigation, observation, action,
network, artifact, and teardown probe—not only process health.

---

## 40. New Quarry concept: Extraction Profiles

Browser-agent actions and web extraction share acquisition, but they require
different optimization.

### 40.1 Extraction profile

```ts
interface ExtractionProfile {
	profileId: string;
	version: number;
	siteProfileId?: string;
	pageClass?: string;

	targetSchema: JsonSchema;
	requiredEvidence: EvidenceRequirement[];

	preferredSources: Array<
		| "json_ld"
		| "microdata"
		| "network_json"
		| "dom"
		| "accessibility"
		| "rendered_text"
		| "visual_region"
		| "document_parser"
	>;

	targetFingerprints: string[];
	paginationStrategy?: PaginationStrategy;
	waitStrategy: WaitStrategy;
	validationRules: ValidationRule[];
	changeTolerance: ChangeTolerancePolicy;
}
```

### 40.2 Evidence-priority order

For extraction, prefer the least lossy and cheapest source:

```text
first-party API/network JSON when policy permits
  -> JSON-LD / structured metadata
  -> stable DOM schema
  -> accessibility/visible text
  -> rendered-page region
  -> model-assisted visual extraction
```

The browser network layer should identify candidate JSON payloads, but Quarry
must preserve provenance from each extracted field to its source response,
DOM region, or artifact.

### 40.3 Markdown quality benchmark

Create a corpus covering:

- articles;
- documentation;
- ecommerce products;
- tables;
- nested lists;
- code blocks;
- footnotes;
- dashboards;
- PDFs and office documents;
- multilingual Norwegian/English pages;
- cookie and navigation noise;
- lazy-loaded content.

Compare:

```text
Quarry transform
Crawl4AI
Firecrawl
Scrapling + markdown conversion
readability-based extraction
Docling/Unstructured/Kreuzberg where relevant
```

Metrics:

- main-content precision and recall;
- structural fidelity;
- table accuracy;
- citation/link preservation;
- token count;
- unsupported content insertion;
- extraction latency and memory;
- field-level provenance coverage.

---

## 41. Durable crawler frontier and adaptive throughput

**Verified 2026-08-03 — see §1a correction 2 for full detail.** Confirmed:
`quarry-orchestrator`'s Go/Temporal workflows use a plain in-process
slice/map today, with only progress *counts* checkpointed, not the actual
frontier/seen-set. **What changes the scope of this section**: the
`FrontierRequest` shape below is largely already built in Rust
(`request_queue.rs`/`postgres_queue.rs`/`crawl_frontier.rs`, migration
`0001_request_queue.sql`, with `SELECT FOR UPDATE SKIP LOCKED` + visibility
timeouts + org isolation) — it is feature-flagged off and unwired, not
unwritten. This section should read as "wire the existing schema into the
Go orchestrator and into `quarry-control`'s read side," which is a smaller,
faster, lower-risk task than building the schema below from scratch.

The codebase audit identifies the in-memory request queue as a current Quarry
gap. This outranks adding several experimental runtimes.

### 41.1 Canonical frontier state

```ts
interface FrontierRequest {
	requestId: string;
	crawlId: string;
	tenantId: string;

	url: string;
	canonicalUrl: string;
	method: string;
	bodyHash?: string;

	priority: number;
	depth: number;
	discoveredFrom?: string;
	sessionAffinity?: string;

	status: "pending" | "leased" | "completed" | "failed" | "blocked";
	attempts: number;
	nextAttemptAt?: string;
	leaseOwner?: string;
	leaseExpiresAt?: string;

	policySnapshotId: string;
	driverHistory: DriverAttempt[];
}
```

Persist:

- request queue;
- seen/fingerprint set;
- crawl scope;
- host budgets;
- session/proxy health;
- checkpoints;
- partial outputs;
- cancellation state;
- backfill/retry decisions.

Use Postgres/Redis/Temporal according to existing ownership. Do not adopt local
pickle checkpoints as production authority.

### 41.2 Adaptive throughput controller

Inputs:

```text
host latency and error rate
429 and challenge rate
robots/crawl-delay
runtime CPU/RAM
browser capacity
proxy health
session health
queue age
model/extraction backlog
tenant cost budget
```

Outputs:

```text
global concurrency
per-host concurrency
request delay
runtime mix
session retirement
proxy retirement
retry schedule
```

Use deterministic AIMD/token-bucket style control first. Train a ranking or
control model only after sufficient verified telemetry exists.

---

## 42. Recorded workflow and deterministic script compiler

**Verified 2026-08-03 — GREENFIELD, confirmed.** The only "replay" concept
in the codebase (`kernel.rs`'s `replay_url`/`enable_replay`) is session
**video replay** for human debugging — unrelated to recording and
replaying an agent's action trajectory. No rollout-state concept
(candidate/shadow/active/quarantined) exists anywhere. This is genuinely
new work with no partial implementation to build from.

Stagehand, Maxun, Scrapling, and Lightpanda all reinforce the same direction:
exploration should become a replayable procedure.

### 42.1 Sources of candidate workflows

- verified agent trajectory;
- human live-takeover recording;
- imported Playwright trace or script;
- repeated extraction profile;
- support-authored procedure;
- existing site-specific automation.

### 42.2 Compilation pipeline

```text
recorded events
  -> normalize into Browser Action IR
  -> remove timing and selector noise
  -> assign semantic target IDs
  -> create ElementFingerprints
  -> infer explicit waits and preconditions
  -> attach expected effects and postconditions
  -> mark risk/approval boundaries
  -> replay against fixtures and live authorized test site
  -> mutate layout and test target repair
  -> security review
  -> candidate compiled workflow
  -> staged deployment
```

### 42.3 Compiled workflow contract

```ts
interface CompiledBrowserWorkflow {
	workflowId: string;
	version: number;
	siteProfileId: string;

	inputSchema: JsonSchema;
	outputSchema: JsonSchema;
	steps: CompiledBrowserStep[];

	allowedDomains: string[];
	requiredRuntimeCapabilities: string[];
	requiredCredentials: CredentialRequirement[];

	approvalBoundaries: ApprovalBoundary[];
	verificationPlan: VerificationPlan;
	compensationPlan?: CompensationPlan;

	fixtureSuiteId: string;
	liveCanarySuiteId?: string;
	rolloutState: "candidate" | "shadow" | "active" | "quarantined" | "retired";
}
```

### 42.4 Replay policy

A compiled workflow may run without a planner only while:

- the site/profile compatibility signature matches;
- preconditions pass;
- target confidence exceeds the workflow threshold;
- risk and policy snapshots remain compatible;
- postconditions can be verified;
- failure rate remains below quarantine threshold.

Otherwise, re-observe and escalate rather than blindly replay.

---

## 43. Change Intelligence

Quarry already has change tracking. Expand it into typed, evidence-aware change
analysis.

### 43.1 Change classes

```ts
type ChangeKind =
	| "text"
	| "structure"
	| "metadata"
	| "json"
	| "network_api"
	| "visual"
	| "document"
	| "availability"
	| "price"
	| "policy"
	| "workflow_breakage";
```

### 43.2 Change record

```ts
interface WebChangeRecord {
	watchId: string;
	sourceVersionBefore: string;
	sourceVersionAfter: string;

	changeKinds: ChangeKind[];
	normalizedDiffArtifactRef: string;
	screenshotDiffRef?: string;
	structuredDiff?: unknown;

	semanticSummary?: string;
	intentMatched: boolean;
	suppressionReason?: string;

	affectedFingerprints: string[];
	affectedExtractionProfiles: string[];
	affectedWorkflows: string[];

	evidenceRefs: string[];
	detectedAt: string;
}
```

### 43.3 Noise suppression

Support deterministic filters before using a model:

- remove dynamic timestamps and counters;
- ignore navigation/ads/footer regions;
- normalize whitespace and attribute order;
- compare selected structured fields;
- threshold visual regions;
- ignore known rotating content;
- use logical target fingerprints rather than absolute DOM paths.

A model may classify business relevance or summarize a diff, but the raw and
normalized evidence must remain inspectable.

### 43.4 Workflow impact analysis

When a site changes:

```text
capture new source version
  -> compute structural/semantic/visual diff
  -> locate affected target fingerprints
  -> run impacted extraction/workflow fixtures
  -> repair low-risk targets where allowed
  -> quarantine unsafe workflows
  -> notify operator with exact evidence
```

This turns change detection into proactive automation maintenance.

---

## 44. Quarry Quality OS

Quarry needs the same quality discipline proposed for Model Plane, specialized
for acquisition, extraction, and browser execution.

### 44.1 Test sources

- historical production failures;
- accepted human repairs;
- site-version pairs;
- synthetic DOM/layout mutations;
- browser/runtime upgrades;
- challenge and block fixtures;
- public benchmark tasks;
- tenant-approved live canaries;
- security incidents and prompt-injection pages;
- provider/runtime outage simulations.

### 44.2 Evaluation layers

```text
Acquisition
  Was the correct driver/runtime selected and was content captured safely?

Observation
  Did Quarry produce sufficient, compact, accurate state?

Targeting
  Did the resolver select the intended semantic element?

Extraction
  Did the output match schema and source evidence?

Action
  Was the exact intended effect attempted under authority?

Verification
  Did independent evidence prove the business outcome?

Recovery
  Did repair preserve semantics and remain within policy?

Operations
  Were latency, resource use, cost, and block rate acceptable?
```

### 44.3 Layout-mutation laboratory

Generate controlled page variants:

- move target to another parent;
- insert wrapper nodes;
- rename classes and IDs;
- alter attribute order;
- translate labels;
- reorder lists;
- add decoy controls;
- hide content behind tabs/frames/shadow DOM;
- change responsive layout;
- modify visual theme without semantic change;
- introduce a semantically dangerous near-match.

Evaluate exact target recovery and false-repair rate.

### 44.4 Runtime comparison matrix

```text
Quarry local Chromium/chromiumoxide
Browserbase
Browserless
Steel
Patchright adapter
Camoufox adapter
nodriver reference adapter
Lightpanda read-only adapter
```

Compare by site/task class, not one global score.

### 44.5 Required metrics

- verified acquisition rate;
- verified task completion rate;
- false-success rate;
- wrong-target rate;
- false-repair rate;
- extraction field precision/recall;
- provenance coverage;
- challenge classification accuracy;
- browser escalation rate;
- intervention rate;
- requests/pages per CPU and GiB;
- p50/p95 latency;
- model tokens;
- network/proxy/browser cost;
- policy and security violations;
- workflow quarantine rate;
- mean time to repair after a site change.

### 44.6 Shadow evaluation

For safe read-only tasks, run alternate routes against the same source version:

```text
selected production route
vs
static alternative
vs
alternate browser runtime
vs
alternate extraction transform
```

Store artifacts once and replay transforms/model extraction where possible.
Do not duplicate external effects for action workflows.

---

## 45. Quarry Proof Bundle

Every important acquisition or browser task should produce a portable evidence
object compatible with the broader Velion Proof Bundle.

```ts
interface QuarryProofBundle {
	taskId: string;
	tenantId: string;
	objective: string;

	sourceUrls: string[];
	sourceVersionRefs: string[];
	policySnapshotId: string;

	discoveryTrace?: SearchTraceRef;
	driverAttempts: DriverAttempt[];
	selectedRuntimeManifest?: BrowserRuntimeManifest;

	dnsAndNetworkDecisions: NetworkSecurityReceipt[];
	sessionProfileRef?: string;
	proxyProfileRef?: string;

	observations: ObservationRef[];
	targetRepairs: TargetRepairReceipt[];
	actions: BrowserActionReceipt[];

	extractedFields?: FieldEvidenceMap[];
	artifacts: ArtifactRef[];
	networkReceipts: NetworkReceipt[];
	verificationResults: VerificationResult[];

	cost: QuarryCostSummary;
	latency: QuarryLatencySummary;
	residency: ResidencyRecord;
	retentionPolicyId: string;

	outcome: "verified_success" | "verified_failure" | "partial" | "unknown";
}
```

This should power:

- Run Console evidence;
- browser replay;
- change alerts;
- audit exports;
- workflow promotion;
- incident response;
- quality replay;
- customer-visible proof of source and action.

---

## 46. Performance architecture updates

### 46.1 Avoid full observations on every step

Use:

```text
initial compact snapshot
  -> action
  -> state/AX/DOM delta
  -> request richer region only if needed
```

Track observation cache keys by page state, frame, and runtime.

### 46.2 Use handles for large payloads

The model should receive:

```text
network_result_17
  12,402 rows
  schema: invoice-list-v3
  filtered preview: 20 rows
```

not the full response. Quarry stores the payload and exposes bounded
projection/filter operations.

### 46.3 Prefer programmatic extraction and action batches

For known read-only processing:

- filter network JSON in a sandbox;
- validate schemas without an LLM;
- batch repeated field reads;
- execute compiled action sequences until an observation boundary;
- summarize evidence after deterministic processing.

Do not batch across approval, unknown state, navigation uncertainty, or
consequential effects.

### 46.4 Runtime specialization

```text
Static HTTP
  highest throughput, no JS

Lightweight browser experiment
  read-heavy JS pages with supported APIs

Local Chromium
  general controlled browser work

Stealth runtime experiment
  authorized sites with demonstrated need

Managed browser
  scale, geographic routing, replay, or persistent identity
```

Runtime routing should optimize verified outcome per total cost, not nominal
page-load speed.

### 46.5 Content-addressed reuse

Deduplicate:

- response bodies;
- screenshots;
- page snapshots;
- downloads;
- rendered PDFs;
- extracted document pages;
- model-ready Markdown;
- network JSON.

Derived outputs must record the exact source and transform versions.

---

## 47. Updated adoption matrix

### 47.1 Build natively now

- durable crawler frontier and checkpoints;
- typed challenge/failure classification;
- acquisition escalation policy;
- Adaptive Target Memory;
- weighted target-candidate scoring;
- repair receipts and semantic invariants;
- extraction profiles and field provenance;
- snapshot/delta observation transport;
- runtime compatibility manifests;
- Quarry Proof Bundle;
- workflow recorder/compiler contracts;
- Change Intelligence impact analysis;
- Quarry Quality OS schemas and replay.

### 47.2 Benchmark behind adapters

| System             | Benchmark focus                                                   |
| ------------------ | ----------------------------------------------------------------- |
| Scrapling          | Adaptive relocation, parser throughput, HTTP/stealth escalation   |
| Crawl4AI           | Markdown, schema extraction, crash recovery, deep-crawl discovery |
| Spider-rs          | Rust frontier, streaming throughput, smart browser escalation     |
| Crawlee            | Queue/session/concurrency behavior                                |
| Patchright         | Authorized anti-detection and closed-shadow-root access           |
| Camoufox           | Cross-engine fingerprint diversity                                |
| nodriver           | Direct CDP, iframe lookup, reconnect behavior                     |
| Lightpanda         | Memory/throughput for read-heavy JS extraction                    |
| Steel              | Self-hosted browser sessions and debugging ergonomics             |
| Firecrawl          | Public API, SDK, stateful interact, output formats                |
| changedetection.io | Diff quality, filters, visual-watch UX                            |
| Maxun              | Recorder and robot authoring UX                                   |

### 47.3 Optional production backends only after gates

- Browserbase;
- Browserless;
- Steel;
- Lightpanda for narrowly approved read-only classes;
- Patchright/Camoufox only if Quarry's compatibility and security suites pass.

### 47.4 Avoid as canonical authorities

- Scrapling/Crawl4AI/Crawlee scheduler or storage;
- a second browser/session control plane;
- arbitrary Python/JavaScript `page_action` callbacks from models;
- automatic CAPTCHA or access-control bypass as a default behavior;
- model-generated selectors stored without verification;
- anti-detect runtime readiness inferred from marketing claims;
- one global runtime score across all sites/tasks;
- self-healing effectful actions based only on similarity;
- AGPL platform embedding without legal and architectural review;
- replacing Quarry evidence with external provider summaries.

---

## 48. Revised dependency-ordered implementation plan

### P0: close current execution and security gaps

**Resequenced 2026-08-03 against verified reality (see §1a).** Item 1 is
confirmed broken today, not just incomplete — promote it above everything
else without qualification. Item 2 is now scoped as "wire an existing
schema," not "build a durable frontier."

1. **Fix the confirmed-broken SSRF/DNS controls first, no exceptions**:
   wire `quarry_security`/`dns_guard` into `chromiumoxide.rs`'s `goto()`/
   `open_tab_page()` (currently zero calls); stop `fetch.rs`'s redirect
   policy from following without a re-check; fix `dns_guard.rs`'s
   resolve-then-discard TOCTOU (pin the checked IP into the actual
   connection, don't re-resolve); add `is_unspecified()` to `heur.rs`'s
   IPv4 checks (0.0.0.0 currently bypasses loopback blocking); un-ignore
   `quarry-browser/tests/ssrf.rs` so a regression here fails CI. Apply the
   identical fix to imports-core's Python guard
   (`network_policy.py`), which has the same TOCTOU gap independently.
2. **Wire the existing durable-frontier schema, don't design a new one** —
   `crates/quarry-runtime/src/{request_queue,postgres_queue,crawl_frontier}.rs`
   is already correct (SKIP LOCKED, visibility timeouts, org isolation);
   enable the `postgres-queue` feature, connect
   `quarry-orchestrator`'s Go/Temporal workflows to it instead of an
   in-process slice/map, and complete `cycle23.go`'s `MountRequestQueues`
   stub on the read side.
3. Finish approval continuation for effectful browser work using the current
   run/step continuation model (confirmed greenfield as a Quarry-owned
   concept, §18 — Quarry validates Model Plane's grants but has no
   approval/pause-resume of its own beyond whole-run pause/resume/cancel).
4. Standardize acquisition, action, verification, and recovery failure codes
   (extend the existing `ErrorCode`/`CrawlDenialReason` enums per §39, don't
   introduce a third).
5. Add runtime compatibility/readiness probes with image and dependency
   digests (confirmed greenfield, §39.4 — and the artifact-backend
   silent-downgrade-to-in-memory finding from this session's audit is a
   live instance of exactly the failure mode this item exists to close).
6. Ensure every effectful action produces an outcome receipt or `UNKNOWN`
   (confirmed greenfield, §12 — today's model is "no exception ⇒ success").

### P1: adaptive reliability without additional model cost

1. Implement `ElementFingerprint` and semantic target IDs.
2. Extend selector ensembles with weighted fingerprint matching.
3. Add `TargetRepairReceipt` and automatic-repair policy by action class.
4. Add challenge classification and typed escalation plans.
5. Add observation deltas and bounded payload handles.
6. Add extraction profiles with network/JSON-LD/DOM/visual source ordering.
7. Create the first Quarry Proof Bundle.

### P2: workflow maintenance and quality

1. Record human takeover and verified agent trajectories.
2. Compile candidate deterministic workflows.
3. Add layout-mutation and dangerous-near-match tests.
4. Add workflow-impact analysis to `/v1/change`.
5. Establish Quarry Quality OS result storage and CI gates.
6. Benchmark Scrapling, Crawl4AI, Spider-rs, and Crawlee.
7. Add daily authorized live canaries for critical runtime profiles.

### P3: specialized runtimes and product ergonomics

1. Benchmark Lightpanda for read-heavy extraction.
2. Benchmark Patchright, Camoufox, and nodriver patterns in isolated labs.
3. Evaluate Steel as a self-hosted runtime adapter.
4. Improve Firecrawl-level SDK and stateful interact ergonomics.
5. Add recorder/procedure authoring UX inspired by Maxun and Stagehand.
6. Add richer semantic/visual change-monitoring UX.

### Research after measured need

- learned fingerprint feature weights;
- neural DOM target embeddings;
- cross-site semantic target transfer;
- WebDriver BiDi production backend;
- browser-runtime bandit routing;
- automatic workflow synthesis from many trajectories;
- distributed browser fleets beyond demonstrated capacity requirements.

---

## 49. Expanded acceptance criteria

In addition to the earlier criteria, Quarry is ready for this expanded posture
when:

- a known extraction survives controlled ID/class/path/wrapper changes without
  invoking a model;
- a dangerous near-match never passes automatic repair for an effectful action;
- every repaired target has a candidate score trace and verification receipt;
- the crawler can restart with no duplicate or lost frontier work after a
  worker/process failure;
- challenge classification distinguishes auth, rate limiting, server errors,
  policy denial, JS requirements, and fingerprint rejection;
- runtime readiness fails closed when browser/fingerprint/driver versions are
  incompatible;
- Markdown and schema-extraction benchmarks are reproducible against fixed
  source artifacts;
- network JSON extraction maps every output field to source evidence;
- static, local-browser, stealth, lightweight, and managed-runtime routes are
  compared on verified outcome, cost, and latency;
- a verified human or agent trajectory can become a reviewed deterministic
  workflow without granting a second system control authority;
- a source change identifies impacted workflows and quarantines unsafe replay;
- browser and extraction regressions block promotion before deployment;
- the Quarry Proof Bundle is sufficient to reconstruct what was fetched,
  observed, repaired, executed, and verified;
- anti-bot or challenge handling never overrides legal, robots, tenant, domain,
  identity, or approval policy.

---

## 50. Additional research references

### Adaptive crawling and extraction

- Scrapling: https://github.com/D4Vinci/Scrapling
- Scrapling adaptive extraction: https://github.com/D4Vinci/Scrapling/blob/main/docs/parsing/adaptive.md
- Scrapling spiders: https://github.com/D4Vinci/Scrapling/blob/main/docs/spiders/architecture.md
- Crawl4AI: https://github.com/unclecode/crawl4ai
- Crawlee Python: https://github.com/apify/crawlee-python
- Spider-rs: https://github.com/spider-rs/spider

### Browser runtimes

- Patchright Python: https://github.com/Kaliiiiiiiiii-Vinyzu/patchright-python
- Camoufox: https://github.com/daijro/camoufox
- nodriver: https://github.com/ultrafunkamsterdam/nodriver
- Lightpanda: https://github.com/lightpanda-io/browser
- Steel Browser: https://github.com/steel-dev/steel-browser

### Recording and change intelligence

- Maxun: https://github.com/getmaxun/maxun
- changedetection.io: https://github.com/dgtlmoon/changedetection.io

### Agent browser interfaces

- Playwright MCP: https://github.com/microsoft/playwright-mcp
- Playwright CLI: https://github.com/microsoft/playwright-cli
