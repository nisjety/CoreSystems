'use client';

import Link from 'next/link';
import { Users, MessageSquare, Zap, TrendingUp, Share2, AlertCircle } from 'lucide-react';
import { useOverviewMetrics } from './hooks/useOverviewMetrics';
import { MetricCard } from './ui/MetricCard';
import { LoadingState } from './ui/LoadingState';
import { EmptyState } from './ui/EmptyState';

interface OverviewLandingProps {
  userId: string;
}

export function OverviewLanding({ userId }: OverviewLandingProps) {
  const { metrics, isLoading, error } = useOverviewMetrics(userId);

  if (error) {
    return <EmptyState icon={AlertCircle} title="Error loading metrics" description={error.message} />;
  }

  if (!metrics) {
    return <LoadingState />;
  }

  return (
    <div className="min-h-dvh bg-[#F7F7FA] p-6">
      <div className="max-w-7xl mx-auto">
        <div className="mb-8">
          <h1 className="text-4xl font-bold text-[#2F3138] mb-2">Overview</h1>
          <p className="text-base text-[#707480]">Your dashboard at a glance</p>
        </div>

        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3 mb-8">
          {Object.entries(metrics).map(([key, metric]) => (
            <MetricCard key={key} metric={metric} />
          ))}
        </div>

        <div>
          <h2 className="text-2xl font-bold text-[#2F3138] mb-4">Quick Navigation</h2>
          <div className="grid gap-4 md:grid-cols-2">
            <Link href="/dashboard/overview/activity" className="rounded-lg border border-[#E6E8EF] bg-white p-6 hover:bg-[#F7F7FA] transition-colors group">
              <MessageSquare className="w-6 h-6 text-blue-600 mb-3 group-hover:scale-110 transition-transform" />
              <h3 className="font-semibold text-[#2F3138] group-hover:underline">Activity Stream</h3>
              <p className="text-sm text-[#707480]">View recent activities and updates</p>
            </Link>

            <Link href="/dashboard/overview/recents" className="rounded-lg border border-[#E6E8EF] bg-white p-6 hover:bg-[#F7F7FA] transition-colors group">
              <TrendingUp className="w-6 h-6 text-purple-600 mb-3 group-hover:scale-110 transition-transform" />
              <h3 className="font-semibold text-[#2F3138] group-hover:underline">Recent Items</h3>
              <p className="text-sm text-[#707480]">Access your recent work</p>
            </Link>

            <Link href="/dashboard/overview/leads" className="rounded-lg border border-[#E6E8EF] bg-white p-6 hover:bg-[#F7F7FA] transition-colors group">
              <Users className="w-6 h-6 text-green-600 mb-3 group-hover:scale-110 transition-transform" />
              <h3 className="font-semibold text-[#2F3138] group-hover:underline">Leads</h3>
              <p className="text-sm text-[#707480]">Manage opportunities and leads</p>
            </Link>

            <Link href="/dashboard/overview/shared-spaces" className="rounded-lg border border-[#E6E8EF] bg-white p-6 hover:bg-[#F7F7FA] transition-colors group">
              <Share2 className="w-6 h-6 text-orange-600 mb-3 group-hover:scale-110 transition-transform" />
              <h3 className="font-semibold text-[#2F3138] group-hover:underline">Shared Spaces</h3>
              <p className="text-sm text-[#707480]">Collaborate with your team</p>
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}
