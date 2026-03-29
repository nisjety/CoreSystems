"use client";

import { m } from 'framer-motion';
import { Chat } from '../types';
import { MessageSquare } from 'lucide-react';

interface RecentChatsGridProps {
  chats: Chat[];
  onChatClick: (chat: Chat) => void;
}

export function RecentChatsGrid({ chats, onChatClick }: RecentChatsGridProps) {
  const formatTime = (date: Date) => {
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const hours = Math.floor(diff / (1000 * 60 * 60));
    const days = Math.floor(diff / (1000 * 60 * 60 * 24));
    const weeks = Math.floor(days / 7);

    if (hours < 24) return `${hours} hours ago`;
    if (days < 7) return `${days} days ago`;
    return `${weeks} weeks ago`;
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <span className="text-sm font-medium text-gray-900">📁 Your recent chats</span>
      </div>
      
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        {chats.slice(0, 3).map((chat, index) => (
          <m.button
            key={chat.id}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: index * 0.05 }}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => onChatClick(chat)}
            className="bg-white rounded-2xl shadow-[0_4px_24px_rgba(0,0,0,0.08)] p-6 hover:shadow-[0_8px_32px_rgba(0,0,0,0.12)] transition-all cursor-pointer text-left group"
          >
            <div className="flex items-start gap-3">
              <div className="w-10 h-10 rounded-xl bg-blue-100 flex items-center justify-center group-hover:bg-blue-200 transition-colors">
                <MessageSquare className="w-5 h-5 text-blue-600" />
              </div>
              <div className="flex-1 min-w-0">
                <h3 className="font-medium text-gray-900 mb-1 group-hover:text-blue-700 transition-colors truncate">
                  {chat.title}
                </h3>
                <p className="text-xs text-gray-500">
                  {formatTime(chat.timestamp)}
                </p>
              </div>
            </div>
          </m.button>
        ))}
      </div>
    </div>
  );
}