'use client';

import { useState, type ReactElement } from 'react';
import { useRouter } from 'next/navigation';
import { Plus, Globe, RotateCw, ExternalLink, Trash2 } from 'lucide-react';
import { CrawlSourceModal } from '@/components/knowledge/modals/CrawlSourceModal';
import type { KnowledgeSourceSummary } from '@/lib/integrations/types';

interface WebsitePaneClientProps {
  sources: KnowledgeSourceSummary[];
}

const STATUS_STYLES: Record<string, string> = {
  active: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
  pending: 'bg-amber-50 text-amber-700 ring-amber-200',
  indexing: 'bg-blue-50 text-blue-700 ring-blue-200',
  error: 'bg-red-50 text-red-700 ring-red-200',
};

function formatRelative(iso?: string | null): string {
  if (!iso) return '—';
  const ms = Date.now() - Date.parse(iso);
  if (Number.isNaN(ms)) return '—';
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export function WebsitePaneClient({ sources }: WebsitePaneClientProps): ReactElement {
  const router = useRouter();
  const [open, setOpen] = useState<boolean>(false);
  const [busy, setBusy] = useState<string | null>(null);

  const handleRecrawl = async (sourceId: string): Promise<void> => {
    setBusy(sourceId);
    try {
      const response = await fetch('/api/ingestion/crawl', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: sourceId }),
      });
      if (response.ok) router.refresh();
    } finally {
      setBusy(null);
    }
  };

  const handleDelete = async (sourceId: string): Promise<void> => {
    setBusy(sourceId);
    try {
      const response = await fetch(
        `/api/knowledge/sources/${encodeURIComponent(sourceId)}`,
        { method: 'DELETE' },
      );
      if (response.ok) router.refresh();
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="px-6 py-6">
      <header className="mb-5 flex items-center justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[20px] font-semibold tracking-[-0.02em] text-[#111827]">
            <Globe className="size-4 text-[#6B7280]" />
            Website
          </h1>
          <p className="mt-0.5 text-[12px] text-[#6B7280]">
            Crawl pages from public URLs with Quarry. Preview the page list
            before committing the ingest.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B]"
        >
          <Plus className="size-3.5" />
          Crawl URL
        </button>
      </header>

      {sources.length === 0 ? (
        <div className="flex min-h-[200px] flex-col items-center justify-center rounded-xl border border-dashed border-[#E5E7EB] bg-[#FAFAFA] px-6 py-10 text-center">
          <p className="text-[14px] font-medium text-[#111827]">No crawls yet</p>
          <p className="mt-1 max-w-[40ch] text-[12px] leading-5 text-[#6B7280]">
            Paste a URL and we&apos;ll discover its pages first — you confirm before
            anything is indexed.
          </p>
        </div>
      ) : (
        <ul className="grid gap-2 md:grid-cols-2">
          {sources.map((source) => (
            <li
              key={source.id}
              className="flex items-start justify-between gap-3 rounded-xl border border-[#E5E7EB] bg-white p-4"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <h3 className="truncate text-[13px] font-semibold text-[#111827]">
                    {source.name}
                  </h3>
                  <span
                    className={`inline-flex items-center rounded-full px-1.5 py-0 text-[9px] font-medium uppercase tracking-wide ring-1 ring-inset ${
                      STATUS_STYLES[source.status] ?? STATUS_STYLES.active
                    }`}
                  >
                    {source.status}
                  </span>
                </div>
                <a
                  href={source.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="mt-0.5 inline-flex items-center gap-1 truncate text-[11px] text-[#6B7280] hover:text-[#111827]"
                >
                  {source.url}
                  <ExternalLink className="size-3" />
                </a>
                <div className="mt-2 flex items-center gap-3 text-[11px] text-[#9CA3AF]">
                  <span>{source.pageCount} pages</span>
                  <span>·</span>
                  <span>Last indexed {formatRelative(source.lastIndexed)}</span>
                </div>
              </div>
              <div className="flex shrink-0 items-start gap-1">
                <button
                  type="button"
                  onClick={() => handleRecrawl(source.url)}
                  disabled={busy === source.id}
                  aria-label="Recrawl"
                  className="rounded-md p-1.5 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827] disabled:opacity-50"
                >
                  <RotateCw className={`size-3.5 ${busy === source.id ? 'animate-spin' : ''}`} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDelete(source.id)}
                  disabled={busy === source.id}
                  aria-label="Delete"
                  className="rounded-md p-1.5 text-[#6B7280] hover:bg-red-50 hover:text-red-600 disabled:opacity-50"
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <CrawlSourceModal open={open} onClose={() => setOpen(false)} />
    </div>
  );
}
