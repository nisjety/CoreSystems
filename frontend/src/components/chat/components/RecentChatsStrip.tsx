'use client';

import { Chat } from '@/components/chat/types';
import { m } from 'framer-motion';
import { MessageSquare } from 'lucide-react';
import { formatDistanceToNow } from 'date-fns';

interface RecentChatsStripProps {
  chats: Chat[];
  onOpen: (chat: Chat) => void;
}

export function RecentChatsStrip({ chats, onOpen }: RecentChatsStripProps) {
  if (!chats.length) return null;
  
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <span className="text-sm font-medium text-gray-900">Your recent chats</span>
      </div>
      
      <div className="grid grid-cols-1 md:grid-cols-3 xl:grid-cols-4 gap-4">
        {chats.slice(0,4).map((chat, index) => (
          <m.button
            key={chat.id}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: index * 0.05 }}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => onOpen(chat)}
            className="bg-white rounded-2xl shadow-[0_4px_24px_rgba(0,0,0,0.08)] p-4 text-left hover:shadow-[0_8px_32px_rgba(0,0,0,0.12)] transition-all group"
          >
            <div className="flex items-start gap-3">
              <div className="w-10 h-10 rounded-xl bg-blue-100 flex items-center justify-center group-hover:bg-blue-200 transition-colors">
                <MessageSquare className="w-5 h-5 text-blue-600" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium text-gray-800 truncate group-hover:text-blue-700 transition-colors">
                  {chat.title}
                </p>
                <p className="text-xs text-gray-500 mt-1">
                  {formatDistanceToNow(chat.timestamp, { addSuffix: true })}
                </p>
              </div>
            </div>
          </m.button>
        ))}
      </div>
    </div>
  );
}