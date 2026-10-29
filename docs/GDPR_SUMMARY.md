# CoreSystem GDPR Summary

Status: draft operating contract for engineering and architecture review.

This document turns GDPR goals into cross-plane system rules. It is not legal
advice; final lawful basis, DPIA, DPA, retention, and processor decisions must
be approved by the responsible business owner, DPO, or counsel.

## Goals

CoreSystem should be privacy-preserving by default:

- Own core customer data in CoreSystem-controlled stores.
- Minimize personal data before persistence.
- Treat third-party calls as explicit processing decisions, not incidental
  networking.
- Propagate Zero Data Retention and privacy flags across every boundary that can
  persist, cache, index, embed, replay, or inspect content.
- Support deterministic deletion, anonymization, and retention exceptions across
  all derived copies.
- Preserve auditability without retaining unnecessary personal data.
- Honor org-level data residency and processor restrictions.

## Required Policy Metadata

Every persisted record or durable processing job that can contain customer
content or personal data must carry, or be linked to, a policy record with these
fields:

| Field | Requirement |
| --- | --- |
| `org_id` | Tenant authority for policy lookup and deletion scope. |
| `subject_id` | User, contact, customer, or external subject when known. |
| `purpose_id` | The purpose for processing, such as support, billing, import, retrieval, or model execution. |
| `lawful_basis` | Contract, consent, legitimate interest, legal obligation, or another approved basis. |
| `privacy_classification` | Data class used for routing, persistence, indexing, and provider policy. |
| `zdr` | Whether Zero Data Retention must be enforced end-to-end. |
| `retention_policy` | Expiry, deletion mode, and any approved retention exception. |
| `residency` | Region or residency boundary required by the org policy. |
| `allow_third_party_processing` | Explicit gate for managed providers, unblockers, model vendors, and SaaS processors. |
| `processor_id` | Approved processor registry entry when third-party processing is allowed. |
| `source_trace_id` | Evidence link without embedding full sensitive payloads in logs or audit events. |

## Privacy Classes

Use a small, enforceable classification set:

- `public_non_personal`: public data with no identifiable person.
- `customer_private`: tenant-confidential content that may not leave approved
  boundaries.
- `personal`: personal data under GDPR.
- `sensitive_personal`: special-category or high-risk personal data; deny by
  default unless there is an approved DPIA and explicit processing policy.
- `credential_or_secret`: OAuth tokens, API keys, passwords, cookies, session
  credentials, and private keys; never persist outside a vault or credential
  store.
- `zdr_ephemeral`: content that may be processed transiently but must not be
  stored, cached, embedded, logged, or sent to non-ZDR providers.

## Plane Responsibilities

### Control Plane

Control Plane owns identity, org policy, compliance configuration, and subject
rights orchestration.

Required controls:

- Maintain org GDPR flags, data residency, encryption, retention, and processor
  settings.
- Own the lawful basis and purpose registry.
- Own user, org, and subject deletion requests.
- Publish deletion, anonymization, retention, and policy-change events to
  derived planes.
- Maintain a processor registry covering billing, support, notification, model,
  browser, proxy, and connector providers.
- Disable mock email/SMS, consent TODO paths, and auth bypasses in production.

### Application Plane

Application Plane is a realtime and collaborative projection layer, not the
authority for identity or long-term data ownership.

Required controls:

- Treat Convex records as derived copies unless a specific Application Plane
  service is documented as the source of truth.
- Apply Control Plane deletion and anonymization events deterministically.
- For conversations, messages, jobs, notifications, and support projections:
  hard delete when in scope, or pseudonymize only when there is a documented
  retention exception.
- Verify webhooks with production-grade signatures.
- Avoid putting full sensitive payloads in notifications, websocket events, or
  support synchronization logs.

### Data Plane v2

Data Plane v2 owns durable documents, chunks, embeddings, retrieval indexes,
graph indexes, wiki records, source traces, object storage, and search
projections.

