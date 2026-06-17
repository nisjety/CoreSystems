import Link from 'next/link';
import { formatDistanceToNow } from 'date-fns';
import type { ActivityEvent } from '../lib/overview-types';

interface ActivityFeedProps {
  events: ActivityEvent[];
  isLoading?: boolean;
}

const eventTypeColors: Record<string, string> = {
  conversation: 'bg-blue-50 text-blue-700',
  agent_action: 'bg-purple-50 text-purple-700',
  knowledge_update: 'bg-amber-50 text-amber-700',
  collaboration: 'bg-green-50 text-green-700',
  system_alert: 'bg-red-50 text-red-700',
  notification: 'bg-gray-50 text-gray-700',
};

export function ActivityFeed({ events, isLoading }: ActivityFeedProps) {
  if (isLoading) {
    return (
      <div className="space-y-3">
        {[...Array(4)].map((_, i) => (
          <div key={i} className="h-16 rounded-lg bg-gray-100 animate-pulse" />
        ))}
      </div>
    );
  }

  if (!events || events.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-[#E6E8EF] p-8 text-center">
        <p className="text-[#707480]">No activity yet</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {events.map((event) => (
        <Link key={event.id} href={event.href || '#'}>
          <div className="rounded-lg border border-[#E6E8EF] bg-white p-4 hover:bg-[#F7F7FA] transition-colors group">
            <div className="flex items-start justify-between">
              <div className="flex-1">
                <div className="flex items-center gap-2 mb-1">
                  <span className={`inline-block px-2 py-1 text-xs font-medium rounded ${eventTypeColors[event.type]}`}>
                    {event.type.replace('_', ' ').toUpperCase()}
                  </span>
                </div>
                <p className="text-sm font-medium text-[#2F3138] group-hover:underline">{event.description}</p>
                <p className="text-xs text-[#707480] mt-1">by {event.actor?.name}</p>
              </div>
              <time className="text-xs text-[#707480]">{formatDistanceToNow(new Date(event.timestamp), { addSuffix: true })}</time>
            </div>
          </div>
        </Link>
      ))}
    </div>
  );
}
