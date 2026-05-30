import { requireEdgeUser } from '@/components/auth/lib/edge-session';
import { getServerSidebarNotifications } from '@/lib/server/sidebar-data';

import { NotificationsPageClient } from './NotificationsPageClient';

type NotificationsPageProps = {
  searchParams?: Promise<{ notification?: string }>;
};

export const dynamic = 'force-dynamic';

export default async function NotificationsPage({ searchParams }: NotificationsPageProps) {
  // G28: edge-gate-stamped user id; falls back to getServerSession() when
  // the header is absent (gate fail-open).
  const { userId } = await requireEdgeUser('/notifications');

  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  const initialNotifications = await getServerSidebarNotifications(userId);

  return (
    <NotificationsPageClient
      key={resolvedSearchParams?.notification ?? 'default'}
      initialNotifications={initialNotifications}
      initialSelectedId={resolvedSearchParams?.notification ?? null}
    />
  );
}
