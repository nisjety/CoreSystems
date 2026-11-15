# Hermes Agent: learning, memory and self-tailoring (deep dive)

Audit 3, source "Hermes Agent", facet "learning-and-memory". Companion data file: `hermes.learning-and-memory.features.json` (102 records, ids `hermes-learning-and-memory-001..102`).

## 0. Scope, method and honesty notes

- Repo: `C:/dev/hermes-agent`, HEAD `b23600860f82` (committed 2026-10-12), latest tag v0.21.7. Read-only. Nothing was run, built, installed or executed; no user data directory (`~/.hermes`, `AppData/*`) was opened, and no `.env` was read.
- Provenance tags: SRC = read in source or files, DOC = repo documentation (website/docs, AGENTS.md, READMEs), INF = my inference (basis stated), REA = not used (no shipped binary was analysed; all of Hermes is open-source Python/TS, so source reading is the primary method).
- Depth: I read end to end or near end to end: `tools/memory_tool.py`, `tools/memory_tool_store.py`, `agent/memory_provider.py`, `agent/memory_manager.py` (most), `agent/background_review.py` (prompts, fork construction, whitelist, spawn path), `agent/curator.py` (config, transitions, prompt, run flow), `tools/skill_usage.py`, `tools/skill_ledger.py` (head and entry shape), `tools/session_search_tool.py`, parts of `hermes_state_common.py`/`hermes_state_search.py`, `agent/learn_prompt.py`, `agent/learning_graph.py`, `agent/system_prompt.py`, large parts of `agent/prompt_builder.py`, `agent/context_compressor.py` (constants, summary template, lean mode, anti-thrash, pruning), `cron/{monitor,notepad,suggestions,quota_hold,scheduler_prompt,job_definition}.py` and the relevant docs. Sampled by header/grep only: `loops.py`, `heartbeat.py`, `skill_commands.py`, `foreign_sessions.py`, `skills_guard.py`, `initiate_setup_*`, the bundled ByteRover/RetainDB plugins. Docs only (external repos not inspected): Honcho, Hindsight, Holographic, Mem0, OpenViking, Supermemory, Memori.
- The repo is large (about 2,500 non-test Python files); this report is not a full read of it. Low-confidence items are marked in the JSON.

### 0.1 Corrections to the scout inventory and to earlier Verevon docs

| Item | Scout / earlier claim | What the code says | Evidence |
|---|---|---|---|
| Session search | "FTS5 search with LLM summarisation" | The tool makes no LLM call; every shape returns stored messages with size caps | SRC `tools/session_search_tool.py:1-9`, `:651-700`; DOC `memory.md:217-244` |
| Cron memory | `cron/AGENTS.md`: cron sessions pass `skip_memory=True` | `cron/scheduler.py:2494-2495` passes `skip_memory=False, skip_background_review=True` | SRC |
| Review writes | "writes go straight to the stores" | True only with the gate off and for adds; unattended replace/remove is staged as a proposal, and `memory.write_approval` stages everything | SRC `tools/memory_tool.py:162-212` |
| Memory backends | "8 pluggable sandbox/memory backends" (earlier doc) | 8 is true of terminal environments; core ships only 2 memory plugins (ByteRover, RetainDB, both leaving core 2026-10-15) and the rest are catalog installs | SRC `plugins/memory/`; DOC `memory-providers.md:795-841` |
| SOUL reload | Scaffold text says SOUL.md is "loaded fresh each message" | Code loads it while building the cached system prompt (once per session and after compaction) | SRC `agent/system_prompt.py:544-552`, `agent/prompt_builder.py:1644` (medium confidence on gateway behaviour) |
| `mem_trim.py` | name suggests agent memory | it is a malloc_trim heap-release helper; unrelated to agent memory | SRC `hermes_cli/mem_trim.py:1-8` |

What the two earlier Verevon docs (`claude-hermes-deepseek.md`, `harness-adoption-execution-plan.md`) cover well: the `MemoryProvider` ABC, the one-external-provider cap, trivial-prompt gating, the compaction recall eval and the fact that `background_review`/`curator` are an online file-writing loop rather than a fine-tuning pipeline. What they never mention (grep counts of 0 in both docs for each term): `write_approval`, the frozen memory snapshot, FTS5/`session_search`, the anchor index, `SKILL_PRUNED`, `reuse_after_patch`, monitor/wake-gate cron, the cron notepad, nudge counters, read-before-write enforcement, curator consolidation, `skill_manage`, closed-enum telemetry. This report concentrates on those.

## 1. Executive summary (what matters for Verevon)

