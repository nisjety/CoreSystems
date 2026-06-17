# Feature Proposal: Sovereign Scraping Reliability Layer

Status: proposed backlog feature  
Owner: Ingestion Plane / Quarry-v2  
Primary goal: compete with Firecrawl and Apify on scrape reliability while
keeping page execution, artifacts, privacy policy, and data ownership inside
CoreSystem.

## Why this exists

Quarry-v2 now has the right privacy posture: GDPR defaults, third-party
processing denied by default, audit metadata on artifacts, and proxy/browser
provider gates. The remaining reliability gap is anti-bot execution quality.

The target is not "call Scrapfly when hard." The target is:

```text
Quarry policy gate
  -> domain intelligence
  -> EgressBroker
  -> owned fetch/TLS/browser runtime
  -> owned transform/redaction/retention
  -> owned artifacts and audit receipts
```

External services may provide network transit only, if explicitly approved.
They must not render pages, parse content, store raw HTML, run extraction, or
own the crawl workflow.

## Competitor anchors

Firecrawl's current strength is a compact product surface: scrape, crawl, map,
search, parse, and interact endpoints, with many output formats on `/scrape`
such as markdown, HTML, raw HTML, links, images, screenshot, JSON, change
tracking, branding, audio/video, question, and highlights. It also exposes
browser actions, location, proxy, cache, PII redaction, and zero-data-retention
options in the scrape request surface.

Apify's current strength is the platform model: Actors for long-running jobs,
storage, proxy infrastructure, scheduling, webhooks, and session pools that
avoid retrying known blocked/non-working proxies.

Quarry should not copy their hosted-control model. Quarry should challenge them
by being the self-owned version:

- same ergonomic API surface for scrape/crawl/search/extract/parse/interact
- stronger tenant auditability and policy enforcement
- deterministic artifact and fingerprint lineage
- runtime ownership over fetch, browser, transform, and retention
- network-only proxy/provider involvement when GDPR policy explicitly allows it

References:

- Firecrawl API introduction: https://docs.firecrawl.dev/api-reference/introduction
- Firecrawl scrape endpoint: https://docs.firecrawl.dev/api-reference/endpoint/scrape
- Firecrawl parse feature: https://docs.firecrawl.dev/features/parse
- Firecrawl interact feature: https://docs.firecrawl.dev/features/interact
- Apify platform guide: https://docs.apify.com/sdk/js/docs/3.1/guides/apify-platform
- Apify session management: https://docs.apify.com/sdk/js/docs/3.0/guides/session-management
- Apify proxy docs: https://docs.apify.com/platform/proxy
- Apify webhooks: https://docs.apify.com/platform/integrations/webhooks

## Existing Quarry primitives

Quarry-v2 already has most of the building blocks:

| Area | Existing primitive |
|---|---|
| Policy gate | `PrivacyPolicy`, ZDR, processor approval, Data Plane privacy metadata |
| Proxy routing | `EgressBroker`, `ProxyPool`, sticky `(org, host)` ordering, 429 rotation |
| Driver fallback | `DriverPlan`, `FallbackDriver`, block-status classification |
| TLS impersonation | `quarry-tls`, `tls_driver`, browser-like TLS/H2 path |
| Browser execution | `quarry-browser`, `chromiumoxide`, Browserless, Browserbase scaffolding |
| Session continuity | browser lease pool, `ProxyAffinity`, persistent profile/session modules |
| Host control | `HostScheduler`, `AutoscaledPool`, retry/backoff classifiers |
| Output ownership | `NormalizedOutput`, artifact store, fingerprints, diffs, Data Plane ingest |
| Safety | SSRF guard, tenant policy, audit/event history |

The feature is mainly a coordination layer across these primitives.

## Proposed capability

### 1. Domain Intelligence Registry

Add a first-party registry keyed by effective top-level domain plus org policy:

```json
{
  "domain": "example.com",
  "bot_wall": "akamai",
  "preferred_driver": "tls_then_browser",
  "requires_js": true,
  "session_policy": "sticky_ip_cookie_jar",
  "max_parallelism": 2,
  "cooldown_seconds": 120,
  "allowed_egress_tiers": ["first_party", "approved_network_proxy"],
  "raw_artifact_policy": "redact_before_persist",
  "challenge_policy": "fail_with_reason"
}
```

