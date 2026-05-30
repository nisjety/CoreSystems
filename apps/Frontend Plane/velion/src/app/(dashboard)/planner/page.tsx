import type { Metadata } from 'next';
import { redirect } from 'next/navigation';

import { PlannerPageClient } from '@/components/planner/PlannerPageClient';
import { resolvePlannerWorkspace } from '@/lib/server/planner-workspace';

export const metadata: Metadata = {
  title: 'Planner',
  description: 'Write, draw and plan in your own frontend, with BlockSuite on the canvas.',
};

export const dynamic = 'force-dynamic';

export default async function PlannerPage() {
  const workspace = await resolvePlannerWorkspace();

  if (!workspace) {
    redirect('/login?redirect=%2Fplanner');
  }

  return <PlannerPageClient initialWorkspace={workspace} />;
}
