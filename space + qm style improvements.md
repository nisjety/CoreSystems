# Space + QM-Style Improvements

Date: 2026-08-16
Scope: Verevon v3 and the active CoreSystem planes
Reference: local QM clone at `/Volumes/Lagring/Triodelab/qm`

## Executive conclusion

CoreSystem has **not yet achieved QM parity as a product/runtime system**.

CoreSystem is stronger than QM in enterprise authority, multi-tenancy, ZDR/residency, signed delegation, grounded retrieval, evidence, typed business actions, cost accounting, and durable orchestration design. QM is currently more coherent end-to-end: one scope resolves the agent, workspace, permissions, memory, tools, background work, delivery, and Slack/web continuity.

The Verevon × QM tracker confirms that all release and adoption sequences remain `[~]`; none is `[x]` ([comparison plan](<apps/Frontend Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md:10>)).

## What makes QM work

### 1. Scope is the runtime spine

QM resolves every conversation to one active `scopeId`. DMs map to personal scope, groups to group scope, and channels to channel scope. The resolver composes organization and lower-scope instructions, workspace layers, command policy, security posture, approvals, egress, and audience grants in one operation ([QM resolver](</Volumes/Lagring/Triodelab/qm/src/resolution/resolution-service.ts:18>)).

CoreSystem currently has several valid but disconnected identities: organization, thread, workspace, project, case, and agent. The required next step is one canonical `SpaceRef`, not another generic project table.

### 2. Audience filtering is explicit

QM only exposes a history entry when every current conversation recipient is entitled to that entry's scope ([QM context filter](</Volumes/Lagring/Triodelab/qm/src/resolution/context-filter.ts:18>)).

CoreSystem has stronger signed audience and privacy primitives, but shared Space lifecycle, participant changes, replay filtering, and visibility-preserving fork/share are not complete.

### 3. One harness boundary, many adapters

QM uses a typed harness contract and runtime router for multiple model/harness adapters. Tool calls and results pass through shared recording, bounded output, provenance, and screening logic.

CoreSystem should borrow the adapter boundary and common provenance/output wrapper. It should **not** collapse the separate Model loops that intentionally enforce different authority boundaries.

### 4. Durable leases and idempotency are built into the run path

QM's Postgres run store uses idempotency keys, `FOR UPDATE SKIP LOCKED`, leases, heartbeats, reaping, and attempt/tool-call records ([QM run store](</Volumes/Lagring/Triodelab/qm/src/runs/postgres-run-store.ts:127>)).

CoreSystem has deeper Temporal/NATS/session infrastructure, but candidate, deployed recovery, and rollback evidence are still open.

### 5. Background work has a complete loop

QM monitors persist process cursors, expiry, deterministic fire keys, and delivery state ([QM monitor poller](</Volumes/Lagring/Triodelab/qm/src/monitors/monitor-poller.ts:119>)). Delivery is idempotent, claimable, retryable, and acknowledged ([QM delivery store](</Volumes/Lagring/Triodelab/qm/src/delivery/postgres-delivery-store.ts:24>)).

CoreSystem has cron and durable runs, and now has a source-level lease-fenced
Application delivery attempt plus content-free Activity/Inbox projection
outbox/callback path. It still lacks a release-proven provider/ZDR contract,
multi-replica replay evidence, and a unified user-facing watch primitive.

### 6. Scope-owned workspace and revisioned memory are first-class

QM provides scope-keyed workspace storage with path traversal protection ([QM workspace](</Volumes/Lagring/Triodelab/qm/src/workspace/workspace-store.ts:6>)) and append-only, CAS-protected memory revisions ([QM memory](</Volumes/Lagring/Triodelab/qm/src/memory/postgres-memory-service.ts:35>)).

CoreSystem has better learned-memory and Data Plane capabilities, but not one durable Space workspace plus authored, revisioned shared memory experience.

### 7. Deployment is a product contract

QM's deployment directory validates configuration, checks live state, records image/digest evidence, and supports rollback. CoreSystem has stronger multi-plane release checks, but no signed candidate and rollback artifact currently satisfy the release gate.

## Capability comparison