The registry should learn from outcomes, but production changes should be
auditable. Automatic learning can propose a domain profile; policy promotion
requires operator approval.

### 2. EgressBroker v2

Extend the current broker from simple proxy retry into a full egress planner:

- rank candidates by host, org, ASN, region, IP type, past success, and recent
  block cooldown
- maintain per-domain health for direct, datacenter, ISP, residential, mobile,
  and first-party POP egress
- keep provider involvement network-only by contract
- emit an `egress_receipt` for every attempt: egress type, processor ID,
  region, ASN class, status, challenge signal, retry reason, and retention
  expiry
- allow BYO proxy networks and customer-owned egress pools
- support an optional first-party regional POP strategy later

Important constraint: GDPR/default mode must still deny third-party processing.
An approved network proxy is not an approved scraping API.

### 3. Session Health and Affinity

Build an Apify-style session health model without moving execution out of
Quarry:

- bind session, proxy identity, user agent profile, viewport, locale, timezone,
  and cookie jar together
- mark sessions `good`, `suspect`, `blocked`, `retired`
- retire sessions after challenge loops, repeated 429/403/503, auth-wall
  detection, or policy violations
- encrypt browser state per tenant
- apply TTLs and erasure hooks to cookies, localStorage, IndexedDB, screenshots,
  and raw HTML
- block session persistence in ZDR unless the request explicitly allows it

This makes anti-bot behavior stateful without letting a third party own the
state.

### 4. Challenge Classifier and Policy Gate

Add a classifier that detects challenge pages and returns structured reasons:

- `rate_limited`
- `bot_wall`
- `captcha`
- `auth_required`
- `paywall`
- `geo_blocked`
- `robots_disallowed`
- `policy_denied`
- `processor_not_approved`

The classifier should not silently bypass high-risk pages. It should either
escalate within the approved Quarry driver chain or fail with a reason and an
audit receipt.

### 5. Owned Browser Fleet

Move "managed browser" from default answer to optional overflow:

- local Chromium pools for default sovereign mode
- persistent CDP sessions for JS-heavy pages
- browser profile snapshots stored in CoreSystem-controlled storage
- browser state redaction before persistence
- Browserbase/Browserless only when `privacy.allow_third_party_processing=true`
  and `processor_id` matches
- no provider-side scraping or extraction in GDPR/default mode

The browser fleet should expose a Firecrawl-compatible interaction surface, but
Quarry keeps the runtime and artifacts.

### 6. Fingerprint Profile Catalog

Create versioned runtime profiles for fetch/TLS/browser paths:

- Chrome/Firefox/Safari-family transport profiles
- HTTP/2 and HTTP/3 capability flags
- header and redirect policy variants
- browser viewport, locale, timezone, font, and WebGL profile labels
- profile fingerprints recorded as metadata, not secrets
- per-domain A/B tests through `lab/evals`

Do not hard-code a single "best" profile. The registry should choose based on
domain evidence and policy.

### 7. Product Surface Parity

Add a self-owned equivalent for the pieces that make Firecrawl/Apify ergonomic:

- `/v1/scrape`: multi-format output in one request
- `/v1/crawl`: durable crawl with live events and typed skip reasons
- `/v1/map`: fast URL discovery without full content capture
- `/v1/search`: search plus optional scrape options
- `/v1/parse`: local/private file bytes to markdown/JSON without third-party
  upload
- `/v1/interact`: post-scrape browser interaction with explicit policy gates
- "recipes": Quarry-owned reusable scrape apps, similar to Actors but stored,
  executed, audited, and billed inside CoreSystem

The point is to challenge the product experience while keeping execution
sovereign.

## Runtime flow

```text
Request
  -> PrivacyPolicyResolver
  -> GDPRGate
  -> URL/SSRF/robots/purpose gate
  -> Domain Intelligence Registry
  -> DriverPlan
  -> EgressBroker v2
  -> Fetch/TLS/Browser
  -> Challenge Classifier
  -> PII Redactor
  -> Artifact/Retention Store
  -> Data Plane ingest
  -> AuditReceipt + Scoreboard event
```

