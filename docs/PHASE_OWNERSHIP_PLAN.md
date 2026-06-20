# Velion — Per-User Data Ownership & Sharing (Fine-Grained Authorization) Execution Plan

> **Status:** Approved 2026-06-19 (recon-grounded + hardened by a 6-persona honesty-first council). **Gated on Phase 0 (org/tenant isolation) — ALREADY MERGED at `ff2a3f59`.** A distinct foundational phase that runs **in parallel** with Phases 1/2; Phase 1/2 surfaces stay honestly org-scoped and adopt ownership **one at a time, after the documents pilot is proven**. Source: `Velion-ai-first.md`.
>
> ## What this phase is
> Make workspace data **private to its creator by default**, shared only via explicit per-resource grants (share-with-user, view-only for MVP), **and make the AI agent only ground/retrieve on data the requesting user can see**. This is fine-grained authorization *below* Phase 0's org isolation. **MVP = documents only** — the only resource type whose ACL scaffolding already flows through retrieval, so the only place a privacy claim is enforceable end-to-end today.
>
> ## THE ABSOLUTE RULE — no false-privacy guarantee (the dominant failure mode)
> A half-enforced ACL that ships a "Private" badge or "the AI only sees your data" claim while the agent retrieval path still grounds on the whole org is **worse than shipping nothing** — it actively lies about confidentiality. **NO surface (UI badge, ShareDialog, Trust Center, agent claim) may advertise any privacy/sharing guarantee until, in the SAME release: (a) document GET/LIST filter-at-source AND (b) the retrieval post-filter AND (c) viewer-identity propagation are all live and enforcement is Strict.** Badges derive from the *same* authority retrieval enforces — a badge can never disagree with what the agent can actually see.
>
> ## Verified ground truth (the substrate is ~60% dormant-but-real, not greenfield)
> - `document_acl` table **exists but is DEAD** (zero code refs) and **DUPLICATED**: Data Plane v2 `init.sql:130` + Control Plane `user-core migration 009`. `documents.created_by` is **real + wired** but every SELECT filters by `org_id` only.
> - retrieval-engine-rs **already has a tested fail-closed policy spine**: `HttpPolicyClient` + moka 5-min cache (`authz/policy.rs:178`), `EnforcementMode{Off,Strict,Permissive}`, `EffectiveAcl` (with `can_read/can_write/can_delete`), `intersect_filter`/`apply_to_request` (`authz/context.rs`). It is fed `allow_all` on the api-key path.
> - The orchestrator has a **post-filter seam**: `filter_live_candidates` (`orchestrator.rs:535`) runs at **step-6 AFTER RRF fusion + rerank** on the **unified dense+sparse+wiki** candidate list, already doing `SELECT document_id FROM documents WHERE document_id=ANY($1) AND <cond>` then dropping non-matches, with a documents/`wiki_pages` split (`orchestrator.rs:557-587`).
> - **The headline hole = identity:** the gateway/agent path takes `AuthContext::org_scoped` (`api/mod.rs:79`) and **discards the viewer**; the agent sends `user_id: None`. The dense Qdrant payload (`qdrant_writer/mod.rs`) carries no ACL keys (so `to_qdrant_conditions` ACL filters match nothing); the sparse/bm25 path has no ACL filter.
> - `user-core` already registers `DocumentAccessService` gRPC + `AclRepository` + the `PublishDocumentAclChanged` NATS publisher. org-core serves `/internal/v1/orgs/{org}/users/{user}/permissions` that `HttpPolicyClient` calls.

---

## Locked decisions

