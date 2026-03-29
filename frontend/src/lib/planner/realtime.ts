import type { Doc as YDoc } from 'yjs';
import { WebsocketProvider } from 'y-websocket';
import { buildPlannerSyncRoom, PLANNER_SYNC_WS_URL } from './sync-config';

export interface PlannerRealtimeConnection {
  provider: WebsocketProvider;
  room: string;
  destroy: () => void;
}

function createPresenceLabel() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `Editor ${crypto.randomUUID().slice(0, 4)}`;
  }

  return `Editor ${Math.random().toString(16).slice(2, 6)}`;
}

export function countLiveEditors(provider: WebsocketProvider) {
  return Array.from(provider.awareness.getStates().values()).filter(Boolean).length || 1;
}

export function connectPlannerRealtime(input: {
  documentId: string;
  workspaceId: string;
  yDoc: YDoc;
}) {
  const room = buildPlannerSyncRoom(input.workspaceId, input.documentId);
  const provider = new WebsocketProvider(PLANNER_SYNC_WS_URL, room, input.yDoc, {
    connect: false,
    params: {
      documentId: input.documentId,
      source: 'planner',
      workspaceId: input.workspaceId,
    },
  });

  provider.awareness.setLocalState({
    cursor: null,
    source: 'planner',
    user: {
      name: createPresenceLabel(),
    },
    workspaceId: input.workspaceId,
    documentId: input.documentId,
  });

  return {
    provider,
    room,
    destroy: () => {
      provider.awareness.setLocalState(null);
      provider.destroy();
    },
  } satisfies PlannerRealtimeConnection;
}