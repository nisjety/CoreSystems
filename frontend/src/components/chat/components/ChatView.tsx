import { useRef, useEffect, useLayoutEffect, useState, useCallback, type CSSProperties } from 'react';
import { m, AnimatePresence, useReducedMotion } from 'framer-motion';
import { ArrowDown, ArrowLeft } from 'lucide-react';
import { useI18n } from '@/components/chat/hooks/i18n';
import { ChatInput } from './ChatInput';
import { ThreeJSOrb, type OrbAnimationState } from '@/components/chat/three/ThreeJSOrb';
import { DASHBOARD_CHAT_STAGE_TRANSITION_NAME } from '@/components/chat/lib/transition';

interface MessageBubbleProps {
  message: {
    id: string;
    content: string;
    role: 'user' | 'assistant' | 'system';
    timestamp: Date | string;
    isThinking?: boolean;
    metadata?: {
      citations?: string[];
    };
  };
  avatarSrc?: string;
  avatarAlt: string;
  role: string;
  orbState?: OrbAnimationState;
  onOrbStateChange?: (state: OrbAnimationState) => void;
}

const MessageBubble: React.FC<MessageBubbleProps> = ({
  message,
  orbState = 'waiting',
  onOrbStateChange,
}) => {
  const isUser = message.role === 'user';

  return (
    <m.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.22, ease: 'easeOut' }}
      className={`flex gap-5 mb-7 ${isUser ? 'justify-end' : 'justify-start'}`}
    >
      {/* AI marker — thin vertical accent line */}
      {!isUser && (
        <div className="shrink-0 flex flex-col items-center pt-1 gap-2">
          <div className="w-px h-full min-h-5 bg-[#D8D2C6]" />
          <div className="shrink-0 w-7 h-7 overflow-hidden">
            <ThreeJSOrb
              state={orbState}
              size={28}
              onStateChange={onOrbStateChange}
            />
          </div>
        </div>
      )}

      <div className={`flex flex-col ${isUser ? 'items-end' : 'items-start'} max-w-2xl lg:max-w-3xl`}>
        <div
          className={`px-5 py-3.5 border ${
            isUser
              ? 'bg-[#EAE6DF] border-[#D8D2C6] text-[#2B2B2B]'
              : 'bg-white border-[#D8D2C6] text-[#2B2B2B]'
          }`}
        >
          {isUser ? (
            <p className="font-inter text-[15px] leading-[1.65] whitespace-pre-wrap text-[#2B2B2B]">
              {message.content}
            </p>
          ) : (
            <p className="font-inter text-[15px] leading-[1.65] whitespace-pre-wrap text-[#2B2B2B]">
              {message.content}
            </p>
          )}
        </div>
        <span className="mt-1.5 px-0.5 font-inter text-[11px] tracking-wide text-[#A09890]">
          {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
        </span>
      </div>

      {/* User right-side spacer keeps alignment symmetric */}
      {isUser && (
        <div className="shrink-0 flex flex-col items-center pt-1">
          <div className="w-px h-full min-h-5 bg-[#D8D2C6]" />
          <div className="w-7 h-7 bg-[#EAE6DF] border border-[#D8D2C6] flex items-center justify-center">
            <span className="font-inter text-[10px] text-[#4A4A48] leading-none select-none">Du</span>
          </div>
        </div>
      )}
    </m.div>
  );
};

const TypingIndicator = ({ orbState = 'thinking', onOrbStateChange }: {
  orbState?: OrbAnimationState;
  onOrbStateChange?: (state: OrbAnimationState) => void;
}) => {
  const { t } = useI18n();
  return (
    <m.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.2 }}
      className="flex gap-5 mb-7"
    >
      <div className="shrink-0 flex flex-col items-center pt-1 gap-2">
        <div className="w-px h-full min-h-5 bg-[#D8D2C6]" />
        <div className="w-7 h-7 overflow-hidden shrink-0">
          <ThreeJSOrb state={orbState} size={28} onStateChange={onOrbStateChange} />
        </div>
      </div>
      <div className="bg-white border border-[#D8D2C6] px-5 py-3.5 flex items-center gap-3">
        <span className="font-inter text-[13px] text-[#A09890] italic">
          {t('chat.placeholder.thinking')}
        </span>
        {/* Three thin pulsing lines — no circles */}
        <div className="flex items-end gap-[3px] h-4">
          {[0, 0.2, 0.4].map((delay, i) => (
            <m.span
              key={i}
              className="w-px bg-[#C8C1B3]"
              animate={{ height: ['4px', '14px', '4px'] }}
              transition={{ duration: 0.9, repeat: Infinity, delay, ease: 'easeInOut' }}
            />
          ))}
        </div>
      </div>
    </m.div>
  );
};