1. Hermes treats learning as two stores with a hard routing rule. Declarative facts about the user and environment go to small bounded files (USER.md 1,375 chars, MEMORY.md 2,200 chars); procedures and the user preferences that govern a class of task go to skills (SKILL.md + references/templates/scripts). The review prompts forbid writing a fact to both places (records 010, 011, 041, 102).
2. Everything is engineered around the provider prompt cache. Memory is a frozen snapshot, per-turn recall rides a user-message sidecar that is replayed byte-identically, goals/heartbeats are plain user messages, skill-index changes arrive as in-band notes, the system prompt has stable/context/volatile tiers and is rebuilt only on compaction. The repo states this as an invariant ("prompt caching is sacred") (records 002, 016, 038, 056, 097).
3. The self-improvement loop is a post-turn fork of the same agent, triggered by counters (memory every 10 user turns, skills after 10 tool iterations without a skill write), run after the reply is delivered, on the same prefix so it reads the cache, with a dispatch-side tool whitelist, an iteration cap (16) and an input-token budget (75% of the context window). Unattended deletes are never applied; they are staged for approval. Skill patches require a fresh read in the same review (records 057-063).
4. The curator is deterministic first (stale 14 d, archive 30 d, pinned and scheduled-task-referenced skills protected, archive never delete) and LLM second (opt-in umbrella consolidation with a structured YAML report). Every skill mutation is ledgered with content-addressed before/after blobs, so a single edit or a whole run can be rolled back (records 064-066).
5. Session search is the long-term recall mechanism for everything that is not in the two small files: FTS5 over all messages, lineage-aware dedupe, cron demotion, adaptive hydration, scroll/read shapes, zero LLM cost. Compaction never deletes rows, so details that were compacted away remain searchable (records 030-033). Verevon has no session search today.
6. Compaction is the most mature part: tool-output pruning first, auxiliary-model summary of the middle with iterative updates, a reference-only framing prefix, a lean mode with a regex-harvested anchor index and verbatim user messages, anti-thrash breaker persisted in the DB, native provider compaction as a first tier where available, and a recall-accuracy eval harness (records 069-078).
7. Automations (cron) are far ahead of Verevon scheduled runs: scripts feed prompts, monitor-mode skips the model when a hash is unchanged, a wake gate or `[SILENT]` suppresses delivery, a per-job notepad carries cursors, at-most-once accounting, incident dedup, quota holds, consent-first suggestions and slot-based blueprints, plus tiered prompt scanning for unattended runs (records 084-091, 099).
8. Visibility and control of learned state are first-class: `/journey` graph with edit/delete, `/memory pending`, `/skills pending|diff`, curator reports and rollback, a deterministic recall indicator, a context breakdown that names memory and skills, and closed-enum opt-in telemetry (records 067, 081, 094).
9. Weak points: tiny built-in memory, no per-user scoping inside a profile, no embeddings or semantic retrieval in core, no implicit-feedback signal, cadence-based triggers, and no evaluation of whether learned memories/skills improve outcomes (records 080, 083, 096, 098).
10. Verevon is already ahead in two places (implicit dissatisfaction with Wilson-bounded skill quarantine, calibration rows) and has comparable pieces for prefetch, provenance projection, Dreaming extraction and compaction tiers. The highest-value Hermes ideas to adopt are: session search with compaction-archive semantics, the add-only unattended review with staged destructive ops and read-before-write, per-edit skill ledger/rollback, the api_content sidecar for cache-safe recall, monitor/wake-gate scheduling, and the closed-enum learning telemetry.

## 2. Architecture: four learning loops at four time scales

```
 turn start --------------------------------------------------------------------------------
   |  MemoryManager.on_turn_start -> (non-trivial prompt?) prefetch_all (<= 8 s, daemon thread)
   |  -> fenced <memory-context> block -> user message api_content sidecar (wire only)
   |  system prompt (frozen): [stable: SOUL + guidance] [context: project files] [volatile: skills index,
   |                            MEMORY.md snapshot, USER.md snapshot, external provider block, timestamp]
 during turn ------------------------------------------------------------------------------
   |  model may call memory (batch ops), skill_manage (ops), session_search, skill_view
   |  tool executor resets nudge counters when memory / skill_manage is used
 turn end (after delivery) -----------------------------------------------------------------
   |  sync_turn + queue_prefetch (serialized background worker, secret-scrubbed)
   |  if final_response and not interrupted and not skip_background_review:
   |      memory counter >= 10 user turns  -> review(memory)
   |      skill  counter >= 10 tool iters  -> review(skills); both -> combined review
   |  review = cache-parity fork (daemon thread): tools {memory?, skills, read_file, search_files}
   |      writes: memory add (immediate) / replace,remove (staged) ; skills create/patch/write_file
   |      -> ledger entry per skill mutation, usage sidecar, shared metrics, "memory updated" notice
 idle / weekly ------------------------------------------------------------------------------
   |  curator: deterministic stale/archive pass (+ optional LLM consolidation, snapshot first)
 session boundary (/new, /resume, end) ----------------------------------------------------
   |  MemoryManager.commit_session_boundary_async: on_session_end(snapshot) THEN on_session_switch
   |  memory snapshot reloads from disk; compaction also rebuilds the system prompt (reloads memory)
 any time ----------------------------------------------------------------------------------
      session_search (FTS5) over all sessions;  /learn; /refine;  /journey;  /memory pending
```

