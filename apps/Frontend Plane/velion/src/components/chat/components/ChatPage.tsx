'use client';

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { m, AnimatePresence } from 'framer-motion';
import { ArrowLeft } from 'lucide-react';

import { ChatView } from './ChatView';
import { useChat, type SendOptions } from '@/components/chat/providers/ChatProvider';
import {
  NEW_CHAT_DRAFT_KEY,
  useChatWorkspace,
} from '@/components/chat/providers/ChatWorkspaceProvider';
import { useI18n } from '@/components/chat/hooks/i18n';
import {
  DASHBOARD_CHAT_COMPOSER_TRANSITION_NAME,
  DASHBOARD_CHAT_ENTRY_DURATION_MS,
} from '@/components/chat/lib/transition';
import { ThreeJSOrb, type OrbAnimationState } from '@/components/chat/three/ThreeJSOrb';

interface ChatPageProps {
  routeSessionId?: string;
  userId?: string;
  userName?: string;
  userEmail?: string;
  onBack?: () => void;
}

export const ChatPage: React.FC<ChatPageProps> = ({
  routeSessionId,
  onBack,
}) => {
  const router = useRouter();
  const { t } = useI18n();
  const {
    state: { currentSession, messages, isTyping, error },
    clearError,
    selectSession,
    startNewChat,
    streamMessage,
    regenerateLastMessage,
  } = useChat();
  const {
    pendingLaunch,
    getDraft,
    setDraft,
    clearDraft,
    consumePendingLaunch,
    resetNewChatDraft,
  } = useChatWorkspace();

  const draftKey = routeSessionId ?? NEW_CHAT_DRAFT_KEY;
  const [message, setLocalMessage] = useState(() => getDraft(draftKey));
  const [isResponding, setIsResponding] = useState(false);
  const responseResetRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const messageRef = useRef(message);

  useEffect(() => {
    messageRef.current = message;
  }, [message]);

  useEffect(() => {
    setLocalMessage(getDraft(draftKey));
  }, [draftKey, getDraft]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDraft(draftKey, message);
    }, 400);

    return () => {
      window.clearTimeout(timer);
    };
  }, [draftKey, message, setDraft]);

  useEffect(() => {
    return () => {
      setDraft(draftKey, messageRef.current);
    };
  }, [draftKey, setDraft]);

  useLayoutEffect(() => {
    if (!routeSessionId) {
      startNewChat();
    }
  }, [routeSessionId, startNewChat]);

  useEffect(() => {
    const handler = (event: KeyboardEvent): void => {
      const meta = event.metaKey || event.ctrlKey;

      if (event.key.toLowerCase() === 'n' && !meta) {
        event.preventDefault();
        startNewChat();
        resetNewChatDraft();
        // eslint-disable-next-line react-doctor/nextjs-no-client-side-redirect
        router.push('/chat', { scroll: false });
      }

      if ((event.key.toLowerCase() === 'k' && meta) || (event.key === '/' && !meta)) {
        event.preventDefault();
        const input = document.getElementById('aquatiq-chat-input') as HTMLInputElement | null;
        input?.focus();
      }
    };

    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [resetNewChatDraft, router, startNewChat]);

  useEffect(() => {
    return () => {
      if (responseResetRef.current) {
        clearTimeout(responseResetRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (routeSessionId && currentSession?.id !== routeSessionId) {
      void selectSession(routeSessionId);
    }
  }, [currentSession?.id, routeSessionId, selectSession]);

  useEffect(() => {
    if (!routeSessionId && currentSession?.id) {
      // eslint-disable-next-line react-doctor/nextjs-no-client-side-redirect
      router.replace(`/chat/${currentSession.id}`, { scroll: false });
    }
  }, [currentSession?.id, routeSessionId, router]);

  useEffect(() => {
    if (!pendingLaunch || routeSessionId) {
      return;
    }

    const autoSendDelay = pendingLaunch.source === 'dashboard'
      ? Math.max(140, DASHBOARD_CHAT_ENTRY_DURATION_MS - 80)
      : 48;

    const timer = window.setTimeout(() => {
      consumePendingLaunch(pendingLaunch.id);
      clearDraft(pendingLaunch.draftKey);
      setIsResponding(false);
      void streamMessage(pendingLaunch.message);
    }, autoSendDelay);

    return () => {
      clearTimeout(timer);
    };
  }, [clearDraft, consumePendingLaunch, pendingLaunch, routeSessionId, streamMessage]);

  const handleMessageChange = useCallback((nextMessage: string) => {
    setLocalMessage(nextMessage);
  }, []);

  const orbState: OrbAnimationState = isTyping
    ? 'thinking'
    : isResponding
      ? 'responding'
      : message.trim()
        ? 'listening'
        : 'waiting';

  const handleMessageSubmit = async (event: React.FormEvent | null, options?: SendOptions): Promise<void> => {
    if (event) {
      event.preventDefault();
    }

    const normalizedMessage = message.trim();
    if (!normalizedMessage || isTyping) {
      return;
    }

    setIsResponding(false);
    clearDraft(draftKey);
    setLocalMessage('');

    await streamMessage(normalizedMessage, routeSessionId, options);

    setIsResponding(true);
    if (responseResetRef.current) {
      clearTimeout(responseResetRef.current);
    }
    responseResetRef.current = setTimeout(() => {
      setIsResponding(false);
    }, 3000);
  };

  const handleBack = useCallback((): void => {
    if (onBack) {
      onBack();
      return;
    }

    router.push('/dashboard');
  }, [onBack, router]);

  const handleErrorDismiss = useCallback((): void => {
    clearError();
  }, [clearError]);

  const resolvedCurrentSession = routeSessionId
    ? (currentSession?.id === routeSessionId ? currentSession : null)
    : currentSession;

  return (
    <div className="flex h-full w-full bg-[var(--linear-main-bg)]">
      <section className="flex min-w-0 flex-1 flex-col bg-[var(--linear-main-bg)]">
        <div className="flex shrink-0 items-center gap-3 border-b border-[var(--linear-border)] bg-[var(--linear-sidebar-bg)] px-5 py-3 lg:hidden">
          <button
            onClick={handleBack}
            className="p-1 text-[#6b6f76] transition-colors hover:text-[#26282f]"
            aria-label={t('chat.back')}
            type="button"
          >
            <ArrowLeft className="h-4 w-4" />
          </button>
          <div className="h-6 w-6 shrink-0 overflow-hidden">
            <ThreeJSOrb state={orbState} size={24} />
          </div>
          <h1
            className="text-[17px] font-normal tracking-tight text-[#26282f]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            {t('chat.appName')}
          </h1>
        </div>

        <div className="min-h-0 flex-1">
          <ChatView
            key={routeSessionId ?? resolvedCurrentSession?.id ?? 'chat-view-new'}
            currentChat={resolvedCurrentSession ? {
              ...resolvedCurrentSession,
              messages: messages.map((chatMessage) => ({
                ...chatMessage,
                sender: chatMessage.role as 'user' | 'assistant' | 'system',
                timestamp: new Date(chatMessage.timestamp),
              })),
            } : null}
            message={message}
            setMessage={handleMessageChange}
            onMessageSubmit={handleMessageSubmit}
            isTyping={isTyping}
            onBack={handleBack}
            botAvatarSrc="/logo.png"
            botName="Aquatiq"
            orbState={orbState}
            composerViewTransitionName={DASHBOARD_CHAT_COMPOSER_TRANSITION_NAME}
            onRegenerate={regenerateLastMessage}
          />
        </div>

        <AnimatePresence>
          {error ? (
            <m.div
              initial={{ opacity: 0, y: 16 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 16 }}
              className="fixed bottom-4 right-4 z-50 flex items-center gap-3 rounded-2xl border border-[var(--linear-border)] bg-white px-4 py-3 shadow-[0_18px_42px_rgba(20,21,24,0.10)]"
              role="alert"
            >
              <span className="font-inter text-[13px] text-[#26282f]">{error}</span>
              <button
                onClick={handleErrorDismiss}
                className="font-inter text-[13px] leading-none text-[#8c9199] transition-colors hover:text-[#26282f]"
                aria-label={t('common.dismiss')}
                type="button"
              >
                ×
              </button>
            </m.div>
          ) : null}
        </AnimatePresence>
      </section>
    </div>
  );
};
