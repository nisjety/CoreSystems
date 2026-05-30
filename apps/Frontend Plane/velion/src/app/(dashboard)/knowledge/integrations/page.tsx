import Link from 'next/link';
import { Plug, ExternalLink } from 'lucide-react';
import { IntegrationList } from '@/components/integrations/IntegrationList';
import { getKnowledgeIntegrations } from '@/app/api/knowledge/_lib/knowledge-data';

export const dynamic = 'force-dynamic';

export default async function KnowledgeIntegrationsPage() {
  // `getKnowledgeIntegrations` already degrades gracefully when
  // integration-engine is offline (returns an empty providers array
  // with the local catalog fallback baked into the data layer).
  let providers: Awaited<ReturnType<typeof getKnowledgeIntegrations>>['providers'] = [];
  try {
    const result = await getKnowledgeIntegrations();
    providers = result.providers;
  } catch {
    providers = [];
  }

  return (
    <div className="px-6 py-6">
      <header className="mb-5 flex items-start justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[20px] font-semibold tracking-[-0.02em] text-[#111827]">
            <Plug className="size-4 text-[#6B7280]" />
            Integrations
          </h1>
          <p className="mt-0.5 max-w-[60ch] text-[12px] text-[#6B7280]">
            Sync from external services. Once connected, the integration
            backfills your knowledge base and keeps it in sync.
          </p>
        </div>
        <Link
          href="/settings/integrations"
          className="inline-flex items-center gap-1.5 rounded-full border border-[#E5E7EB] bg-white px-3.5 py-1.5 text-[12px] font-medium text-[#6B7280] hover:border-[#111111] hover:text-[#111827]"
        >
          Manage connections
          <ExternalLink className="size-3" />
        </Link>
      </header>

      <IntegrationList providers={providers} />
    </div>
  );
}