Process model note (SRC `agent/background_review.py`, `agent/curator.py`): all of this runs inside the same Python process as the agent (daemon threads). Profile and secret scope are `ContextVar`s that must be copied into every worker (`spawn_context_thread`), and many fixes in the repo are about losing that scope.

## 3. Data model (concrete)

### 3.1 Built-in memory (files)

- `~/.hermes/memories/MEMORY.md`, `USER.md` (per profile; `get_hermes_home()`); entries are plain text joined by `\n§\n`; budgets 2,200 and 1,375 characters (`MemoryStore.__init__`). SRC `tools/memory_tool_store.py:25,150`.
- Companion `*.lock` files (flock, 0600, O_NOFOLLOW), temp-file atomic replace, `*.bak.<unix_ts>` on drift detection (an entry longer than the store limit). SRC `:222,591`.
- Prompt block: header `MEMORY (your personal notes) [NN% - used/limit chars]` / `USER PROFILE (who the user is) [...]`, rendered once at load into `_system_prompt_snapshot`; threat hits become `[BLOCKED: ...]` placeholders in the snapshot only. SRC `:22,187,544`.
- Pending writes: `~/.hermes/pending/{memory,skills}/<8-hex>.json` = `{id, subsystem, action, summary, origin(foreground|background_review), created_at, payload}`; destructive ops carry `matched_entry` (the full entry text reviewed). SRC `tools/write_approval.py:74-90`, `tools/memory_tool.py:64-100`.

### 3.2 Skills (procedural memory)

- Tree: `~/.hermes/skills/[category/]<skill>/SKILL.md` + `references/`, `templates/`, `scripts/`, `assets/`; `DESCRIPTION.md` per category; `.archive/` (flat, recoverable), `.hub/lock.json`, `.bundled_manifest` (`name:originhash`), `.curator_suppressed`, `.usage.json`, `.curator_state`, `.curator_ledger.jsonl`, `.curator_backups/` (tar.gz snapshots + `blobs/` content-addressed), `.locks/`. SRC `tools/skill_usage.py`, `tools/skill_ledger.py`, `agent/curator_backup.py`.
- Usage record (`.usage.json[skill]`): `created_by` (`agent|learn|installed|null`), `use_count`, `view_count`, `patch_count`, `last_used_at`, `last_viewed_at`, `last_patched_at`, `created_at`, `first_seen_at`, `state` (`active|stale|archived`), `pinned`, `archived_at`, `patch_generation`, `last_reused_patch_generation`. SRC `tools/skill_usage.py:348-352`.
- Ledger entry (JSONL): `{id(12 hex), ts, actor(curator|agent|user), action, skill, evidence{}, before[], after[]}` where before/after are file manifests whose bytes live as sha256 blobs; ledger bounded to 5 MB with dedup compaction. SRC `tools/skill_ledger.py:300-322`.
- Curator state: `{last_run_at, last_run_duration_seconds, last_run_summary, last_run_summary_shown_at, last_report_path, paused, run_count}`; per-run report dir `logs/curator/<ts>/{run.json,REPORT.md}`. SRC `agent/curator.py:44-60,473-480`.
- Index in prompt: category -> `name: description[:57]...` rows, cached in `.skills_prompt_snapshot.json` keyed by a manifest of file stats; mid-session deltas delivered by `agent/skills_index_delta.py`. SRC `agent/prompt_builder.py:1191-1260`.

### 3.3 Sessions (the only database)

- SQLite (WAL), schema v31: `sessions` (id, source, parent_session_id, end_reason, model, token/cost counters, title+title_source, pinned/archived/hidden, compression cooldown/streak columns, system_prompt_hash...), `messages` (role, content, tool_calls, reasoning, `active`, `compacted`, `_compressed_summary`, `api_content`, `message_uid`, `absorbed_message_uids`...), `system_prompts(hash, prompt)` (dedup), `session_model_usage` (per session/model/billing/task), `state_meta` (k/v for goals, loops, heartbeats, FTS markers), `compression_locks`, `session_turn_leases`. SRC `hermes_state_common.py:372-580`.
- Indexes: `messages_fts` (external-content FTS5 over a view that truncates role=tool to 8,192 chars), a trigram FTS table (all sources except cron/subagent), an optional CJK bigram FTS table (C extension). SRC `hermes_state_common.py:316,773,880,898`; `native/fts5_cjk/README.md`.

