'use client';

import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { ArrowRight, ChevronRight, FileText } from 'lucide-react';

import { IntegrationList } from '@/components/integrations/IntegrationList';
import { IndexStatus } from '@/components/knowledge/IndexStatus';
import { SourcesList, type KnowledgeSource } from '@/components/knowledge/SourcesList';
import type {
  KnowledgeDocumentSummary,
  KnowledgeIntegrationsResponse,
  KnowledgeSourcesResponse,
} from '@/lib/integrations/types';

const SECTIONS = [
  {
    href: '/knowledge/api-integrations',
    label: 'API integrations',
    description: 'View product integrations already connected to the workspace.',
  },
  {
    href: '/knowledge/sources',
    label: 'Sources',
    description: 'See links and websites crawled through Quarry for the organization.',
  },
  {
    href: '/knowledge/documents',
    label: 'Documents',
    description: 'Browse files and indexed documents stored in the knowledge layer.',
  },
];

interface KnowledgePageClientProps {
  documents: KnowledgeDocumentSummary[];
  integrations: KnowledgeIntegrationsResponse;
  sources: KnowledgeSourcesResponse;
}

export function KnowledgePageClient({
  documents,
  integrations,
  sources,
}: KnowledgePageClientProps) {
  const router = useRouter();
  const integrationProviders = integrations.providers ?? [];
  const sourceRows: KnowledgeSource[] = (sources.sources ?? []).map((source) => ({
    id: source.id,
    name: source.name,
    type: 'website',
    url: source.url,
    status: source.status,
    pageCount: source.pageCount,
    lastIndexed: source.lastIndexed ?? undefined,
  }));

  const indexStatus = {
    totalSources: sourceRows.length,
    activeSources: sourceRows.filter((source) => source.status === 'active').length,
    totalPages: sourceRows.reduce((sum, source) => sum + (source.pageCount ?? 0), 0),
    lastIndexed: sourceRows
      .map((source) => source.lastIndexed)
      .filter((value): value is string => Boolean(value))
      .sort()
      .at(-1),
    isIndexing: sourceRows.some((source) => source.status === 'indexing' || source.status === 'pending'),
    errorCount: sourceRows.filter((source) => source.status === 'error').length,
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[760px] px-6 py-10">
        <h1 className="mb-10 text-[22px] font-semibold tracking-tight text-[#111111]">
          Knowledge
        </h1>

        <div className="space-y-10">
          <section>
            <p className="mb-4 text-[12px] font-medium text-[#6B7280]">Index status</p>
            <IndexStatus data={indexStatus} />
          </section>

          <section>
            <p className="mb-4 text-[12px] font-medium text-[#6B7280]">Connected integrations</p>
            <IntegrationList providers={integrationProviders.slice(0, 4)} mode="connected" />
          </section>

          <section>
            <SourcesList
              sources={sourceRows.slice(0, 5)}
              onAdd={() => router.push('/knowledge/sources')}
            />
          </section>

          <section>
            <div className="mb-4 flex items-center justify-between">
              <p className="text-[12px] font-medium text-[#6B7280]">Recent documents</p>
              <Link
                href="/knowledge/documents"
                className="text-[12px] text-[#6B7280] transition-colors hover:text-[#111111]"
              >
                View all
              </Link>
            </div>

            {documents.length === 0 ? (
              <div className="rounded-[10px] border border-dashed border-[#E0E0E0] py-12 text-center">
                <FileText className="mx-auto mb-3 h-5 w-5 text-[#D0D0D0]" />
                <p className="text-[13px] font-medium text-[#111111]">No documents yet</p>
                <p className="mt-1 text-[12px] text-[#9BA3AF]">
                  Documents appear here once websites or other data sources are indexed.
                </p>
              </div>
            ) : (
              <div className="border-t border-[#F0F0F0]">
                {documents.map((document) => (
                  <div
                    key={document.id}
                    className="flex items-center gap-3 border-b border-[#F0F0F0] py-3"
                  >
                    <FileText className="h-3.5 w-3.5 shrink-0 text-[#9BA3AF]" />
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[13px] font-medium text-[#111111]">
                        {document.title}
                      </p>
                      <p className="truncate text-[11px] text-[#9BA3AF]">
                        {document.sourceUrl || document.source}
                      </p>
                    </div>
                    <span className="shrink-0 text-[11px] uppercase tracking-wide text-[#9BA3AF]">
                      {document.type}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section>
            <p className="mb-4 text-[12px] font-medium text-[#6B7280]">Explore</p>
            <div className="border-t border-[#F0F0F0]">
              {SECTIONS.map((section) => (
                <Link
                  key={section.href}
                  href={section.href}
                  className="flex items-center justify-between border-b border-[#F0F0F0] py-4 transition-colors hover:bg-[#F9F9F9]"
                >
                  <div>
                    <p className="text-[13px] font-medium text-[#111111]">{section.label}</p>
                    <p className="mt-0.5 text-[12px] leading-5 text-[#6B7280]">
                      {section.description}
                    </p>
                  </div>
                  <ChevronRight className="ml-4 h-4 w-4 shrink-0 text-[#D0D0D0]" />
                </Link>
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