## GDPR and data ownership rules

- Default mode is sovereign: no provider-side scraping, rendering, extraction,
  screenshots, parsing, or cache.
- Third-party network transit must be explicit, processor-scoped, and logged.
- The provider must not receive Quarry job metadata beyond what network transit
  requires.
- No raw HTML, screenshots, cookies, auth state, browser profiles, or extracted
  PII are stored outside CoreSystem-controlled storage.
- All artifacts carry source URL, org, run ID, purpose, lawful basis, processor
  involvement, retention expiry, and erasure lineage.
- High-risk jobs require a DPIA gate before execution.
- Challenge pages, auth walls, paywalls, and special-category/minor-risk signals
  should fail closed unless a documented policy permits continuation.

## Benchmark gates

Do not promise a fixed Akamai success percentage until the harness proves it.
Use a measured release gate instead:

| Metric | Gate |
|---|---|
| GDPR/default third-party scraping calls | 0 |
| Artifact privacy metadata coverage | 100% |
| General public-page scrape success | measured against benchmark corpus |
| Akamai/Bot-wall success | measured separately by domain and egress tier |
| False persistence of raw/ZDR artifacts | 0 |
| Profile restore success | >= 99% on allowed persistent-session corpus |
| Block reason classification coverage | >= 95% of failed benchmark cases |
| Cost per successful page | tracked by domain, driver, egress tier |

The scoreboard should compare:

- Quarry direct
- Quarry approved network proxy
- Quarry local browser
- Quarry TLS impersonation
- self-hosted Firecrawl
- Firecrawl cloud, when approved for benchmark only
- Apify Actor/proxy baseline, when approved for benchmark only

Benchmark runs must never send customer data to competitor services.

## Phased backlog

### Phase 1: Observability and receipts

- Add `egress_receipt` to runtime attempts and artifacts.
- Add challenge classifier outputs to `DriverInfo.attempts`.
- Extend `SCOREBOARD.md` with block-wall success metrics.
- Add benchmark corpus tags: `static`, `js`, `akamai`, `cloudflare`,
  `captcha`, `paywall`, `auth_required`, `geo`.

### Phase 2: EgressBroker v2

- Add egress tiers and per-domain health scoring.
- Add ASN/region/IP-type metadata.
- Add cooldown and quarantine controls per `(domain, egress_identity)`.
- Add first-party and BYO proxy pool adapters.
- Keep third-party network transit denied unless privacy policy approves it.

### Phase 3: Session health

- Introduce session health states.
- Bind session state to egress identity and fingerprint profile.
- Add encrypted profile TTLs and erasure hooks.
- Add ZDR/session persistence test cases.

### Phase 4: Domain intelligence

- Add domain profile schema and registry.
- Emit learning suggestions from benchmark/runtime outcomes.
- Require operator approval before production domain profile changes.
- Route DriverPlan from domain profile plus request policy.

### Phase 5: Owned browser fleet

- Make local/persistent browser pools production-grade.
- Add cleanup, profile validation probes, and pressure-aware admission control.
- Restrict managed browser providers to explicitly approved processor policies.

### Phase 6: Product parity layer

- Fill remaining Firecrawl-compatible output gaps.
- Add `/parse` for private file bytes.
- Add `/interact` with policy gates and browser receipts.
- Add Quarry "recipes" as first-party reusable scrape apps.

## Non-goals

- No Scrapfly/ScrapingBee-style provider API as a default fallback.
- No provider-side rendering/extraction in GDPR/default mode.
- No CAPTCHA solving as a silent default.
- No storing auth state, cookies, screenshots, or raw HTML without policy and
  retention metadata.
- No claim of 97% Akamai success until measured against a named corpus.

## Open questions

- Which network-only proxy providers are acceptable under the DPA/SCC/vendor
  review model?
- Do we want CoreSystem-owned regional POPs as a long-term first-party egress
  option?
- Should high-risk domain profiles require legal approval, security approval,
  or both?
- Should recipes live in Quarry Control or in a separate marketplace-like
  registry owned by Control Plane?