### 3.4 Cron

- Job record (jobs.json): `id`, `name`, `prompt`, `schedule{kind,...}`, `repeat{times,completed}`, `deliver`, `origin`, `skills[]`, `model/provider/base_url`, `script`, `no_agent`, `monitor_script|monitor_url` (+ `monitor_state{hash,last_changed_at}`), `context_from`, `enabled_toolsets`, `workdir`, `attach_to_session`, `reasoning_effort`, `failure_deliver`, scheduler-owned `next_run_at`, `pending_slot`, `state`, `quota_hold_until`. SRC `cron/jobs.py:1813`, `cron/job_definition.py:13`.
- Side stores: `cron/executions.db` (attempts and incidents, immutable terminal states, 1,000 rows kept), `cron/notepad.db` (`(job_id,key,value,updated_at)`, 16 KB/value, 64 KB/job), `cron/suggestions.json` (max 5 pending), per-job output docs. SRC `cron/executions.py:33`, `cron/notepad.py:26`, `cron/suggestions.py:37`.

### 3.5 Telemetry vocabulary (opt-in)

`memory_op{op, origin(background_review|foreground), outcome, failure_class(20 values), provider}`, `curator_run{outcome, trigger(manual|scheduled), *_bucket}`, `delegation_run{...}` with count buckets; consent windows, default off. SRC `hermes_cli/observability/shared_metrics_contract.py:262-275,918-927`.

## 4. Algorithms worth porting

### 4.1 Memory batch apply (all-or-nothing, final-state budget)

```
apply_batch(target, ops):
  scan every add/replace content for threats  -> reject whole batch on any hit
  lock(file); raw = read(file) ; abort if exists-but-unreadable ; detect drift (skip for append-only)
  working = copy(entries)
  for op in ops:
      add:    if content not in working: working.append(content)            # idempotent
      replace|remove: idx = unique_match(working, old_text, pinned=op.matched_entry?)
                      # exact whole-entry match > byte substring > quote/dash/space-folded substring
                      # ambiguous -> error; none -> error + closest_entries(word overlap >= 0.3)
                      record previous_content ; replace whole entry or delete
  if entries and not working: refuse (would_empty)
  if len(join(working, "\n§\n")) > limit: failure (counted toward per-turn cap of 3, then terminal "stop retrying")
  write temp -> atomic rename ; return {usage, entry_count, replaced_entries, removed_entries, done:true}
```
SRC `tools/memory_tool_store.py:472-527`.

### 4.2 Review spawn and fork safety

```
after_turn(agent, final_response, interrupted):
  memory_due = (nudge_interval>0 and memory tool available and store): ++turns_since_memory >= 10 -> reset, True
  skills_due = (skill_nudge_interval>0 and iters_since_skill >= 10 and skill_manage available)  -> reset, True
  if final_response and not interrupted and not skip_background_review and (memory_due or skills_due):
      prompt = COMBINED | MEMORY | SKILL by (memory_due, skills_due)
      fork = build_cache_parity_fork(agent)    # same prefix bytes, no persistence, nudges=0, tools[] unchanged
      fork.dispatch_whitelist = {memory (only if memory_due), skills.*, read_file, search_files}
      fork.approval_callback = deny ; fork.max_iterations = 16 ; fork.max_input_tokens = min(600k, 0.75*ctx)
      run in daemon thread; new live turn or session end cancels it (2 s ack bound)
      usage -> parent session (task=background_review); summary line to user (off|on|verbose)
```
SRC `agent/turn_context.py:752-760`, `agent/turn_finalizer.py:763-790`, `agent/background_review.py:956-1122`.

### 4.3 Curator transitions (no LLM)

```
for skill in curated_report():            # agent-created only (+bundled if curator.prune_builtins)
    skip if pinned or referenced by any cron job (even paused)
    if no record: seed record (clock starts now) ; continue
    anchor = latest(last_used, last_viewed, last_patched) or created_at
    if use_count == 0 and anchor within stale_after_days: keep/reactivate ; continue
    if anchor <= now - archive_after_days (30): archive (move to .archive/, ledger actor=curator)
    elif anchor <= now - stale_after_days (14) and active: mark stale
    elif anchor newer and stale: reactivate
```
Gate: enabled, not paused, `last_run_at` older than 168 h (first sight only seeds), idle measured >= 2 h in CLI/TUI (gateway passes infinite idle). SRC `agent/curator.py:160-250,1198`.

### 4.4 Session search discovery

