export const PLANNER_TRANSPORT_WS_URL =
  process.env.NEXT_PUBLIC_PLANNER_TRANSPORT_WS_URL ??
  process.env.NEXT_PUBLIC_PLANNER_SYNC_WS_URL ??
  'ws://localhost:47813';

export function buildPlannerSyncRoom(workspaceId: string, documentId: string) {
  return `${workspaceId}::${documentId}`;
}
