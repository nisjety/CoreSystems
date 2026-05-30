'use client';

import React from 'react';
import Image from 'next/image';
import { useRouter } from 'next/navigation';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';

interface Notification {
  id: string;
  actor: string;
  actorAvatar?: string;
  actorInitials?: string;
  actionText: string;
  href?: string;
  boldText?: string;
  subText?: string;
  timestamp: string;
  badgeColor: string;
  badgeIcon: React.ReactNode;
  read: boolean;
  hasActions?: boolean;
  category?: 'message' | 'mention' | 'system';
}

interface NotificationsDropdownProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  notifications: Notification[];
  trigger: React.ReactNode;
  onNotificationOpen?: (notificationId: string) => void;
}

interface NotificationItemContentProps {
  notification: Notification;
}

function NotificationItemContent({ notification }: NotificationItemContentProps) {
  return (
    <div className="flex items-start gap-3">
      {/* Avatar with badge */}
      <div className="relative shrink-0 w-9 h-9">
        {notification.actorAvatar ? (
          <Image
            src={notification.actorAvatar}
            alt={notification.actor}
            width={36}
            height={36}
            className="w-9 h-9 rounded-full object-cover"
          />
        ) : (
          <div className="w-9 h-9 rounded-full bg-[#E8E8E8] flex items-center justify-center text-xs font-semibold text-[#555555]">
            {notification.actorInitials ?? notification.actor.charAt(0)}
          </div>
        )}
        <div
          className={cn(
            'absolute -bottom-0.5 -right-0.5 w-4 h-4 rounded-full',
            'flex items-center justify-center border-2 border-white',
            notification.badgeColor
          )}
        >
          <div className="text-white [&>svg]:w-2 [&>svg]:h-2 [&>svg]:stroke-[2.5]">
            {notification.badgeIcon}
          </div>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        <div className="flex items-start justify-between gap-2">
          <div className="flex-1 min-w-0">
            <span className="text-sm font-bold text-[#111111]">{notification.actor}</span>
            <span className="text-sm text-[#9B9B9B] ml-1.5">{notification.timestamp}</span>
          </div>
          {!notification.read && (
            <div className="w-2.5 h-2.5 rounded-full bg-[#22C55E] shrink-0 mt-1 mr-1" />
          )}
        </div>
        <p className="text-sm text-[#555555] mt-0.5 leading-snug">
          {notification.actionText}
          {notification.boldText && (
            <span className="font-semibold text-[#111111]"> {notification.boldText}</span>
          )}
        </p>
        {notification.subText && (
          <p className="text-sm text-[#9B9B9B] mt-0.5 truncate">{notification.subText}</p>
        )}
      </div>
    </div>
  );
}

type NotificationTab = 'all' | 'systems' | 'unread';

const TABS: { id: NotificationTab; label: string }[] = [
  { id: 'all',     label: 'All' },
  { id: 'systems', label: 'Systems' },
  { id: 'unread',  label: 'Unread' },
];

export function NotificationsDropdown({
  isOpen,
  onOpenChange,
  notifications,
  trigger,
  onNotificationOpen,
}: NotificationsDropdownProps) {
  const router = useRouter();
  const [hoveredParent, setHoveredParent] = React.useState(false);
  const [activeTab, setActiveTab] = React.useState<NotificationTab>('all');

  React.useEffect(() => {
    if (hoveredParent && !isOpen) {
      onOpenChange(true);
    } else if (!hoveredParent && isOpen) {
      onOpenChange(false);
    }
  }, [hoveredParent, isOpen, onOpenChange]);

  const handleNotificationClick = (notification: Notification, e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    onNotificationOpen?.(notification.id);
    onOpenChange(false);
    router.push(notification.href ?? `/notifications?notification=${encodeURIComponent(notification.id)}`);
  };

  const filtered = React.useMemo(() => {
    switch (activeTab) {
      case 'systems': return notifications.filter((n) => n.category === 'system');
      case 'unread':  return notifications.filter((n) => !n.read);
      default:        return notifications;
    }
  }, [activeTab, notifications]);

  return (
    <div onMouseEnter={() => setHoveredParent(true)} onMouseLeave={() => setHoveredParent(false)}>
      <DropdownMenu open={isOpen} onOpenChange={onOpenChange}>
        <DropdownMenuTrigger asChild id="dashboard-notifications-trigger">
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
            {TABS.map((tab) => (
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

          {/* Notifications List */}
          <div className="overflow-y-auto max-h-[380px]">
            {filtered.length > 0 ? (
              filtered.map((notification, index) => (
                <div
                  key={notification.id}
                  className={cn(
                    index < filtered.length - 1 && 'border-b border-[#F0F0F0]'
                  )}
                >
                  {notification.hasActions ? (
                    <div className="px-4 py-3">
                      <NotificationItemContent notification={notification} />
                      <div className="flex gap-2 mt-2.5 ml-12">
                        <button
                          onClick={(e) => e.stopPropagation()}
                          className="flex-1 py-2 rounded-[10px] bg-[#F2F2F2] text-[13px] font-semibold text-[#111111] hover:bg-[#E8E8E8] transition-colors"
                        >
                          Decline
                        </button>
                        <button
                          onClick={(e) => e.stopPropagation()}
                          className="flex-1 py-2 rounded-[10px] bg-[#111111] text-[13px] font-semibold text-white hover:bg-[#333333] transition-colors"
                        >
                          Accept
                        </button>
                      </div>
                    </div>
                  ) : (
                    <button
                      onClick={(e) => handleNotificationClick(notification, e)}
                      className="w-full px-4 py-3 text-left hover:bg-[#FAFAFA] transition-colors focus:outline-none"
                    >
                      <NotificationItemContent notification={notification} />
                    </button>
                  )}
                </div>
              ))
            ) : (
              <div className="px-5 py-10 text-center">
                <p className="text-sm text-[#888888]">No notifications</p>
              </div>
            )}
          </div>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