```
discover(query, role_filter=user|assistant, limit=3, after/before, exclude_ids):
  title_hit = title match(query)
  rows = FTS5 BM25 over messages (scan 300), exclude hidden sources (kanban|subagent|tool), time bounds in SQL
  rows = stable_sort(rows, key=(source == cron))        # demote cron, keep BM25 order inside each class
  for row in rows until limit lineages: root = walk parent_session_id to lineage root
      skip current lineage unless transcript left live context (compression child, /new reset, archived compacted row)
      keep first row per lineage
  hydrate only the top result fully; others get the anchor message only ; each carries @session:<profile>/<id> link
scroll(session_id, around_message_id, window<=20) ; read(session_id, head/tail, 2,000 chars/message) ; browse()
```
SRC `tools/session_search_tool.py:354-433`.

### 4.5 Compaction pipeline (default engine)

```
should_compress: prompt_tokens >= threshold (0.50 of window; >= 0.75 below 512k; 85% cap when floor binds)
                 and not anti-thrash blocked (ineffective strikes persisted; one probe every 300 s)
pre_compress hooks -> memory providers (v2 = fail-closed checkpoint) -> text into summary prompt (<= 6,000 chars)
prune old tool results (dedupe, bounded summaries, image retirement; spare pending round + just-loaded skills)
choose protected head (3) and token-budgeted tail (lean: 10k-25k tokens, <= 20% window)
summary = auxiliary model(template sections, previous summary for iterative update, ~20% budget 2k-10k tokens)
lean extras (no LLM): user messages verbatim (<= 24k chars), anchor index (PRs, SHAs, files, errors, URLs; <= 7k chars)
commit: archive absorbed rows (active=0) or rotate to child session; keep last assistant reply and a user turn;
        re-inject todo list; SKILL_PRUNED markers + "Pruned Skills" list; rebuild system prompt (reload MEMORY/USER)
failure ladder: cooldown 60/300/900 s, main-model retry, deterministic fallback summary, summary model benched after 2 fallbacks
```
SRC `agent/context_compressor.py` (see JSON records 069-077).

### 4.6 Cron tick with monitor and wake gate

```
tick(): advance next_run_at BEFORE dispatch (at-most-once), stamp pending_slot, take per-home tick lock
run(job): if monitor_script|url: out = run(source); if hash(out) == last_hash -> record no_change, STOP (no LLM)
                                  else prompt += "MONITOR CHANGE DETECTED" + capped diff
          if script: stdout injected ; last line {"wakeAgent": false} -> STOP
          if context_from: prepend upstream job's latest non-silent output (<= 8,000 chars)
          scan assembled prompt (strict for user prompt, narrow when skills/data included) ; guard base_url exfil
          run agent (skip_background_review=True, idle watchdog 600 s) ; response "[SILENT]" -> no delivery
          record execution (immutable terminal state) ; group failures into incidents ; honor provider quota hold
```
SRC `cron/AGENTS.md`, `cron/monitor.py`, `cron/scheduler_prompt.py`.

## 5. Interfaces and protocols

| Surface | Detail | Evidence |
|---|---|---|
| Model tools | `memory` (add/replace/remove/operations), `skill_manage` (operations[]), `skill_view`, `skills_list`, `session_search`, `cronjob_manage`, `todo_list`; provider tools (one external provider) with reserved core names | SRC `tools/memory_tool.py:346`, `tools/skill_manager_tool.py:862`, `tools/session_search_tool.py:651`, `agent/memory_manager.py:434` |
| MemoryProvider ABC | `initialize, system_prompt_block, prefetch, queue_prefetch, recall_status, sync_turn, on_turn_start, on_session_end, on_session_switch, on_pre_compress (v1/v2), on_delegation, on_memory_write, identity_signature, get_config_schema, save_config, backup_paths` | SRC `agent/memory_provider.py:84-206` |
| ContextEngine ABC | `update_from_response, should_compress(_info), compress(messages, current_tokens, focus_topic, force, memory_context)`, token-state attributes | SRC `agent/context_engine.py:48-130` |
| Plugin hook | `on_skill_lifecycle(action, skill_name, provenance, use_count, reused, reuse_after_patch, ...)`, plus the 33-event hook vocabulary | SRC `tools/skill_usage.py:470-482`, `hermes_cli/plugins.py:134` |
| Slash/CLI | `/memory pending|approve|reject|approval`, `/skills pending|diff|approve|reject`, `/learn`, `/refine`, `/journey` (`hermes journey list|delete|edit --play --json`), `/goal`, `/loop`, `/heartbeat`, `/cron`, `/blueprint`, `/suggestions`, `hermes curator status|run|pause|resume|pin|unpin|archive|restore|prune|backup|rollback|ledger`, `hermes memory setup|status|off|reset`, `hermes insights`, `hermes cron notepad` | SRC `hermes_cli/commands.py:263`, `hermes_cli/subcommands/{curator,memory,journey,sessions}.py`; DOC feature pages |
| Wire | `api_content` sidecar column; JSON-RPC (TUI/desktop) for journey frames; HTTP `/api/jobs`; none of this is a model-facing protocol beyond tool schemas | SRC |

