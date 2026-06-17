'use client';

import { AlertCircle } from 'lucide-react';
import { useActivityStream } from './hooks/useActivityStream';
import { ActivityFeed } from './ui/ActivityFeed';
import { LoadingState } from './ui/LoadingState';
import { EmptyState } from './ui/EmptyState';

interface ActivityViewProps {
  userId: string;
}

export function ActivityView({ userId }: ActivityViewProps) {
  const { events, isLoading, error } = useActivityStream(userId);

  if (error) {
    return <EmptyState icon={AlertCircle} title="Error loading activity" description={error.message} />;
  }

  return (
    <div className="min-h-dvh bg-[#F7F7FA] p-6">
      <div className="max-w-4xl mx-auto">
        <div className="mb-8">
          <h1 className="text-4xl font-bold text-[#2F3138] mb-2">Activity Stream</h1>
          <p className="text-base text-[#707480]">Recent activities and updates</p>
        </div>

        {isLoading ? <LoadingState /> : events && events.length > 0 ? <ActivityFeed events={events} /> : <EmptyState title="No activity" description="Start by creating something new" />}
      </div>
    </div>
  );
}