- **Architecture — Option C Hybrid (no Zanzibar, no new core):** per-core `owner_id` + `visibility ENUM(private|org|shared)` for the cheap common-case check resolved **in-core SQL**; ONE central grant store — generalize the dormant `document_acl` → `resource_grants(org_id, resource_type, resource_id, subject_type[user|team], subject_id, role[view|edit], granted_by, granted_at)` **folded into user-core**; coarse org RBAC stays in org-core. A single internal **authz facade** (`Check` / `BatchCheck` / `ListVisible`) preserves a future Zanzibar extraction as a deployment move. *Why not OpenFGA/SpiceDB:* it documents `ListObjects` as "small collections only" — exactly the retrieval-scale visible-set primitive that is our hardest call, and adopting it discards the already-tested spine. Revisit only when grant graphs gain depth (nested teams / role inheritance).
- **Retrieval — POST-FILTER at orchestrator step-6 for MVP** (no reindex): add a third ACL pass mirroring the documents/`wiki_pages` split — keep candidates where `owner_id=$viewer OR visibility='org' OR EXISTS(resource_grant for $viewer)` (+ the existing published-wiki branch). Because step-6 runs **post-fusion**, this gates dense + sparse uniformly — **closing the sparse leak for free**; do NOT disable hybrid-sparse and do NOT add a second per-arm filter (it will drift and reopen the leak). The canonical-Postgres post-filter is the **authoritative gate and permanent backstop**. PRE-FILTER (Qdrant payload `owner_id`/acl + per-document re-stamp + sparse query-time wiring) is **FULL-only** and becomes defense-in-depth.
- **Sequencing:** foundation-early + parallel-with-Phases-1/2 + **adopt-one-surface-at-a-time post-pilot**. Reject coupled "adopt-as-built across Phase 1/2" (spreads partial enforcement / stalls them) and reject post-Phase-2 big-bang (retrofits privacy onto live shared data).
- **Migration:** grandfather-as-org-shared (`visibility='org'`, `owner_id=created_by`, one transaction, verified zero-NULL post-condition, NULL-`created_by` → org-system-account). NEW resources default **org** (Private is an explicit, audited per-resource opt-in); store the default as an org-policy row so enterprise tenants can flip later without code change.

## Hard invariants (every PR)
- Front-load **Phase 0 merged (`ff2a3f59`)**. Ownership enforcement is **ALWAYS-ON when a `user_id` is present, DECOUPLED from `CONTROL_PLANE_ENFORCEMENT`** (which gates only the coarse org-axis ACL for back-compat) so a default-off deploy cannot silently disable per-user privacy.
- Each PR is a focused, revertible commit on its own branch off `main`; never reset/clean the user's tree.
- Quality gates green: `cargo test --workspace` + `clippy -- -D warnings` (retrieval-engine + gateway); `go test ./... -race` (user-core, documents-api-go, erasure subscriber).

---

## PR sequence

### `PR-1` — Generalize grant store + kill duplicates *(Control Plane / user-core)* — deps: Phase 0 merged
- Migration generalizing `document_acl` → `resource_grants` (subject_type + role present from day one; MVP uses `subject_type=user`, `role=view`).
- **Drop BOTH duplicate `document_acl` copies** via migration (DPv2 `init.sql` + user-core 009) so `resource_grants` is the single authority.
- Add `AclRepository.ListVisible(subject,org,resource_type) → {ids[], all-org sentinel}` (explicit-grant ids only — private/org handled by columns, keeping the set small) + `Check` + `BatchCheck`; widen `DocumentAccessService`; generalize the publisher to `resource_grants.changed`.
- Lock the **ownable-vs-team taxonomy** as a compile-time constant (Go + Rust): `OWNABLE={document,...}` vs `TEAM_SHARED={inbox/conversation/ticket, billing, audit_log, org_settings, quarry_source, quarry_run, capability_registry}`; CI lint fails any visibility-bearing type not in exactly one set; grant API rejects private/user-grant on a TEAM_SHARED type.
- **DoD:** `resource_grants` is the single authority; both old tables gone with a test asserting no code reads them; `ListVisible`/`Check`/`BatchCheck` unit-tested; taxonomy lint green; `go test ./... -race` green.

### `PR-2` — Document ownership columns + backfill + filter-at-source *(documents-api-go)* — deps: PR-1
- Add `documents.owner_id` + `visibility ENUM(private|org|shared) DEFAULT 'org'`; grandfather backfill in ONE transaction (`visibility='org'`, `owner_id=created_by`; NULL-`created_by` → org-system-account; verified zero-NULL post-condition). `CreateDocument` sets `owner_id` from authctx on new inserts.
- GET/LIST filter-at-source: `WHERE org_id=$1 AND (owner_id=$user OR visibility IN ('org','shared') OR EXISTS resource_grant for $user)`.
- **DoD:** migration test proves no existing row became inaccessible to any teammate (non-breaking); integration test (non-owner → empty/404 on private; org-visible seen by all; per-user grant scopes to exactly that user+doc); `go test -race` green.

