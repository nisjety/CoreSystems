## WS23-S6.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after line 233 (the 'integrations.extend_inbox_history' row) add two rows at 2-space indent: 'integrations.browse_connection': exempt('no_owner_executor', 'read'), and 'integrations.fetch_connection_content': exempt('no_owner_executor', 'read'),. After line 260 (the 'chat.extract_document' row) add 'chat.set_connector_scope': exempt('agent_cannot_self_govern', 'context_write'),. Both reasons and both classes are valid union members, and the placement follows registry order.

## WS23-S6.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts: (a) after line 3006, the streamChat request inside sendContent, after 'retrievalScope: options.retrievalScope ?? retrievalScope(),', add 'connectorScope: options.connectorScope,' at 10-space indent. (b) after line 3363, inside handleComposerSubmit's sendContent options, after 'retrievalScope: payload.retrievalScope,', add 'connectorScope: payload.connectorScope,' at 6-space indent. Leave lines 2768 and 2798 unset: they are transcript records, not the wire. The SendOptions field already exists in chat-types.ts (done in this round).

## WS23-S6.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/api/contracts/agent-action-admission.json (canonical golden): copy the regenerated SPA file over it. The two are byte-identical today. Then rewrite C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/api/contracts/agent-action-admission.json.sha256 as '<new sha256> *agent-action-admission.json' (sha256sum -b, the existing 94-byte format).

## WS23-S6.code
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/scripts/tests/zdr-contract-sync.test.sh line 157: replace digest 8eb89148f5bdfb9e914bc7d9ad7f307659b033009c94df617dae2123d277335f with the new digest. This is the only other tracked pin of the golden.

## WS23-S6.code
shared-edit the three vendored copies, each byte-identical to the golden today: C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/go/services/capability-core/internal/api/testdata/agent-action-admission.json, C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/execution-core/tests/fixtures/agent-action-admission.json and C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/model-gateway/tests/fixtures/agent-action-admission.json. Copy the new golden over each.

## WS19-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, beside SKILL_FILE_TOOL_ENABLED, add SKILL_BUNDLE_MOUNT_ENABLED: ${SKILL_BUNDLE_MOUNT_ENABLED:-false}. Not applied here because it was not in the fix list. An unset value already means off.

