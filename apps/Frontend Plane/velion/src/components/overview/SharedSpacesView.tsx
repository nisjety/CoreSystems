'use client';

import Link from 'next/link';
import { AlertCircle, Lock, Globe } from 'lucide-react';
import { useSharedSpaces } from './hooks/useSharedSpaces';
import { LoadingState } from './ui/LoadingState';
import { EmptyState } from './ui/EmptyState';

interface SharedSpacesViewProps {
  userId: string;
}

const typeIcons: Record<string, typeof Lock> = {
  private: Lock,
  public: Globe,
  restricted: Lock,
};

export function SharedSpacesView({ userId }: SharedSpacesViewProps) {
  const { spaces, isLoading, error } = useSharedSpaces(userId);

  if (error) {
    return <EmptyState icon={AlertCircle} title="Error loading shared spaces" description={error.message} />;
  }

  if (isLoading) {
    return <LoadingState />;
  }

  if (!spaces || spaces.length === 0) {
    return <EmptyState title="No shared spaces" description="Create or join a shared space to collaborate" />;
  }

  return (
    <div className="min-h-dvh bg-[#F7F7FA] p-6">
      <div className="max-w-6xl mx-auto">
        <div className="mb-8">
          <h1 className="text-4xl font-bold text-[#2F3138] mb-2">Shared Spaces</h1>
          <p className="text-base text-[#707480]">Collaborate with your team</p>
        </div>

        {spaces && spaces.length > 0 ? (
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {spaces.map((space) => {
              const Icon = typeIcons[space.type] || Lock;
              return (
                <Link key={space.id} href={`/dashboard/spaces/${space.id}`}>
                  <div className="rounded-[22px] border border-[#E6E8EF] bg-white p-6 hover:shadow-md transition-shadow group">
                    <div className="flex items-start justify-between mb-3">
                      <Icon className="w-6 h-6 text-[#707480]" />
                      <span className="text-xs font-medium text-[#707480] capitalize">{space.type}</span>
                    </div>
                    <h3 className="text-lg font-semibold text-[#2F3138] group-hover:underline mb-2">{space.name}</h3>
                    <p className="text-sm text-[#707480] mb-4 line-clamp-2">{space.description}</p>
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-[#707480]">{space.members} members</span>
                      <span className="text-[#707480]">Updated {new Date(space.lastUpdated).toLocaleDateString()}</span>
                    </div>
                  </div>
                </Link>
              );
            })}
          </div>
        ) : (
          <EmptyState title="No spaces" description="Create your first shared space" />
        )}
      </div>
    </div>
  );
}
