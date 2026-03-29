'use client';

import { useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { flushSync } from 'react-dom';
import { authClient } from '@/components/auth/lib/auth-client-enterprise';
import { useChat } from '@/components/chat/providers/ChatProvider';
import {
  NEW_CHAT_DRAFT_KEY,
  useChatWorkspace,
} from '@/components/chat/providers/ChatWorkspaceProvider';
import {
  DASHBOARD_CHAT_COMPOSER_TRANSITION_NAME,
  DASHBOARD_CHAT_STAGE_TRANSITION_NAME,
} from '@/components/chat/lib/transition';
import {
  DashboardTabs,
  DashboardHeader,
  DashboardContentArea,
  DashboardCards,
  useGreeting,
  useDashboardStats,
  getCardStat,
  DASHBOARD_CARDS,
  type ActiveTab,
  type DashboardCardData,
} from '@/components/dashboard';

interface User {
  id?: string;
  name?: string;
  email?: string;
}

interface Session {
  user?: User;
}

type ViewTransitionDocument = Document & {
  startViewTransition?: (update: () => void | Promise<void>) => {
    finished: Promise<void>;
  };
};

export default function DashboardPage() {
  const router = useRouter();
  const { startNewChat } = useChat();
  const {
    getDraft,
    setDraft,
    queueDashboardLaunch,
    isDashboardLaunching,
  } = useChatWorkspace();
  const [session, setSession] = useState<Session | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<ActiveTab>('Chat');
  const [searchQuery, setSearchQuery] = useState('');
  const fallbackNavigationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const newChatDraft = getDraft(NEW_CHAT_DRAFT_KEY);

  const greeting = useGreeting();
  const { data: stats = null } = useDashboardStats();

  useEffect(() => {
    const checkSession = async () => {
      try {
        setIsLoading(true);
        const sessionData = await authClient.getSession();
        if (sessionData && 'data' in sessionData && sessionData.data?.user) {
          setSession({ user: sessionData.data.user as User });
        } else {
          setError('No session found');
          setTimeout(() => router.push('/sign-in'), 2000);
        }
      } catch (err) {
        console.error('Failed to get session:', err);
        setError('Failed to load session');
        setTimeout(() => router.push('/sign-in'), 2000);
      } finally {
        setIsLoading(false);
      }
    };
    checkSession();

    return () => {
      if (fallbackNavigationTimeoutRef.current) {
        clearTimeout(fallbackNavigationTimeoutRef.current);
      }
    };
  }, [router]);

  if (isLoading) {
    return (
      <div className="flex h-full min-h-0 items-center justify-center">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-[#E5E0D8] border-t-[#2B2B2B]" />
      </div>
    );
  }

  if (error || !session?.user) {
    return (
      <div className="flex h-full min-h-0 items-center justify-center p-8">
        <p className="text-[13px] text-[#A09890]">
          {error || 'Ingen økt funnet. Videresender til innlogging...'}
        </p>
      </div>
    );
  }

  const firstName =
    session?.user?.name?.split(' ')[0] ||
    session?.user?.email?.split('@')[0] ||
    'there';

  // Build stats map for cards
  const cardStats: Record<string, string | null> = {};
  DASHBOARD_CARDS.forEach((card) => {
    cardStats[card.id] = getCardStat(card.id, stats);
  });

  // Filter cards based on search query
  const displayedCards = searchQuery.trim()
    ? DASHBOARD_CARDS.filter(
        (c) =>
          c.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
          c.description.toLowerCase().includes(searchQuery.toLowerCase()),
      )
    : DASHBOARD_CARDS;

  const handleChatLaunch = (message: string) => {
    const normalized = message.trim();
    if (!normalized || isDashboardLaunching) {
      return;
    }

    flushSync(() => {
      startNewChat();
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
        'relative flex h-full min-h-0 flex-col overflow-y-auto transition-[opacity,transform,filter] duration-300 ease-out',
        isDashboardLaunching ? 'pointer-events-none opacity-90 scale-[0.992] blur-[1px]' : '',
      ].join(' ')}
    >
      {/* Decorative grid lines — vertical */}
      <div className="absolute inset-y-0 left-[20%] w-px bg-black/8 hidden lg:block pointer-events-none z-0" />
      <div className="absolute inset-y-0 right-[20%] w-px bg-black/8 hidden lg:block pointer-events-none z-0" />

      <DashboardTabs activeTab={activeTab} onTabChange={setActiveTab} />

      <DashboardHeader
        greeting={greeting}
        firstName={firstName}
        onUpgrade={() => router.push('/settings')}
      />

      {/* Horizontal line under header */}
      <div className="h-px bg-black/8 hidden lg:block pointer-events-none shrink-0" />

      <DashboardContentArea
        activeTab={activeTab}
        chatMessage={newChatDraft}
        setChatMessage={(nextMessage) => setDraft(NEW_CHAT_DRAFT_KEY, nextMessage)}
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

      {/* Horizontal line between content area and cards */}
      <div className="h-px bg-black/8 hidden lg:block pointer-events-none shrink-0" />

      <div className="w-full px-4 pb-10">
        <div className="mx-auto w-full max-w-5xl">
          <DashboardCards
            cards={displayedCards}
            stats={cardStats}
            onChatClick={(prompt) => {
              setDraft(NEW_CHAT_DRAFT_KEY, prompt);
              setActiveTab('Chat');
              window.scrollTo({ top: 0, behavior: 'smooth' });
            }}
          />
        </div>
      </div>
    </div>
  );
}
