# T3 / Category 6: Computer & environment control (comparison summary)

Inputs: 237 reference records from 12 sources (233 clustered, 4 excluded as trivia), plus 31 Verevon baseline records. Output: 49 clusters in `06-computer-environment-control.clusters.json`. Provenance tags: SRC = read in code, DOC = documentation, REA = observed in a shipped artifact, INF = inference. "Comparator-verified" means I read the Verevon code during this step myself and no baseline record covered it.

## Shared baseline (table-stakes / common)
- **Desktop computer use** (screenshots, accessibility tree, input). 8/12 sources have it. Antigravity has only a model enum. Verevon: **absent**, parked by the owner.
- **Git worktrees for parallel agents** (10/12). This is IDE territory, so Verevon should skip it.
- **Per-task cloud sandbox computer** (5). Verevon: **absent**. Today it has only bwrap plus a ~30 s networkless code_interpreter.
- **OS-level command sandbox** (5). Verevon: **shipped** on Linux.
- **Agent browser toolset with a11y refs** (5). Verevon: **partial**. Quarry is rich, but agent runs fail closed on a fake grant id.
- **Hosted/in-app shared browser** (5). Verevon: **shipped**, outside chat.
- **Human takeover** (5). Verevon: **partial**. Manual takeover works; the agent cannot request one.
- **Live computer panel** (4). Verevon: **partial**.
- **User terminal panes** (4). Verevon: **absent**.
- **Drive the user's own browser** (6). Verevon: **absent**, parked.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Cloud sandbox per task | Perplexity SPACE / Manus (DOC), Cursor (SRC) | Firecracker microVMs with credentials injected only when needed, egress control, snapshots, sleep/wake lifecycle |
| Local VM sandbox | Claude (SRC/REA) | One interface over Hyper-V, Apple Virtualization and KVM; per-process mounts and egress; gVisor networking |
| OS command sandbox | Codex + Claude (SRC) | One policy type with four native backends; richest network policy engine |
| Credential masking | Claude (SRC) | The sandbox sees sentinels; the host proxy injects the secret or re-signs SigV4 |
| PTY exec | Codex (SRC) | exec_command with yield_time_ms / max_output_tokens plus write_stdin polling |
| Execution backends | Hermes + DeepSeek (SRC) | Seven backends behind one spawn contract; "fs + subprocess define the world" |
| Browser toolset | Antigravity (SRC) + Manus (REA) | DOM, pixel or hybrid modes; per-action state diffs; hybrid a11y/DOM snapshot |
| Browser contract | Manus (SRC) | 22-tool versioned MCP schema; snapshot/ref/revision binding; retryable error catalogue |
| Takeover | Manus + T3 (SRC) | The agent suggests takeover; real human-vs-agent input arbitration |
| Computer panel | Manus (SRC) | A viewer for each tool kind, plus picture-in-picture |
| Desktop CU engine | Manus (SRC) | Accessibility-first handles, window-identity ladder, honest foreground model |
| Code-mode control | Manus + DeepSeek (SRC) | Model writes JS run in QuickJS-WASM against SDK bindings; typed failure taxonomy |
| CU consent | Claude (SRC) | Per-app tiers (view, click, full), deny list, machine lock, restoration at turn end |
| Live desktop transport | Manus (SRC) | Rust LiveKit publisher; strictly validated normalized input protocol |

## Differentiators worth noting
Credential injection proxy (Claude), per-call enforcement report (DeepSeek), terminal output offload to files (Cursor), browser subagent with its own model and recording (Antigravity), teach-by-demonstration (Claude), code-mode control (Manus/DeepSeek), WebMCP (Antigravity), persistent cloud computers (Manus).

## Verevon's biggest gaps (priority tier)
1. **C-t3-06-023 Agent browser runs are unreachable.** S5, effort M. The gateway sends `grant_id "session:{id}"`, which browser-broker never minted (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:1235`). browser_agent is also excluded from chat. To fix: mint a real grant, enable the tool in chat and Do mode, and add per-action state diffs. Most of the executor already exists in Quarry.
2. **C-t3-06-032 Agent-requested human takeover and hand-back.** S4, effort M. Add a `request_human_takeover` tool that pauses the run. On hand-back, resume with the user's note and take a fresh snapshot.
3. **C-t3-06-001 Per-task sandbox VM.** S5, effort XL. Add a SandboxBackend seam (C-t3-06-015), then microVMs driven by sandbox-manager with a Rust guest agent. Keep bwrap as the second layer and the fail-closed egress proxy. Under ZDR: no snapshots.
4. **C-t3-06-034 Computer use, scoped first to the agent's own sandbox desktop.** S5, effort XL. This respects the owner's decision to park user-desktop control and the rule that the Model proposes and the executor acts.

Recommended next: live computer panel with a viewer per tool and a read-only terminal viewer (C-t3-06-033 / 014); browser recording artifact (027); site policy (028); browser subagent (026); environment images (004); credential injection (009); PTY / yield exec (011); enforcement report consumer (007, effort S); output offload (013, effort S); reuse of the Endpoint Plane as the desktop stream transport (039).

Skipped on purpose: local VM, SSH/WSL remotes, worktrees and VCS, keep-awake, local folder grants, on-device agent, simulators, the app builder/hosting product line, and the user's own Chrome and local device bridges. The last two are excluded by the owner rulings and by the Quarry-executes rule.

## Verevon strengths (equal or ahead)
- **OS sandbox:** bwrap + Landlock + seccomp, probed at boot (`apps/Model Plane/rust/services/execution-core/src/sandbox.rs`).
- **Egress:** fail-closed AllowDomains, plus Quarry's DNS-pinned SSRF CONNECT proxy (`quarry-runtime/src/browser_egress_proxy.rs`, comparator-verified).
- **Revision-bound refs:** browser refs are bound to snapshot and generation, and effectful actions are never silently retargeted (`quarry-runtime/src/observation.rs:1030`, `target_repair.rs`, comparator-verified).
- **Step receipts:** append-only, plus deterministic postcondition verifiers.
- **Browser profiles:** org-scoped profile store with cookies, storage and IndexedDB, backed by Postgres/S3/Redis.
- **Durable Space workspace:** CAS-backed, with snapshot and promotion into the Data Plane.
- **Process teardown:** `--die-with-parent`, a grace period before kill, and kill-on-lease-release (comparator-verified).
- **Hosted governed browser** with manual takeover.
- **Native remote-desktop stack** in the Endpoint Plane. Its "live-verified" claim comes only from the plane's own DOC.

## Caveats
- Web benchmark sources (chatgpt, claude.ai, manus, perplexity) are DOC-only. Clusters that rest only on them (app builder, hosting, Comet, Portable Computer) carry DOC confidence. Their prevalence is undercounted for internals such as shells.
- "Shipped" means present in source and reachable. Per the baseline, almost nothing is deployed or live-proven. Agent browser runs fail closed today.
- Six statuses come from my own reads with no baseline record behind them, at medium/low confidence: C-t3-06-012, 025, 031, 036, 039, 049. Cluster 044 (file-tool write safety) is **unknown**. Check whether the CAS workspace or `update_artifact` rejects stale versions.
- The Endpoint Plane's status is taken from its own architecture doc. I did not check whether it is deployed, or whether its git history is clean-room.
- Antigravity records 13 and 28 document the *absence* of desktop CU. They are members of C-t3-06-034 but do not count toward its prevalence.
