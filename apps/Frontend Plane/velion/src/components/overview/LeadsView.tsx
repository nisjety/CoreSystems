'use client';

import Link from 'next/link';
import { AlertCircle } from 'lucide-react';
import { useLeadFlow } from './hooks/useLeadFlow';
import { LoadingState } from './ui/LoadingState';
import { EmptyState } from './ui/EmptyState';

interface LeadsViewProps {
  userId: string;
}

const stageColors: Record<string, string> = {
  prospect: 'bg-blue-50 text-blue-700',
  qualified: 'bg-purple-50 text-purple-700',
  negotiating: 'bg-amber-50 text-amber-700',
  won: 'bg-green-50 text-green-700',
  lost: 'bg-red-50 text-red-700',
};

export function LeadsView({ userId }: LeadsViewProps) {
  const { leads, isLoading, error } = useLeadFlow(userId);

  if (error) {
    return <EmptyState icon={AlertCircle} title="Error loading leads" description={error.message} />;
  }

  if (isLoading) {
    return <LoadingState />;
  }

  if (!leads || leads.length === 0) {
    return <EmptyState title="No leads" description="Start by creating a new lead opportunity" />;
  }

  const leadsByStage = leads.reduce(
    (acc, lead) => {
      if (!acc[lead.stage]) acc[lead.stage] = [];
      acc[lead.stage].push(lead);
      return acc;
    },
    {} as Record<string, typeof leads>
  );

  return (
    <div className="min-h-dvh bg-[#F7F7FA] p-6">
      <div className="max-w-6xl mx-auto">
        <div className="mb-8">
          <h1 className="text-4xl font-bold text-[#2F3138] mb-2">Leads</h1>
          <p className="text-base text-[#707480]">Manage your sales opportunities</p>
        </div>

        <div className="space-y-8">
          {Object.entries(leadsByStage).map(([stage, stageLeads]) => (
            <div key={stage}>
              <h2 className="text-lg font-semibold text-[#2F3138] mb-4 capitalize">{stage}</h2>
              <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
                {stageLeads.map((lead) => (
                  <Link key={lead.id} href={`/dashboard/leads/${lead.id}`}>
                    <div className="rounded-lg border border-[#E6E8EF] bg-white p-6 hover:shadow-md transition-shadow group">
                      <div className="flex items-start justify-between mb-3">
                        <h3 className="font-semibold text-[#2F3138] group-hover:underline">{lead.company ?? lead.name}</h3>
                        <span className={`inline-block px-2 py-1 text-xs font-medium rounded ${stageColors[stage] || 'bg-gray-50 text-gray-700'}`}>{lead.assignedTo?.name ?? lead.name}</span>
                      </div>
                      <p className="text-sm text-[#707480] mb-3">{lead.source}</p>
                      <div className="flex justify-between items-center text-xs text-[#707480]">
                        <span className="font-semibold text-[#2F3138]">{lead.score}</span>
                        <span>{Math.random() > 0.5 ? 'Hot' : 'Warm'}</span>
                      </div>
                    </div>
                  </Link>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
