'use client';

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
} from 'react';
import { usePathname } from 'next/navigation';

export const NEW_CHAT_DRAFT_KEY = '__new__';

type ChatDraftKey = typeof NEW_CHAT_DRAFT_KEY | string;
type ChatLaunchSource = 'dashboard';

interface PendingChatLaunch {
  id: string;
  draftKey: ChatDraftKey;
  message: string;
  source: ChatLaunchSource;
}

interface ChatWorkspaceState {
  drafts: Record<string, string>;
  pendingLaunch: PendingChatLaunch | null;
  isDashboardLaunching: boolean;
}

type ChatWorkspaceAction =
  | { type: 'SET_DRAFT'; payload: { draftKey: ChatDraftKey; message: string } }
  | { type: 'CLEAR_DRAFT'; payload: { draftKey: ChatDraftKey } }
  | { type: 'TRANSFER_DRAFT'; payload: { fromKey: ChatDraftKey; toKey: ChatDraftKey } }
  | { type: 'QUEUE_DASHBOARD_LAUNCH'; payload: PendingChatLaunch }
  | { type: 'CONSUME_PENDING_LAUNCH'; payload: { id: string } }
  | { type: 'SET_DASHBOARD_LAUNCHING'; payload: boolean }
  | { type: 'RESET_NEW_CHAT' };

interface ChatWorkspaceContextValue {
  pendingLaunch: PendingChatLaunch | null;
  isDashboardLaunching: boolean;
  getDraft: (draftKey?: ChatDraftKey) => string;
  setDraft: (draftKey: ChatDraftKey, message: string) => void;
  clearDraft: (draftKey?: ChatDraftKey) => void;
  transferDraft: (fromKey: ChatDraftKey, toKey: ChatDraftKey) => void;
  queueDashboardLaunch: (message: string) => PendingChatLaunch | null;
  consumePendingLaunch: (pendingLaunchId: string) => void;
  resetNewChatDraft: () => void;
}

const initialState: ChatWorkspaceState = {
  drafts: {},
  pendingLaunch: null,
  isDashboardLaunching: false,
};

function createPendingLaunchId() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }

  return `chat-launch-${Date.now()}`;
}

function chatWorkspaceReducer(
  state: ChatWorkspaceState,
  action: ChatWorkspaceAction,
): ChatWorkspaceState {
  switch (action.type) {
    case 'SET_DRAFT': {
      const { draftKey, message } = action.payload;
      return {
        ...state,
        drafts: {
          ...state.drafts,
          [draftKey]: message,
        },
      };
    }

    case 'CLEAR_DRAFT': {
      const { draftKey } = action.payload;
      const nextDrafts = { ...state.drafts };
      delete nextDrafts[draftKey];

      return {
        ...state,
        drafts: nextDrafts,
      };
    }

    case 'TRANSFER_DRAFT': {
      const { fromKey, toKey } = action.payload;
      const sourceDraft = state.drafts[fromKey];

      if (!sourceDraft || fromKey === toKey) {
        return state;
      }

      const nextDrafts = { ...state.drafts };
      delete nextDrafts[fromKey];
      nextDrafts[toKey] = sourceDraft;

      return {
        ...state,
        drafts: nextDrafts,
      };
    }

    case 'QUEUE_DASHBOARD_LAUNCH':
      return {
        ...state,
        drafts: {
          ...state.drafts,
          [action.payload.draftKey]: action.payload.message,
        },
        pendingLaunch: action.payload,
        isDashboardLaunching: true,
      };

    case 'CONSUME_PENDING_LAUNCH':
      if (state.pendingLaunch?.id !== action.payload.id) {
        return state;
      }

      return {
        ...state,
        pendingLaunch: null,
      };

    case 'SET_DASHBOARD_LAUNCHING':
      return {
        ...state,
        isDashboardLaunching: action.payload,
      };

    case 'RESET_NEW_CHAT': {
      const nextDrafts = { ...state.drafts };
      delete nextDrafts[NEW_CHAT_DRAFT_KEY];

      return {
        ...state,
        drafts: nextDrafts,
        pendingLaunch: null,
        isDashboardLaunching: false,
      };
    }

    default:
      return state;
  }
}

const ChatWorkspaceContext = createContext<ChatWorkspaceContextValue | undefined>(undefined);

export function ChatWorkspaceProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  const [state, dispatch] = useReducer(chatWorkspaceReducer, initialState);

  useEffect(() => {
    if (pathname !== '/dashboard' && state.isDashboardLaunching) {
      dispatch({ type: 'SET_DASHBOARD_LAUNCHING', payload: false });
    }
  }, [pathname, state.isDashboardLaunching]);

  const getDraft = useCallback(
    (draftKey: ChatDraftKey = NEW_CHAT_DRAFT_KEY) => state.drafts[draftKey] ?? '',
    [state.drafts],
  );

  const setDraft = useCallback((draftKey: ChatDraftKey, message: string) => {
    dispatch({ type: 'SET_DRAFT', payload: { draftKey, message } });
  }, []);

  const clearDraft = useCallback((draftKey: ChatDraftKey = NEW_CHAT_DRAFT_KEY) => {
    dispatch({ type: 'CLEAR_DRAFT', payload: { draftKey } });
  }, []);

  const transferDraft = useCallback((fromKey: ChatDraftKey, toKey: ChatDraftKey) => {
    dispatch({ type: 'TRANSFER_DRAFT', payload: { fromKey, toKey } });
  }, []);

  const queueDashboardLaunch = useCallback((message: string) => {
    const normalized = message.trim();
    if (!normalized) {
      return null;
    }

    const pendingLaunch: PendingChatLaunch = {
      id: createPendingLaunchId(),
      draftKey: NEW_CHAT_DRAFT_KEY,
      message: normalized,
      source: 'dashboard',
    };

    dispatch({ type: 'QUEUE_DASHBOARD_LAUNCH', payload: pendingLaunch });
    return pendingLaunch;
  }, []);

  const consumePendingLaunch = useCallback((pendingLaunchId: string) => {
    dispatch({ type: 'CONSUME_PENDING_LAUNCH', payload: { id: pendingLaunchId } });
  }, []);

  const resetNewChatDraft = useCallback(() => {
    dispatch({ type: 'RESET_NEW_CHAT' });
  }, []);

  const value = useMemo<ChatWorkspaceContextValue>(
    () => ({
      pendingLaunch: state.pendingLaunch,
      isDashboardLaunching: state.isDashboardLaunching,
      getDraft,
      setDraft,
      clearDraft,
      transferDraft,
      queueDashboardLaunch,
      consumePendingLaunch,
      resetNewChatDraft,
    }),
    [
      clearDraft,
      consumePendingLaunch,
      getDraft,
      queueDashboardLaunch,
      resetNewChatDraft,
      setDraft,
      state.isDashboardLaunching,
      state.pendingLaunch,
      transferDraft,
    ],
  );

  return (
    <ChatWorkspaceContext.Provider value={value}>
      {children}
    </ChatWorkspaceContext.Provider>
  );
}

export function useChatWorkspace() {
  const context = useContext(ChatWorkspaceContext);

  if (!context) {
    throw new Error('useChatWorkspace must be used within a ChatWorkspaceProvider');
  }

  return context;
}
