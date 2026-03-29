'use client';

import React from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { MessageSquare, Plus } from 'lucide-react';

import type { SessionListItem } from '@/components/chat/api/orpc/chat';
import { useChat } from '@/components/chat/providers/ChatProvider';
import { useChatWorkspace } from '@/components/chat/providers/ChatWorkspaceProvider';
import { useChatHistory } from '../hooks/useRealData';
import { cn, formatTime } from '../utils';

type ChatHistoryResponse = {
  sessions?: SessionListItem[];
  totalCount?: number;
};

interface MessagesSidebarPanelProps {
  searchQuery?: string;
  onNavigate?: () => void;
}

function normalizeSearchText(value: string): string {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase();
}

function getRelativeTimestamp(value: string) {
  const date = new Date(value);

  if (Number.isNaN(date.getTime())) {
    return '';
  }

  return formatTime(date);
}

export function MessagesSidebarPanel({
  searchQuery = '',
  onNavigate,
}: MessagesSidebarPanelProps) {
  const router = useRouter();
  const pathname = usePathname();
  const { selectSession, startNewChat } = useChat();
  const { resetNewChatDraft } = useChatWorkspace();
  const { data, isLoading, error } = useChatHistory();

  const sessions = React.useMemo(() => {
    const response = (data ?? null) as ChatHistoryResponse | null;
    return (response?.sessions ?? []).filter((session) => (session.messageCount ?? 0) > 0);
  }, [data]);

  const normalizedQuery = normalizeSearchText(searchQuery.trim());
  const filteredSessions = React.useMemo(() => {
    if (!normalizedQuery) {
      return sessions;
    }

    return sessions.filter((session) => {
      return normalizeSearchText(`${session.title} ${session.id}`).includes(normalizedQuery);
    });
  }, [normalizedQuery, sessions]);

  const handleOpenChat = React.useCallback((sessionId: string) => {
    const href = `/chat/${sessionId}`;

    void selectSession(sessionId);

    if (pathname !== href) {
      router.push(href);
    }

    onNavigate?.();
  }, [onNavigate, pathname, router, selectSession]);

  const handleNewChat = React.useCallback(() => {
    startNewChat();
    resetNewChatDraft();
    router.push('/chat');
    onNavigate?.();
  }, [onNavigate, resetNewChatDraft, router, startNewChat]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="mb-3 flex items-center justify-between gap-3 px-1">
        <div>
          <h2 className="text-[15px] font-semibold tracking-[-0.02em] text-[#1C1C1E]">Chat history</h2>
          <p className="mt-1 text-[12px] leading-5 text-[#8A8D96]">
            Continue earlier conversations or start a fresh one.
          </p>
        </div>

        <button
          type="button"
          onClick={handleNewChat}
          className="flex h-8 w-8 items-center justify-center rounded-[10px] border border-[#E2E3E9] bg-white text-[#3A3C44] transition-colors hover:bg-[#F7F7F9] hover:text-[#1C1C1E]"
          aria-label="Start new chat"
          title="Start new chat"
        >
          <Plus className="h-4 w-4" strokeWidth={2.1} />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto pr-1">
        {isLoading ? (
          <div className="flex flex-col gap-2 py-1">
            {Array.from({ length: 4 }).map((_, index) => (
              <div
                key={index}
                className="h-[74px] animate-pulse rounded-2xl border border-[#ECECF1] bg-[#F7F7F9]"
              />
            ))}
          </div>
        ) : error ? (
          <div className="rounded-2xl border border-[#ECECF1] bg-[#F7F7F9] px-4 py-5 text-[13px] leading-6 text-[#8A8D96]">
            Failed to load chat history.
          </div>
        ) : filteredSessions.length === 0 ? (
          <div className="flex h-full min-h-[240px] flex-col items-center justify-center rounded-2xl border border-dashed border-[#E2E3E9] bg-[#FAFAFB] px-6 text-center">
            <div className="mb-3 flex h-11 w-11 items-center justify-center rounded-2xl bg-white text-[#A3A6AE] shadow-[0_1px_2px_rgba(28,28,30,0.06)]">
              <MessageSquare className="h-5 w-5" strokeWidth={1.8} />
            </div>
            <p className="text-[14px] font-medium text-[#3A3C44]">
              {normalizedQuery ? 'No matching conversations' : 'No chats yet'}
            </p>
            <p className="mt-1 text-[12px] leading-5 text-[#8A8D96]">
              {normalizedQuery ? 'Try a different search term.' : 'Your recent sessions will appear here.'}
            </p>
          </div>
        ) : (
          <div className="space-y-2 pb-2">
            {filteredSessions.map((session) => {
              const href = `/chat/${session.id}`;
              const isActive = pathname === href;

              return (
                <button
                  key={session.id}
                  type="button"
                  onClick={() => handleOpenChat(session.id)}
                  className={cn(
                    'w-full rounded-2xl border px-3 py-3 text-left transition-colors',
                    isActive
                      ? 'border-[#D7D9E0] bg-[#F4F5F8]'
                      : 'border-[#ECECF1] bg-white hover:bg-[#F8F8FA]'
                  )}
                >
                  <div className="flex items-start gap-3">
                    <div
                      className={cn(
                        'mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border',
                        isActive ? 'border-[#D7D9E0] bg-white text-[#1C1C1E]' : 'border-[#ECECF1] bg-[#F7F7F9] text-[#6B6E78]'
                      )}
                    >
                      <MessageSquare className="h-4 w-4" strokeWidth={1.8} />
                    </div>

                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-3">
                        <span className="truncate text-[14px] font-medium leading-5 tracking-[-0.01em] text-[#1C1C1E]">
                          {session.title || 'Untitled chat'}
                        </span>
                        <span className="shrink-0 text-[11px] uppercase tracking-[0.12em] text-[#A0A3AD]">
                          {getRelativeTimestamp(session.updatedAt)}
                        </span>
                      </div>

                      <div className="mt-1 flex items-center gap-2 text-[12px] text-[#8A8D96]">
                        <span>{session.messageCount ?? 0} messages</span>
                        <span className="h-1 w-1 rounded-full bg-[#D2D4DB]" />
                        <span>{new Date(session.updatedAt).toLocaleDateString()}</span>
                      </div>
                    </div>
                  </div>
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
