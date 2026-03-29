'use client';

import React from 'react';
import { useSearchParams } from 'next/navigation';
import { Bell, Sparkles } from 'lucide-react';
import { NotificationPanel } from '@/components/core/sidebar/components/NotificationPanel';
import { SidebarItemDetailDrawer, type SidebarDetailItem } from '@/components/core/sidebar/components/SidebarItemDetailDrawer';
import {
  useClearAllNotifications,
  useMarkNotificationAsRead,
  useNotifications,
} from '@/components/core/sidebar/hooks/useRealData';
import type { Notification } from '@/components/core/sidebar/types';

function normalizeNotifications(data: unknown): Notification[] {
  if (Array.isArray(data)) {
    return data as Notification[];
  }

  if (data && typeof data === 'object') {
    const wrapped = data as { notifications?: unknown };
    if (Array.isArray(wrapped.notifications)) {
      return wrapped.notifications as Notification[];
    }
  }

  return [];
}

export default function NotificationsPage() {
  const searchParams = useSearchParams();
  const { data } = useNotifications({ enabled: true });
  const markNotificationAsRead = useMarkNotificationAsRead();
  const clearAllNotifications = useClearAllNotifications();
  const [selectedItem, setSelectedItem] = React.useState<SidebarDetailItem | null>(null);

  const notifications = React.useMemo(() => normalizeNotifications(data), [data]);
  const selectedId = searchParams.get('notification');

  React.useEffect(() => {
    if (!selectedId) {
      return;
    }

    const selectedNotification = notifications.find((notification) => notification.id === selectedId);
    if (selectedNotification) {
      setSelectedItem({ kind: 'notification', item: selectedNotification });
    }
  }, [notifications, selectedId]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-[#F5F3EE]">
      <div className="w-full space-y-6 px-4 py-6 md:px-6 md:py-7 xl:px-7">
        <div className="rounded-[28px] border border-black/8 bg-[#FCFBF8] p-6 shadow-[0_22px_48px_rgba(22,20,17,0.08)]">
          <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
            <div>
              <div className="text-[11px] font-semibold uppercase tracking-[0.28em] text-[#B96618]">Inbox Surface</div>
              <h1 className="mt-2 text-3xl font-semibold tracking-tight text-black">Notifications</h1>
              <p className="mt-2 max-w-2xl text-sm leading-6 text-black/54">
                Review system updates, mentions, and workflow alerts in a dedicated destination that matches the sidebar language.
              </p>
            </div>
            <div className="flex items-center gap-2 rounded-full border border-[#E8D4BF] bg-[#FFF8EF] px-3 py-2 text-sm text-[#B96618]">
              <Sparkles className="h-4 w-4" />
              {notifications.length} items synced
            </div>
          </div>
        </div>

        <div className="grid gap-6 xl:grid-cols-[390px_minmax(0,1fr)]">
          <div className="overflow-hidden rounded-[28px] border border-black/8 bg-[#FCFBF8] shadow-[0_18px_40px_rgba(22,20,17,0.08)]">
            <NotificationPanel
              notifications={notifications}
              onNotificationClick={(notification) => {
                setSelectedItem({ kind: 'notification', item: notification });
                if (!notification.read) {
                  markNotificationAsRead.mutate(notification.id);
                }
              }}
              onMarkAsRead={(id) => markNotificationAsRead.mutate(id)}
              onClearAll={() => clearAllNotifications.mutate()}
            />
          </div>

          <div className="rounded-[28px] border border-black/8 bg-[#FCFBF8] p-6 shadow-[0_18px_40px_rgba(22,20,17,0.08)]">
            <div className="flex h-full flex-col justify-between">
              <div>
                <div className="flex h-12 w-12 items-center justify-center rounded-2xl border border-[#E8D4BF] bg-[#FFF8EF] text-[#B96618]">
                  <Bell className="h-5 w-5" />
                </div>
                <h2 className="mt-5 text-2xl font-semibold text-black">Action-forward notification review</h2>
                <p className="mt-3 max-w-xl text-sm leading-6 text-black/56">
                  Select any notification to open the inspector drawer. From there you can open the source page, jump into the related chat, or keep triaging from the list.
                </p>
              </div>

              <div className="mt-8 grid gap-3 md:grid-cols-2">
                <div className="rounded-[22px] border border-black/8 bg-[#F7F4EE] p-4">
                  <div className="text-[10px] font-semibold uppercase tracking-[0.24em] text-black/34">Source page</div>
                  <div className="mt-2 text-sm leading-6 text-black/62">Use the primary CTA to open the most relevant destination for the selected alert.</div>
                </div>
                <div className="rounded-[22px] border border-black/8 bg-[#F7F4EE] p-4">
                  <div className="text-[10px] font-semibold uppercase tracking-[0.24em] text-black/34">Related chat</div>
                  <div className="mt-2 text-sm leading-6 text-black/62">Notifications that connect to conversations can jump you directly back into the right thread.</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {selectedItem && (
        <SidebarItemDetailDrawer item={selectedItem} onClose={() => setSelectedItem(null)} />
      )}
    </div>
  );
}
