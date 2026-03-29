'use client';

import React, { useState } from 'react';
import { Notification } from '../types';
import { cn, formatTime, getPriorityColor } from '../utils';
import { 
  Bell, 
  Mail, 
  Users, 
  Calendar, 
  Settings, 
  Check,
  Trash2
} from 'lucide-react';

interface NotificationPanelProps {
  notifications: Notification[];
  onNotificationClick: (notification: Notification) => void;
  onMarkAsRead: (notificationId: string) => void;
  onClearAll: () => void;
  onTitleClick?: () => void;
}

export function NotificationPanel({ 
  notifications, 
  onNotificationClick, 
  onMarkAsRead, 
  onClearAll,
  onTitleClick
}: NotificationPanelProps) {
  const [filter, setFilter] = useState<'all' | 'unread'>('all');

  const handleNotificationActivate = (notification: Notification) => {
    onNotificationClick(notification);
    if (!notification.read) {
      onMarkAsRead(notification.id);
    }
  };
  
  const filteredNotifications = notifications.filter(notification => 
    filter === 'all' || !notification.read
  );
  
  const unreadCount = notifications.filter(n => !n.read).length;

  const getNotificationIcon = (type: Notification['type']) => {
    switch (type) {
      case 'email':
        return <Mail className="w-4 h-4" />;
      case 'teams':
        return <Users className="w-4 h-4" />;
      case 'calendar':
        return <Calendar className="w-4 h-4" />;
      case 'system':
        return <Settings className="w-4 h-4" />;
      default:
        return <Bell className="w-4 h-4" />;
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 border-b border-black/8 bg-[#F7F4EE]">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <Bell className="w-5 h-5 text-[#B96618]" />
            {onTitleClick ? (
              <button
                type="button"
                className="text-lg font-semibold text-black transition-colors hover:text-[#B96618]"
                onClick={onTitleClick}
                title="Go to Notifications page"
              >
                Notifications
              </button>
            ) : (
              <h3 className="text-lg font-semibold text-black">Notifications</h3>
            )}
            {unreadCount > 0 && (
              <div className="px-2 py-1 bg-[#FFF1DE] text-[#B96618] text-xs font-medium rounded-full border border-[#F2C89C]">
                {unreadCount}
              </div>
            )}
          </div>
          
          {notifications.length > 0 && (
            <button
              onClick={onClearAll}
              className="text-sm text-black/48 hover:text-black flex items-center gap-1"
            >
              <Trash2 className="w-4 h-4" />
              Clear all
            </button>
          )}
        </div>
        
        {/* Filter Tabs */}
        <div className="flex space-x-1 rounded-xl border border-black/8 bg-white p-1">
          {[
            { key: 'all', label: 'All', count: notifications.length },
            { key: 'unread', label: 'Unread', count: unreadCount }
          ].map((tab) => (
            <button
              key={tab.key}
              onClick={() => setFilter(tab.key as 'all' | 'unread')}
              className={cn(
                'flex-1 py-2 px-3 text-sm font-medium rounded-md transition-all duration-200',
                filter === tab.key
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

      {/* Notifications List */}
      <div className="flex-1 overflow-y-auto">
        {filteredNotifications.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-black/45">
            <Bell className="w-12 h-12 mb-4 text-black/18" />
            <p className="text-sm">
              {filter === 'unread' ? 'No unread notifications' : 'No notifications yet'}
            </p>
          </div>
        ) : (
          <div className="space-y-2 p-3">
            {filteredNotifications.map((notification) => (
              <div
                key={notification.id}
                className={cn(
                  'flex items-start gap-3 p-3 rounded-[18px] border transition-all duration-200 cursor-pointer group',
                  !notification.read 
                    ? 'border-[#E8D4BF] bg-[#FFF8EF] hover:bg-[#FFF3E1]' 
                    : 'border-black/8 bg-white hover:bg-[#FBFAF7]'
                )}
                onClick={() => handleNotificationActivate(notification)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    handleNotificationActivate(notification);
                  }
                }}
                role="button"
                tabIndex={0}
              >
                <div className={cn(
                  'shrink-0 w-8 h-8 rounded-lg flex items-center justify-center',
                  getPriorityColor(notification.priority)
                )}>
                  {getNotificationIcon(notification.type)}
                </div>
                
                <div className="flex-1 min-w-0">
                  <div className="flex items-start justify-between mb-1">
                    <h4 className={cn(
                      'text-sm font-medium',
                      !notification.read ? 'text-black' : 'text-black/74'
                    )}>
                      {notification.title}
                    </h4>
                    <div className="flex items-center gap-2 ml-2">
                      <span className="text-xs text-black/42 shrink-0">
                        {formatTime(notification.timestamp)}
                      </span>
                      {!notification.read && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            onMarkAsRead(notification.id);
                          }}
                          className="opacity-0 group-hover:opacity-100 w-6 h-6 flex items-center justify-center hover:bg-emerald-500/12 rounded-full transition-all"
                        >
                          <Check className="w-3 h-3 text-emerald-600" />
                        </button>
                      )}
                    </div>
                  </div>
                  
                  <p className="text-sm text-black/50 line-clamp-2">
                    {notification.description}
                  </p>
                  
                  {!notification.read && (
                    <div className="w-2 h-2 bg-[#DD7A1F] rounded-full mt-2"></div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}