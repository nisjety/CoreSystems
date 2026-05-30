'use client';

import dynamic from 'next/dynamic';
import { Suspense } from 'react';

import type { PlannerWorkspaceResolution } from '@/lib/server/planner-workspace';

const PlannerWorkspaceClient = dynamic(
  () => import('./PlannerWorkspaceClient').then((mod) => mod.PlannerWorkspaceClient),
  {
    ssr: false,
    loading: () => (
      <div className="flex h-full items-center justify-center">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-stone-200 border-t-stone-600" />
      </div>
    ),
  }
);

interface PlannerPageClientProps {
  initialWorkspace: PlannerWorkspaceResolution | null;
}

export function PlannerPageClient({ initialWorkspace }: PlannerPageClientProps) {
  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center">
          <div className="h-8 w-8 animate-spin rounded-full border-2 border-stone-200 border-t-stone-600" />
        </div>
      }
    >
      <PlannerWorkspaceClient initialWorkspace={initialWorkspace} />
    </Suspense>
  );
}
