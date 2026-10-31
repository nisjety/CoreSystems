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
      {/* Header */}
      <div className="p-4 border-b border-white/10">
        <div className="flex items-center justify-between mb-4">
          <h3 
            className={cn(
              "text-lg font-semibold",
              onTitleClick ? "text-white cursor-pointer hover:text-white/80 transition-colors" : "text-white"
            )}
            onClick={onTitleClick}
            title={onTitleClick ? "Go to Messages page" : undefined}
          >
            Messages
          </h3>
          {unreadCount > 0 && (
            <div className="px-2 py-1 bg-white/20 text-white text-xs font-medium rounded-full">
              {unreadCount}
            </div>
          )}
        </div>
        
        {/* Tabs */}
        <div className="flex space-x-1 bg-white/10 rounded-lg p-1">
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
                  ? 'bg-white/20 text-white shadow-sm'
                  : 'text-white/60 hover:text-white'
              )}
            >
              {tab.label}
              {tab.count > 0 && (
                <span className="ml-1 text-xs text-white/60">
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
          <div className="flex flex-col items-center justify-center h-full text-white/60">
            <MessageSquare className="w-12 h-12 mb-4 text-white/30" />
            <p className="text-sm">No messages yet</p>
          </div>
        ) : (
          <div className="space-y-1 p-2">
            {filteredMessages.map((message) => (
              <div
                key={message.id}
                className={cn(
                  'flex items-start gap-3 p-3 rounded-xl transition-all duration-200 cursor-pointer group',
                  !message.read 
                    ? 'bg-white/20 hover:bg-white/30' 
                    : 'hover:bg-white/10'
                )}
                onClick={() => {
                  onMessageClick(message);
                  if (!message.read) {
                    onMarkAsRead(message.id);
                  }
                }}
              >
                <div className={cn(
                  'flex-shrink-0 w-8 h-8 rounded-lg flex items-center justify-center',
                  getPriorityColor(message.priority)
                )}>
                  {getMessageIcon(message.type)}
                </div>
                
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between mb-1">
                    <h4 className={cn(
                      'text-sm truncate',
                      !message.read ? 'font-semibold text-white' : 'font-medium text-white/80'
                    )}>
                      {message.from.name}
                    </h4>
                    <span className="text-xs text-white/60 flex-shrink-0 ml-2">
                      {formatTime(message.timestamp)}
                    </span>
                  </div>
                  
                  <p className={cn(
                    'text-sm text-white/60 line-clamp-2',
                    !message.read && 'text-white/80'
                  )}>
                    {truncateText(message.content, 80)}
                  </p>
                  
                  {!message.read && (
                    <div className="w-2 h-2 bg-white/60 rounded-full mt-2"></div>
                  )}
                </div>
                
                <ChevronRight className="w-4 h-4 text-white/60 opacity-0 group-hover:opacity-100 transition-opacity" />
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}