## 6. Notable design decisions and the failures behind them

1. **Memory as a tiny curated file, skills as the real knowledge store.** The memory tool description tells the model that anything learned while doing a task belongs in the task skill, and that memory must stay small because it is injected every turn. SRC `tools/memory_tool.py:346-372`, `agent/prompt_builder.py:192-232`.
2. **Frozen snapshot over live memory.** A fact saved mid-session is not in the prompt until the next boundary. The doc admits the cost (a weeks-long chat never sees its own memories) and chooses cache stability. DOC `memory.md:57-66`.
3. **Terminal, low-chatter tool results.** After a successful memory write the result is "done, do not repeat" and omits the entry list; failures carry the repair hint. This came from loops of repeated consolidation calls (#42405, #97316). SRC `tools/memory_tool_store.py:150-176,529-541`.
4. **Consent staging, not silent writes.** The unattended reviewer can only add; destructive ops are staged with a pinned target. SRC `tools/memory_tool.py:162-212`.
5. **Read-before-write for autonomous edits.** Introduced after reviews produced patches from transcript memory; read tools were then whitelisted because denying them caused a denial storm (about 142 denials and 204 refusals in two days). SRC `agent/background_review.py:1079-1115`, `tools/skill_manager_guards.py:55-100`.
6. **Skills are lessons, not logs.** Rules against PR numbers, dates, narrative, negative claims about tools, unresolved failures presented as workflows; a linter warns on incident-reference density, more than 60 reference files, bodies over about 24k chars. SRC `agent/background_review.py:385-440`, `tools/skill_linter.py`.
7. **Archive, never delete.** Curator and reviewer both obey it; plus ledger, blobs and snapshots; rollback fails closed and snapshots first. SRC `agent/curator.py:1-8`, `tools/skill_ledger.py:569`.
8. **Deterministic before LLM everywhere.** Curator pruning, compaction anchors, cron monitor hashing, wake gates, title stage 1, `reuse_after_patch`, closed-enum metrics: cheap code first, model second.
9. **Provider quirks shape prompts.** `SKILLS_GUIDANCE` was reworded because a provider-side filter rejected the earlier phrasing on subscription OAuth credentials with a billing-shaped HTTP 400 (bisected to one sentence; "empirically validated, not understood"). The summary prefix gained clauses after post-compaction misbehaviour (#41607, #65848). SRC `agent/prompt_builder.py:243-257`, `agent/context_compressor.py:248-292`.
10. **Narrow waist.** New capability should arrive as CLI + skill, service-gated tool or plugin; tool schema size is A/B tested (session_search schema 1,570 to 695 tokens per call). SRC `AGENTS.md:24-27`, `evals/session_search_schema/README.md`.
11. **Core stays small, ecosystem carries the rest.** External memory backends moved to a pinned-SHA plugin catalog; core keeps the ABC, the cap of one, the reserved-name guard and the scrubbing chokepoint. DOC `memory-providers.md:818-838`.

## 7. What is best-in-class (relative to the audited peers)

Peer grounding comes from the Audit 3 peer inventories (`claude`, `codexagent`, `cursor`, `manus`, `deepseek`, `t3code`, `antigravity`), which inventory shipped binaries and source and mostly list names, not mechanisms; "not reported" below is weak evidence of absence (server-side behaviour is invisible). claude.ai and ChatGPT web were not audited.

| Capability | Why it stands out | Records |
|---|---|---|
| Memory write ergonomics and safety (batch atomic ops, healing errors, approval staging, entry pinning) | Peers show memory RPCs or subsystems (Claude Cowork read/write/delete/reset, Codex summaries/consolidation/versions, Antigravity CortexMemory) but none reports a hard budget, staged approval or pinned targets | 003, 004, 008, 009 |
| Cache-parity learning fork | Learning at about the cost of cache reads, no live-turn interference | 058-061 |
| Skill ledger and rollback; curator | Undo/audit for agent-authored procedural memory | 064-066 |
| Compaction (lean mode, anchors, anti-thrash, eval) | No peer inventory reports an anchor ledger or a recall-measured compaction eval (DeepSeek: LLM summary + pruner + image offload; Codex: Pre/PostCompact hooks) | 069-078 |
| Session search with compaction-aware lineage | Recall that survives compaction at zero LLM cost; DeepSeek harness has a comparable FTS5 provider but its model-facing tools are experimental | 030-033 |
| Cron/automation engine | Peers have scheduled tasks or automations (Claude, Codex, Cursor, Manus, DeepSeek) but none shows monitor hashing, wake gates or notepads | 084-091, 099 |
| Learning visibility (journey graph, pending queues) | Editable graph of learned skills and memories; no peer inventory reports one | 067 |
| Eval culture (A/B diets, forensics, compaction recall) | Methodology is reusable even where code is not | 077-079 |

Nothing here is a model-quality feature; the strengths are infrastructure around the model. Where peers are ahead of Hermes in this facet: Antigravity and Claude Cowork have scoped memory (global, project, account, Space) while Hermes has one scope per profile; Codex lists memory import and versions; none of that was verified in mechanism.

## 8. Weaknesses and limitations

1. **Tiny, single-scope memory.** 2.2k + 1.4k chars per profile, one user per profile; shared chats depend on external providers (Honcho peers, identity mapping). SRC `tools/memory_tool_store.py:25`, DOC `memory.md:20-26`.
2. **No semantic retrieval or user model in core.** Recall is FTS5 plus the model reading an index; embeddings exist only in external plugins. INF (greps over agent, tools, hermes_state*, cron found no vector store).
3. **Cadence-based triggers.** Every 10 turns / 10 tool iterations; a correction in turn 3 of a 6-turn session may never be reviewed unless the model writes a skill itself or the user runs `/refine`. SRC `agent/agent_init.py:1301,1380`.
4. **No outcome evaluation of learning.** Evals cover compaction, schemas and cost; none checks whether learned memories/skills help later tasks. `reuse_after_patch` is exposed but no consumer was found. INF (absence), medium confidence.
5. **No implicit feedback loop.** The only sentiment detector is a cosmetic affection regex. SRC `agent/reactions.py`.
6. **Files and locks do not give multi-tenancy.** Docs warn never to share one home between two agent processes; hand edits can drift (guarded but refused).
7. **Frozen snapshot staleness** until the next boundary; relies on user hygiene (`/new`). DOC `memory.md:59-66`.
8. **Aggressive curator prompt** ("fewer than 10 archives means you stopped too early") and default-off consolidation show the calibration is unsettled. SRC `agent/curator.py:290-470`.
9. **Doc drift** (cron memory, session search summarisation, SOUL reload) indicates docs lag a fast-moving codebase (about 900k lines of Python). Code is ground truth.
10. **Skill over-triggering cost.** The index prompt says to load any partially relevant skill and "err on the side of loading"; combined with 60-char descriptions this trades tokens for recall. SRC `agent/prompt_builder.py:1467-1490`.
11. **External provider capabilities are doc-only here.** Honcho, Hindsight, Holographic, Mem0, OpenViking, Supermemory, Memori behaviours were not verified in code.

## 9. Mapping to Verevon Model Plane (grounded in the Audit 3 baseline)

Verevon facts used (baseline `v-modelplane.baseline.md` section 8 and 3): memory in session-core `agent_memory` + letta-bridge semantic recall; Dreaming extraction; prefetch with trivial-prompt skip and 400 ms bound; provenance projection; `save_memory`/`recall_memory`; learned skills in `agent_skills` injected up to 3 per turn under a character budget with `reattach_skill`; capability-core `sessionreview` runs after `RUN_COMPLETED`; implicit dissatisfaction to Wilson-bounded quarantine; explicit feedback to skill credit; tier 1/2 compaction with `reattach_context`; no session search; scheduled runs are one no-tool call; Do mode is context-blind; ZDR path persists nothing; Space access never widens owner ACLs; cross-plane communication is by events, not DB.

| Hermes mechanism | Verevon today | Recommendation | Constraint to respect |
|---|---|---|---|
| Session search (FTS, lineage, cron demotion, scroll/read, @session links) | absent | Copy (priority 1). Postgres `tsvector` + `pg_trgm` in session-core; expose as a Model Plane tool; keep compaction archive rows searchable | ZDR threads are never indexed; Space/org scoping at query time; do not read Data Plane DB |
| Compaction archive + search | summary persisted, `reattach_context` | Adapt: keep shed turns as archived rows, add search tool | Retention policy, GDPR erasure must reach archived rows |
| Add-only unattended review; staged destructive ops; read-before-write | sessionreview upserts skills | Adopt as policy for sessionreview (add/patch allowed, delete/merge staged) | Control-signed approvals for Space/org skills |
| Skill ledger (before/after manifests, rollback) | quarantine only | Copy: append-only table + blobs in MinIO | Org audit requirements |
| Curator lifecycle (stale/archive, pin, cron-reference protection) | none seen | Adopt deterministic part; LLM consolidation optional | Scheduled tasks referencing skills must be rewritten on merge |
| api_content sidecar for recall | system message "Relevant memory" per request | Adapt to keep provider prefix caches warm | Rust request builder must store exact injected bytes per turn |
| Three-tier prompt | tools/system/history breakpoints | Adapt: stable org/Space tier, volatile per-user tier | ADR-0003 hierarchy |
| Closed-enum learning telemetry | none specific | Copy | Aligns with ZDR/GDPR |
| Monitor-mode and wake gate for scheduled runs | one no-tool call | Copy | Control decision per effect; delivery path missing |
| Notepad per scheduled task | none | Copy (small) | size caps, prompt injection scan |
| Anchor index in compaction | none | Copy (no LLM; Rust regex crate is linear time) | none |
| Pre-compress v2 fail-closed checkpoint | best-effort hook | Adapt | memory tier outage must not block compaction forever; add timeout then fallback |
| `/learn` skill authoring from sources | none seen | Adapt for Spaces and org knowledge | Copyright (synthesize, do not reproduce); source hygiene for hidden Unicode |
| Honcho-style user representation | Dreaming facts + provenance | Evaluate later | cost knobs (cadences) |
| Implicit-feedback loop | already shipped (Wilson quarantine) | Keep Verevon design; add `reuse_after_patch` as a positive signal | none |

### 9.1 Rust-portable notes

- Per-request scope (HERMES_HOME + secret scope ContextVars) maps to tokio task-locals or an explicit `Scope` struct passed into background tasks; the Python bug class "worker thread lost the scope" has a direct Rust analogue (spawned task without the span/extension).
- The memory batch semantics are a pure function over `Vec<String>` with a budget; implement it as such and test with proptest. Folded matching = Unicode NFKC plus a small quote/dash map.
- File locks become Postgres row locks or advisory locks; atomic temp+rename becomes a transaction.
- FTS5 -> Postgres `tsvector` + `pg_trgm` (or Tantivy if a local index is preferred); copy "index only the first 8 KB of tool output" and "exclude automation sources from the trigram index".
- Skill ledger blobs: sha256 content addressing in MinIO; ledger rows in Postgres (append-only); snapshots as tarball objects.
- Review fork cache parity needs the exact prefix bytes; with a service-based reviewer, store the serialized request prefix (or its hash) with the run so the reviewer can replay it within the provider TTL, or accept a cold cache and use a cheaper model with a digest (Hermes docs claim 3 to 5 times cheaper; not verified).
- Regex threat scanning with bounded filler maps cleanly to the `regex` crate (no backtracking), which removes the ReDoS caveat Hermes handles with a 65k scan cap.

## 10. Copy / adapt / avoid

Copy: session search design; staged destructive ops + read-before-write; ledger; closed-enum telemetry; monitor/wake-gate/notepad; anchor index; error payloads with repair hints and retry caps; "prompt cache impact" as a review rule; A/B eval methodology; lessons-not-logs authoring doctrine and the do-not-capture list (these can be lifted nearly verbatim, translated for Norwegian).

Adapt: frozen memory snapshot (refresh at thread start and compaction, not per turn); sidecar for recall; curator (deterministic part first); profile distributions (shareable agent definition, private learned state) for Space agents; consent-first suggestions.

Avoid or defer: tiny global memory budgets as the only memory (Verevon already has a richer store); file-based storage and flock in a multi-tenant cloud; cadence-only triggers (combine with feedback signals); aggressive consolidation prompts by default; novelty personalities; assuming docs equal behaviour (verify against code, as the doc/code drifts above show).

## 11. Open questions

1. Does the review fork measurably improve later outcomes? No evidence in repo; what would a held-out replay eval look like (replay session N+1 with and without the learned skill)?
2. How often does the 10-turn/10-iteration cadence miss corrections, and does a "correction detected" trigger (as Verevon already has via implicit feedback) beat counters?
3. Real-world cost of the cache-parity fork versus a routed cheap-model fork (docs cite about 26 percent end to end and 3-5x; only comments and docs seen).
4. Production behaviour of the idle-deferral queue and the 2-second cancel handshake under load (not exercised).
5. What do the Honcho, Hindsight and Holographic plugins actually do inside (external repos not read), and is the dialectic loop worth its LLM cost?
6. How does `memory.write_approval` interact with multi-user gateway chats (whose approval counts)?
7. The `usage` and `integration` suggestion sources are declared but no producer was found: planned feature or private plugin?
8. Does the Hermes achievements plugin or the star-map share code feed any learning decision (not inspected)?

## 12. Evidence index (record families)

- Memory store and guards: records 001-011, 096. Memory manager/provider/plugins: 012-028.
- Sessions and search: 029-036. Skills: 037-049, 101 (slash commands and bundles). Persona/profile/context: 050-056.
- Review loop, curator, ledger, journey: 057-068; adaptation to the user: 102. Compaction, working memory and evals: 069-083, 100.
- Cron/automations/goals: 084-093, 099. Transparency and doctrine: 094-098.
- All evidence paths are under `C:/dev/hermes-agent/` with line ranges verified to exist; ranges were spot-checked against the first line they cite.
