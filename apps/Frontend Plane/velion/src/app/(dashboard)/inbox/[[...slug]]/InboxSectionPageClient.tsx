'use client';

import {
  useNotifications,
} from '@/components/core/sidebar/hooks/useRealData';
import type { Notification } from '@/components/core/sidebar/types';
import { InboxWorkspacePage } from '@/components/dashboard/product-section-pages';

interface InboxSectionPageClientProps {
  initialNotifications: Notification[];
  slug?: string[];
}

export function InboxSectionPageClient({
  initialNotifications,
  slug = [],
}: InboxSectionPageClientProps) {
  const { data = initialNotifications } = useNotifications({
    enabled: true,
    initialData: initialNotifications,
  });

  return <InboxWorkspacePage liveCount={data.length} routeSlug={slug} />;
}
