'use client';

import React, { useState } from 'react';
import { cn, formatTime } from '../utils';
import { 
  MessageSquare, 
  Search, 
  Plus, 
  MoreVertical,
  Trash2,
  Edit3,
  Pin,
  Archive
} from 'lucide-react';
import { useChatHistory, usePinChat, useArchiveChat, useDeleteChat } from '../hooks/useRealData';

interface ChatHistory {
  id: string;
  title: string;
  lastMessage: string;
  timestamp: Date;
  unreadCount: number;
  isPinned: boolean;
  participants: string[];
}

interface ChatHistoryTabProps {
  onChatSelect: (chat: ChatHistory) => void;
  onNewChat: () => void;
}

export function ChatHistoryTab({ onChatSelect, onNewChat }: ChatHistoryTabProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedChat, setSelectedChat] = useState<string | null>(null);

  // Real data hooks instead of mock data
  const { data: chatHistoryData, error: chatHistoryError } = useChatHistory();
  const pinChatMutation = usePinChat();
  const archiveChatMutation = useArchiveChat();
  const deleteChatMutation = useDeleteChat();

  // Use real data or fallback to empty array
  const chatHistory: ChatHistory[] = chatHistoryError ? [] : (chatHistoryData || []);

  const filteredChats = chatHistory.filter((chat: ChatHistory) =>
    chat.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
    chat.lastMessage.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const pinnedChats = filteredChats.filter((chat: ChatHistory) => chat.isPinned);
  const regularChats = filteredChats.filter(chat => !chat.isPinned);

  const handleChatClick = (chat: ChatHistory) => {
    setSelectedChat(chat.id);
    onChatSelect(chat);
  };

  const handlePinChat = (chatId: string, isPinned: boolean) => {
    pinChatMutation.mutate({ chatId, isPinned: !isPinned });
  };

  const handleArchiveChat = (chatId: string) => {
    archiveChatMutation.mutate(chatId);
  };

  const handleDeleteChat = (chatId: string) => {
    deleteChatMutation.mutate(chatId);
  };

  const ChatItem = ({ chat }: { chat: ChatHistory }) => {
    const [showMenu, setShowMenu] = useState(false);
    const isSelected = selectedChat === chat.id;

    return (
      <div
        className={cn(
          'relative group cursor-pointer rounded-xl transition-all duration-200',
          isSelected ? 'bg-white/20' : 'hover:bg-white/10'
        )}
        onClick={() => handleChatClick(chat)}
      >
        <div className="flex items-start gap-3 p-3">
          <div className="flex-shrink-0 mt-1">
            <div className="w-8 h-8 bg-gradient-to-br from-blue-500 to-purple-600 rounded-lg flex items-center justify-center">
              <MessageSquare className="w-4 h-4 text-white" />
            </div>
          </div>
          
          <div className="flex-1 min-w-0">
            <div className="flex items-center justify-between mb-1">
              <div className="flex items-center gap-2">
                {chat.isPinned && (
                  <Pin className="w-3 h-3 text-white/60" />
                )}
                <h4 className={cn(
                  'text-sm font-medium truncate',
                  isSelected ? 'text-white' : 'text-white'
                )}>
                  {chat.title}
                </h4>
              </div>
              
              <div className="flex items-center gap-2">
                {chat.unreadCount > 0 && (
                  <span className="px-2 py-1 bg-white/20 text-white text-xs font-medium rounded-full">
                    {chat.unreadCount}
                  </span>
                )}
                <span className="text-xs text-white/60">
                  {formatTime(chat.timestamp)}
                </span>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setShowMenu(!showMenu);
                  }}
                  className="opacity-0 group-hover:opacity-100 w-6 h-6 flex items-center justify-center hover:bg-white/10 rounded-lg transition-all"
                >
                  <MoreVertical className="w-4 h-4 text-white/60" />
                </button>
              </div>
            </div>
            
            <p className="text-sm text-white/60 truncate">
              {chat.lastMessage}
            </p>
            
            <p className="text-xs text-white/40 mt-1">
              {chat.participants.join(', ')}
            </p>
          </div>
        </div>

        {/* Dropdown Menu */}
        {showMenu && (
          <div className="absolute right-2 top-12 w-40 bg-[#1a2359] rounded-xl shadow-lg border border-white/20 z-10 overflow-hidden">
            <div className="py-1">
              <button 
                onClick={(e) => {
                  e.stopPropagation();
                  handlePinChat(chat.id, chat.isPinned);
                  setShowMenu(false);
                }}
                className="w-full text-left px-3 py-2 text-sm text-white hover:bg-white/10 flex items-center gap-2"
              >
                <Pin className="w-4 h-4" />
                {chat.isPinned ? 'Unpin' : 'Pin'}
              </button>
              <button className="w-full text-left px-3 py-2 text-sm text-white hover:bg-white/10 flex items-center gap-2">
                <Edit3 className="w-4 h-4" />
                Rename
              </button>
              <button 
                onClick={(e) => {
                  e.stopPropagation();
                  handleArchiveChat(chat.id);
                  setShowMenu(false);
                }}
                className="w-full text-left px-3 py-2 text-sm text-white hover:bg-white/10 flex items-center gap-2"
              >
                <Archive className="w-4 h-4" />
                Archive
              </button>
              <button 
                onClick={(e) => {
                  e.stopPropagation();
                  handleDeleteChat(chat.id);
                  setShowMenu(false);
                }}
                className="w-full text-left px-3 py-2 text-sm text-red-300 hover:bg-red-500/20 flex items-center gap-2"
              >
                <Trash2 className="w-4 h-4" />
                Delete
              </button>
            </div>
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="p-4 border-b border-white/10">
        <div className="flex items-center justify-between mb-4">
          <h3 
            className="text-lg font-semibold text-white"
          >
            Chat History
          </h3>
          <button
            onClick={onNewChat}
            className="w-8 h-8 rounded-lg bg-white/20 hover:bg-white/30 flex items-center justify-center text-white transition-colors"
          >
            <Plus className="w-4 h-4" />
          </button>
        </div>
        
        {/* Search */}
        <div className="relative">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-white/60" />
          <input
            type="text"
            placeholder="Search chats..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-10 pr-4 py-3 text-sm bg-white/10 rounded-xl focus:bg-white/20 focus:outline-none focus:ring-2 focus:ring-white/30 transition-all text-white placeholder-white/60"
          />
        </div>
      </div>

      {/* Chat List */}
      <div className="flex-1 overflow-y-auto p-2">
        {/* Pinned Chats */}
        {pinnedChats.length > 0 && (
          <div className="mb-4">
            <h4 className="text-xs font-medium text-white/60 uppercase tracking-wide mb-2 px-2">
              Pinned
            </h4>
            <div className="space-y-1">
              {pinnedChats.map((chat) => (
                <ChatItem key={chat.id} chat={chat} />
              ))}
            </div>
          </div>
        )}

        {/* Regular Chats */}
        {regularChats.length > 0 && (
          <div>
            <h4 className="text-xs font-medium text-white/60 uppercase tracking-wide mb-2 px-2">
              Recent
            </h4>
            <div className="space-y-1">
              {regularChats.map((chat) => (
                <ChatItem key={chat.id} chat={chat} />
              ))}
            </div>
          </div>
        )}

        {filteredChats.length === 0 && (
          <div className="flex flex-col items-center justify-center h-full text-white/60">
            <MessageSquare className="w-12 h-12 mb-4 text-white/30" />
            <p className="text-sm">No chats found</p>
          </div>
        )}
      </div>
    </div>
  );
}