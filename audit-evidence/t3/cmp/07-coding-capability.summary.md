# T3 / 07 Coding capability: cross-source comparison

The inputs were 84 reference records from 9 sources: cursor 40, claude 8, codexagent 7, t3code 7, antigravity 5, deepseek 5, web-chatgpt 5, web-claudeai 5, manus 2. Hermes, Perplexity and web-Manus contributed no records to this category. I grouped them into 25 clusters, which cover 83 records; 1 record is excluded (web-chatgpt-089, a billing item filed in the wrong category). I compared them with the 10 Verevon baseline records. Provenance: mostly SRC, with REA for Cursor's crepe index and DOC for the web benchmarks. Clusters: `07-coding-capability.clusters.json`.

Framing: Verevon is **not an IDE**. Only the harness techniques that make agents good at writing code and documents carry over to the chat and agent pages: edit tools, diffs, review, diagnostics and previews. IDE surfaces were judged and skipped: Tab completion, code indexes, PR bots and git attribution.

## Shared baseline (table-stakes and common)
- **Diff review surface** (C-004, 5/9 sources, table-stakes): diffs per turn, against a base, or for the working tree, with accept/reject per hunk or file.
- **Incremental file-edit engine** (C-001, 3/9): exact-match replace with tolerant matching. Every coding-capable source has one in some form. In the INF reading, Codex's apply_patch (C-002) is the same capability.
- **Workspace grep, glob and read tools** (C-008, 3/9).
- **Review as a run with structured findings** (C-014, 4/9).
- **PR lifecycle automation** (C-016, 4/9): skipped for Verevon.
- **Build-and-preview of generated apps** (C-019, 4/9).

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Edit engine (C-001) | Cursor StrReplace (cursor-harness-057, cursor-ai-009, cursor-harness-059) | Matching ladder: exact, then trimmed lines, then a did-you-mean block. Refuses multiple matches, returns structured line stats and uses per-file locks. DeepSeek adds a stale-read version guard; Antigravity adds lazy-edit detection. |
| Model-native edit format (C-002) | Codex apply_patch (codexagent-063) | Lark-grammar freeform tool, shell-heredoc interception, streaming patch events. Cursor switches edit tools by vendor in production. |
| Per-turn changed files (C-003) | T3 Code (t3code-ui-041) | Auto-expands only for <=5 files and <=200 lines, shows a compact directory tree, and remembers expansion per turn. |
| Diff accept/reject (C-004) | Antigravity (agent-ux-073), with T3 Code scopes (038) | Scope FILE/HUNK/ALL, and the written rejection feedback goes back to the agent. |
| Comments sent into the prompt (C-005) | T3 Code (039) | A serialised `<review_comment>` block that round-trips into a card. One annotation layer serves diffs, files and PRs. |
| Post-edit diagnostics (C-006) | Cursor shadow workspace and ReadLints (ai-010, harness-067) | Lints are computed for the change without touching the user's files. Antigravity adds auto-fix and a verification mode. |
| Review runs (C-014) | Codex review/start (agent-runtime-086, plugins-116) | Four targets, P0-P3 JSON findings, a "no invented findings" rule. Claude has the best findings-card UX (103). |
| App preview (C-019) | Claude dev-server preview (product-ux-136) | Server detection, explanations for cold starts and port conflicts. Manus adds checkpoint previews (087). |
| PR automation (C-016, skip) | Claude PR pane (102); T3 Code forge registry (061) | CI watch, auto-fix and auto-merge. Four forges run through the user's own CLIs. |

## Differentiators (2+ sources, not most)
- Vendor-native edit formats (C-002).
- Changed-files card and replay (C-003).
- Diff comments sent into the prompt (C-005).
- Post-edit diagnostics (C-006).
- LSP navigation (C-007).
- Semantic code index (C-009): Cursor only, at q5.
- Hosted PR review bots (C-015).
- Security scanning products (C-017).
- Terminal view (C-018).
- AI code attribution (C-023).

Unique to one source: Cursor's Tab completion, crepe n-gram index, best-of-N, debug mode and Cmd+K; Manus's worktree retention; ChatGPT's composer prediction.

