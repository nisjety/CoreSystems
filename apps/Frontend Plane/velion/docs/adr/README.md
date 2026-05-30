# Architecture Decision Records

This directory holds the architectural decisions that affect velion's
integration with the rest of the CoreSystem pyramid. ADRs capture **why** a
decision was made — the alternatives considered and their trade-offs — so
future contributors don't have to re-derive context from code alone.

## Format

We use a lightweight MADR-flavoured template:

1. **Title + Number** — `NNNN-short-noun-phrase.md`
2. **Status** — `proposed | accepted | superseded by NNNN | deprecated`
3. **Context** — what's broken, why now, who's affected
4. **Options** — at least two viable alternatives, each with pros/cons
5. **Decision** — the option picked + the rationale
6. **Consequences** — what follow-up work this implies, and what we're giving up
7. **Implementation notes** — actionable, file-level guidance for the team
   that will execute the decision

## Index

| # | Title | Status | Closes |
|---|---|---|---|
| 0001 | (reserved — implicit pyramid charter, see `docs/ARCHITECTURE_DIAGRAM.md`) | accepted | — |
| 0002 | [CP session-core repurpose](./0002-cp-session-core-repurpose.md) | accepted | velion-gap.md G10 |
| 0003 | [L5 boundary policy](./0003-l5-boundary-policy.md) | accepted | velion-gap.md G17 |

When you author a new ADR:

- Pick the next number, even if you supersede an earlier one (mark the old
  one `superseded by NNNN`, never delete).
- Reference it in `velion-gap.md` if the decision closes or downgrades a
  gap.
- Keep ADRs short enough to read in 10 minutes. If the doc grows past
  ~400 lines, split implementation guidance into a sibling design doc.
