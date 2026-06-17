'use client';

import { useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { flushSync } from 'react-dom';

import {
  DASHBOARD_CHAT_COMPOSER_TRANSITION_NAME,
  DASHBOARD_CHAT_STAGE_TRANSITION_NAME,
} from '@/components/chat/lib/transition';
import {
  NEW_CHAT_DRAFT_KEY,
  useChatWorkspace,
} from '@/components/chat/providers/ChatWorkspaceProvider';
import {
  DASHBOARD_CARDS,
  DashboardCards,
  DashboardContentArea,
  DashboardHeader,
  DashboardTabs,
  getCardStat,
  type ActiveTab,
} from '@/components/dashboard';
import type { DashboardStats } from '@/lib/rpc/contract';

type DashboardUser = {
  id?: string;
  name?: string | null;
  email?: string | null;
};

type ViewTransitionDocument = Document & {
  startViewTransition?: (update: () => void | Promise<void>) => {
    finished: Promise<void>;
  };
};

interface DashboardPageClientProps {
  greeting: string;
  stats: DashboardStats | null;
  user: DashboardUser;
}

export function DashboardPageClient({
  greeting,
  stats,
  user,
}: DashboardPageClientProps) {
  const router = useRouter();
  const { getDraft, isDashboardLaunching, queueDashboardLaunch, setDraft } = useChatWorkspace();
  const [activeTab, setActiveTab] = useState<ActiveTab>('Chat');
  const [searchQuery, setSearchQuery] = useState('');
  const [newChatDraft, setNewChatDraft] = useState(() => getDraft(NEW_CHAT_DRAFT_KEY));
  const fallbackNavigationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const newChatDraftRef = useRef(newChatDraft);

  const firstName = user.name?.split(' ')[0] || user.email?.split('@')[0] || 'there';

  const cardStats: Record<string, string | null> = {};
  DASHBOARD_CARDS.forEach((card) => {
    cardStats[card.id] = getCardStat(card.id, stats);
  });

  const displayedCards = searchQuery.trim()
    ? DASHBOARD_CARDS.filter(
        (card) =>
          card.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
          card.description.toLowerCase().includes(searchQuery.toLowerCase()),
      )
    : DASHBOARD_CARDS;

  useEffect(() => {
    return () => {
      if (fallbackNavigationTimeoutRef.current) {
        clearTimeout(fallbackNavigationTimeoutRef.current);
      }
      setDraft(NEW_CHAT_DRAFT_KEY, newChatDraftRef.current);
    };
  }, [setDraft]);

  useEffect(() => {
    newChatDraftRef.current = newChatDraft;
  }, [newChatDraft]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDraft(NEW_CHAT_DRAFT_KEY, newChatDraft);
    }, 400);

    return () => window.clearTimeout(timer);
  }, [newChatDraft, setDraft]);

  const handleChatLaunch = (message: string) => {
    const normalized = message.trim();
    if (!normalized || isDashboardLaunching) {
      return;
    }

    flushSync(() => {
      queueDashboardLaunch(normalized);
    });

    const pushToChat = () => {
      router.push('/chat', { scroll: false });
    };

    const transitionDocument = document as ViewTransitionDocument;
    if (typeof transitionDocument.startViewTransition === 'function') {
      transitionDocument.startViewTransition(() => {
        pushToChat();
      });
      return;
    }

    fallbackNavigationTimeoutRef.current = setTimeout(() => {
      pushToChat();
    }, 180);
  };

  return (
    <div
      className={[
        'relative flex h-full min-h-0 flex-col overflow-hidden transition-[opacity,transform,filter] duration-300 ease-out',
        isDashboardLaunching ? 'pointer-events-none opacity-90 scale-[0.992] blur-[1px]' : '',
      ].join(' ')}
    >
      <div className="absolute inset-y-0 left-[20%] z-0 hidden w-px bg-black/8 pointer-events-none lg:block" />
      <div className="absolute inset-y-0 right-[20%] z-0 hidden w-px bg-black/8 pointer-events-none lg:block" />

      <DashboardTabs activeTab={activeTab} onTabChange={setActiveTab} />

      <DashboardHeader greeting={greeting} firstName={firstName} onUpgrade={() => router.push('/settings')} />

      <div className="hidden h-px shrink-0 bg-black/8 pointer-events-none lg:block" />

      <DashboardContentArea
        activeTab={activeTab}
        chatMessage={newChatDraft}
        setChatMessage={setNewChatDraft}
        onChatSubmit={handleChatLaunch}
        searchQuery={searchQuery}
        setSearchQuery={setSearchQuery}
        onSearch={(query) => {
          router.push(`/search?q=${encodeURIComponent(query)}`);
        }}
        onKnowledgeNavigate={(href) => {
          router.push(href);
        }}
        isChatLaunching={isDashboardLaunching}
        stageTransitionName={activeTab === 'Chat' ? DASHBOARD_CHAT_STAGE_TRANSITION_NAME : undefined}
        composerTransitionName={activeTab === 'Chat' ? DASHBOARD_CHAT_COMPOSER_TRANSITION_NAME : undefined}
      />

      <div className="hidden h-px shrink-0 bg-black/8 pointer-events-none lg:block" />

      <div className="min-h-0 flex-1 px-4 pb-4">
        <div className="mx-auto w-full max-w-5xl">
          <DashboardCards
            cards={displayedCards}
            stats={cardStats}
            onChatClick={(prompt) => {
              setNewChatDraft(prompt);
              setActiveTab('Chat');
              window.scrollTo({ top: 0, behavior: 'smooth' });
            }}
          />
        </div>
      </div>
    </div>
  );
}
