'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';

interface Message {
  id: string;
  senderName: string;
  senderAvatar?: string;
  preview: string;
  timestamp: string;
  unread: boolean;
  href?: string;
  category?: 'message' | 'mention';
}

interface MessagesDropdownProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  messages: Message[];
  trigger: React.ReactNode;
  onMessageOpen?: (messageId: string) => void;
}

type MessageTab = 'all' | 'messages' | 'mentions';

const MESSAGE_TABS: { id: MessageTab; label: string }[] = [
  { id: 'all',      label: 'All' },
  { id: 'messages', label: 'Messages' },
  { id: 'mentions', label: 'Mentions' },
];

const BG: Record<string, string> = {
  A:'#FEE2E2',B:'#FEF3C7',C:'#D1FAE5',D:'#DBEAFE',E:'#EDE9FE',
  F:'#FCE7F3',G:'#F0FDF4',H:'#FFF7ED',J:'#EFF6FF',K:'#F5F3FF',
  L:'#FDF4FF',M:'#FFF1F2',N:'#F0F9FF',O:'#ECFDF5',P:'#FDF2F8',
  R:'#FFF7ED',S:'#EFF6FF',T:'#F0FDF4',V:'#FEF9C3',W:'#FFF7ED',
};
const FG: Record<string, string> = {
  A:'#DC2626',B:'#D97706',C:'#059669',D:'#2563EB',E:'#7C3AED',
  F:'#DB2777',G:'#16A34A',H:'#EA580C',J:'#1D4ED8',K:'#6D28D9',
  L:'#A21CAF',M:'#E11D48',N:'#0284C7',O:'#047857',P:'#9D174D',
  R:'#C2410C',S:'#1E40AF',T:'#15803D',V:'#A16207',W:'#B45309',
};

function avatarStyle(name: string) {
  const k = name.charAt(0).toUpperCase();
  return { backgroundColor: BG[k] ?? '#E8E8E8', color: FG[k] ?? '#555555' };
}

export function MessagesDropdown({
  isOpen,
  onOpenChange,
  messages,
  trigger,
  onMessageOpen,
}: MessagesDropdownProps) {
  const router = useRouter();
  const [hoveredParent, setHoveredParent] = React.useState(false);
  const [activeTab, setActiveTab] = React.useState<MessageTab>('all');

  React.useEffect(() => {
    if (hoveredParent && !isOpen) { onOpenChange(true); }
    else if (!hoveredParent && isOpen) { onOpenChange(false); }
  }, [hoveredParent, isOpen, onOpenChange]);

  const filtered = React.useMemo(() => {
    switch (activeTab) {
      case 'messages': return messages.filter((m) => !m.category || m.category === 'message');
      case 'mentions': return messages.filter((m) => m.category === 'mention');
      default:         return messages;
    }
  }, [activeTab, messages]);

  return (
    <div onMouseEnter={() => setHoveredParent(true)} onMouseLeave={() => setHoveredParent(false)}>
      <DropdownMenu open={isOpen} onOpenChange={onOpenChange}>
        <DropdownMenuTrigger asChild id="dashboard-messages-trigger">
          {trigger}
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          side="bottom"
          sideOffset={8}
          onClick={(e) => e.stopPropagation()}
          className={cn(
            'w-[360px] max-h-[480px] overflow-hidden rounded-2xl',
            'border border-[#E9EBF2] bg-white shadow-[0_20px_80px_rgba(17,17,17,0.15)]',
            'p-0'
          )}
        >
          {/* Tab bar */}
          <div className="flex border-b border-[#EBEBEB]">
            {MESSAGE_TABS.map((tab) => (
              <button
                key={tab.id}
                onClick={(e) => { e.stopPropagation(); setActiveTab(tab.id); }}
                className={cn(
                  'relative flex-1 py-3.5 text-[11px] font-medium whitespace-nowrap transition-colors focus:outline-none',
                  activeTab === tab.id
                    ? 'text-[#111111] font-bold'
                    : 'text-[#AAAAAA] hover:text-[#666666]'
                )}
              >
                {tab.label}
                {activeTab === tab.id && (
                  <span className="absolute bottom-0 inset-x-0 h-0.5 bg-[#111111] rounded-sm" />
                )}
              </button>
            ))}
          </div>

          {/* Messages list */}
          <div className="overflow-y-auto max-h-[380px]">
            {filtered.length > 0 ? (
              filtered.map((message, index) => {
                const initials = message.senderName
                  .split(' ').map((w) => w.charAt(0)).slice(0, 2).join('').toUpperCase();
                return (
                  <div
                    key={message.id}
                    className={cn(index < filtered.length - 1 && 'border-b border-[#F0F0F0]')}
                  >
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        onMessageOpen?.(message.id);
                        onOpenChange(false);
                        router.push(message.href ?? '/inbox');
                      }}
                      className="w-full px-4 py-3 text-left hover:bg-[#FAFAFA] transition-colors focus:outline-none"
                    >
                      <div className="flex items-start gap-3">
                        <div
                          className="w-9 h-9 rounded-full shrink-0 flex items-center justify-center text-xs font-bold"
                          style={avatarStyle(message.senderName)}
                        >
                          {initials}
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className="flex items-start justify-between gap-2">
                            <span className="text-sm font-bold text-[#111111] truncate">{message.senderName}</span>
                            <span className="text-xs text-[#9B9B9B] shrink-0">{message.timestamp}</span>
                          </div>
                          <p className="text-xs text-[#9B9B9B] mt-0.5 truncate">{message.preview}</p>
                        </div>
                        {message.unread && (
                          <div className="w-2.5 h-2.5 rounded-full bg-[#22C55E] shrink-0 mt-1" />
                        )}
                      </div>
                    </button>
                  </div>
                );
              })
            ) : (
              <div className="px-5 py-10 text-center">
                <p className="text-sm text-[#888888]">No messages</p>
              </div>
            )}
          </div>

          {/* Footer */}
          <div className="border-t border-[#EBEBEB] px-4 py-2.5">
            <button
              onClick={(e) => { e.stopPropagation(); onOpenChange(false); router.push('/inbox'); }}
              className="w-full text-center text-xs font-semibold text-[#555555] hover:text-[#111111] transition-colors py-1 focus:outline-none"
            >
              View all messages
            </button>
          </div>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
