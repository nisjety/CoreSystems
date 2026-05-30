# Model Plane — Reference Matrix

This file records what each reference system contributes to the next-goal plan, **and what Model Plane has implemented today against that reference**.

The rule is simple:

- adopt product ideas
- preserve Rust/Go ownership
- reject architecture drift that would turn references into the new source of truth

Status legend: ✅ done · 🟡 partial / stubbed · ❌ not started. See gap-analysis.md § 13 for the canonical stub-replacement inventory.

## Hard parity references

### `apps/Model Plane v2`

**What it has**

- broad agent-core shell
- orchestration HTTP surface
- explicit plan mode and approvals
- hooks, MCP, skills, cron, tasks, plugins
- broad capability-core HTTP surface
- multimodal AI gateway breadth

**What Model Plane should adopt**

- orchestration endpoints and user-facing run lifecycle
- explicit plan/approval workflow
- capability discovery and scoped enablement
- tasks/cron/plugin/MCP surface definitions
- multimodal API breadth

**Where it lands**

- `session-core`: durable run/session/approval/plan records
- `execution-core`: step loop, hooks, tool execution, multimodal execution
- `orchestrator-core`: tasks, cron, team coordination, recovery orchestration
- `capability-core`: tools, commands, skills, plugins, MCP, models, routing, memory, safety
- `model-gateway`: public invoke and shell-facing ingress

**Current Model Plane status**

- run/session/checkpoint records: ✅ live in `session-core` (Postgres + outbox).
- plan / approval / todo / subagent-lineage records: 🟡 `session-core/orchestration_store.rs` scaffolded; 12 RPC handlers Unimplemented (gap-analysis § 13.1).
- step loop + hook gates: ✅ live in `execution-core` (PR-2 secret scrub + PR-8 SLO gates closed).
- capability registries: 🟡 `capability-core` has 6 RPCs against an in-memory registry; 0 of 8 product registries (tools/commands/skills/plugins/MCP/models/routing/memory adapters/safety) populated.
- multimodal: ❌ chat/completions only.

**What not to copy**

- Python monolith as runtime source of truth

### `/Volumes/Lagring/Triodelab/claude-code-fork`

**What it has**

- command registry
- tool registry
- IDE bridge
- multi-agent coordinator
- plugins
- skills
- memory directory
- tasks
- voice
- remote/server modes

**What Model Plane should adopt**

- large command and tool ecosystem surface
- explicit bridge and coordinator product surfaces
- plugin and skill lifecycle
- memory/task/voice product-level parity

**Where it lands**

- `capability-core`: tool/command/plugin/skill metadata and policy
- `orchestrator-core`: coordinator, tasks, team workflows
- client shell layer: CLI/TUI/Web/bridge/voice surfaces
- `session-core`: durable memory/task/session state

**Current Model Plane status**

- tool / command / plugin / skill registries: ❌ none of the 4 registries populated.
- coordinator / tasks / team workflows: ❌ no API surface (`/v1/tasks/*`, `/v1/cron/*` planned).
- bridge / voice / channel ingress: ❌ no service exists.
- memory: 🟡 `letta-bridge` in-memory `memstore` only — no real Letta upstream wired.

**What not to copy**

- TypeScript-specific runtime structure

## Enhancement references

### `graphify`

Source: https://github.com/safishamsi/graphify

**Observed strengths**

- multimodal corpus ingestion
- deterministic AST pass for code
- graph export
- extracted vs inferred labeling
- clustering and graph-centric exploration

**Model Plane improvement**

- add graph-backed workspace/org memory
- add codebase knowledge extraction pipeline
- label knowledge edges as extracted, inferred, or ambiguous
- support graph view and graph-aware context assembly

**Owner**

- Rust extraction/runtime path
- Go durability, registry, and policy for graph artifacts

### `GraphRAG`

Source: https://github.com/microsoft/graphrag

**Observed strengths**

- graph-based indexing pipeline
- structured extraction from unstructured text
- graph retrieval for synthesis
- explicit operator warning that indexing is expensive

**Model Plane improvement**

- document offline and incremental graph indexing
- add graph retrieval before final context assembly
- add operator controls for rebuild cadence, cost, and scope

**Owner**

- Rust for extraction and retrieval execution
- Go for durable index orchestration and scheduling

### `LLM Wiki`

Source: https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f

**Observed strengths**

- persistent wiki between sources and answers
- compounding knowledge artifact
- index/log files
- contradiction surfacing
- answer outputs that can be filed back into memory

**Model Plane improvement**

- add writable wiki memory layer per workspace/org
- add source log and knowledge-maintenance log
- add knowledge lint passes for contradictions, stale pages, and orphan pages
- treat analyses and comparisons as durable artifacts, not chat exhaust

**Owner**

- Rust for synthesis and file generation
- Go for durable orchestration and policy

### `autoresearch`

Source: https://github.com/karpathy/autoresearch

**Observed strengths**

- bounded autonomous research loop
- fixed wall-clock budgets
- explicit program file that steers experiments