Required controls:

- Attach policy metadata to documents, chunks, embeddings, indexes, wiki pages,
  graph nodes, source traces, and object artifacts.
- Deny direct external embedding, reranking, or enrichment calls unless the
  processor is approved and `allow_third_party_processing=true`.
- Prefer Model Plane provider policy gates for model and embedding calls instead
  of ad hoc direct provider HTTP paths.
- Propagate delete and retention events to Postgres, Qdrant, Quickwit, MinIO,
  Redis, cache tables, and derived graph/wiki/search projections.
- Block production GDPR claims until ZDR rejection and cache invalidation tests
  cover durable stores and provider paths.

### Ingestion Plane

Ingestion Plane captures evidence, imports content, syncs connectors, and feeds
durable knowledge through Data Plane contracts.

Required controls:

- Classify content before import, crawl, browser capture, or connector sync.
- Redact or reject secrets and high-risk personal data before persistence when
  policy requires it.
- Treat OAuth tokens, provider sessions, connector state, cookies, proxy
  credentials, and browser sessions as `credential_or_secret`.
- Require explicit processor authorization before using managed browser,
  unblocker, proxy, or scraping vendors.
- Default Quarry egress to `allow_third_party_processing=false`.
- Disable development bypasses, in-memory production state, and unauthenticated
  control paths outside local development.

### Model Plane

Model Plane owns reasoning sessions, inference routing, execution loops,
capabilities, browser grants, sandboxes, and model cost policy.

Required controls:

- Gate every provider call with org policy, privacy classification, residency,
  ZDR, and processor approval.
- Do not send prompts, context, embeddings, tool outputs, browser captures, or
  files to third-party model providers when policy denies it.
- Ensure inference caches, prompt logs, session records, memory, runs, and
  sandbox snapshots obey retention and deletion policy.
- Audit browser and sandbox grants with expiry and minimal payloads.
- Prefer self-hosted or private-region providers for `customer_private`,
  `personal`, and `zdr_ephemeral` workloads unless an approved processor policy
  says otherwise.

## Third-Party Processing Policy

Third-party processing is denied by default for private, personal, sensitive,
secret, and ZDR data. It is allowed only when all of these are true:

- The org policy allows third-party processing for the specific purpose.
- The processor is registered with DPA, residency, subprocessors, retention, and
  incident-contact metadata.
- The request carries `allow_third_party_processing=true` and a `processor_id`.
- The target provider can satisfy the retention and ZDR requirements for the
  data class.
- The event is auditable without logging sensitive content.

| Provider Type | Default Position |
| --- | --- |
| Network-only proxy egress | Allowed only when metadata exposure is acceptable and logged. |
| Managed browser or unblocker | Denied by default; requires explicit processor approval. |
| Model, embedding, reranking providers | Denied for GDPR/ZDR content unless policy and processor allow it. |
| Billing processors such as Stripe or Lago | Allowed for billing purpose only with processor records and retention policy. |
| Support and notification systems | Allowed for support/notification purpose only with minimized payloads. |

## Quarry Proxy And Unblocker Position

Quarry should own the egress broker, policy checks, session metadata, logs, and
data minimization layer. External unblocker, browser, residential proxy, or
scraping providers should be adapters behind that broker, not direct product
dependencies exposed to tenants or upper planes.

Default posture:

- Use CoreSystem-owned fetch, browser, retry, cache, and rate-limit logic first.
- Use provider adapters only for domains or workloads that require them and only
  after processor policy allows it.
- Keep customer content, credentials, cookies, OAuth tokens, and tenant secrets
  out of third-party provider requests unless the approved processor contract
  explicitly covers that data class and purpose.
- Prefer region-bound egress pools and self-managed infrastructure for
  `customer_private`, `personal`, and `zdr_ephemeral` workloads.