| Capability | Current verdict |
|---|---|
| Canonical person/room/project scope | QM leads. CoreSystem needs canonical `SpaceRef` and mappings. |
| Multi-tenancy, residency, ZDR | CoreSystem leads. Preserve Control authority and privacy boundaries. |
| Membership and revocation | Split. CoreSystem authority is stronger; revision fencing must cover membership, grants, entitlements, privacy, and recipient audience. |
| Context resolution | QM leads in coherence; CoreSystem leads in retrieval depth. Add `ResolvedSpaceContext`. |
| Space/project cockpit | QM leads. V3 needs a scope-centric `/spaces/:spaceId` cockpit. |
| Shared conversations and safe fork | QM leads. Space-owned threads and audience-filtered replay remain incomplete. |
| Business actions | CoreSystem leads in catalog breadth, but runtime execution coverage is incomplete. |
| Human/agent action parity | CoreSystem gap. TS registry, gateway dispatch, and Model eligibility are still separate contracts. |
| Approvals and proof | CoreSystem has stronger primitives; release/deployed continuation evidence remains open. |
| Runs and recovery | CoreSystem has stronger architecture; QM has useful lease/idempotency invariants to port as tests. |
| Workspace and memory | QM leads in coherent scope ownership; CoreSystem leads in retrieval and learned-memory depth. |
| Skills | Split. CoreSystem has versioned capability/skill infrastructure; QM has the better authorable/progressive-disclosure product. |
| Watches and delivery | QM leads. CoreSystem now has the source delivery/reconciliation boundary, but candidate/HA proof and the watch primitive remain incomplete. |
| Slack/web continuity | QM leads. Channel Plane remains future/docs-only in CoreSystem. |
| Deployment packaging | QM leads in one coherent deployment contract; CoreSystem has broader infrastructure but no candidate/rollback proof. |

## What is already better in CoreSystem

CoreSystem should not copy QM's trust model. QM explicitly assumes one internal organization and is not hardened as a public or multi-tenant boundary ([QM security scope](</Volumes/Lagring/Triodelab/qm/SECURITY.md:24>)).

CoreSystem is materially stronger in:

- Control-owned identity, membership, grants, entitlements, and signed decisions;
- multi-tenant isolation and ZDR/residency contracts;
- Data Plane hybrid retrieval, graph, citations, provenance, and Ingestion evidence;
- Quarry as the browser execution/evidence boundary;
- typed business-action breadth and owner-plane authorization;
- Temporal/NATS durable orchestration;
- proof bundles, cost accounting, and release evidence contracts.

Do not copy QM's documented weaknesses: fail-open `unscreened` screening ([QM screen](</Volumes/Lagring/Triodelab/qm/src/core/orchestrator/security-screen.ts:14>)), bypassable command policy, plaintext sandbox credentials, browser paths outside some gates, and audit writes that can be logged and dropped ([QM audit](</Volumes/Lagring/Triodelab/qm/src/admin/postgres-audit-log.ts:51>)).

## Current release reality

The Model Plane release register distinguishes source, test, integration, staging, candidate, rollback, and production evidence. Only candidate and rollback evidence support promotion ([release evidence](<apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md:9>)). The current register has no candidate or rollback evidence for the major claims ([register](<apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md:57>)).

The local Docker stack is healthy, but the Model status explicitly says the images are dirty-worktree, unsigned, unattested, and not a rollback target ([Model status](<apps/Model Plane/MODEL_PLANE_STATUS.md:18>)). V3 `pnpm typecheck` passes. QM's full test command could not complete in the clone because dependencies such as `fastify`, `lru-cache`, `jose`, and `@earendil-works/pi-ai` are not installed; the QM comparison is therefore source-grounded, not a claim of passing QM runtime tests.

The Agent Quality plan has the correct completion rule: tests, deployed artifact, and observed real chat/log behavior are all required ([quality plan](<apps/Model Plane/docs/AGENT_QUALITY_PLAN_2026-07-29.md:3>)). It also records that deployed chat grounding has historically failed at the configuration/credential boundary ([quality plan](<apps/Model Plane/docs/AGENT_QUALITY_PLAN_2026-07-29.md:12>)).

## Highest-priority improvements

### P0 — Establish the Space spine

