# T3 / 08: Memory, learning and personalization (cross-source comparison)

Inputs: 117 reference records from 12 sources (Hermes 47, Cursor 13, Claude desktop 10, ChatGPT web 9, Claude web 9, Antigravity 6, Perplexity web 6, Codex 5, Manus web 5, Manus 4, DeepSeek 2, T3 Code 1) and 35 Verevon baseline records. Output: 36 clusters, 117 records assigned, 0 excluded. Cluster detail: `08-memory-learning-personalization.clusters.json`.

## Shared baseline (table-stakes)
- **C-001 Account memory with user controls** (9/12): browse, edit, delete, reset, pause, plus in-chat "remember/forget this".
- **C-009 Layered custom instructions** (8/12): instructions at org, user and project/space scope, merged in precedence order, applied on every surface.
- Common, short of table-stakes: background extraction and consolidation (C-002), scoped/project memory (C-003), incognito chats (C-006), personality presets (C-011), memory in agent tasks (C-014), past-chat search (C-015) and reference (C-016), and learning proposals the user approves (C-019).

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Account memory UX (C-001) | Claude web/desktop | Topic entries saved during chat, pause vs reset, org opt-in, read/write indicator (SRC+DOC) |
| Extraction + consolidation (C-002) | Codex | Two-stage extract/consolidate with idle, age and rate-limit gates, a "polluted" flag, citations (SRC) |
| Project-isolated memory (C-003) | ChatGPT | Clear isolation contract: nothing in, nothing out; shared projects always isolated (DOC) |
| Memory sources on answers (C-005) | ChatGPT | Per-answer list of the memories, chats and files used, each with correct/delete (DOC) |
| Instructions hierarchy (C-009) | Claude | Seven scopes, org > user precedence, folder instructions Claude edits as it learns |
| Session search (C-015) | Hermes | One tool with four modes, no LLM calls, BM25 + lineage dedup, compacted turns kept searchable (SRC) |
| Cross-session reference (C-016) | DeepSeek | Bounded, read-only @session snapshot flagged as untrusted (SRC) |
| Self-improvement loop (C-017) | Hermes | Post-turn review on a cadence; user corrections become rules in task skills (SRC) |
| Approval of learned items (C-019) | Manus + Hermes | Knowledge events linked to their source, accept/reject; background deletes always held for approval (SRC) |
| Skill lifecycle (C-020/021) | Hermes | Skill linter, "lessons, not logs" rules, usage telemetry, stale/archive curator (SRC) |
| Learning by demonstration (C-022) | Manus | Recorder addon, review, task that builds the skill (SRC) |
| Self-configuring prefs (C-035) | Claude | get/set_setting tools with a model-facing meaning and lock reasons for each setting (SRC) |

## Differentiators (2+ sources, not most)
Self-improvement loop (C-017), user-triggered `/learn` (C-018), named persistent agents with their own memory (C-013), learning by demonstration (C-022), sensitive-topic exclusion (C-007), standing user model (C-026), onboarding and context-aware defaults (C-033). Unique but notable: the Perplexity Brain wiki (C-027, DOC only) and Claude's self-configuring preferences (C-035).

## Verevon's biggest gaps (priority tier)
1. **C-009 Instructions do not reach Do runs** (partial, sig 5). The ADR-0003 hierarchy (platform > org > Space > agent > persona) applies only in Ask turns. `agentic_run_stream` (MG/sse.rs:9527-10128) has no persona, org or Space instructions. Fix: one shared resolver for both paths.
2. **C-014 Do runs are memory-blind** (partial, sig 5). Do runs have save/recall_memory tools, but at run start they get no memory prefetch, no thread history and no persona, and automations keep no notes between runs. Reuse `fetch_chat_memory_context` at run start and add per-task notes protected by a version check.
3. **C-015 No conversation search** (absent, sig 5). `ListThreadsQuery` has no query field, there is no FTS index, and the SPA only does a substring match on loaded titles. Build Postgres tsvector search in session-core plus a `recall_conversations` tool (Hermes design).
4. **C-017 The learning loop has never been proven** (partial, sig 5). RUN_COMPLETED → sessionreview → UpsertAgentSkill is wired and on by default, but it has never produced a skill from a real run (I23). It only reviews Do runs.
5. **C-019 No approval gate for learned items** (planned, sig 4). A learned skill becomes matchable across the whole org without review. That is a governance and authority-model gap.

Recommended next: a memory pause/off switch and reset (C-001), org memory policy (C-008, status unknown), memory import (C-004), personality presets built on the existing `verbosity.rs` (C-011), past-chat @references (C-016), `/learn` (C-018), a deterministic sensitive-data check on every memory write path (C-007), the planned user profile (C-026), and making the existing settings actions model-eligible (C-035). Skip: AGENTS.md files (C-010, IDE-only), a memory-provider plugin registry (C-025, already deferred by owner decision), Chronicle screen history (C-036, conflicts with ZDR), and consumer recaps (C-034).

## Verevon strengths (tier n/a; keep)
- **C-005 Memory provenance.** The `memory_recall` SSE event labels each memory stated, inferred or unrecorded, is saved on the turn, and offers correct/forget in place. This equals ChatGPT and goes further than Claude and Perplexity.
- **C-023 Feedback learning.** Thumbs plus a reason category, server-side skill credit, an implicit-dissatisfaction classifier with Wilson-bounded quarantine, and calibration rows. No reference source has all of these; Hermes has none of them (negative finding).
- **C-002 Dreaming.** Extraction with supersession, provenance and dedup. The extractor prompt already excludes secrets, payment details and special-category data. Grounding is still off by default (`MEMORY_GROUNDING_ENABLED`).
- **C-006 Temporary chat.** The ZDR path is an enforced turn that persists nothing. **C-024** Fine-tuning lifecycle. **C-029** Bounded prefetch with a Norwegian trivial-prompt gate (a Hermes port). **C-028** Plan mode and PlanPanel.

## Caveats
- Every record from ChatGPT, Claude web, Manus web and Perplexity web is DOC. Clusters C-005, C-007, C-024, C-026, C-027 and C-034 rest only on DOC/INF reference records (marked in their rationale). C-024 and C-027 are low confidence.
- Hermes provides 47 of 117 records, so many "unique" clusters reflect how deeply Hermes was read rather than what other products lack.
- Verevon "unknown" (C-008, C-012, C-013, C-022, C-033, C-034) needs a code check. A bounded grep over the whole monorepo timed out, so those statuses are unverified.
- C-015 is "common" (3/12) only in these records. Every major chat product has history search, so treat it as table-stakes in practice.
- The Verevon working tree has uncommitted Model Plane changes (sse.rs, dreaming.rs, agent.rs), so line numbers may drift.
- Several Verevon "shipped" learning items are wired but not proven live (DOC: I23, quality loop).
