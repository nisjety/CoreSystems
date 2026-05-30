Novu backend-only first if the goal is to keep Velion as the main frontend pattern.# AFFiNE Core Deferred Plan

## Decision

`affine-core` is deferred from the primary V1 scope.

The current live `affine-core/` service remains in place because it is already
wired into the Application Plane and still participates in planner workspace
resolution. We are not reclaiming that directory name for a README-only stub at
this stage.

## Why It Is Deferred

The immediate priority is competitive parity with Chatbase and Intercom on the
core AI agent operating loop:

1. Train agents on business knowledge, guidance, and policies.
2. Test agents with playground flows, previews, and regression-style checks.
3. Deploy agents across first-party channels and API surfaces.
4. Manage integrations, actions, and authenticated external access.
5. Analyze runs, failures, and optimization opportunities.
6. Support human handoff, notifications, and operator workflows.

An AFFiNE-powered workspace layer is still valuable, but it is a later-phase
product layer rather than a blocker for reaching competitive baseline parity.

## Current Constraint

The existing live planner stack still assumes a running `affine-core` boundary.
That means AFFiNE cannot be reduced to a placeholder folder yet without first
moving or removing current planner dependencies.

Known dependency areas include:

1. Application Plane compose wiring.
2. Planner sync workspace resolution.
3. AFFiNE runtime boundary and session exchange behavior.

## V1 Focus Instead

V1 should focus on the surfaces that most directly match competitor expectations:

### Agent Platform

1. Agent configuration and environment management.
2. Knowledge ingestion and source management.
3. Guidance, policy, and procedure controls.
4. Action and integration management.
5. Playground and preview workflows.
6. Observability, analytics, and audit trails.

### Support And Operations

1. Human handoff patterns.
2. Notification delivery and status visibility.
3. Internal admin workflows.
4. Content governance for what the agent can use.

## Future Role For AFFiNE

When reintroduced, AFFiNE should be treated as an agent workspace layer, not as
the main chat playground and not as the canonical knowledge plane.

Target responsibilities for the future AFFiNE-backed product layer:

1. Agent briefs and operating notes.
2. Playbooks and investigation documents.
3. Workflow maps and decision logs.
4. Collaborative planning artifacts around agent behavior.
5. Draft-to-published flows into the canonical Data Plane.

## Re-Entry Criteria

AFFiNE should come back into active scope only after the following are true:

1. Core agent parity surfaces are functional end to end.
2. The planner boundary can be renamed or reshaped safely without breaking live
   dependencies.
3. There is a clear product distinction between:
   - Playground
   - Knowledgebase
   - Integrations
   - Notifications
   - Agent Workspace

## Naming Follow-Up

If we later want `affine-core/` to become a clean, plan-only placeholder or a
new implementation root, the safe sequence is:

1. Rename the current live planner boundary to a truth-based service name.
2. Update compose, env, and runtime references.
3. Remove hard dependencies on the old `affine-core` service name.
4. Reclaim `affine-core/` only after the stack is no longer using it as a live
   runtime boundary.