1. Choose one canonical Application-owned Space aggregate for immutable ID, kind, name, lifecycle, and collaboration metadata.
2. Let Control own Space membership, grants, decisions, and authority revisions.
3. Define `SpaceAccessDecision` and `ResolvedSpaceContext` with service audience, recipient audience, resource authorization, privacy/ZDR, idempotency, and payload/schema digests.
4. Require effective access to be the intersection of Space authority, current recipient audience, and owner-plane resource authorization.
5. Complete one Personal Space chat journey with live create, append, revoke, reload, and cross-organization isolation proof.

### P0 — Make one governed operation real

1. Finish the Action Catalog as a versioned, owner-approved contract artifact.
2. Generate actor-specific views; preserve human-only decisions outside Model APIs.
3. Complete one ticket/action operation through Control, Capability Core, Conversation Core, Execution Core, Session Core, Model, and V3.
4. Add distributed reservation/commit, durable receipt reconciliation, approval continuation, and unknown-outcome handling.

### P1 — Add QM's coherence layer

1. Build `ResolvedSpaceContext` as the common input to chat, retrieval, tools, runs, approvals, memory, skills, files, and delivery.
2. Add a durable Space workspace with per-run overlays, CAS promotion, and conflict receipts.
3. Add authored instructions and revisioned shared memory; keep learned memory and Data Plane knowledge separate.
4. Add skills-as-files with descriptions always available, bodies on demand, scoped grants, evaluation, promotion, and rollback.
5. Add a scoped credential broker with purpose, host, path, method, TTL, revocation, and usage receipts; never expose secrets to the model.

### P1 — Complete background collaboration

1. Add durable watches with cursor, expiry, deterministic fire keys, and cancellation.
2. Run the Application-owned at-least-once delivery outbox with claim leases, callbacks, reconciliation, and HA replay through an immutable candidate; the source boundary now exists.
3. Represent `pending`, `claimed`, `sent_unconfirmed`, `acknowledged`, and `unknown` explicitly; do not promise exactly-once delivery.
4. Finish service-owned scheduled execution. The current scheduled preparation lane is source-verified, but ordinary user-bound `ExecuteStep` cannot execute a service-owned scheduled run.

### P2 — Expand surfaces after the contract is stable

1. Shared rooms, participant projections, safe fork/share, and presence.
2. Slack/web continuity through a surface-neutral delivery contract.
3. Checkpoint/rewind, message siblings, branch conversations, slash commands, and read-only subagents.
4. Governed internal-app build/publish lifecycle.

## Acceptance test for “QM parity achieved”

Do not declare parity until one immutable candidate demonstrates this complete flow:

```text
Create Space
  -> resolve signed Space context
  -> start/reconnect chat
  -> retrieve only authorized evidence
  -> use durable workspace/memory/skill
  -> execute one governed operation
  -> pause and resume approval after restart
  -> watch background progress
  -> deliver completion to Inbox
  -> revoke participant/resource and deny further access
  -> erase/export with per-plane receipts
  -> roll back the candidate without rebuilding
```

## 2026-08-17 implementation checkpoint

The dedicated Capability Core owner-action receiver is no longer only a dead
endpoint contract: Conversation Core now has a source-wired reporter for
`tickets.create`. It mints a short-lived, narrowly scoped token, reads the
current capability version, and posts only content-free readiness metadata.
The reporter is disabled unless the complete Control-bound execution lane and
its separate service-principal block are configured; any mint/read/write error
keeps the capability unavailable. Conversation Core's full Go suite passes.

This closes a source-level gap, not the release gate. The next work is live
dev observation of that reporter, authenticated transport evidence, the
Control/Conversation reservation commit fence, provider/ZDR proof, approval
continuation after restart, Application HA delivery replay, and an immutable
candidate plus rollback target. Until those are observed, `tickets.create`
must remain unavailable to Model and the system remains QM-inspired rather
than QM-parity complete.

## Final decision

CoreSystem should be described as a **QM-inspired, enterprise-governed agent platform under construction**, not as a completed QM-equivalent system. The right strategy is to adopt QM's scope coherence, resolver, harness-adapter discipline, durable watch/delivery loop, scoped workspace, revisioned authored memory, and deployment evidence while keeping CoreSystem's Control, Data, Ingestion, Quarry, Model, Application, and privacy boundaries authoritative.