- Make each provider decision auditable with domain, purpose, processor ID,
  privacy class, residency, retention policy, and fallback reason.
- Apply per-domain rate limits, robots/terms policy, abuse controls, and
  operator approval where legal or contractual risk is high.

This gives Quarry the operational ability to survive bot walls and 429s while
keeping GDPR ownership in CoreSystem. Providers become replaceable processors,
not the authority for captured data or policy.

## Deletion And Retention Flow

Deletion must be command-driven and verifiable:

1. Control Plane verifies request authority and scope.
2. Control Plane emits a deletion or anonymization command with request ID,
   subject scope, org scope, source scope, retention exception, and deadline.
3. Application Plane deletes or pseudonymizes derived conversations, messages,
   jobs, notifications, and support projections.
4. Data Plane deletes documents, chunks, vectors, graph nodes, wiki pages,
   search records, source traces, objects, and caches.
5. Ingestion Plane deletes import jobs, crawl artifacts, provider sessions,
   connector state, and source credentials that are in scope.
6. Model Plane deletes sessions, messages, runs, memory, provider caches,
   execution artifacts, sandbox snapshots, and browser traces.
7. Audit records completion, failures, retries, and retained exceptions without
   preserving erased payloads.

Retention exceptions are allowed only for documented legal, security, billing,
or audit obligations. Exceptions must retain the minimum data necessary and use
pseudonymization where possible.

## Audit Rules

Audit events should prove what happened without becoming a shadow data store.

- Log identifiers, policy references, source trace IDs, hashes, status, and
  processor decisions instead of full content.
- Never log OAuth tokens, cookies, API keys, prompts containing private content,
  document bodies, raw connector payloads, or browser screenshots by default.
- Failed audit writes must retry or enter a dead-letter path for security and
  deletion events.
- Audit retention must be governed by org policy and legal exceptions.
- Erasure responses must distinguish deleted content from retained audit facts.

## Production Readiness Checklist

CoreSystem should not claim GDPR-ready production behavior until these controls
are implemented and tested:

- Cross-plane policy metadata schema is used by Control, Application, Data,
  Ingestion, and Model Plane records.
- Processor registry exists and is consulted before external calls.
- DSAR export, deletion, anonymization, and retention workflows are tested
  across all derived stores.
- ZDR tests cover provider rejection, cache invalidation, logs, audit events,
  Data Plane indexes, Model Plane sessions, and Quarry captures.
- Direct provider paths are either removed or guarded by the same provider
  policy engine as Model Plane.
- Development bypasses and in-memory operational state are production-denied.
- Webhooks and cross-plane requests use verified signatures or strong service
  identity.
- Retention sweeps run for documents, vectors, objects, conversations, sessions,
  jobs, connector state, and audit events.
- Billing, support, notification, scraping, browser, proxy, model, and connector
  processors have DPA and residency records.
- DPIA review is required for sensitive personal data, large-scale monitoring,
  profiling, scraping, browser automation, or model training/evaluation use.

## Current Documentation Gaps To Close

The existing architecture is directionally aligned with GDPR, but these gaps
must be closed before treating it as complete:

- Convex/application derived copies need mandatory hard-delete or documented
  pseudonymization semantics; anonymization cannot remain optional for GDPR
  flows.
- Data Plane direct Azure embedding/provider paths need the same policy gates as
  Model Plane provider routing.
- Documents API authentication must move beyond transitional or observe-only
  claim handling before production use.
- Ingestion connector sessions, OAuth state, proxy state, and provider catalogs
  need explicit privacy classification and processor rules.
- Quarry development bypasses and in-memory defaults need production gates.
- Auth consent TODOs and mock email/SMS fallbacks need production-deny checks.
- Audit-core needs retry/dead-letter behavior for security and deletion events,
  plus explicit retention and erasure-exception semantics.
- Model Plane provider fallback, cache, prompt log, and session retention rules
  need policy enforcement tests.
