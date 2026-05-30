# Compact summaries (caveman)

## Purpose

Optional, ultra-compressed summary style for memory and context that preserves technical substance while cutting token usage ~75%. Used when the underlying record is already source-bearing and the summary is a derived view.

## When to use

- Context injection for agent prompts where the full record exceeds the model budget.
- Operator dashboards surfacing "what changed" rollups.
- Long-lived session context compaction after N turns.

## When NOT to use

- The primary record of any fact (always store verbatim).
- Audit logs, policy decisions, or anything legally material.
- Safety-critical instructions or tool schemas.
- Any surface that feeds contradiction resolution (Phase 7).

## Invariants

- Every caveman summary points back to its source record by ID.
- Summaries are **derived**, never **authoritative**.
- Re-deriving a summary from the source MUST be idempotent.
- Summaries carry a `style_version` so consumers can re-render.

## Style rules

- Subject → verb → object, dropped articles.
- Preserve: identifiers, error codes, numeric thresholds, URLs, file paths, code.
- Drop: hedges, transitions, redundant framing.
- One fact per line where practical.

## Storage

- Summaries live alongside source records in the memory layer, not in place of them.
- Memory reads default to verbatim; callers opt into summary view.
- Contradiction detection (Phase 7) runs against verbatim, never summary.

## Reversibility

- Full record is always retrievable by source ID.
- Summary deletion does not cascade to source.
- Summary regeneration is deterministic given `(source_id, style_version)`.

## Benchmarks (required before enabling)

Per summary type, publish:

- Token ratio (verbatim → caveman).
- Fact retention score (manual eval sample).
- Regeneration cost (p50/p99).

## Feature flag

- `CAPABILITY_CORE_COMPACT_SUMMARIES_ENABLED` (default `false`).
