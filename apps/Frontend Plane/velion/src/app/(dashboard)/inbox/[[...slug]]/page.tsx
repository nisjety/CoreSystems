import { requireEdgeUser } from '@/components/auth/lib/edge-session';
import { getServerSidebarNotifications } from '@/lib/server/sidebar-data';

import { InboxSectionPageClient } from './InboxSectionPageClient';

export const dynamic = 'force-dynamic';

export default async function InboxSectionPage({
  params,
}: {
  params: Promise<{ slug?: string[] }>;
}) {
  // G28: edge-gate-stamped user id; falls back to getServerSession() when
  // the header is absent (gate fail-open).
  const { userId } = await requireEdgeUser('/inbox');

  const initialNotifications = await getServerSidebarNotifications(userId);
  const resolvedParams = await params;

  return (
    <InboxSectionPageClient
      initialNotifications={initialNotifications}
      slug={resolvedParams.slug ?? []}
    />
  );
}
