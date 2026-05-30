# Skill References

Curated reference docs for Model Plane services. Most files here are
copied verbatim from external Claude Code patterns work
(`everything-claude-code`, "ECC") with `origin: ECC` preserved in
frontmatter. One — [caveman.md](./caveman.md) — is native to Model
Plane and authored as a forward-looking spec that the
`capability-core` registry should expose.

These are **patterns**, not runtime entries. They describe how the
runtime services and capability registry should be shaped. Adding a
new pattern doc here does not register a runtime capability —
see `go/services/capability-core/` for the registry entry shape and
`rust/services/execution-core/src/tool_bridge` for tool dispatch.

## Mapping to Model Plane / Quarry / Data Plane

### Reasoning, agent loops, and orchestration (Model Plane core)

| Skill | Model Plane home |
|---|---|
| [agent-harness-construction](./agent-harness-construction.md) | `execution-core` tool definitions, observation formatting, action-space design. The most directly applicable skill — tool I/O shape determines completion rates more than model choice. |
| [agentic-engineering](./agentic-engineering.md) | `orchestrator-core` eval-first execution, decomposition, cost-aware routing. Frames how `AutoresearchWorkflow` should sequence experiments. |
| [continuous-agent-loop](./continuous-agent-loop.md) | Canonical successor to `autonomous-loops`. Quality gates, evals, recovery controls — `orchestrator-core` Temporal workflows + `execution-core` runtime loop. |
| [continuous-learning-v2](./continuous-learning-v2.md) | Instinct-based learning with project scoping. Maps to `letta-bridge` memory store + `capability-core` skill registry promotion + `mp-events` consumer. |
| [enterprise-agent-ops](./enterprise-agent-ops.md) | `orchestrator-core` + `cost-core` + `mp-telemetry` for production agent ops: observability, security boundaries, lifecycle. |
| [eval-harness](./eval-harness.md) | `mp-slo` harness + `eval-lab-py`. Eval-driven development pattern for agent regression testing. |
| [ralphinho-rfc-pipeline](./ralphinho-rfc-pipeline.md) | `orchestrator-core` multi-agent DAG with quality gates and merge queues. Reference for extending wide-research fan-out into a full DAG planner. |
| [verification-loop](./verification-loop.md) | `orchestrator-core` autoresearch verification gates + `execution-core` post-tool hooks. |

### Context, prompts, and response style (Model Plane reasoning surface)

| Skill | Model Plane home |
|---|---|
| [strategic-compact](./strategic-compact.md) | `session-core` `CompactNow` RPC + `capability-core` `/compact` slash command + `mp-toon` for terse summaries. |
| [context-budget](./context-budget.md) | `session-core` `assemble_segments` budget math; spec for a future `/v1/context/budget` endpoint. |
| [cost-aware-llm-pipeline](./cost-aware-llm-pipeline.md) | `cost-core` (Go) ledger + `inference-core` provider router. Frozen-record cost-tracker pattern aligns with `cost-core`'s in-memory ledger. |
| [prompt-optimizer](./prompt-optimizer.md) | `inference-core` prompt strategy + provider-specific routing. Reference for adding a `/v1/prompt/optimize` endpoint. |
| [regex-vs-llm-structured-text](./regex-vs-llm-structured-text.md) | `inference-core` structured_output_schema decision — start regex, escalate to LLM only for low-confidence edge cases. |
| [caveman](./caveman.md) | **Native Model Plane spec.** Terse response-style profile registered in `capability-core` as `style.caveman`; gateway prepends a system-prompt fragment when `metadata.style = "caveman"`. Pairs with TOON for 50–70% subagent-hop savings. |

### Capability registry and ecosystem (`capability-core`)

| Skill | Model Plane home |
|---|---|
| [mcp-server-patterns](./mcp-server-patterns.md) | `capability-core` MCP integration (Node/TypeScript SDK reference). Informs how Model Plane MCP capabilities are registered and surfaced. |
| [skill-stocktake](./skill-stocktake.md) | `capability-core` registry quality audit. Spec for a `/v1/capabilities/audit` endpoint that scores registered skills. |
| [claude-api](./claude-api.md) | Reference for `inference-core` Anthropic provider — Messages API, streaming, tool use, vision, extended thinking, batches, prompt caching. |

### Quarry — browser, scrape, search

| Skill | Quarry home |
|---|---|
| [data-scraper-agent](./data-scraper-agent.md) | Scheduled public-source scraping with LLM enrichment — directly informs Quarry's `browser-agent` + scrape pipeline + `data-quality-go` enrichment hooks. |
| [exa-search](./exa-search.md) | Quarry search-tool capability. Wraps Exa MCP for web/code/company research. Surfaced in `capability-core` as a tool. |
| [search-first](./search-first.md) | "Research-before-coding" workflow → in agent terms, "retrieve-before-generate". Maps to the `session-core` retrieval segment running ahead of the prompt segment. |

### Data Plane — knowledge, retrieval, research

| Skill | Data Plane handling in Model Plane |
|---|---|
| [iterative-retrieval](./iterative-retrieval.md) | Most directly applicable. Pattern for progressive refinement of context retrieval — informs how `session-core` `fetch_retrieval_segments` should iterate when `low_confidence: true` is returned by `RetrievalService.Retrieve`. |
| [deep-research](./deep-research.md) | Multi-source synthesis with citation. Model Plane's role: drive `RetrievalService` + `WikiService` calls, synthesize, return cited reports. Quarry's role: capture the raw evidence. |

## How These Differ from CapabilityCore Capabilities

CapabilityCore registers **runtime capabilities** that agents invoke at
execution time. The docs in this folder are **patterns** — design
references that should inform how those capabilities are shaped.

`caveman.md` is the exception: it's authored *as* a spec for a future
capability registry entry (`style.caveman`). When that entry lands in
`capability-core`, this doc continues as the reference describing why
the entry exists and how it should behave.

## Attribution

ECC-origin files were copied verbatim from
`/Volumes/Lagring/Triodelab/everything-claude-code/skills/<name>/SKILL.md`
on 2026-05-08. Their `origin: ECC` frontmatter is preserved. Code or
hook configurations they reference (e.g. `suggest-compact.sh`,
`evaluate-session.sh`, `agents/`, `scripts/`) are not ported — only
the design narrative is adopted.

`caveman.md` is `origin: Model Plane`.