### `PR-3` — Permission-aware retrieval post-filter + viewer-identity *(ATOMIC — gate + identity together)* *(Data Plane retrieval + Model Plane agent)* — deps: PR-2
- Third ACL pass in `filter_live_candidates` (`orchestrator.rs:535`), AFTER fusion+rerank, mirroring the documents/`wiki_pages` split (published wiki stays via its branch).
- When `x-user-id` rides alongside `INTERNAL_API_KEY`, build a **per-user `AuthContext` via `HttpPolicyClient`** instead of `AuthContext::org_scoped`/`allow_all` (`api/mod.rs:79`); gateway forwards `x-user-id` on the retrieve call; **agent path threads the run's verified `user_id` into `RetrieveRequest.user_id`** (stop sending org-only/`user_id:None`); forbid an org-wide service-account grounding path. Ownership enforcement **always-on when `user_id` present**.
- Over-fetch: `k_fetch = min(top_k*4, ~1000)`; iterate up to a bounded cap then **honestly undershoot** — never backfill with non-visible docs. User-facing counts/ranks/scores derive from the **post-filter** set only (no pre-filter total-hit or cross-doc score leak). Record raw-vs-survivor counts on the trace + starvation alarm.
- **DoD:** the **FOUR-PATH honesty test** (below) green and release-blocking; code review confirms no second divergeable ACL filter in `sparse.rs`/`dense.rs`; low-visibility-user test confirms iterate-or-undershoot with no silent backfill; `cargo test --workspace` + clippy green.

### `PR-4` — Enforcement rollout + honesty gate *(Data Plane + config)* — deps: PR-3
- Promote `CONTROL_PLANE_ENFORCEMENT` **Off → Permissive (fail-open + audit-only shadow; review deny-rate on real data; load-test moka + org-core `/permissions` QPS) → Strict**. Never flip Strict and claim the guarantee in the same step.
- Add a retrieval NATS subscriber for `resource_grants.changed`/`DocumentAclChanged` that **evicts the affected `(user_id,org_id)` moka key on revoke** (revoke effective within one query; TTL is the backstop). Retain the **positive-only cache guard** (`policy.rs:191`) as load-bearing. Guarantee wording: "revoke effective within seconds, guaranteed within 5-min TTL".
- Gate the UI/agent privacy **claim** on Strict AND viewer identity reaching retrieval.
- **DoD:** Permissive deny-rate reviewed before Strict; revoke effective within one query post-eviction; load test passes; a **written phase-doc honesty statement** records that MVP enforcement is POST-FILTER in canonical Postgres (recall-bounded under skew), that the Qdrant/Quickwit ACL conditions are inert in MVP, and that no surface claims privacy until gate+identity are both merged.

### `PR-5` — GDPR erasure subscriber + org-admin bypass *(Control Plane + Data Plane)* — deps: PR-1, PR-2
- `velion.gdpr.erasure.requested` subscriber (owned-resource scope, idempotent): owned docs transfer `owner_id` to org admin (default) or delete per policy; grants `WHERE subject_id=$user` revoked; emit `velion.gdpr.ownership.transferred`. Coordinate with Phase 2's deferred cross-plane erasure (one consumer per plane — this phase owns transfer+revocation; Phase 2 owns byte-purge).
- Org-admin **super-visibility**: a distinct path gated on a discrete `org:data:read_all` capability (NOT generic admin), org-scoped only (never cross-org), every bypass read emits `reason='admin_bypass:read_all'` + an `access_audit_log` row with target doc ids, UI/agent-labelled "admin override", **excluded from default agent grounding**.
- **DoD:** full erase-and-verify cycle test (owned docs transferred, inbound grants gone, `ownership.transferred` emitted); admin_bypass audit row asserted; test proves an admin cannot silently read private data and that bypass does not extend into default agent grounding.

### `PR-6` — Share UI + badges behind the honesty gate *(velionv3)* — deps: PR-4 (Strict + gate), PR-5 (bypass copy)
- Private/Org/Shared badges derived from the SAME authority retrieval enforces (`resource_grants` + visibility, never a separate display flag); ShareDialog (single user, role fixed "Can view" for MVP, live grant list with remove); read-only "shared with me" paged off `ListVisible`; Trust Center copy describing only enforced guarantees.
- **DoD:** automated test confirms **no badge/Private option renders when `CONTROL_PLANE_ENFORCEMENT!=strict` or identity is not live**; reviewer checklist confirms no surface claims a privacy property the retrieval path does not deliver.

### The release-blocking FOUR-PATH honesty test
A document marked **Private by user A** must be absent from user B's: (1) GET/LIST, (2) **dense** retrieval, (3) **sparse/bm25** retrieval (via the same post-filter — an assertion that would FAIL if only dense were filtered), and (4) **agent grounding** (e2e through model-gateway, as A then B).

---

