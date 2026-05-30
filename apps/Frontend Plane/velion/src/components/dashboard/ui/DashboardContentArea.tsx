'use client';

import type { CSSProperties } from 'react';
import { ChatInput } from '@/components/chat/components/ChatInput';
import { SearchInput } from './SearchInput';
import { KnowledgeTab } from './KnowledgeTab';
import type { ActiveTab } from './DashboardTabs';

interface DashboardContentAreaProps {
  activeTab: ActiveTab;
  chatMessage: string;
  setChatMessage: (msg: string) => void;
  onChatSubmit: (message: string) => void;
  searchQuery: string;
  setSearchQuery: (query: string) => void;
  onSearch: (query: string) => void;
  onKnowledgeNavigate?: (href: string) => void;
  isChatLaunching?: boolean;
  stageTransitionName?: string;
  composerTransitionName?: string;
}

export function DashboardContentArea({
  activeTab,
  chatMessage,
  setChatMessage,
  onChatSubmit,
  searchQuery,
  setSearchQuery,
  onSearch,
  onKnowledgeNavigate,
  isChatLaunching = false,
  stageTransitionName,
  composerTransitionName,
}: DashboardContentAreaProps) {
  const stageStyle = stageTransitionName
    ? ({ viewTransitionName: stageTransitionName } as CSSProperties)
    : undefined;
  const composerStyle = composerTransitionName
    ? ({ viewTransitionName: composerTransitionName } as CSSProperties)
    : undefined;

  return (
    <div className="relative pb-6 pt-8" style={stageStyle}>
      {/* Centre the content between the two lines (60 % of the viewport) */}
      <div className="relative mx-auto w-full px-4 lg:px-0 lg:w-[60%]">
        <div
          className="mx-auto w-full max-w-[720px]"
          style={activeTab === 'Chat' ? composerStyle : undefined}
        >
        {activeTab === 'Chat' ? (
          <ChatInput
            message={chatMessage}
            setMessage={setChatMessage}
            onSubmit={(e) => {
              e.preventDefault();
              if (chatMessage.trim()) {
                onChatSubmit(chatMessage.trim());
              }
            }}
            context="dashboard"
            placeholder="How can I help you today?"
            isLoading={isChatLaunching}
          />
        ) : activeTab === 'Søk' ? (
          <SearchInput
            value={searchQuery}
            onChange={setSearchQuery}
            onSearch={onSearch}
          />
        ) : (
          <KnowledgeTab onNavigate={onKnowledgeNavigate} />
        )}
        </div>
      </div>
    </div>
  );
}
