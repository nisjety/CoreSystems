# External Ideas Harvest — Model Plane

> Source-mined via [`opensrc`](https://github.com/vercel-labs/opensrc) on 2026-05-30 from five OSS/reference agent stacks. This document maps their strongest ideas onto our actual services and roadmap gaps (`ARCHITECTURE.md`, `ROADMAP.md`, `gap-model.md`). It is a **planning artifact**, not an implementation. Every adoption is gated by the license matrix in §1 — read that first.

Repos analyzed (cloned to `~/.opensrc/repos/github.com/<owner>/<repo>/main`):

| Repo | What it is | Lang | License | Use mode |
|---|---|---|---|---|
| `openai/codex` | OpenAI's coding agent; `codex-rs` Rust workspace | **Rust** | **Apache-2.0** | **Vendor / fork** (best fit — Rust like us) |
| `nousresearch/hermes-agent` | Self-improving agent w/ learning loop, channels, cron | Python | **MIT** | Port ideas + prompt text freely |
| `earendil-works/pi` | Self-extensible coding-agent harness | TypeScript | **MIT** | Port ideas freely |
| `daytonaio/daytona` | Secure elastic sandbox runtime for AI code | TS (NestJS) + Go | **AGPL-3.0** | **Ideas only — clean-room Go reimpl** |
| `yasasbanukaofficial/claude-code` | Leaked/de-minified Claude Code source (npm sourcemap) | TypeScript | **None (proprietary)** | **Clean-room shapes only — ZERO code** |

---

## 1. License & Provenance Matrix (READ FIRST)

This governs *how* we may use each source. Violating it creates legal/IP risk.

| Repo | License | What we MAY do | What we MUST NOT do |
|---|---|---|---|
| **codex** | Apache-2.0 | Vendor or fork crates into our tree; modify; ship internally. Retain `LICENSE`+`NOTICE`, state changes, attribute in `Cargo.toml`. | Use "Codex"/"OpenAI" branding/trademarks. Copy the ChatGPT-account-coupled crates (`login`, `chatgpt`, `codex_auth`). |
| **hermes-agent** | MIT | Port architecture, interface shapes, and **prompt text verbatim** (e.g. the skill-review prompt). Attribution courtesy in ADRs. | Treat its Python monolith coupling as a model. |
| **pi** | MIT | Port designs freely; retain MIT attribution comment if an algorithm is copied verbatim (e.g. diff-render). | — |
| **daytona** | **AGPL-3.0** | Re-implement *patterns* in freshly-written Go. Its `libs/runner-proto/*.proto` are Apache-2.0 and adoptable. | **Copy any code body** — AGPL network-use clause would force us to open-source our service. Legal sign-off before any line is reused. |
| **claude-code** | **Leaked proprietary** | Use module boundaries, type-name vocabulary, status enums, and state-machine *shapes* as a clean-room spec. Cross-check API surfaces against public `docs.anthropic.com`. | Copy any TS code/comments/string literals; use internal codenames (`tengu`, `KAIROS`); commit any excerpt to our repo. |

**Net:** codex is our highest-leverage source (Rust + Apache). hermes/pi are safe idea ports. daytona and claude-code are reference-only.

---

## 2. Cross-Repo Convergence (strongest signals)

Where multiple independent stacks converge on the same design, the signal is strongest. These are the highest-confidence adoptions:

| Theme | Repos that converge | Our gap | Lead source (license) |
|---|---|---|---|
| **A. Sandbox isolation** | codex (`linux-sandbox`/bwrap/seccomp/Landlock), daytona (Docker+iptables+OCI snapshots), hermes (`BaseEnvironment` 6-backend) | `sandbox-manager` create paths **Unimplemented**; execution-core spawns w/ no isolation | **codex** (Apache, vendorable) |
| **B. Provider abstraction** | pi (`ApiProvider` registry + no-throw stream), codex (`ModelProvider` + `ProviderCapabilities`) | inference-core has ad-hoc provider structs; blocks P5 multimodal | pi + codex |
| **C. MCP client/registry** | codex (`rmcp-client`, stdio/HTTP/OAuth), claude-code (MCP tool taxonomy) | capability-core MCP registry = stub (P2) | **codex** (fork) |
| **D. Plugin/extension host** | codex (`ext/extension-api` contributor traits), hermes (`plugin.yaml` discovery), claude-code (`plugins/bundled`) | capability-core plugin registry = stub (P2) | codex + hermes |
| **E. Memory adapters + learning loop** | hermes (`MemoryProvider` ABC ×8 + closed learning loop + FTS5), claude-code (`memdir` freshness) | letta-bridge in-mem stub; no P7 knowledge; ties to **Wave 7 fine-tuning** | **hermes** (MIT — port incl. prompt) |
| **F. Hooks / approval / plan-mode** | codex (10-event hooks + `SandboxPolicy`/`AppToolApproval`), pi (`beforeToolCall`/`afterToolCall`), claude-code (plan-mode + `allowed_prompts[]`) | execution-core hook gates are seams; no approval engine; no plan mode (P3) | codex + pi + claude-code |
| **G. Tasks / cron / coordinator** | hermes (cron `deliver=` fan-out + subagent-RPC), claude-code (`Task` 5-state, durable/non-durable cron, coordinator+swarm) | no task-core durability; no subagent coordinator (P4) | hermes + claude-code |
| **H. Channels / bridge / TUI** | hermes (`BasePlatformAdapter` single-gateway), claude-code (`ReplBridgeTransport`+JWT), pi (TUI diff-render) | zero operator shell (P6) | hermes + pi |
| **I. Secret-scrub / hardening** | codex (`redact_secrets`, `process-hardening`, `keyring-store`) | execution-core secret-scrub is a seam | **codex** (Apache, ~40 LOC verbatim) |
| **J. Token efficiency** | claude-code (deferred tool-schema, memory 200-line cap) | P8 TOON only | claude-code (shapes) |

---

## 3. Prioritized Adoption Plan (highest leverage first)

Ordered by `unblock × low-friction × low-legal-risk`. Each item names the **target file/service**, **source**, **effort**, and **license gate**.

### Tier 1 — Quick, high-value, low-risk (do first)

1. **Secret-scrub upgrade** — vendor codex `secrets/src/sanitizer.rs` `redact_secrets` (4 `LazyLock<Regex>`: `sk-*`, `AKIA*`, `Bearer`, `secret/token/password=`) into `execution-core` secret-scrub seam; extend with our JWT/Postgres-DSN/NATS patterns; call before every NATS publish + tool-output boundary. **Effort S · Apache vendor.**
2. **OS keychain abstraction** — vendor codex `keyring-store` (`KeyringStore` trait + `DefaultKeyringStore`) for infra secret loading (macOS Keychain / Linux Secret Service). **Effort S · Apache vendor.**
3. **Sandbox-policy types** — copy codex `SandboxPolicy` + `PermissionProfile` + `NetworkSandboxPolicy` enums into a new `execution-core/src/policy.rs` and mirror as proto in `mp-contracts` (see §4). This is the vocabulary everything else hangs off. **Effort S · Apache vendor.**
4. **Provider-capabilities struct** — adopt codex `ProviderCapabilities { supports_tools, supports_vision, supports_thinking, context_window, … }` in inference-core; immediately gates P5 multimodal exposure. **Effort S.**

### Tier 2 — Infra unblock (our biggest stub: real sandboxing)

5. **execution-core OS isolation** — vendor codex `linux-sandbox` (`bwrap.rs` namespace builder + `landlock.rs` `apply_permission_profile_to_current_thread` seccomp/no_new_privs) + `process-hardening`. Branch the step executor on `SandboxPolicy`: `WorkspaceWrite`/`ReadOnly` → exec via sidecar; `DangerFullAccess` → direct. Linux-gated feature. **Effort M · Apache vendor.**
6. **sandbox-manager real create paths (Go, clean-room from daytona patterns)** —
   - Desired-state / current-state split + reconcile loop (daytona `model_sandbox_desired_state.go`); Temporal in orchestrator-core is the reconciler.
   - Per-sandbox **iptables egress** package (clean-room of daytona `runner/pkg/netrules/` — assign/limiter/delete; ~400 LOC of *ideas*, reimplement in Go).
   - **Snapshot-as-OCI → MinIO**: run a `distribution/distribution/v3` OCI registry with `s3` driver pointed at our MinIO; snapshot = `docker commit`+`push`, restore = `pull`+`run`; Redis lease stores `snapshotRef`.
   - Redis `SET NX PX` state-change lock + Postgres single-flight job index.
   **Effort L · AGPL → CLEAN-ROOM ONLY (no code copy; legal sign-off).**
7. **Backend abstraction** — define a Rust `SandboxBackend { init_session, execute, cleanup }` + `SandboxHandle { poll, kill, wait, stdout }` trait (hermes `BaseEnvironment`/`ProcessHandle` shape, MIT); ship `local` + `docker` first, leave `ssh/modal/daytona` as future impls. **Effort M.**

### Tier 3 — Provider + capability platform (P2, highest roadmap leverage)

8. **inference-core provider trait** — merge pi's no-throw streaming envelope (`InferenceEvent::{Start,Delta,Done,Error}`, errors-in-stream not thrown) with codex's `ModelProvider` trait + capabilities. Registry = `HashMap<ApiTag, Arc<dyn InferenceProvider>>`. See §5 for the proposed trait. **Effort M.**
9. **MCP registry** — fork codex `rmcp-client` (strip `codex_login` OAuth; replace with our NATS token refresh). `RmcpClient` = connection handle; `LocalStdioServerLauncher` spawns servers; `ToolWithConnectorId` = registry key; `Elicitation` → our P3 approval type. Lands in capability-core MCP registry. **Effort M · Apache fork.**
10. **Plugin/extension host** — adopt codex `ExtensionRegistry` + contributor traits (`ToolContributor`, `ThreadLifecycleContributor`, `TurnLifecycleContributor`, `ContextContributor`, `TokenUsageContributor`) as the plugin host API; hermes `plugin.yaml` manifest-discovery pattern for Go capability-core hydration. **Effort M · Apache fork + MIT idea.**
11. **Tool contract + deferred schema** — clean-room `ToolDef` trait (claude-code shape: `inputSchema`, `isReadOnly()`, `isEnabled(ctx)`, `call()`, `render()`) as capability-core's tools-registry entry + execution-core exec contract. Register non-core tools as **deferred** (name-only stub; schema fetched on first use via a `ResolveSchema` RPC) — direct prompt-size win. Use `schemars` (Rust)/`go-jsonschema`, not Zod. **Effort M · shapes-only.**
12. **Feature-flag system** — codex `features` `Stage { UnderDevelopment, Experimental, Stable, Deprecated }` registry; emit warning on deprecated-flag use. **Effort S · Apache.**

### Tier 4 — Behavioral layers (P3 plan mode, P7 learning loop)

13. **Hook engine** — codex `HOOK_EVENT_NAMES` (10 events incl. `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`/`PostCompact`, `SubagentStart`/`Stop`) + pi's two-phase gate signature (`before → {block, reason}`, `after → {override_result}`). capability-core supplies the policy; execution-core fires the gate. **Effort M · Apache + MIT.**
14. **Plan mode + approvals (P3)** — clean-room claude-code shape: `RunMode {Normal, Plan}` on session state; in Plan mode execution-core blocks non-`is_read_only` tools; `ExitPlanMode(allowed_prompts[])` emits a structured approval request over run-event SSE; codex `AppToolApproval {AUTO_APPROVE, REQUIRE_HUMAN, DENY}` + `PermissionRequestDecision` is the response type. Durable in session-core. **Effort M · shapes + Apache.**
15. **Closed learning loop (P7 + Wave 7 tie-in)** — hermes `agent/background_review.py`: a Temporal `PostSessionSkillReview` activity (orchestrator-core) fires at session end, feeds transcript + the **`_SKILL_REVIEW_PROMPT` (MIT, usable verbatim)** to an LLM, writes versioned skill records to capability-core tagged `origin=background_review` vs `user`. Skills become Wave 7 fine-tune training pairs. Bias-to-action ("a pass that does nothing is a missed learning opportunity") encoded in the prompt. **Effort M · MIT.**
16. **Memory-adapter registry** — hermes `MemoryProvider` ABC → Go `MemoryAdapter` interface (see §6) in capability-core; letta-bridge becomes `LettaMemoryAdapter`. Rust session-core calls only `Prefetch` (pre-assembly) + `SyncTurn` (post-step) via gRPC; everything else stays Go. `on_delegation` hook wires the P4 coordinator. **Effort M · MIT.**
17. **FTS5 session search** — hermes dual FTS5 tables (BM25 + trigram) → Postgres `tsvector`+`GIN` over `messages.content` in session-core; three-shape API (Discovery w/ ±5 bookend window, Scroll, Browse) as a `SearchSessions` RPC. No LLM in the search path. **Effort M.**

### Tier 5 — Tasks/coordination + operator shell (P4, P6)

18. **Task/cron model** — claude-code `TaskStatus {pending→running→completed|failed|killed}` enum verbatim (shape) in Go task-core; cron `durable=true` → Temporal cron workflow, `durable=false` → tokio timer; single `ScheduleTask` gRPC. hermes `deliver=` fan-out (`origin|local|all|platform:chat:thread`) → bridge-core channel router. `RemoteTrigger` → bridge-core HTTP→NATS `task.trigger.<name>`. **Effort M · shapes + MIT.**
19. **Subagent coordinator (P4)** — orchestrator-core `CoordinatorMode` session flag (mode-per-session, not a new binary; claude-code pattern) exposing `AgentDispatch {description, prompt, subagent_type}` + `SendMessage {to_task_id, message}`; `WorkerBackend {Spawn, Kill, Send}` Go interface (in-process first); worker approvals bubble to owner via NATS (`leaderPermissionBridge` pattern). hermes `process_registry` `watch_patterns→notify` shape for completion events. **Effort L · shapes.**
20. **Channels + IDE bridge (P6)** — bridge-core `ChannelAdapter {Start, Stop, Send, Platform}` (hermes `BasePlatformAdapter` shape) for Slack/Discord/Telegram; `BridgeTransport {write, writeBatch, setOnData, connect}` + `PermissionPending` frame + ring-buffer reconnect replay (claude-code shape); **reuse Verevon JWT** for bridge auth — no new auth path. **Effort M-L · shapes + MIT.**
21. **Rust TUI (P6)** — pi `Component::render(width)->Vec<String>` trait + `prev_lines` differential flush + overlay stack, backed by `crossterm`/`ratatui` (which already does cell diffing). Study codex `tui` for keymap structure. **Effort M · MIT.**

### Tier 6 — Token efficiency (P8)
22. **Deferred-everything + freshness notes** — generalize #11's deferred schema; adopt claude-code's memory-freshness annotation (`This memory is N days old` `<system-reminder>`, 200-line/25KB cap) in our memdir/context assembly. Complements existing TOON plan. **Effort S · shapes.**

---

## 4. Sandbox-policy extract (codex → execution-core + capability-core)

Add `execution-core/src/policy.rs` (Apache-vendored shape, extended with our network-allowlist):

```rust
pub enum MpSandboxPolicy {
    DangerFullAccess,
    ReadOnly      { network: MpNetworkPolicy },
    WorkspaceWrite { writable_roots: Vec<PathBuf>, network: MpNetworkPolicy },
    External      { network: MpNetworkPolicy },
}
pub enum MpNetworkPolicy { Disabled, AllowAll, AllowDomains(Vec<String>) } // AllowDomains = our network-proxy extension
pub enum MpPermissionProfile {
    Managed  { fs: MpFsPolicy, network: MpNetworkPolicy },
    Disabled,
    External { network: MpNetworkPolicy },
}
```

capability-core safety registry proto (`safety.proto`):

```proto
message ToolApprovalPolicy { enum Mode { AUTO_APPROVE=0; REQUIRE_HUMAN=1; DENY=2; } string tool_name=1; Mode mode=2; }
message SafetyConfig { MpSandboxPolicy sandbox_policy=1; repeated ToolApprovalPolicy tool_policies=2; NetworkConstraints network_constraints=3; }
```

---

## 5. Provider-trait extract (pi + codex → inference-core)

```rust
#[async_trait]
pub trait InferenceProvider: std::fmt::Debug + Send + Sync {
    fn id(&self) -> &str;                          // "anthropic" | "openai" | "ollama"
    fn capabilities(&self) -> ProviderCapabilities; // supports_tools/vision/thinking, context_window, max_output
    fn preferred_model(&self) -> &str;
    async fn base_url(&self) -> Result<Url>;
    async fn auth_header(&self) -> Result<HeaderValue>;
    /// Never returns Err — failures are InferenceEvent::Error in the stream (pi's no-throw contract).
    async fn stream(&self, req: InferenceRequest) -> Result<InferenceStream>;
    fn models(&self) -> Vec<ModelInfo>;
}
pub enum InferenceEvent { Start{partial:Box<AssistantMessage>}, Delta{chunk:ContentDelta}, Done{reason:StopReason,message:Box<AssistantMessage>}, Error{reason:StopReason,message:Box<AssistantMessage>} }
pub enum StopReason { Stop, Length, ToolUse, Error, Aborted }
```
Registry: `HashMap<ApiTag, Arc<dyn InferenceProvider>>`. `ProviderOptions` carries `session_id` (cache-key), `cache_retention {None,Short,Long}`, `reasoning {effort, budget_tokens}`, `max_retry_delay_ms` cap, `CancellationToken`.

---

## 6. Memory-adapter interface (hermes → capability-core, Go)

```go
type MemoryAdapter interface {
    Name() string
    IsAvailable() bool
    Initialize(ctx context.Context, sessionID string, opts SessionOpts) error
    SystemPromptBlock() string
    Prefetch(ctx context.Context, query, sessionID string) string   // ← Rust session-core calls this pre-assembly
    SyncTurn(ctx context.Context, p TurnPayload) error              // ← and this post-step
    ToolSchemas() []ToolSchema
    HandleTool(ctx context.Context, name string, args json.RawMessage) (string, error)
    OnSessionEnd(ctx context.Context, msgs []Message) error
    OnDelegation(ctx context.Context, e DelegationEvent) error      // ← wires P4 coordinator
    Shutdown(ctx context.Context) error
}
```
Registry = `map[string]MemoryAdapter`, hydrated from `$MP_HOME/plugins/memory/<name>/plugin.yaml`. The only cross-language calls are `Prefetch`/`SyncTurn` (gRPC). 8 hermes backends → MP: `honcho`→letta-bridge, `mem0`/`hindsight`/`supermemory`/`retaindb`/`byterover`/`openviking`→external adapters (P7), `holographic`→our graph memory (P7).

---

## 7. Learning-loop mechanics (hermes → P7 + Wave 7)

- **Trigger:** session end (exit / `/reset` / `/new` / gateway timeout / compaction). A separate periodic *memory nudge* fires via turn-count in `on_turn_start`.
- **Mechanism:** isolated "background review fork" agent receives full transcript + `_SKILL_REVIEW_PROMPT`; instructed to (a) update loaded skills, (b) patch umbrella skills, (c) add `references/templates/scripts`, (d) create new skills only as last resort. Bias-to-action is explicit.
- **Provenance:** writes tagged `background_review` (ContextVar in Python → pass explicitly in gRPC metadata for us) so the auto-curator never prunes user-directed skills. AST security scan before any skill-script install.
- **Storage (ours):** Postgres in capability-core, versioned `(name, version, origin, content_hash, created_at)` — not filesystem. Skills feed the Wave 7 Azure fine-tune dataset.

---

## 8. What NOT to copy (per repo)

- **daytona:** the NestJS/TS control plane; GPU allocator; SSH gateway (model-gateway already owns ingress); ADB/Android probes. **And no AGPL code bodies at all.**
- **codex:** `chatgpt/`, `login/`, `codex_auth`/`codex_api` (ChatGPT-account coupled); `responses-api-proxy`; `cloud-tasks*` (we use Temporal); `realtime-webrtc`; product telemetry/rollout crates; the generated `exec-server` proto (reimplement the boundary).
- **hermes:** Python-monolith import coupling; filesystem job/skill stores (use Temporal+Postgres); one-active-memory-provider limit (we want namespaced multi-adapter); runtime GitHub skill-fetch (admin-gated registry instead).
- **pi:** module-level global registries (use `Arc<RwLock>`/`inventory`); TypeBox/Node-stdout hacks; **its zero-durability model** — do not mirror it for session-core schema.
- **claude-code:** anything verbatim. No internal codenames. Never commit excerpts. Validate API shapes against public docs.

---

## 9. Recommended next steps

1. **Tier 1 (this week-ish):** vendor codex `secrets`+`keyring-store`+`process-hardening`; land `MpSandboxPolicy` enum + proto; add `ProviderCapabilities`. All Apache, all low-risk, all unblock later tiers.
2. **Legal sign-off** on the daytona clean-room plan (item 6) before any Go is written against its patterns; confirm Apache vendoring policy for codex crates.
3. **Spike:** fork `rmcp-client` (item 9) — proves the codex-fork workflow end-to-end and unblocks P2 MCP.
4. **Wire to canon:** add `daytona` + `pi` to `GOAL.md` §"Ecosystem-derived enhancements"; open `gap-model.md` PAR-IDs for items 5–11; mirror this prioritization into `ROADMAP.md` per phase.
5. Track each adopted item with its source + license in an ADR so provenance is auditable.

> Appendix: full per-repo agent reports (tables of 8–18 ideas each, with exact source paths and effort) were produced during the harvest and can be regenerated via `opensrc fetch` + the analysis prompts. The synthesis above is the deduped, prioritized view.