interface ChatMessage {
  id: string;
  content: string;
  role: 'user' | 'assistant' | 'system';
  timestamp: Date | string;
  isThinking?: boolean;
  metadata?: {
    citations?: string[];
  };
}

interface ChatSessionLike {
  id: string;
  title?: string;
  messages: ChatMessage[];
}

interface ChatViewProps {
  currentChat: ChatSessionLike | null;
  message: string;
  setMessage: (message: string) => void;
  onMessageSubmit: (e: React.FormEvent) => void | Promise<void>;
  isTyping: boolean;
  onBack?: () => void;
  botAvatarSrc?: string;
  botName?: string;
  userAvatarSrc?: string;
  orbState?: OrbAnimationState;
  onOrbStateChange?: (state: OrbAnimationState) => void;
  composerViewTransitionName?: string;
}

export function ChatView({
  currentChat,
  message,
  setMessage,
  onMessageSubmit,
  isTyping,
  onBack,
  botAvatarSrc = '/logo.png',
  botName,
  orbState = 'waiting',
  onOrbStateChange,
  composerViewTransitionName,
}: ChatViewProps) {
  const { t } = useI18n();
  const effectiveBotName = botName || 'Aquatiq';
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const messagesEndRef = useRef<HTMLDivElement | null>(null);
  const previousMessageCountRef = useRef(0);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [canShowScrollCta, setCanShowScrollCta] = useState(false);
  const prefersReducedMotion = useReducedMotion();
  const hasMessages = (currentChat?.messages?.length ?? 0) > 0;
  const hasStreamingAssistant = currentChat?.messages?.some(
    (msg) => msg.role === 'assistant' && msg.isThinking,
  ) ?? false;

  const scrollToBottom = useCallback(
    (smooth = true) => {
      if (messagesEndRef.current) {
        messagesEndRef.current.scrollIntoView({
          behavior: smooth && !prefersReducedMotion ? 'smooth' : 'auto',
          block: 'end',
        });
      }
    },
    [prefersReducedMotion],
  );

  useLayoutEffect(() => {
    if (isTyping) {
      scrollToBottom(false);
      return;
    }

    if (isAtBottom) {
      scrollToBottom(true);
    }
  }, [currentChat?.messages, isTyping, scrollToBottom, isAtBottom]);

  useLayoutEffect(() => {
    const nextMessageCount = currentChat?.messages.length ?? 0;
    const previousMessageCount = previousMessageCountRef.current;
    previousMessageCountRef.current = nextMessageCount;

    if (nextMessageCount <= previousMessageCount) {
      return;
    }

    window.requestAnimationFrame(() => {
      setCanShowScrollCta(false);
      scrollToBottom(isTyping ? false : previousMessageCount > 0);
    });
  }, [currentChat?.messages.length, isTyping, scrollToBottom]);

  useEffect(() => {
    const el = viewportRef.current;
    if (!el) return;
    const onScroll = () => {
      const delta = el.scrollHeight - el.scrollTop - el.clientHeight;
      const nextIsAtBottom = delta < 8;
      setIsAtBottom(nextIsAtBottom);

      if (nextIsAtBottom) {
        setCanShowScrollCta(false);
      }
    };
    el.addEventListener('scroll', onScroll, { passive: true });
    onScroll();
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  const stageTransitionStyle = {
    viewTransitionName: DASHBOARD_CHAT_STAGE_TRANSITION_NAME,
  } as CSSProperties;
  const composerTransitionStyle = composerViewTransitionName
    ? ({ viewTransitionName: composerViewTransitionName } as CSSProperties)
    : undefined;

  return (
    <div
      className="flex h-full min-h-0 flex-col overflow-hidden bg-[#F4F1EB]"
      style={stageTransitionStyle}
    >

      {/* ── Header — visible only when there are messages ── */}
      <AnimatePresence>
        {hasMessages && (
          <m.header
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.18 }}
            className="shrink-0 sticky top-0 z-20 bg-[#F4F1EB]/90 backdrop-blur-sm border-b border-[#D8D2C6]"
          >
            <div className="max-w-4xl mx-auto flex items-center justify-between px-6 py-3">
              <div className="flex items-center gap-4">
                {onBack && (
                  <button
                    onClick={onBack}
                    className="lg:hidden p-1 text-[#4A4A48] hover:text-[#2B2B2B] transition-colors"
                    aria-label="Back"
                  >
                    <ArrowLeft className="w-4 h-4" />
                  </button>
                )}
                <div className="flex items-center gap-3">
                  <div className="w-7 h-7 overflow-hidden shrink-0">
                    <ThreeJSOrb
                      state={orbState}
                      size={28}
                      onStateChange={onOrbStateChange}
                    />
                  </div>
                  <h1
                    className="text-[17px] font-normal text-[#2B2B2B] tracking-tight"
                    style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
                  >
                    {effectiveBotName}
                  </h1>
                </div>
              </div>
              {/* Status dot */}
              <div className="flex items-center gap-2">
                <span className="inline-block w-1.5 h-1.5 bg-[#2B2B2B]" />
                <span className="font-inter text-[11px] text-[#A09890] uppercase tracking-widest">
                  {t('chat.status.online')}
                </span>
              </div>
            </div>
          </m.header>
        )}
      </AnimatePresence>

      {/* ── Messages / Empty state ── */}
      <div
        ref={viewportRef}
        onWheelCapture={() => {
          if (hasMessages && !isTyping) {
            setCanShowScrollCta(true);
          }
        }}
        onTouchMoveCapture={() => {
          if (hasMessages && !isTyping) {
            setCanShowScrollCta(true);
          }
        }}
        className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
      >
        {!hasMessages ? (
          /* Empty state — centered brand moment */
          <div className="flex flex-col items-center justify-center min-h-full px-8 pt-32 pb-16">
            <div className="opacity-20 mb-10">
              <ThreeJSOrb
                state={orbState}
                size={160}
                onStateChange={onOrbStateChange}
              />
            </div>
            <h2
              className="text-[32px] font-normal text-[#2B2B2B] text-center tracking-tight leading-snug mb-3"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              {t('chat.empty.title')}
            </h2>
            <p className="font-inter text-[14px] text-[#A09890] text-center max-w-sm leading-relaxed">
              {t('chat.empty.description')}
            </p>
          </div>
        ) : (
          /* Message list */
          <m.div className="max-w-4xl mx-auto px-6 py-10">
            <AnimatePresence initial={false}>
              {currentChat?.messages.map((msg) => (
                <m.div key={msg.id} layout>
                  <MessageBubble
                    message={msg}
                    avatarSrc={msg.role === 'assistant' ? botAvatarSrc : undefined}
                    avatarAlt={
                      msg.role === 'assistant'
                        ? `${effectiveBotName} avatar`
                        : t('chat.avatar.user')
                    }
                    role={msg.role}
                    orbState={orbState}
                    onOrbStateChange={onOrbStateChange}
                  />
                </m.div>
              ))}
            </AnimatePresence>

            <AnimatePresence>
              {isTyping && !hasStreamingAssistant && (
                <TypingIndicator
                  orbState={orbState}
                  onOrbStateChange={onOrbStateChange}
                />
              )}
            </AnimatePresence>

            <div ref={messagesEndRef} className="h-4" />
          </m.div>
        )}
      </div>

      {/* ── Scroll-to-bottom button ── */}
      <AnimatePresence>
        {!isTyping && canShowScrollCta && !isAtBottom && hasMessages && (
          <m.button
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 6 }}
            onClick={() => scrollToBottom(true)}
            className="fixed bottom-32 right-6 z-30 flex items-center gap-2 border border-[#D8D2C6] bg-[#F4F1EB] px-3 py-2 font-inter text-[12px] text-[#4A4A48] shadow-sm hover:border-[#2B2B2B] hover:text-[#2B2B2B] transition-colors"
          >
            <ArrowDown className="w-3.5 h-3.5" />
            <span>{t('chat.action.scrollBottom')}</span>
          </m.button>
        )}
      </AnimatePresence>

      {/* ── Input area ── */}
      <div className="z-10 shrink-0 border-t border-[#D8D2C6] bg-[#F4F1EB]">
        <div
          className="mx-auto w-full max-w-[720px] px-6 py-6 lg:px-0"
          style={composerTransitionStyle}
        >
          <ChatInput
            message={message}
            setMessage={setMessage}
            onSubmit={onMessageSubmit}
            disabled={false}
            isLoading={isTyping}
            isTyping={isTyping}
            context="chatpage"
          />
        </div>
      </div>
    </div>
  );
}