**Model Plane improvement**

- add research/task loop mode with cost and duration budgets
- add durable "program" artifacts for experiments and long-running agent work
- add evaluation-first task loop templates

**Owner**

- `orchestrator-core` for durable loops
- `execution-core` for run execution

### `openclaw`

Source: https://github.com/openclaw/openclaw

**Observed strengths**

- local-first gateway
- multi-channel inbox
- multi-agent routing
- voice wake/talk mode
- live canvas
- first-class tools, sessions, cron, skills, webhooks

**Model Plane improvement**

- document channels and gateway as first-class operator surfaces
- add agent routing across channels, peers, and sessions
- add voice and canvas to product-shell roadmap
- add remote-safe sandbox defaults to docs

**Owner**

- Go durable gateway/control features
- Rust runtime execution behind those shells

### `hermes-agent`

Source: https://github.com/nousresearch/hermes-agent

**Observed strengths**

- shared command model across CLI and messaging
- toolsets
- skills system
- persistent memory
- MCP integration
- cron scheduling
- many execution backends

**Model Plane improvement**

- unify command semantics across terminal, web, and channels
- define toolset presets and scope-aware enablement
- make MCP and memory UX explicit
- make cron and platform backends first-class in docs

**Owner**

- `capability-core` for tools/toolsets/MCP
- `orchestrator-core` for scheduling
- client shell layer for CLI/channel parity

### `openai/codex`

Source: https://github.com/openai/codex

**Observed strengths**

- lightweight local-first coding agent shell
- AGENTS/context-file model
- clear docs/runtime split

**Model Plane improvement**

- standardize AGENTS/skills/context-file guidance in docs
- keep local-first execution path central to operator story
- separate shell docs from runtime docs cleanly

**Owner**

- docs and client shell layer

### `logseq`

Source: https://github.com/logseq/logseq

**Observed strengths**

- privacy-first knowledge management
- graph-oriented mental model
- knowledge workspace positioning

**Model Plane improvement**

- position knowledge artifacts as private/local-first by default
- expose graph/wiki knowledge workspace language in docs
- design operator-facing knowledge browsing around graph + page metaphors

**Owner**

- docs, knowledge UX, client shell layer

### `TOON`

Source: https://github.com/toon-format/toon

**Observed strengths**

- compact structured encoding for JSON-like payloads
- token-efficiency for LLM input

**Model Plane improvement**

- optional compact transport for large tool outputs
- optional compact context payloads and memory summaries
- benchmark compact transport vs JSON in verification

**Owner**

- Rust runtime transport + compaction layers

### `caveman`

Source: https://github.com/juliusbrussee/caveman

**Observed strengths**

- terse operator-facing response modes
- token-aware output ergonomics

**Model Plane improvement**

- document response-style profiles for agents
- add operator-selectable verbosity/compression modes
- separate output style controls from reasoning/runtime controls

**Owner**

- client shell layer
- response-format policy in capability/runtime docs

## Adoption rules

### Adopt now

- `Model Plane v2` orchestration and capability shell concepts
- `claude-code-fork` command/tool/bridge/coordinator/plugin/skill/task/voice surfaces
- `graphify` provenance-aware graph extraction
- `LLM Wiki` persistent wiki memory layer

### Adopt later

- `GraphRAG` indexing and graph retrieval depth
- `openclaw` channel, voice, canvas, and gateway breadth
- `hermes-agent` broad toolset/backend surface
- `TOON` structured compact transport
- `caveman` operator-facing terse modes
- `autoresearch` dedicated research loops
- `logseq` knowledge workspace UX ideas

### Inspiration only

- exact repo layouts
- exact language/runtime choices of references

### Explicit do-not-copy list

- Python monolith hot path from `Model Plane v2`
- TypeScript runtime ownership from `claude-code-fork`
- summary-only memory that drops source provenance

## Per-reference current status (one-liner)

| Reference | Adoption status in Model Plane today |
|---|---|
| `Model Plane v2` orchestration shell | ❌ stubs only (gap-analysis § 13.1) |
| `Model Plane v2` capability shell | 🟡 6 RPCs + in-memory registry; 0 of 8 product registries populated |
| `Model Plane v2` multimodal | ❌ chat/completions only |
| `claude-code-fork` command/tool surface | ❌ none |
| `claude-code-fork` bridge/coordinator | ❌ none |
| `claude-code-fork` skill/plugin lifecycle | ❌ none |
| `claude-code-fork` memory/task/voice | ❌ none |
| `graphify` extracted-vs-inferred provenance | ❌ none |
| `GraphRAG` graph indexing | ❌ none |
| `LLM Wiki` writable wiki layer | ❌ none |
| `autoresearch` bounded research loops | ❌ none |
| `openclaw` channels/voice/canvas | ❌ none |
| `hermes-agent` shared command model | ❌ none |
| `openai/codex` AGENTS/context-file model | ❌ none |
| `logseq` graph workspace UX | ❌ none |
| `TOON` compact transport | ❌ none |
| `caveman` terse output modes | ❌ none |

