# T3 / Category 13: Platform architecture (cross-source comparison)

297 reference records from 12 sources were grouped into 60 capability clusters. 285 records are assigned to a cluster and 12 are excluded (lineage notes, Rust-port meta-analyses, IDE-only plumbing, dated release notes). All 22 Verevon baseline records are mapped. Verevon status across the clusters: 15 shipped, 29 partial, 8 absent, 3 planned, 5 unknown. Prevalence classes are: table-stakes for 6 or more sources, common for 4-5, differentiator for 2-3, unique for 1. The 4 web-only benchmarks cannot show internals, so the table-stakes threshold is lowered to 6.

## Shared baseline (table-stakes)
- **One runtime, many surfaces** (C-001): the GUI, CLI, headless mode, daemon and SDK all run on one engine (Codex, Antigravity, T3, Hermes, DeepSeek). Verevon status: partial. There is one Model Plane, but its only consumers are the SPA (through the BFF) and cron.
- **Desktop shell plus a supervised backend child** (C-002), **desktop distribution and auto-update** (C-031), and **multi-environment connections** (C-004). These are table-stakes only for desktop and IDE tools. Verevon has none of them. That fits a web-first product, so they are rated skip or nice-to-have.
- **One typed RPC contract** (C-006): Connect/protobuf or a JSON-RPC app-server. Verevon status: shipped. Internal gRPC uses proto with `buf breaking` in CI.
- **Explicit state layout and per-conversation workspace** (C-016). Verevon status: partial. There is no per-chat workspace outside Spaces.

## Best of breed
| Capability | Best source (records) | Why |
|---|---|---|
| Executor protocol: cloud proposes, host executes (C-003) | Manus (cca-002) | One protobuf envelope protocol (Execute/Progress/Cancel/Transfer) serves both the local operator and the cloud sandbox. |
| Event-sourced commands and receipts (C-013) | T3 Code (a-008, uam-091) | Serial single writer. Events, projections and an idempotent receipt are written in one SQL transaction. DeepSeek (ha-094) derives the model-visible surface from the same log. |
| Durable log formats (C-014) | DeepSeek (ha-095, tac-117) | Checksummed zstd frames, immutable generations and adjacent migrations. A reader refuses unknown required events. |
| Live UI state sync (C-010) | T3 Code (a-092, uam-047) | Attaches to the live stream before taking the snapshot, coalesces updates per aggregate, resumes after the client's sequence, and paints cache-first. |
| Schema codegen (C-007) | Hermes (ahs-139) | Pydantic contracts generate TypeScript and OpenRPC. Unknown keys are rejected, and contract violations fail in tests. |
| Blocking UI requests (C-041) | Hermes (ahs-140) | The agent thread waits for the answer. Open requests are replayed when a client reconnects, and clients declare up front that they can answer. |
| Release safety (C-032) | T3 Code (a-013); Claude (ra-114) | The new version is trial-started against a database snapshot and rolled back on failure. Releases use a signed manifest with staged rollout. |
| Per-session VM (C-036) | claude.ai (wp-119, DOC) | One VM per conversation. It stops when idle, keeps an encrypted snapshot, and expires after 30 days. |
| Settings with an agent-access class (C-020) | Claude (ra-116, pu-120) | A Zod registry marks keys the agent may not read or change. Grant maps can only be reduced. |
| Privacy-typed fields (C-017) | Cursor (ha-045, af-120) | `RedactedString` and privacy-classified proto fields. Blobs are content-addressed and encrypted. |
| Fitness gates (C-046) | DeepSeek (ha-143); Hermes (ahs-155) | AST-based architecture rules, plus a complexity ratchet whose settings are read from main. |
| Telemetry scrubbing (C-018) | Claude (ra-121) | Allow-list redaction of tool, MCP, plugin and skill names, with an extra rescrub pass for HIPAA orgs. |

