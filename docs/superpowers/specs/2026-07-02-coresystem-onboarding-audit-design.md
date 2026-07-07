# CoreSystem Onboarding And Audit Design

Date: 2026-07-02

## Scope

Refresh CoreSystem documentation and produce an audit backlog for the current main projects:

- Frontend Plane: `apps/Frontend Plane/velionv3`
- Data Plane v2
- Ingestion Plane
- Model Plane
- Control Plane
- Application Plane

This pass is documentation and audit backlog only. It must not patch implementation files, generated files, build outputs, or pre-existing dirty worktree changes.

## Goals

- Update the system overview to reflect `velionv3` as the focused frontend target while preserving `velionv2` as historical/reference context where needed.
- Document the current service map, ownership boundaries, entry points, contracts, and common quality gates across the six requested planes.
- Identify bugs, missing implementations, stale docs, incomplete wiring, boundary risks, and test gaps with file-level evidence.
- Produce a prioritized backlog with confidence levels and recommended next actions.

## Approach

Use an evidence-first workflow:

1. Read the root orientation docs and plane docs before running broad scans.
2. Use CodeGraph for structure and symbol-aware context.
3. Inspect manifests, entry points, docs, and non-mutating command outputs for each plane.
4. Run focused quality checks only where practical and record failures without mutating code.
5. Update shared docs and add a consolidated audit backlog.

## Outputs

- Updated root orientation docs: `AGENTS.md` and `CLAUDE.md`.
- Updated cross-plane overview: `apps/CODEBASE_INFORMATION_SYSTEM.md`.
- New or updated consolidated backlog: `apps/CORESYSTEM_AUDIT_BACKLOG.md`.
- If plane-specific doc drift is clearly isolated and low-risk, update those docs as well; otherwise link the backlog item to the stale source.

## Guardrails

- Do not reset, clean, revert, or overwrite user changes.
- Do not edit implementation code during this pass.
- Do not suppress quality tools by changing linter, formatter, or TypeScript config.
- Mark findings as `Confirmed`, `Likely`, or `Needs verification`.
- Keep plane ownership boundaries explicit: Control owns identity/governance, Data owns durable knowledge, Ingestion owns evidence capture, Model owns reasoning/execution, Application owns realtime workspace projections, and Frontend owns UI/gateway normalization.

## Verification

- Self-review docs for stale `velionv2` focus, contradictions, placeholders, and missing owner-plane links.
- Record commands run, command failures, and skipped checks in the backlog.
- Leave remediation planning for a follow-up pass after the user reviews the documentation and backlog.
