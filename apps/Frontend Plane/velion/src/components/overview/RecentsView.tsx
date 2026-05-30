'use client';

import Link from 'next/link';
import { AlertCircle } from 'lucide-react';
import { useRecentItems } from './hooks/useRecentItems';
import { LoadingState } from './ui/LoadingState';
import { EmptyState } from './ui/EmptyState';

interface RecentsViewProps {
  userId: string;
}

export function RecentsView({ userId }: RecentsViewProps) {
  const { items, isLoading, error } = useRecentItems(userId);

  if (error) {
    return <EmptyState icon={AlertCircle} title="Error loading recents" description={error.message} />;
  }

  return (
    <div className="min-h-dvh bg-[#F7F7FA] p-6">
      <div className="max-w-6xl mx-auto">
        <div className="mb-8">
          <h1 className="text-4xl font-bold text-[#2F3138] mb-2">Recent Items</h1>
          <p className="text-base text-[#707480]">Your recently accessed items</p>
        </div>

        {isLoading ? (
          <LoadingState />
        ) : items && items.length > 0 ? (
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {items.map((item) => (
              <Link key={item.id} href={`/dashboard/${item.type}/${item.id}`}>
                <div className="rounded-[22px] border border-[#E6E8EF] bg-white p-6 hover:shadow-md transition-shadow group">
                  <span className="inline-block px-2 py-1 text-xs font-medium bg-blue-50 text-blue-700 rounded mb-3">{item.type.toUpperCase()}</span>
                  <h3 className="text-lg font-semibold text-[#2F3138] group-hover:underline mb-2">{item.title}</h3>
                  <p className="text-sm text-[#707480] mb-3 line-clamp-2">{item.description}</p>
                  <div className="flex justify-between items-center text-xs text-[#707480]">
                    <span>Modified by {item.actor?.name ?? 'Unknown'}</span>
                    <span>{new Date(item.lastTouched).toLocaleDateString()}</span>
                  </div>
                </div>
              </Link>
            ))}
          </div>
        ) : (
          <EmptyState title="No recent items" description="Items you view will appear here" />
        )}
      </div>
    </div>
  );
}