## Differentiators worth noting
- Cursor's session-as-a-service API (fork, watch, interrupt, attach) (C-011).
- Hermes's 25-platform messaging gateway (C-038) and its confirmed truncating rewind with durable row ids (C-043).
- Artifact hosting with viewer-scoped storage and identity: ChatGPT Sites and claude.ai (C-059; DOC evidence only).
- Prompt-cache doctrine: skill changes arrive as per-turn notes instead of a rebuilt prompt (C-056).
- Antigravity's host-capability injection, which runs one UI bundle as web, PWA, desktop and remote view (C-033).

## Verevon's biggest gaps (priority tier)
1. **Per-session VM / hosted computer** (C-036, significance 5, XL). Today the sandbox is bwrap inside execution-core. There is no VM, no persistent per-chat workspace and no computer use. This blocks the Manus/Claude "acts like a PC" vision. Recommended route: add a SandboxBackend seam in sandbox-manager with a microVM backend, ship it together with the executor protocol (C-003, recommended), and keep Quarry as the browser executor.
2. **Multi-replica-safe runtime state** (C-057, significance 4, M). These registries live in process memory: cancel, queued input, plan grants, hooks, idempotency and teams. The SSE connection pins a run to one process, and grants are lost on restart. No reference solves this, because the references are single-host.
3. **Agent loop resumes after approval or ask-user** (C-041, significance 4, L). After approval only three effect kinds are re-executed, and the LLM loop itself does not continue.
4. **Release safety** (C-032, significance 4, M). Artifact v3 tooling exists, but no signed candidate or rollback artifact has been built (DOC). The audit-trail fix is built but not deployed.

Recommended next, in order of leverage:
- MCP dispatch reachability (C-040).
- Conversation full-text search (C-015).
- Rewind/supersede (C-043).
- Command receipts with a per-send idempotency key (C-013).
- A feature-flag registry to replace the env-flag sprawl (C-019).
- Generated SPA types (C-007).
- Live sidebar sync (C-010).
- Fork and attach (C-011).
- Privacy-typed fields aligned with ZDR (C-017).
- Fitness gates for the plane rules (C-046).
- PWA (C-033).
- Artifact hosting (C-059).
- Prompt-cache ordering (C-056).
- Safe-fs check in sandbox-manager (C-025).
- Real scheduled-run delivery (C-037).
- Public Agent API (C-001).

## Verevon strengths (tier n/a: keep)
- **Credential isolation** (C-035): Integration Core owns tokens, and the BFF mints audience-bound bearers per request.
- **Multi-tenant isolation** (C-039): org, user and Space scoping with audience revisions.
- **Server-resolved session contract** (C-042): the client can only tighten it.
- **Two deliberately separate loops** (C-049): this is the authority boundary.
- **Durable outbox and JetStream** (C-050), **Postgres store with migrations and reverts** (C-014), **lifecycle drain and terminalization** (C-028), **service-level isolation** (C-027).
- **Redis-backed resumable streams** that work across replicas (C-009), and per-client event families (C-012).
- **Plane ownership matrix** (C-045) and **decision ledger with postmortems** (C-047).
- **Cross-plane contract goldens** (C-007/C-046 strengths): these go beyond any reference.

## Caveats
- Some clusters rest only on DOC or INF evidence: C-036 per-session VM, C-055 metering, C-052 retrieval, C-054 EDR, C-059 artifact hosting, C-060 generative UI (INF only), and parts of C-004/C-005. Their confidence was not raised.
- C-057 has only one reference record (a DeepSeek limitation, q2). Its priority comes from Verevon's own code, not from reference prevalence.
- 5 clusters are "unknown" and need code checks:
  - C-021: Control Plane org-policy schema.
  - C-025: symlink containment in sandbox-manager file APIs.
  - C-048: a product-help skill.
  - C-052: Data Plane retrieval engine.
  - C-053: Quarry recording.
- C-010 (no server-pushed sidebar) and C-056 (prompt prefix stability) were not fully re-verified in code.
- The release-safety status relies on the dated MODEL_PLANE_ROADMAP (2026-07-16). The repo's doc register warns that many docs are stale, so this status needs a code-level re-check.
- Several gaps overlap other categories: agent runtime (C-041), tools (C-040), chat (C-015, C-043) and models (C-034). Here they are scored only for their architectural part.
