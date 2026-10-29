# Response-style profiles

## Purpose

Presentation-layer styles for agent and operator responses, **decoupled from runtime logic**. A style controls how an answer is rendered; it never controls what the system decides.

## Profiles

| Profile | Characteristics | Use case |
| --- | --- | --- |
| `default` | full prose, markdown, citations inline | human-facing chat |
| `concise` | 1–3 sentence answers, bullets when listing | operator CLI |
| `caveman` | ultra-compressed, telegraphic | LLM-to-LLM handoff |
| `structured` | JSON body + short human summary | programmatic consumers |
| `audit` | full prose + explicit reasoning trace | compliance review |

## Invariants

- Style is a **rendering** decision, never a **policy** decision.
- Same input + same state → same decision, regardless of style.
- Errors render in every style with identical error codes and causes.
- Citations, identifiers, and idempotency keys survive every style.
- No style removes safety disclaimers where policy requires them.

## Selection

- Default: `default`.
- Client opt-in: `X-Response-Style: concise` or per-call parameter.
- Operator-pinned per session/tenant via config.
- Unknown style → fall back to `default`, warn via response header.

## Boundaries

- Styles are implemented in a response renderer that sits **after** the handler returns its canonical result.
- Handlers MUST NOT branch on style.
- Tests assert style-invariance: same canonical result → same decoded facts across all styles.

## Benchmarks

Per style, publish:

- Token count ratio vs `default`.
- Fact retention (eval sample).
- Render latency.

## Relationship to other phase-8 features

- `caveman` style uses the summary rules from [compact-summaries.md](./compact-summaries.md) at rendering time, not at storage time.
- `structured` style may be combined with TOON transport (see [compact-transport.md](./compact-transport.md)) via content negotiation.
- Style is orthogonal to [verbosity](./verbosity-profiles.md): verbosity controls diagnostics, style controls the user-facing answer.
