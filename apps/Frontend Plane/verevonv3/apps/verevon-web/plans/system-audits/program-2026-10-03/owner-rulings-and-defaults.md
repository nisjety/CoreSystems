# Owner rulings and build defaults (2026-10-03)

These bind every implementer and reviewer of the contextual-retrieval, Quarry option B, and zero-retention (ZDR) work.

## Rulings given by the owner (verbatim intent)

1. We never do small upgrades that hurt in the long term. We always choose the best option for Verevon and CoreSystem. No shortcuts.
2. Contextual retrieval (CR) is NOT switched off. It is fixed so it works perfectly. It may need the same cross-plane tuning (service principals, audiences, scopes, metering) that Quarry-v2 and the Data Plane needed.
3. Reranking never uses subscription-based reranking. Paid reranking is fine.
4. Quarry: option B. The Data Plane owns a rerank capability that accepts caller-supplied candidate documents; Quarry calls it instead of prompting a general model. The full system must work.
5. ZDR must be workable, only for orgs that have it on, and we respect it. If a user uses their OWN subscription or API key that does not support ZDR, we do not stop them: when they ADD the connection we check ZDR support and show a prompt that says whether it does or not (stronger warning when their org has ZDR on, because it contradicts the org setting). They can continue. ZDR is then turned off ONLY for runs that use that connection. NEVER org-wide, never for other members, never for other connections.
6. The trust edits are completed, but only claims that are true on the code and the live stack. Sentences about ZDR, rerank and contextual retrieval are re-verified after the build.

## Defaults chosen for the questions the owner has not answered (change only on a new owner ruling)

- ZDR attestation (D-A): no model provider is attested as ZDR-capable today. This is a commercial/operator step. The org ZDR switch is gated on an attested provider (design task Z-04), with an operator-only seed for a test org. Build everything so the attestation is data, not code.
- Derived jobs on a waived run (D-B): on a run whose ZDR was waived for an own-connection, in an org that has ZDR on, content-derived background jobs that would send run content to OTHER processors (memory indexing, dreaming extraction, learning review) are SKIPPED. The consent text names this. The run itself is persisted like a normal run.
- Quarry's other Model Plane dependents (D-C): autoprompt, answer synthesis, ai-formats and page-title cannot mint a model-plane token today (403). Until a proper org-bound, metered design exists, their defaults are switched off in config so they stop failing; the proper design is a scheduled task (Quarry T17), not a stopgap that is forgotten.
- Rerank capacity (D-D): a shared Foundry deployment with a limiter now; dedicated capacity before general availability.
- Data Plane background-LLM budget (D-E): configurable per org and global; default is alert-only (no stop). The owner supplies the figure later.

## Standing rules

- Live tests run on gpt-5.6-terra via openai-codex-subscription, never on Verevon Balance. Paid reranking, embedding and contextualisation calls are intrinsic to those features: keep live proof volumes tiny and say how many calls were made.
- Never commit, stash, reset or revert. The tree is shared with other sessions and agents. Edit hunks in place, re-read before each edit, keep line endings (CRLF for .rs/.go/.md and Python; LF for .ts/.tsx/.js/.json/.yml/.sh).
- Never build images or restart containers from an agent: the orchestrator deploys through the launcher stage copies.
- Never print secrets. If a command might print one, filter it. Report any exposure.
- Contract tests must apply the REAL rules (real policy file, real middleware, real session-core credential checks), not permissive doubles. The A6 proof found six defects that stubs had hidden.
