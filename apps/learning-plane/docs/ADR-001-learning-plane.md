# ADR-001: A shared Learning Plane with Teachme as its first product

**Status:** Accepted for foundation; production integration pending evidence.
**Date:** 2026-09-24
**Deciders:** Product owner through this task; implementation choices recorded by Codex.

## Context

Teachme will guide students aged 13–18 through Windows applications with a shortcut, typed requests, voice, captions and on-screen pointing. Teachers create/review courses, assign work and eventually request device assistance. Verevon will later expose the same learning services for internal courses and employee onboarding. Agency may consume them for individual learning.

The user selected Next.js for marketing, Vite + Solid 2 for the dashboard, C#/.NET for native Windows and self-hosted Whisper for speech recognition. macOS is deferred. Existing CoreSystem changes in other products are independent of this scaffold.

## Decision

Create `apps/learning-plane` and `apps/Frontend Plane/learn-core`. Learning begins as a single Go service with domain modules and its own future database. Frontend is an independent pnpm workspace containing marketing, dashboard and a framework-independent learning SDK. Windows is a C#/.NET WPF application: WPF is chosen for a small native shell and future transparent desktop overlays, without WebView dependence for privileged operations. The first executable only registers a shortcut and opens a local request window.

Solid 2 is currently a release candidate. Pin the resolved Solid/web/plugin versions together; do not silently downgrade to Solid 1. Native .NET SDK installation for development is workspace-local, not a global workstation upgrade.

### Ownership

| Owner | Authority and records |
|---|---|
| Learning | Course structure, published revisions, assignment rules, attempts, formative mastery, rewards, course compatibility and maintenance, teaching-assistance intent/receipts |
| Control | Users, org membership, canonical capabilities, identity sessions, entitlements/budgets, device-access authorization integration and canonical audit; shared WhisperFlow API, speech-job metadata and processing policy |
| Data | Source documents/assets, versions, retrieval and evidence references; no independent Learning embeddings |
| Model | Tutoring/course generation, provider execution, speech routing, execution runs and capability grants |
| Application | Collaborative/realtime projections and notifications; projections cannot grant access |
| Ingestion/Quarry | Source capture, integration calls and governed browser execution |
| Frontend/remote-core | Product UX, observation/action contracts, local and remote guidance adapters; native device enforcement |
| Support | Rendezvous and relay transport only; no school identities, learning records or permission authority |

All cross-plane access uses versioned APIs/events and verified principals. No direct access to another plane's database. Learning attempt IDs and Model run IDs remain distinct, linked by reference. Course revision manifests belong to Learning; teaching-source document versions belong to Data.

Follow-up direction: build the shared WhisperFlow system under Control Plane for all products, including later meeting summaries. The proposed `speech-core` owns service management and scoped job records; Model executes speech and summary models, and Data owns any retained audio/transcript/summary content. This records the intended ownership only; no speech service has been implemented. See the [delivery sequence](IMPLEMENTATION_PLAN.md#sequencing-clarification--shared-whisperflow).

The existing `verevonv3/packages/remote-core` remains the shared remote implementation. Do not fork it into Teachme. The frontend SDK is independent of React/Solid, and desktop contracts must be serializable across TypeScript/C#/Go. Reuse implementation code only where runtimes actually allow it.

## Options considered

| Option | Complexity | Cost | Scalability | Familiarity |
|---|---|---|---|---|
| Put all learning in Verevon | Low initially; coupled product lifecycle | Low start, costly second product | Shared code tied to Verevon | Existing Solid team |
| Learning Plane, one Go service | Moderate contracts, bounded service surface | Reuses platform; one new service | Modules can split later if measured | Existing Go/TS patterns |
| New complete Teachme platform | High duplicated infrastructure | Highest operational cost | Independent but repeated work | Multiple new systems |

The dedicated domain is selected. Marketing, dashboard and Windows are separate applications because they serve different surfaces. They share contracts and design language, not assumed interchangeable components.

## Trade-offs and consequences

- A product-independent domain adds integration work now but prevents a later backend rewrite for Verevon.
- WPF adds C# maintenance alongside Rust/Go/TS. It supports the requested native Windows pilot; macOS will require another host adapter.
- Full sovereignty excludes externally hosted Astra in the default deployment. A separately approved external-processing profile may be designed later, but is not enabled by this foundation.
- Remote-core has live-host verification gaps and a documented RustDesk-compatible cipher limitation. Neither a new dashboard nor self-hosting the relay closes those gates.
- Native login, UAC/secure-desktop automation and unattended control are not part of the school teaching pilot. School IT provisioning must be designed independently if requested.

## Action items

- [x] Establish folders, ownership and explicit HTTP status contract.
- [x] Add supplemental sovereignty and assistance policy tests.
- [ ] Prove scoped Control integration and organization/class denial/revocation paths.
- [ ] Add durable course revisions/assignments/attempts with tenant-bound references.
- [ ] Connect one reviewed lesson through the Windows observation/annotation loop.
- [ ] Validate self-hosted speech and approved on-premise reasoning against the same lesson.
- [ ] Resolve remote release gates before allowing real classroom assistance.
