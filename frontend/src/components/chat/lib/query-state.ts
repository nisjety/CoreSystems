import type { QueryClient } from '@tanstack/react-query';

import type {
  GetSessionsResponse,
  Session,
  SessionListItem,
} from '@/components/chat/api/orpc/chat';

export const chatQueryKeys = {
  all: ['chat'] as const,
  history: ['chat', 'history'] as const,
  session: (sessionId: string) => ['chat', 'session', sessionId] as const,
  legacyMessages: ['messages'] as const,
};

export function sessionToListItem(session: Session): SessionListItem {
  return {
    id: session.id,
    title: session.title,
    createdAt: session.createdAt,
    updatedAt: session.updatedAt,
    userId: session.userId,
    messageCount: session.messages.length,
  };
}

function sortSessionsByUpdatedAt(sessions: SessionListItem[]) {
  return [...sessions].sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
}

function buildSessionsPayload(sessions: SessionListItem[]): GetSessionsResponse {
  return {
    sessions: sortSessionsByUpdatedAt(sessions),
    totalCount: sessions.length,
  };
}

function mergeSessionListItem(
  sessions: SessionListItem[],
  nextSession: SessionListItem,
): SessionListItem[] {
  const remaining = sessions.filter((session) => session.id !== nextSession.id);
  return [nextSession, ...remaining];
}

export function writeChatHistoryCache(
  queryClient: QueryClient,
  payload: GetSessionsResponse,
) {
  queryClient.setQueryData(chatQueryKeys.history, payload);
  queryClient.setQueryData(chatQueryKeys.legacyMessages, payload);
}

export function upsertChatHistorySession(
  queryClient: QueryClient,
  session: Session | SessionListItem,
) {
  const nextSession =
    'messages' in session ? sessionToListItem(session) : session;

  const updateHistory = (previous: GetSessionsResponse | undefined) => {
    const previousSessions = previous?.sessions ?? [];
    return buildSessionsPayload(mergeSessionListItem(previousSessions, nextSession));
  };

  queryClient.setQueryData<GetSessionsResponse>(chatQueryKeys.history, updateHistory);
  queryClient.setQueryData<GetSessionsResponse>(chatQueryKeys.legacyMessages, updateHistory);
}

export function removeChatHistorySession(queryClient: QueryClient, sessionId: string) {
  const removeFromHistory = (previous: GetSessionsResponse | undefined) => {
    const remaining = (previous?.sessions ?? []).filter((session) => session.id !== sessionId);
    return buildSessionsPayload(remaining);
  };

  queryClient.setQueryData<GetSessionsResponse>(chatQueryKeys.history, removeFromHistory);
  queryClient.setQueryData<GetSessionsResponse>(chatQueryKeys.legacyMessages, removeFromHistory);
  queryClient.removeQueries({ queryKey: chatQueryKeys.session(sessionId) });
}

export function setChatSessionCache(queryClient: QueryClient, session: Session) {
  queryClient.setQueryData(chatQueryKeys.session(session.id), session);
  upsertChatHistorySession(queryClient, session);
}
