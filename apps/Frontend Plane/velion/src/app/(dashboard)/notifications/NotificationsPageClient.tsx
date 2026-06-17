'use client';

import { useMemo, useState } from 'react';
import { Bell } from 'lucide-react';

import { NotificationPanel } from '@/components/core/sidebar/components/NotificationPanel';
import {
  SidebarItemDetailDrawer,
  type SidebarDetailItem,
} from '@/components/core/sidebar/components/SidebarItemDetailDrawer';
import {
  useClearAllNotifications,
  useMarkNotificationAsRead,
  useNotifications,
} from '@/components/core/sidebar/hooks/useRealData';
import type { Notification } from '@/components/core/sidebar/types';

interface NotificationsPageClientProps {
  initialNotifications: Notification[];
  initialSelectedId: string | null;
}

export function NotificationsPageClient({
  initialNotifications,
  initialSelectedId,
}: NotificationsPageClientProps) {
  const { data = initialNotifications } = useNotifications({
    enabled: true,
    initialData: initialNotifications,
  });
  const markNotificationAsRead = useMarkNotificationAsRead();
  const clearAllNotifications = useClearAllNotifications();
  const [selectedNotificationId, setSelectedNotificationId] = useState<string | null>(
    initialSelectedId,
  );

  const notifications = useMemo(() => data, [data]);
  const selectedItem = useMemo<SidebarDetailItem | null>(() => {
    if (!selectedNotificationId) {
      return null;
    }

    const selectedNotification = notifications.find(
      (notification) => notification.id === selectedNotificationId,
    );

    return selectedNotification
      ? { kind: 'notification', item: selectedNotification }
      : null;
  }, [notifications, selectedNotificationId]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[960px] px-6 py-10">
        <h1 className="mb-8 text-[22px] font-semibold tracking-tight text-[#111111]">
          Notifications
        </h1>

        <div className="grid gap-6 xl:grid-cols-[360px_minmax(0,1fr)]">
          <div className="overflow-hidden rounded-[12px] border border-[#F0F0F0]">
            <NotificationPanel
              notifications={notifications}
              onNotificationClick={(notification) => {
                setSelectedNotificationId(notification.id);
                if (!notification.read) {
                  markNotificationAsRead.mutate(notification.id);
                }
              }}
              onMarkAsRead={(id) => markNotificationAsRead.mutate(id)}
              onClearAll={() => clearAllNotifications.mutate()}
            />
          </div>

          <div className="flex flex-col gap-4">
            {selectedItem ? (
              <SidebarItemDetailDrawer
                item={selectedItem}
                onClose={() => setSelectedNotificationId(null)}
              />
            ) : (
              <div className="flex h-full flex-col items-center justify-center rounded-[12px] border border-[#F0F0F0] px-8 py-16 text-center">
                <div className="flex h-10 w-10 items-center justify-center rounded-full bg-[#F4F6FA]">
                  <Bell className="h-4 w-4 text-[#9BA3AF]" />
                </div>
                <p className="mt-4 text-[13px] font-medium text-[#111111]">
                  Select a notification
                </p>
                <p className="mt-1 text-[12px] leading-5 text-[#9BA3AF]">
                  Click any item in the list to open the details here.
                </p>
              </div>
            )}

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="rounded-[10px] border border-[#F0F0F0] p-4">
                <p className="text-[12px] font-medium text-[#111111]">Source page</p>
                <p className="mt-1 text-[12px] leading-5 text-[#6B7280]">
                  Use the primary action to open the most relevant destination for the selected alert.
                </p>
              </div>
              <div className="rounded-[10px] border border-[#F0F0F0] p-4">
                <p className="text-[12px] font-medium text-[#111111]">Related chat</p>
                <p className="mt-1 text-[12px] leading-5 text-[#6B7280]">
                  Notifications that connect to conversations can jump you directly back into the right thread.
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {selectedItem && (
        <SidebarItemDetailDrawer item={selectedItem} onClose={() => setSelectedNotificationId(null)} />
      )}
    </div>
  );
}
