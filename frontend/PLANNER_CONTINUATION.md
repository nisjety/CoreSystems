# Planner Continuation

## Current State

- The planner is now an app-owned BlockSuite editor mounted inside the existing frontend.
- Document metadata and serialized editor state are persisted through the planner API and Convex.
- AFFiNE backend connectivity is available through the internal proxy routes, but the planner is not using native AFFiNE collaboration yet.
- The runtime no longer claims fake cloud sync. Current truth is local-first editing with persisted snapshots.

## What Is Done

- BlockSuite editor mount is stable in the app UI.
- Planner document CRUD is wired through the existing Convex-backed API.
- Planner document snapshots save and restore correctly.
- AFFiNE backend runs in the shared Docker environment with service-name routing.
- Dev-mode auth fallbacks keep the planner usable while the missing auth-service is unresolved.

## What This Phase Adds

- Editor parity: the page title inside BlockSuite is now bridged back into planner metadata so the toolbar and sidebar stay aligned with the editor.
- Collaboration parity: the planner now reports truthful same-browser live presence across tabs/windows instead of implying remote AFFiNE collaboration.
- Persistence parity: the toolbar now distinguishes initialization, in-flight saves, saved state, and the last successful save time.

## Remaining Gaps To Full AFFiNE Parity

- Real multi-user collaboration across browsers and users.
- AFFiNE-native workspace membership, permissions, and presence.
- Rich document features beyond the current embedded editor shell.
- Blocks, comments, backlinks, databases, publishing, and workspace management parity.
- WebSocket or protocol-level sync instead of snapshot persistence.

## Recommended Next Milestones

1. Decide the real collaboration backend.
   Use either an owned Yjs sync service or a true AFFiNE workspace/doc integration. The current reverse proxy is HTTP-only and does not provide native collaborative editing.

2. Add remote presence and session identity.
   Extend the current same-browser presence model into authenticated multi-user presence with cursor and selection awareness.

3. Expand document metadata parity.
   Add icons, cover state, timestamps, ownership, and document summaries so the app shell can match editor state more closely.

4. Replace snapshot-only persistence.
   Move from periodic full-document snapshots to incremental sync and recovery semantics.

5. Revisit AFFiNE service reuse.
   If full product parity remains the goal, model planner documents as real AFFiNE workspace docs instead of an app-owned abstraction.

## Architectural Note

Right now the planner should be treated as an app-owned editor surface with AFFiNE-style capabilities, not as a full AFFiNE clone. That distinction matters when choosing the next sync architecture.