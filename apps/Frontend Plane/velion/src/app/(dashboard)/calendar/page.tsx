import { cookies } from 'next/headers';

import { requireEdgeUser } from '@/components/auth/lib/edge-session';
import { getServerSidebarCalendarEvents } from '@/lib/server/sidebar-data';

import { CalendarPageClient } from './CalendarPageClient';

type CalendarPageProps = {
  searchParams?: Promise<{ event?: string }>;
};

export const dynamic = 'force-dynamic';

export default async function CalendarPage({ searchParams }: CalendarPageProps) {
  // G28-followup: full identity from edge-gate headers; no auth-core round-trip.
  const { userId, email, name } = await requireEdgeUser('/calendar');

  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  const cookieHeader = (await cookies()).toString();
  const initialEvents = await getServerSidebarCalendarEvents({
    userId,
    email,
    name,
    cookieHeader,
  });

  return (
    <CalendarPageClient
      key={resolvedSearchParams?.event ?? 'default'}
      initialEvents={initialEvents}
      initialSelectedId={resolvedSearchParams?.event ?? null}
    />
  );
}
