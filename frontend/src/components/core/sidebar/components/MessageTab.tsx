'use client';

import React, { useState } from 'react';
import { Message } from '../types';
import { cn, formatTime, truncateText, getPriorityColor } from '../utils';
import { MessageSquare, Mail, Users, ChevronRight } from 'lucide-react';

interface MessageTabProps {
  messages: Message[];
  onMessageClick: (message: Message) => void;
  onMarkAsRead: (messageId: string) => void;
  onTitleClick?: () => void;
}

export function MessageTab({ messages, onMessageClick, onMarkAsRead, onTitleClick }: MessageTabProps) {
  const [activeTab, setActiveTab] = useState<'all' | 'unread' | 'teams' | 'email'>('all');

  const handleMessageActivate = (message: Message) => {
    onMessageClick(message);
    if (!message.read) {
      onMarkAsRead(message.id);
    }
  };
  
  const filteredMessages = messages.filter(message => {
    if (activeTab === 'all') return true;
    if (activeTab === 'unread') return !message.read;
    return message.type === activeTab;
  });
  
  const unreadCount = messages.filter(m => !m.read).length;

  const getMessageIcon = (type: Message['type']) => {
    switch (type) {
      case 'teams':
        return <Users className="w-4 h-4" />;
      case 'email':
        return <Mail className="w-4 h-4" />;
      case 'chat':
        return <MessageSquare className="w-4 h-4" />;
      default:
        return <MessageSquare className="w-4 h-4" />;
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 border-b border-black/8 bg-[#F7F4EE]">
        <div className="flex items-center justify-between mb-4">
          {onTitleClick ? (
            <button
              type="button"
              className="text-lg font-semibold text-black transition-colors hover:text-[#B96618]"
              onClick={onTitleClick}
              title="Go to Messages page"
            >
              Messages
            </button>
          ) : (
            <h3 className="text-lg font-semibold text-black">Messages</h3>
          )}
          {unreadCount > 0 && (
            <div className="px-2 py-1 bg-[#FFF1DE] text-[#B96618] text-xs font-medium rounded-full border border-[#F2C89C]">
              {unreadCount}
            </div>
          )}
        </div>
        
        {/* Tabs */}
        <div className="flex space-x-1 rounded-xl border border-black/8 bg-white p-1">
          {[
            { key: 'all', label: 'All', count: messages.length },
            { key: 'teams', label: 'Teams', count: messages.filter(m => m.type === 'teams').length },
            { key: 'email', label: 'Email', count: messages.filter(m => m.type === 'email').length }
          ].map((tab) => (
            <button
              key={tab.key}
              onClick={() => setActiveTab(tab.key as 'all' | 'unread' | 'teams' | 'email')}
              className={cn(
                'flex-1 py-2 px-3 text-sm font-medium rounded-md transition-all duration-200',
                activeTab === tab.key
                  ? 'bg-[#171311] text-white shadow-sm'
                  : 'text-black/50 hover:text-black'
              )}
            >
              {tab.label}
              {tab.count > 0 && (
                <span className="ml-1 text-xs text-inherit/70">
                  ({tab.count})
                </span>
              )}
            </button>
          ))}
        </div>
      </div>

      {/* Messages List */}
      <div className="flex-1 overflow-y-auto">
        {filteredMessages.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-black/45">
            <MessageSquare className="w-12 h-12 mb-4 text-black/18" />
            <p className="text-sm">No messages yet</p>
          </div>
        ) : (
          <div className="space-y-2 p-3">
            {filteredMessages.map((message) => (
              <button
                key={message.id}
                type="button"
                className={cn(
                  'group flex w-full items-start gap-3 rounded-[18px] border p-3 text-left transition-all duration-200',
                  !message.read 
                    ? 'border-[#E8D4BF] bg-[#FFF8EF] hover:bg-[#FFF3E1]' 
                    : 'border-black/8 bg-white hover:bg-[#FBFAF7]'
                )}
                onClick={() => handleMessageActivate(message)}
              >
                <div className={cn(
                  'shrink-0 w-8 h-8 rounded-lg flex items-center justify-center',
                  getPriorityColor(message.priority)
                )}>
                  {getMessageIcon(message.type)}
                </div>
                
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between mb-1">
                    <h4 className={cn(
                      'text-sm truncate',
                      !message.read ? 'font-semibold text-black' : 'font-medium text-black/80'
                    )}>
                      {message.from.name}
                    </h4>
                    <span className="ml-2 shrink-0 text-xs text-black/42">
                      {formatTime(message.timestamp)}
                    </span>
                  </div>
                  
                  <p className={cn(
                    'text-sm text-black/50 line-clamp-2',
                    !message.read && 'text-black/70'
                  )}>
                    {truncateText(message.content, 80)}
                  </p>
                  
                  {!message.read && (
                    <div className="w-2 h-2 bg-[#DD7A1F] rounded-full mt-2"></div>
                  )}
                </div>
                
                <ChevronRight className="w-4 h-4 text-black/30 opacity-0 group-hover:opacity-100 transition-opacity" />
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}