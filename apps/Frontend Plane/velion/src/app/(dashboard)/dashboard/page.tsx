import { cookies } from 'next/headers';

import { getDashboardStatsRPC } from '@/lib/rpc/server';
import { requireEdgeUser } from '@/components/auth/lib/edge-session';
import { getGreeting } from '@/components/dashboard/hooks/useGreeting';

import { DashboardPageClient } from './DashboardPageClient';

export const dynamic = 'force-dynamic';

export default async function DashboardPage() {
  // G28-followup: full identity from edge-gate headers (userId+email+name)
  // — no per-page auth-core round-trip. Falls back to getServerSession()
  // when the gate fail-open'd (auth-core was unreachable at gate time).
  const { userId, email, name } = await requireEdgeUser('/dashboard');

  const cookieHeader = (await cookies()).toString();
  const stats = await getDashboardStatsRPC(cookieHeader);

  return (
    <DashboardPageClient
      greeting={getGreeting()}
      stats={stats}
      user={{ id: userId, name, email }}
    />
  );
}
