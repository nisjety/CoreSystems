'use client';

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
} from 'react';
import { usePathname } from 'next/navigation';
import { useQuery, useConvexAuth } from 'convex/react';

import {
  chatApiClient,
  type ChatActor,
  type ErrorResponse,
  type Message,
  type Session,
  type SessionListItem,
} from '@/components/chat/api/orpc/chat';
import { useI18n } from '@/components/chat/hooks/i18n';
import { useAuth } from '@/components/auth/hooks/use-auth';
import { api } from '@/lib/convex-api-stub';
import { useChatProviderState } from './use-chat-provider-state';

export interface SendOptions {
  model?: string
  responseMode?: 'auto' | 'quick' | 'deep'
  browseWeb?: boolean
  attachmentUrls?: string[]
}

interface ConvexMessageRecord {
  _id: string;
  clientId?: string;
  content: string;
  role: 'user' | 'assistant' | 'system';
  createdAt: number;
  isStreaming?: boolean;
  metadata?: {
    source?: string;
    citations?: string[];
    error?: string;
  };
}

interface ConvexConversationRecord {
  _id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messageCount?: number;
  messages?: ConvexMessageRecord[];
  userId: string;
  orgId: string;
}

interface ChatState {
  sessions: SessionListItem[];
  currentSession: Session | null;
  messages: Message[];
  isLoading: boolean;
  isTyping: boolean;
  error: string | null;
  connected: boolean;
}

interface ChatContextType {
  state: ChatState;
  loadSessions: () => Promise<void>;
  createNewSession: (title?: string) => Promise<Session | null>;
  selectSession: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;
  updateSessionTitle: (sessionId: string, title: string) => Promise<void>;
  sendMessage: (content: string, sessionId?: string, options?: SendOptions) => Promise<void>;
  streamMessage: (content: string, sessionId?: string, options?: SendOptions) => Promise<void>;
  regenerateLastMessage: () => Promise<void>;
  clearError: () => void;
  startNewChat: () => void;
  resetChat: () => void;
}

const initialState: ChatState = {
  sessions: [],
  currentSession: null,
  messages: [],
  isLoading: true,
  isTyping: false,
  error: null,
  connected: false,
};

const ChatContext = createContext<ChatContextType | undefined>(undefined);

function toChatMessage(sessionId: string, message: ConvexMessageRecord): Message {
  const metadata =
    message.metadata?.source === 'reasoning-plane'
      ? {
          source: 'reasoning-plane' as const,
          citations: message.metadata.citations,
        }
      : undefined;

  return {
    id: message._id,
    clientId: message.clientId,
    content: message.content,
    role: message.role,
    timestamp: new Date(message.createdAt).toISOString(),
    sessionId,
    isThinking: message.isStreaming ?? false,
    error: message.metadata?.error,
    metadata,
  };
}

function toSessionListItem(
  conversation: ConvexConversationRecord,
  actor: ChatActor,
): SessionListItem {
  return {
    id: conversation._id,
    title: conversation.title,
    createdAt: new Date(conversation.createdAt).toISOString(),
    updatedAt: new Date(conversation.updatedAt).toISOString(),
    messageCount: conversation.messageCount ?? conversation.messages?.length ?? 0,
    userId: actor.userId,
    orgId: actor.orgId,
  };
}

function toSession(
  conversation: ConvexConversationRecord | null | undefined,
  actor: ChatActor | null,
): Session | null {
  if (!conversation || !actor) {
    return null;
  }

  const messages = (conversation.messages ?? []).map((message) =>
    toChatMessage(conversation._id, message),
  );

  return {
    id: conversation._id,
    title: conversation.title,
    createdAt: new Date(conversation.createdAt).toISOString(),
    updatedAt: new Date(conversation.updatedAt).toISOString(),
    userId: actor.userId,
    orgId: actor.orgId,
    messages,
  };
}

async function drainMessageStream(content: string, sessionId: string, clientId: string, options?: SendOptions) {
  const stream = await chatApiClient.streamMessage({ content, sessionId, clientId, ...options });
  const reader = stream.getReader();

  try {
    while (true) {
      const { done } = await reader.read();
      if (done) {
        break;
      }
    }
  } finally {
    reader.releaseLock();
  }
}