## MVP scope
**IN:** documents-only; `owner_id`+`visibility`; `resource_grants` activated (user subject, view role); `Check`/`BatchCheck`/`ListVisible`; document GET/LIST filter-at-source; retrieval step-6 post-filter (dense+sparse+wiki); viewer-identity propagation (HTTP + agent), always-on, decoupled from `CONTROL_PLANE_ENFORCEMENT`; org-admin `org:data:read_all` audited bypass excluded from agent grounding; GDPR erasure subscriber (transfer/revoke); taxonomy constant + CI lint; drop both `document_acl` duplicates; grandfather migration; Off→Permissive→Strict rollout; the four-path honesty test.
**DEFERRED (FULL / later):** teams as a subject_type + team tables; edit-role grants + write-authz; other ownable resource types; full "shared with me"; PRE-FILTER (Qdrant payload + re-stamp + sparse query-time wiring); a NATS-projected grant read-model table in Data Plane (revocation-staleness hazard — MVP resolves synchronously); sub-second revocation; cross-plane content-byte PII purge (stays Phase 2); OpenFGA/SpiceDB + a standalone authz-core; private-by-default for new resources; per-surface ownership across Phase 1/2 (adopt one at a time, post-pilot).

## Risk register
- **FALSE-PRIVACY guarantee (headline)** → absolute honesty gate: no private/shared affordance or claim until filter-at-source + retrieval post-filter + identity are all live in the same release and enforcement is Strict.
- **Sparse/bm25 leak** → enforce at the post-fusion step-6 seam (gates all arms); never a per-arm filter or disabled hybrid-sparse.
- **Migration data-exposure/loss** → grandfather-as-org-shared in one atomic transaction with verified zero-NULL post-condition + NULL-`created_by` fallback.
- **Every-surface blast radius** → documents-only pilot, one enforcement point per concern; Phase 1/2 decoupled.
- **Fail-open / silent-disable** → ownership enforcement always-on when `user_id` present, decoupled from `CONTROL_PLANE_ENFORCEMENT`; never flip straight to Strict on a Control-Plane blip.
- **Revocation staleness** → fine-grained gate resolved synchronously against canonical Postgres (instant); NATS only for cache-invalidation; positive-only cache guard retained.
- **Top-k starvation side-channel** → inflate `k_fetch`; iterate-then-honestly-undershoot, never backfill; counts/scores from the visible set only.
- **Unaudited/implicit org-admin bypass** → discrete `org:data:read_all` capability, org-scoped, every read audited, excluded from agent grounding.

## Tooling & skills kit for the executor
- **Structural search:** `codegraph` (`codegraph_context`→`codegraph_explore`); `context-mode` (`ctx_execute_file`) for big files; re-confirm with `rg`.
- **Library/API docs:** `context7` MCP / `ctx7` for axum, sqlx, Qdrant Rust client, Quickwit/tantivy, NATS/JetStream, gin, SolidJS, Better Auth.
- **Rust (PR-3 retrieval, PR-4, gateway):** skills `rust-patterns`, `rust-testing`, `rust-asyncdrop-pattern-guide`; agents `rust-reviewer`, `rust-build-resolver`.
- **Go (PR-1 user-core, PR-2 documents-api, PR-5 subscriber):** skills `golang-pro`, `golang-patterns`, `golang-testing`, `go-concurrency-patterns`; agents `go-reviewer`, `go-build-resolver`.
- **DB/migrations (PR-1, PR-2):** skills `database-migrations`, `postgres-patterns`; agent `database-reviewer`. **AuthZ/security:** skill `security-review`; agent `security-reviewer` (MUST review PR-3/PR-4 — the privacy gate).
- **SolidJS (PR-6):** skills `solidjs-vite-typescript`, `solid-*`, `vite` (NO Tailwind; `<For>/<Show>`; `props.x`); agents `typescript-reviewer`, `build-error-resolver`.
- **Method:** skills `tdd`/`tdd-workflow` (RED-first for the four-path honesty test, the migration non-breaking test, the starvation test), `code-review`, `verification-loop`; agents `code-reviewer`, `tdd-guide`, `e2e-runner`. **Browser smoke:** `playwright`/`chrome-devtools` MCP. **PRs:** `github` MCP / `gh` CLI.

## Global Definition of Done
`resource_grants` is the single authority (both `document_acl` duplicates dropped); documents have `owner_id`+`visibility` with a proven non-breaking grandfather backfill; the authz facade exposes fail-closed `Check`/`BatchCheck`/`ListVisible` with a machine reason on every decision; document GET/LIST and the retrieval step-6 post-filter both enforce ownership; viewer identity reaches retrieval and the agent grounds as the human; the **four-path honesty test is green and release-blocking**; enforcement promoted Off→Permissive→Strict with NATS revoke-invalidation; **no surface advertises a privacy guarantee until Strict + identity are live**; org-admin bypass is discrete, audited, and excluded from agent grounding; the GDPR erasure subscriber transfers/revokes idempotently; the ownable-vs-team taxonomy is a CI-enforced constant; Phase 1/2 remain unchanged and honestly org-scoped; all quality gates green.