## Verevon's biggest gaps (code-verified)
1. **No incremental edit tool at all** (C-001, **priority**, significance 4, effort M, Model Plane). The only ways to change content are a whole python/sh program in `code_interpreter` (`execution-core/src/code_interpreter.rs:24-34`) or a full rewrite through `update_artifact` (`model-gateway/src/tool_loop.rs:2679`). A grep for apply_patch, str_replace, write_file and edit_file found only MCP test fixtures. Every long revision is re-emitted in full. This gap blocks C-002, C-003, C-004, C-006, C-013 and C-019.
2. **No apply_patch for the default route** (C-002, recommended). The product route is the Codex app-server (gpt-5.6-terra, `inference-core/src/provider/codex_subscription.rs:26`), a model family trained on apply_patch.
3. **No workspace grep, glob or read tools** (C-008, recommended, effort S). The governed `shell` is read-only, human-gated and not offered in Do mode.
4. **Diagnostics after writes are deferred** (C-006, recommended). There is LSP pass-through plumbing (`model-gateway/src/lsp.rs:1-15`) but it is off. Decision H11 (diagnostics instead of an LSP bridge) is the right call; it is parked in Wave 6.
5. **Diffs are read-only** (C-004 and C-003 partial; C-005 absent). There is no accept/reject or hunk scope, no review step before PromoteWorkspace, no per-turn changed-files summary, and no comment-on-selection in the artifact panel.
6. **No multi-file app build or preview** (C-019 partial, effort L). Only single-file HTML artifacts render, and the sandbox has no network or port exposure. Screenshots should go through Quarry: the Model proposes, Quarry executes.
7. **No review run** (C-014, recommended). It can reuse subagents and merge findings with the deterministic `source_validation.rs` checks.

Suggested build order: C-001, then C-008, then C-005, then C-003/C-004, then C-006, then C-002, then C-014, then C-019. Nice-to-have: C-013 (selection edit), C-018 (read-only terminal renderer for `process_*`), C-020 (best-of-N). Skipped as IDE or out of scope: C-007, C-009 to C-012, C-015 to C-017, C-021, C-023, C-024.

## Verevon strengths (keep)
- **Hermetic code interpreter** (`code_interpreter.rs`): bwrap, no network, size caps, file in/out. A Space-scoped durable workspace is built on CAS snapshots and a PromoteWorkspace overlay merge into Data Plane documents (C-022, tier n/a). This is an equivalent of Manus's retained worktrees that fits a non-git product.
- **Shared DiffView** (`shared/chat-nodes/diff.ts:61-243`) for both tool patches and artifact revisions, with an honest stat-only fallback above 2,000 lines. Artifact versions come with a "show changes" view. HTML artifacts render in a sandboxed iframe, at parity with claude.ai artifacts.
- **Deterministic source-bound drafting review** (`model-gateway/src/source_validation.rs`): checks numbers, dates, word counts and revision preservation against the sources. No reference source has an equivalent for documents.
- **AI follow-up chips** (`chat-types.ts:422-427`): equivalent to ChatGPT's composer prediction (C-025, n/a).
- **Durable process registry under capability policy** (`runtime_loop/agent.rs:4089-4109`): ahead of most web chat products. It only lacks a terminal-style renderer.

## Caveats
- I did not run Verevon. "Absent" and "partial" come from code reads and greps on the 2026-10-10 HEAD plus the working tree; nothing was proven live.
- C-018 (terminal), C-019 (preview), C-022 (workspaces) and C-025 (follow-ups) are not covered by this category's baseline records. I cited their evidence from the baseline narratives and spot reads. The retention limit and preview-before-cleanup were not checked. Debug mode (C-021) is "unknown".
- Web benchmark records (web-chatgpt, web-claudeai) are DOC only. C-024 and C-025 rest on DOC alone. cursor-ai-146 (Origin) is INF from flags.
- Cursor contributes 40 of the 84 records, so counts per cluster understate Cursor-only depth and overstate how widespread IDE features are.
- The sandbox-manager and ledger paths come from the baseline narratives and were not re-read.