export function ChatProvider({ children }: { children: React.ReactNode }) {
  const { t } = useI18n();
  const { user, isLoading: authLoading } = useAuth();
  const { isAuthenticated: convexAuthenticated } = useConvexAuth();
  const pathname = usePathname();

  const [providerState, dispatch] = useChatProviderState();
  const { actor, actorLoading, activeSessionId, pendingRequests, error, optimisticMessages } =
    providerState;

  const handleError = useCallback((nextError: unknown) => {
    console.error('Chat error:', nextError);

    if (nextError instanceof Error) {
      dispatch({ type: 'SET_ERROR', payload: nextError.message });
      return;
    }

    if (typeof nextError === 'object' && nextError !== null && 'message' in nextError) {
      dispatch({ type: 'SET_ERROR', payload: (nextError as ErrorResponse).message });
      return;
    }

    dispatch({ type: 'SET_ERROR', payload: t('error.generic') });
  }, [t, dispatch]);

  const actorRetryRef = useRef(0);
  const MAX_ACTOR_RETRIES = 2;
  const shouldAutoLoadActor = pathname === '/chat' || pathname.startsWith('/chat/');

  const loadActor = useCallback(async () => {
    if (!user) {
      dispatch({ type: 'ACTOR_CLEAR' });
      actorRetryRef.current = 0;
      return;
    }

    // Stop retrying after MAX_ACTOR_RETRIES to prevent infinite 500 loop
    if (actorRetryRef.current >= MAX_ACTOR_RETRIES) {
      return;
    }

    try {
      dispatch({ type: 'ACTOR_LOAD_START' });
      const nextActor = await chatApiClient.getActor();
      dispatch({ type: 'ACTOR_LOAD_SUCCESS', payload: nextActor });
      actorRetryRef.current = 0;
    } catch (nextError) {
      actorRetryRef.current += 1;
      dispatch({ type: 'ACTOR_LOAD_FAILURE', payload: nextError instanceof Error ? nextError.message : t('error.generic') });
    }
  }, [dispatch, user, t]);

  useEffect(() => {
    if (authLoading) {
      return;
    }

    if (!shouldAutoLoadActor) {
      dispatch({ type: 'SET_ACTOR_LOADING', payload: false });
      dispatch({ type: 'SET_ERROR', payload: null });
      return;
    }

    void loadActor();
  }, [authLoading, dispatch, loadActor, shouldAutoLoadActor, user?.id]);

  const convexSessions = useQuery(
    api.conversations.listForCurrentUser,
    actor && convexAuthenticated
      ? {
          externalOrgId: actor.orgId,
        }
      : 'skip',
  ) as ConvexConversationRecord[] | undefined;

  const convexCurrentSession = useQuery(
    api.conversations.getForCurrentUser,
    actor && convexAuthenticated && activeSessionId
      ? {
          conversationId: activeSessionId,
          externalOrgId: actor.orgId,
        }
      : 'skip',
  ) as (ConvexConversationRecord | null) | undefined;

  // Last-good cache for both queries. Convex `useQuery` returns
  // `undefined` while reconnecting the WebSocket — without this, the
  // chat blanks out for the duration of every reconnect and re-renders
  // as if loading. We keep the previous payload visible so the user
  // sees a stable view while the socket reattaches.
  const lastSessionsRef = useRef<ConvexConversationRecord[] | undefined>(undefined);
  const lastCurrentSessionRef = useRef<ConvexConversationRecord | null | undefined>(undefined);
  // Per-sessionId stash so switching conversations doesn't display the
  // previous one's messages during the new query's first load.
  const lastBySessionIdRef = useRef<Map<string, ConvexConversationRecord | null>>(new Map());

  if (convexSessions !== undefined) {
    lastSessionsRef.current = convexSessions;
  }
  if (convexCurrentSession !== undefined) {
    lastCurrentSessionRef.current = convexCurrentSession;
    if (activeSessionId && convexCurrentSession) {
      lastBySessionIdRef.current.set(activeSessionId, convexCurrentSession);
    }
  }

  const stableSessions = convexSessions ?? lastSessionsRef.current;
  // Prefer the per-session stash when the active session matches a
  // previously-loaded conversation; otherwise fall back to the most
  // recent value. This avoids flashing "old conversation" content when
  // the user switches.
  const stableCurrentSession =
    convexCurrentSession ??
    (activeSessionId ? lastBySessionIdRef.current.get(activeSessionId) ?? null : undefined) ??
    lastCurrentSessionRef.current;

  const sessions = useMemo(() => {
    if (!actor || !stableSessions) {
      return [];
    }

    return stableSessions.map((conversation) => toSessionListItem(conversation, actor));
  }, [actor, stableSessions]);

  const currentSession = useMemo(() => {
    return toSession(stableCurrentSession ?? null, actor);
  }, [actor, stableCurrentSession]);

  // Merge Convex-confirmed messages with optimistic user bubbles. An
  // optimistic entry is hidden once a real user message with matching
  // content appears in Convex (typically <300ms post-submit), giving
  // ChatGPT-style instant feedback without ever rendering duplicates.
  const messages = useMemo(() => {
    const base = currentSession?.messages ?? [];
    if (!activeSessionId) {
      return base;
    }
    const relevantOptimistic = optimisticMessages.filter(
      (om) => om.sessionId === activeSessionId,
    );
    if (relevantOptimistic.length === 0) {
      return base;
    }
    const confirmedUserClientIds = new Set(
      base.filter((m) => m.role === 'user' && m.clientId).map((m) => m.clientId),
    );
    const confirmedUserContents = new Set(
      base.filter((m) => m.role === 'user').map((m) => m.content),
    );
    const stillPending = relevantOptimistic.filter(
      (om) => !confirmedUserClientIds.has(om.clientId) && !confirmedUserContents.has(om.content),
    );
    if (stillPending.length === 0) {
      return base;
    }
    const synthesized = stillPending.map((om) => ({
      id: `optimistic-${om.clientId}`,
      role: 'user' as const,
      content: om.content,
      timestamp: new Date(om.createdAt).toISOString(),
      sessionId: om.sessionId,
    }));
    return [...base, ...synthesized];
  }, [currentSession, activeSessionId, optimisticMessages]);

  const hasStreamingAssistant = messages.some(
    (message) => message.role === 'assistant' && message.isThinking,
  );

  // Drop optimistic entries once Convex catches up. Runs whenever the
  // confirmed message list changes; cheap because the filter is O(n)
  // on a typically-tiny optimistic queue.
  useEffect(() => {
    if (optimisticMessages.length === 0) return;
    const base = currentSession?.messages ?? [];
    const confirmedUserClientIds = new Set(
      base.filter((m) => m.role === 'user' && m.clientId).map((m) => m.clientId),
    );
    const confirmedUserContents = new Set(
      base.filter((m) => m.role === 'user').map((m) => m.content),
    );
    for (const om of optimisticMessages) {
      if (confirmedUserClientIds.has(om.clientId) || confirmedUserContents.has(om.content)) {
        dispatch({ type: 'OPTIMISTIC_DROP', payload: { clientId: om.clientId } });
      }
    }
  }, [currentSession, optimisticMessages, dispatch]);

  const state = useMemo<ChatState>(() => {
    // Loading is only true on the INITIAL fetch — once we have data
    // (or a cached last-good value), a transient `undefined` from a
    // WebSocket reconnect must not flip us back into a loading state.
    // Without this, every Convex reconnect flickered the chat between
    // the rendered conversation and an empty/loading shell.
    const hasEverLoaded = stableCurrentSession !== undefined;
    const isSessionLoading =
      Boolean(actor && activeSessionId) && !hasEverLoaded;

    return {
      sessions,
      currentSession,
      messages,
      isLoading: authLoading || actorLoading || isSessionLoading,
      isTyping: pendingRequests > 0 || hasStreamingAssistant,
      error,
      connected: Boolean(actor),
    };
  }, [
    activeSessionId,
    actor,
    actorLoading,
    authLoading,
    stableCurrentSession,
    error,
    hasStreamingAssistant,
    messages,
    pendingRequests,
    sessions,
    currentSession,
  ]);

  const loadSessions = useCallback(async () => {
    await loadActor();
  }, [loadActor]);

  const clearError = useCallback(() => {
    dispatch({ type: 'SET_ERROR', payload: null });
  }, [dispatch]);

  const createNewSession = useCallback(async (title?: string): Promise<Session | null> => {
    try {
      dispatch({ type: 'SET_ERROR', payload: null });
      const session = await chatApiClient.createSession(title);
      dispatch({ type: 'SET_ACTIVE_SESSION', payload: session.id });
      return session;
    } catch (nextError) {
      handleError(nextError);
      return null;
    }
  }, [handleError, dispatch]);

  const selectSession = useCallback(async (sessionId: string) => {
    dispatch({ type: 'SET_ERROR', payload: null });
    dispatch({ type: 'SET_ACTIVE_SESSION', payload: sessionId });
  }, [dispatch]);

  const deleteSession = useCallback(async (sessionId: string) => {
    try {
      dispatch({ type: 'SET_ERROR', payload: null });
      await chatApiClient.deleteSession(sessionId);
      if (activeSessionId === sessionId) {
        dispatch({ type: 'SET_ACTIVE_SESSION', payload: null });
      }
    } catch (nextError) {
      handleError(nextError);
    }
  }, [activeSessionId, handleError, dispatch]);

  const updateSessionTitle = useCallback(async (sessionId: string, title: string) => {
    try {
      dispatch({ type: 'SET_ERROR', payload: null });
      await chatApiClient.updateSessionTitle(sessionId, title);
    } catch (nextError) {
      handleError(nextError);
    }
  }, [handleError, dispatch]);

  const ensureSession = useCallback(async (content: string, sessionId?: string) => {
    const nextSessionId = sessionId ?? activeSessionId;
    if (nextSessionId) {
      return nextSessionId;
    }

    const title = content.length > 60 ? `${content.slice(0, 57)}...` : content;
    const created = await chatApiClient.createSession(title);
    dispatch({ type: 'SET_ACTIVE_SESSION', payload: created.id });
    return created.id;
  }, [activeSessionId, dispatch]);

  const streamMessage = useCallback(async (content: string, sessionId?: string, options?: SendOptions) => {
    const normalizedContent = content.trim();
    if (!normalizedContent) {
      return;
    }

    const clientId =
      typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.random().toString(36).slice(2)}`;

    try {
      dispatch({ type: 'SET_ERROR', payload: null });
      const nextSessionId = await ensureSession(normalizedContent, sessionId);
      dispatch({ type: 'SET_ACTIVE_SESSION', payload: nextSessionId });
      // Optimistic user bubble — visible immediately, reconciled when
      // Convex confirms the persisted message.
      dispatch({
        type: 'OPTIMISTIC_ADD',
        payload: {
          clientId,
          sessionId: nextSessionId,
          content: normalizedContent,
          createdAt: Date.now(),
        },
      });
      dispatch({ type: 'INCREMENT_PENDING' });
      await drainMessageStream(normalizedContent, nextSessionId, clientId, options);
    } catch (nextError) {
      // On failure, drop the optimistic bubble so the user doesn't see
      // a ghost message lingering after an error toast.
      dispatch({ type: 'OPTIMISTIC_DROP', payload: { clientId } });
      handleError(nextError);
    } finally {
      dispatch({ type: 'DECREMENT_PENDING' });
    }
  }, [ensureSession, handleError, dispatch]);

  const sendMessage = useCallback(async (content: string, sessionId?: string, options?: SendOptions) => {
    await streamMessage(content, sessionId, options);
  }, [streamMessage]);

  const regenerateLastMessage = useCallback(async () => {
    const lastUserMessage = [...messages].reverse().find((message) => message.role === 'user');
    if (!lastUserMessage) {
      return;
    }

    await streamMessage(lastUserMessage.content, lastUserMessage.sessionId);
  }, [messages, streamMessage]);

  const startNewChat = useCallback(() => {
    dispatch({ type: 'SET_ERROR', payload: null });
    dispatch({ type: 'SET_ACTIVE_SESSION', payload: null });
  }, [dispatch]);

  const resetChat = useCallback(() => {
    dispatch({ type: 'RESET' });
  }, [dispatch]);

  const contextValue = useMemo<ChatContextType>(() => ({
    state,
    loadSessions,
    createNewSession,
    selectSession,
    deleteSession,
    updateSessionTitle,
    sendMessage,
    streamMessage,
    regenerateLastMessage,
    clearError,
    startNewChat,
    resetChat,
  }), [
    clearError,
    createNewSession,
    deleteSession,
    loadSessions,
    regenerateLastMessage,
    resetChat,
    selectSession,
    sendMessage,
    startNewChat,
    state,
    streamMessage,
    updateSessionTitle,
  ]);

  return <ChatContext.Provider value={contextValue}>{children}</ChatContext.Provider>;
}

export function useChat() {
  const context = useContext(ChatContext);
  if (context === undefined) {
    throw new Error('useChat must be used within a ChatProvider');
  }
  return context;
}

/** Returns null when called outside a ChatProvider — safe for optional usage. */
export function useChatSafe() {
  return useContext(ChatContext) ?? null;
}
