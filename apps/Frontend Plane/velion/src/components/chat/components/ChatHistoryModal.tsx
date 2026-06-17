'use client';

import { useState, useRef, useEffect, cloneElement, useSyncExternalStore } from 'react';
import { createPortal } from 'react-dom';
import { useRouter } from 'next/navigation';
import { MessageSquare, LayoutList } from 'lucide-react';
import { m, AnimatePresence } from 'framer-motion';
import { useChatSafe } from '@/components/chat/providers/ChatProvider';

function groupByDate(items: Array<{ updatedAt: string; id: string; title: string; messageCount?: number }>) {
  const now = new Date();
  const todayStr     = now.toDateString();
  const yesterdayStr = new Date(now.getTime() - 86_400_000).toDateString();

  const today: typeof items     = [];
  const yesterday: typeof items = [];
  const earlier: typeof items   = [];

  for (const item of items) {
    const d = new Date(item.updatedAt).toDateString();
    if (d === todayStr)          today.push(item);
    else if (d === yesterdayStr) yesterday.push(item);
    else                         earlier.push(item);
  }
  return { today, yesterday, earlier };
}

interface ChatHistoryModalProps {
  trigger: React.ReactNode;
}

export function ChatHistoryModal({ trigger }: ChatHistoryModalProps) {
  const router = useRouter();
  const chat = useChatSafe();
  const mounted = useSyncExternalStore(() => () => {}, () => true, () => false);
  const [isOpen, setIsOpen] = useState(false);
  const [pos, setPos] = useState<{ bottom: number; right: number; maxHeight: number }>({
    bottom: 0, right: 0, maxHeight: 400,
  });
  const wrapperRef = useRef<HTMLDivElement>(null);
  const modalRef   = useRef<HTMLDivElement>(null);

  const sessions = chat?.state.sessions ?? [];
  const selectSession = chat?.selectSession ?? (async () => {});

  // Show only the 2 most recent sessions
  const recent = [...sessions]
    .sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime())
    .slice(0, 2);

  const { today, yesterday, earlier } = groupByDate(recent);
  const empty = sessions.length === 0;

  // Close on click-outside
  useEffect(() => {
    if (!isOpen) return;
    const handleDown = (e: MouseEvent) => {
      const inWrapper = wrapperRef.current?.contains(e.target as Node);
      const inModal   = modalRef.current?.contains(e.target as Node);
      if (!inWrapper && !inModal) setIsOpen(false);
    };
    document.addEventListener('mousedown', handleDown);
    return () => document.removeEventListener('mousedown', handleDown);
  }, [isOpen]);

  const handleTriggerClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!isOpen) {
      const rect = wrapperRef.current?.getBoundingClientRect();
      if (rect) {
        setPos({
          bottom: window.innerHeight - rect.top + 8,
          right:  window.innerWidth  - rect.right,
          maxHeight: Math.min(400, rect.top - 16),
        });
      }
    }
    setIsOpen(o => !o);
  };

  const openSession = async (id: string) => {
    await selectSession(id);
    setIsOpen(false);
    router.push(`/chat/${id}`);
  };

  const fmt = (iso: string, isToday: boolean) =>
    isToday
      ? new Date(iso).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
      : new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });

  const triggerEl = cloneElement(trigger as React.ReactElement<{ onClick?: React.MouseEventHandler }>, { onClick: handleTriggerClick });

  return (
    <>
      <div ref={wrapperRef}>
        {triggerEl}
      </div>

      {mounted && createPortal(
        <AnimatePresence>
          {isOpen && (
            <m.div
              ref={modalRef}
              initial={{ opacity: 0, y: 8, scale: 0.97 }}
              animate={{ opacity: 1, y: 0,  scale: 1 }}
              exit={{   opacity: 0, y: 8,  scale: 0.97 }}
              transition={{ type: 'spring', stiffness: 420, damping: 32 }}
              style={{
                position: 'fixed',
                bottom: pos.bottom,
                right:  pos.right,
                zIndex: 9999,
                maxHeight: pos.maxHeight,
                transformOrigin: 'bottom right',
              }}
              className="w-[280px] rounded-[20px] bg-white shadow-[0_-4px_32px_rgba(0,0,0,0.10),0_4px_16px_rgba(0,0,0,0.05)] overflow-hidden"
            >
              <div className="p-2 overflow-y-auto" style={{ maxHeight: pos.maxHeight }}>

                {empty ? (
                  <div className="flex flex-col items-center justify-center py-6 gap-2">
                    <MessageSquare size={20} strokeWidth={1.5} className="text-[#ccc]" />
                    <p className="text-[12.5px] text-[#bbb]">No conversations yet</p>
                  </div>
                ) : (
                  <>
                    {([
                      { label: 'Today',     items: today,     isToday: true  },
                      { label: 'Yesterday', items: yesterday, isToday: false },
                      { label: 'Earlier',   items: earlier,   isToday: false },
                    ] as const).map(group =>
                      group.items.length === 0 ? null : (
                        <div key={group.label}>
                          <p className="px-3 pt-1.5 pb-0.5 text-[10.5px] font-semibold text-[#bbb] uppercase tracking-wider">
                            {group.label}
                          </p>
                          {group.items.map(session => (
                            <button
                              key={session.id}
                              type="button"
                              onClick={() => openSession(session.id)}
                              className="w-full flex items-center gap-3 px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors text-left"
                            >
                              <MessageSquare size={16} strokeWidth={1.7} className="text-[#333] shrink-0" />
                              <span className="flex-1 text-[13.5px] font-medium text-[#1a1a1a] truncate">
                                {session.title || 'Untitled'}
                              </span>
                              <span className="text-[11.5px] text-[#bbb] shrink-0">
                                {fmt(session.updatedAt, group.isToday)}
                              </span>
                            </button>
                          ))}
                        </div>
                      )
                    )}
                  </>
                )}

                <div className="h-px bg-black/8 mx-1 my-1" />
                <button
                  type="button"
                  onClick={() => { setIsOpen(false); router.push('/chat'); }}
                  className="w-full flex items-center gap-3 px-3 py-2.5 rounded-[12px] hover:bg-[#f2f2f2] transition-colors text-left"
                >
                  <LayoutList size={16} strokeWidth={1.7} className="text-[#333] shrink-0" />
                  <span className="text-[13.5px] font-medium text-[#1a1a1a]">View all conversations</span>
                </button>

              </div>
            </m.div>
          )}
        </AnimatePresence>,
        document.body
      )}
    </>
  );
}
