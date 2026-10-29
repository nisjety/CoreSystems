---
name: caveman
description: Terse "caveman" response style profile. Drops articles, hedging, and ceremony from model output to maximize token efficiency on tool-heavy and machine-readable response paths. Native Model Plane skill (not ported).
origin: Model Plane
status: spec
---

# Caveman

A response-style profile that strips natural-language ceremony from model
output and emits ultra-compact, telegraphic prose. Intended for paths
where the consumer is another agent, a tool dispatcher, a UI badge, or a
terminal — anywhere that the explanation tax outweighs human readability.

**This is a UX/policy profile, not an architecture.** It does not change
which provider runs, how context is assembled, or how tools dispatch. It
sets a system-prompt fragment + a style policy that the gateway forwards
into `InferRequest.metadata` and that capability-core advertises as a
selectable capability.

## When to Activate

- **Subagent-to-subagent**: when the consumer is another LLM call (e.g.
  `WideResearchWorkflow` aggregating from 10 fan-out probes), the
  intermediate prose is overhead. Caveman saves 30–60% tokens on the
  intermediate hop.
- **Tool-call summarization**: when a step's output is fed back into the
  next reasoning step rather than shown to a human.
- **Status badges / chips**: short answer surfaces where one-liners win.
- **Cost-aware paths**: when `max_cost_usd` is tight and the user has
  opted into terser output.

Do **not** activate for:

- Direct user-facing chat unless the user explicitly opts in (`/caveman`
  or `?style=caveman` in the request).
- Structured output paths (JSON schema, function calling) — those are
  governed by `structured_output_schema`, not by style.
- Wiki proposal text, public-facing summaries, or anything destined for
  durable storage in the wiki/docs surfaces.

## The Style Rules

Caveman output follows these constraints, in order of priority:

1. **Drop articles**: no "the", "a", "an" unless ambiguity would change
   meaning.
2. **Drop copulas and hedges**: no "is/are/was/were" when the verb can
   carry the load. No "I think", "perhaps", "it seems".
3. **Drop transitions**: no "however", "moreover", "in conclusion".
4. **Imperative > descriptive**: "fix import" beats "you should fix
   the import".
5. **Numbers as digits**: `3` not "three"; `42ms` not "forty-two
   milliseconds".
6. **One-line claims**: each fact on its own line; no run-on prose.
7. **Code paths verbatim**: never abbreviate or describe — paste.
8. **No apologies, no recap**: never restate the question.

Examples:

| Default | Caveman |
|---|---|
| "I've reviewed the file and it looks like there's an issue on line 42 where the import is missing." | "missing import: foo.rs:42" |
| "The retrieval call took about 230 milliseconds, which is within the SLO." | "retrieve 230ms ok" |
| "It seems the user wants us to add a new endpoint." | "add endpoint." |
| "Here is a summary of what I found in the three documents..." | "doc1: X. doc2: Y. doc3: Z." |

## Model Plane Wiring

**Capability registry entry** (`capability-core`):

```yaml
- id: style.caveman
  kind: skill
  scope: [run, thread, workspace, user]
  description: Terse caveman response style.
  payload:
    style_token: caveman
    system_prompt_fragment: |
      Output style: caveman.
      Rules: no articles, no copulas, no hedges, no transitions.
      Imperatives over descriptions. Digits not words. One claim per line.
      Code paths verbatim. No apologies. No recap.
```

**Gateway request** sets `metadata.style = "caveman"` on `InvokeRequest`.
The gateway resolves the capability via `capability-core` and prepends
the system_prompt_fragment to the `messages[0]` of `InferRequest` —
without consuming a structured_output_schema slot.

**Slash command** (`/caveman` in `capability-core`): toggles a thread-
scoped policy bit so subsequent invokes inherit caveman style without
the caller re-specifying.

**TOON synergy**: caveman + TOON pairs naturally on subagent paths —
the model emits caveman-style claims, and the gateway encodes the
structured response (e.g. `{ findings: [...] }`) in TOON. Combined
savings on a typical fan-out hop: 50–70% vs. JSON + default style.

## Style vs. Truth

Caveman is style, not content. The model must not drop:

- Refusals or safety messages (those override style).
- Numbers, identifiers, paths, or quoted strings.
- Confidence qualifiers when they materially change the meaning
  ("low_confidence: true" survives; "I think" does not).
- Citations or source refs (`[doc:abc123]` survives; "according to" does
  not).

If a caveman response would be ambiguous, fall back to the default
style for that segment only and note `style_fallback: ambiguity` in the
response metadata.

## Acceptance Tests

A caveman response passes if all hold on a representative corpus:

- Token count ≤ 60% of the default style's token count for the same
  question.
- All numbers, identifiers, and paths from the source survive verbatim.
- No first-person pronouns, no hedges, no transitions in the output.
- Refusal messages render at full default length (style does not
  apply to safety output).

## Open Questions

- Should caveman respect locale (de-articulating English vs. inflected
  languages)? Current proposal: English-only at v1, fall back to default
  for other locales.
- Should the style fragment vary per provider (some models follow style
  instructions more reliably than others)? Current proposal: same
  fragment for all; rely on capability-core fallback if quality drops.

## Related

- [strategic-compact](./strategic-compact.md) — when to compact, what
  survives. Caveman compacts the *output*, strategic-compact compacts
  the *context*.
- [context-budget](./context-budget.md) — measure the savings.
- [cost-aware-llm-pipeline](./cost-aware-llm-pipeline.md) — caveman
  is one of the levers; model routing is another.
