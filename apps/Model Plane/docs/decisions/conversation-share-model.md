# ADR: conversations are shared as org-only immutable snapshots

**Status:** Proposed — 2026-11-03. Not accepted: the owner decides it as OD-5 (DEC-40) in `docs/plans/wave3/EP-12-sharing-export-and-agent-api.md`, section 7. Until then the snapshot store (EP-12 S14) is built dark behind `SESSION_THREAD_SHARES_ENABLED` and `MODEL_GATEWAY_THREAD_SHARES`, both off.  
**Decision owner:** Model Plane (session-core owns the snapshot store, model-gateway its routes). Control Plane (user-core) owns the grant and the resource type. Frontend Plane owns the viewer.

## Context

A thread lives in session-core and belongs to one person. Today it can be forked by its owner, and EP-12 S9 adds an owner-only export. Nothing lets a colleague read a conversation.

Documents are already shared through Control's `resource_grants`, the single grant authority: the owning plane proves the actor owns the resource, then Control writes the grant (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/shares.rs`; owner-asserted mode in `apps/Control Plane/user-core/internal/authztaxonomy/owner_service.go`). A grant can only name a resource type that is `ownable` in `apps/Control Plane/user-core/internal/authztaxonomy/resource_taxonomy.json`. `conversation` is `team_shared` there, and a type must sit in exactly one set.

A thread is not a fixed record. New turns arrive, runs call connectors as the owner, and widgets and tools replay against live authority. Some runs are ZDR or waived, so their content was never persisted, and EP-12 D8 already refuses to export a thread with any non-persistent run, a Space thread, or a ZDR caller.

Three shapes were weighed (EP-12 D10):

- (a) an immutable snapshot copied into session-core, granted per person through Control on a new ownable type;
- (b) a live link to the thread;
- (c) a copy of the conversation as a Data Plane document.

## Decision

1. **A snapshot, not a live link.** Sharing copies the thread's active branch at one moment into session-core (`thread_share_snapshots`, built in S14), using the S9 export read with the `redacted` profile. Later turns never appear in a snapshot. A live link (b) would let a viewer read turns written after the share and act through the owner's connectors.
2. **Org-only.** A snapshot can be granted only to active members of the owner's organisation. There are no public links, no cross-org grants and no Space subjects in Wave 3. Reaching beyond the organisation needs a new ADR and reopens `docs/architecture/adr-user-content-origin.md`.
3. **One grant authority.** Grants live in Control's `resource_grants` on a new ownable type `conversation_snapshot`, resource id = the snapshot id. `conversation` stays `team_shared`; the snapshot is a distinct type so each type sits in exactly one set. session-core proves ownership and becomes the one owner service for the type in `owner_service.go` (S14); model-gateway never holds an `authz:*` scope. Role is `view` only.
4. **The viewer sees static text.** The snapshot body is the redacted text: turns, run summaries and persisted artifact text. Tool arguments and results, connector payloads, citation query strings and attachment names are dropped (EP-12 D7). Reading a snapshot calls no model, no connector and no tool, replays no widget or app HTML, and runs nothing as the owner or as the viewer.
5. **Snapshots never feed a model.** A snapshot is not indexed, not retrievable by memory or search, not attachable as run context and not copied into Data Plane. Option (c) is rejected because it moves chat state across planes into retrieval.
6. **Refused for non-exportable threads.** Creating a snapshot follows the S9 rules in the same transaction as the read: refused for a ZDR or waived caller (`zdr_feature_not_supported`, feature `conversation_share`), for a thread with any run whose `retention_class` is not `persistent` (`thread_not_exportable`, `details.reason = "non_persistent_run"`), for a Space thread (`details.reason = "space_thread"`), and for a caller who does not own the thread (not found).
7. **Revocation deletes the body.** The owner can revoke a snapshot. Revoking sets `revoked_at`, deletes the body in the same statement, and removes its grants in Control. A revoked snapshot reads as not found to everyone.
8. **Retention follows the source thread.** A snapshot is erased with its thread (`ON DELETE CASCADE`) and with the organisation purge (`DELETE FROM threads WHERE org_id = $1` in session-core's GDPR path). It never outlives the conversation it copies.

## Consequences

- A shared conversation is frozen: the viewer must ask the owner for a new snapshot to see later turns.
- Widgets, MCP App HTML and tool output do not appear in a shared conversation; the viewer sees their text placeholders only.
- A thread with one ZDR or waived turn cannot be shared at all, so a shared transcript is never silently incomplete.
- Adding `conversation_snapshot` to the ownable set is inert on its own: the type has no owner service and no Space grant, and no plane reads its grants until S14 ships and is switched on.
- The ZDR posture matrix gains `e_zdr_caller_share_refused`, `e_waived_thread_snapshot_refused` and `e_snapshot_erased_with_thread` in S14, and `docs/ZDR.md` gains a row for the snapshot body store.

## Open points for S14

- Whether Control exposes a grant-check decision for a non-document type that session-core can verify on read. If it does not, S14 ships owner-only reads and records the gap in the depth register.
- Grant clean-up when a grantee leaves the organisation: membership is checked at read time, so a former member is refused even before the grant row is removed.

## Reopen triggers

1. Sharing outside the organisation (public links, cross-org grants, Space subjects).
2. A need to share live content: later turns, widgets, app HTML or tool replay.
3. A request to make shared conversations searchable or usable as model context.
