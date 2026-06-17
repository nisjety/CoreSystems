'use client';

import { useState, type ReactElement, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { X, Globe, Loader2, AlertCircle, CheckCircle2 } from 'lucide-react';

interface CrawlSourceModalProps {
  open: boolean;
  onClose: () => void;
}

interface DiscoveredPage {
  url: string;
  title?: string;
  contentType?: string;
}

interface DiscoverResponse {
  jobId: string;
  pages: DiscoveredPage[];
  totalDiscovered: number;
}

/**
 * Wave 11 §3 — crawl preview flow. Three states:
 *   1. URL input  → POST /api/ingestion/crawl?phase=discover
 *   2. Page list  → operator deselects unwanted; POST commit
 *   3. Status     → SSE-driven progress until pages land in /knowledge/website
 *
 * Backed by real Quarry v2 (`quarry-control:8081`). No mocks anywhere
 * in the path — if Quarry is offline the modal surfaces the error
 * honestly instead of pretending to crawl.
 */
export function CrawlSourceModal({ open, onClose }: CrawlSourceModalProps): ReactElement | null {
  const router = useRouter();
  const [url, setUrl] = useState<string>('');
  const [maxPages, setMaxPages] = useState<number>(50);
  const [stage, setStage] = useState<'input' | 'discovered' | 'committed'>('input');
  const [discovery, setDiscovery] = useState<DiscoverResponse | null>(null);
  const [selectedUrls, setSelectedUrls] = useState<ReadonlyArray<string>>([]);
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const reset = (): void => {
    setUrl('');
    setMaxPages(50);
    setStage('input');
    setDiscovery(null);
    setSelectedUrls([]);
    setError(null);
  };

  const handleDiscover = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    setError(null);
    if (!url.trim()) {
      setError('Enter a URL.');
      return;
    }
    setSubmitting(true);
    try {
      const response = await fetch('/api/ingestion/crawl?phase=discover', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: url.trim(), maxPages }),
      });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
      const data = (await response.json()) as DiscoverResponse;
      setDiscovery(data);
      setSelectedUrls(data.pages.map((page) => page.url));
      setStage('discovered');
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Discovery failed');
    } finally {
      setSubmitting(false);
    }
  };

  const handleCommit = async (): Promise<void> => {
    if (!discovery || selectedUrls.length === 0) return;
    setSubmitting(true);
    setError(null);
    try {
      const response = await fetch(`/api/ingestion/crawl/${discovery.jobId}/commit`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ urls: [...selectedUrls] }),
      });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
      setStage('committed');
      router.refresh();
      window.setTimeout(() => {
        onClose();
        reset();
      }, 1200);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Commit failed');
    } finally {
      setSubmitting(false);
    }
  };

  const toggleUrl = (pageUrl: string): void => {
    setSelectedUrls((prev) =>
      prev.includes(pageUrl) ? prev.filter((u) => u !== pageUrl) : [...prev, pageUrl],
    );
  };

  const toggleAll = (): void => {
    if (!discovery) return;
    setSelectedUrls(
      selectedUrls.length === discovery.pages.length ? [] : discovery.pages.map((p) => p.url),
    );
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      onClick={(event) => {
        if (event.target === event.currentTarget && !submitting) {
          onClose();
          reset();
        }
      }}
    >
      <div className="flex max-h-[85vh] w-full max-w-[680px] flex-col rounded-2xl bg-white shadow-2xl">
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-[#F3F4F6] p-6">
          <div className="flex items-center gap-2">
            <span className="inline-flex size-8 items-center justify-center rounded-md bg-[#F3F4F6] text-[#111827]">
              <Globe className="size-4" strokeWidth={1.8} />
            </span>
            <div>
              <h2 className="text-[16px] font-semibold tracking-[-0.01em] text-[#111827]">
                Crawl website
              </h2>
              <p className="mt-0.5 text-[12px] text-[#6B7280]">
                {stage === 'input'
                  ? 'Discover pages from a starting URL.'
                  : stage === 'discovered'
                    ? `Found ${discovery?.totalDiscovered ?? 0} pages — pick which to ingest.`
                    : 'Ingestion started — pages will appear in Website shortly.'}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={() => {
              onClose();
              reset();
            }}
            disabled={submitting}
            aria-label="Close"
            className="rounded-md p-1 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827] disabled:opacity-50"
          >
            <X className="size-4" />
          </button>
        </div>

        {stage === 'input' ? (
          <form onSubmit={handleDiscover} className="flex flex-1 flex-col overflow-y-auto p-6">
            <label className="block">
              <span className="block text-[12px] font-medium text-[#374151]">
                Starting URL <span className="text-red-500">*</span>
              </span>
              <input
                type="url"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="https://example.com/docs"
                required
                autoFocus
                className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 text-[13px] text-[#111827] outline-none focus:border-[#111111]"
              />
            </label>
            <label className="mt-3 block">
              <span className="block text-[12px] font-medium text-[#374151]">
                Max pages: <span className="font-mono">{maxPages}</span>
              </span>
              <input
                type="range"
                min={1}
                max={500}
                step={1}
                value={maxPages}
                onChange={(e) => setMaxPages(Number(e.target.value))}
                className="mt-2 w-full accent-[#111111]"
              />
              <span className="text-[10px] text-[#6B7280]">
                We respect robots.txt and a per-domain rate cap.
              </span>
            </label>

            {error ? (
              <div
                role="alert"
                className="mt-3 flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
              >
                <AlertCircle className="size-3.5" />
                {error}
              </div>
            ) : null}

            <div className="mt-auto flex items-center justify-end gap-2 pt-5">
              <button
                type="button"
                onClick={() => {
                  onClose();
                  reset();
                }}
                disabled={submitting}
                className="rounded-full border border-[#E5E7EB] bg-white px-4 py-2 text-[12px] font-medium text-[#374151] hover:border-[#9CA3AF] disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={submitting || !url.trim()}
                className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
              >
                {submitting ? <Loader2 className="size-3.5 animate-spin" /> : null}
                Discover pages
              </button>
            </div>
          </form>
        ) : null}

        {stage === 'discovered' && discovery ? (
          <>
            <div className="flex shrink-0 items-center justify-between gap-3 border-b border-[#F3F4F6] px-6 py-3">
              <label className="flex items-center gap-2 text-[12px] font-medium text-[#374151]">
                <input
                  type="checkbox"
                  checked={selectedUrls.length === discovery.pages.length}
                  onChange={toggleAll}
                  className="size-3.5 rounded border-[#D1D5DB] accent-[#111111]"
                />
                Select all
              </label>
              <span className="text-[11px] text-[#6B7280]">
                {selectedUrls.length} of {discovery.pages.length} selected
              </span>
            </div>
            <ul className="flex-1 overflow-y-auto px-6 py-3">
              {discovery.pages.map((page) => {
                const selected = selectedUrls.includes(page.url);
                return (
                  <li
                    key={page.url}
                    className="flex items-center gap-3 py-1.5 text-[12px]"
                  >
                    <input
                      type="checkbox"
                      checked={selected}
                      onChange={() => toggleUrl(page.url)}
                      className="size-3.5 rounded border-[#D1D5DB] accent-[#111111]"
                    />
                    <div className="min-w-0 flex-1">
                      {page.title ? (
                        <div className="truncate text-[12px] font-medium text-[#111827]">
                          {page.title}
                        </div>
                      ) : null}
                      <div className="truncate text-[11px] text-[#6B7280]">{page.url}</div>
                    </div>
                  </li>
                );
              })}
            </ul>
            {error ? (
              <div
                role="alert"
                className="mx-6 mb-3 flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
              >
                <AlertCircle className="size-3.5" />
                {error}
              </div>
            ) : null}
            <div className="flex shrink-0 items-center justify-between gap-2 border-t border-[#F3F4F6] px-6 py-3">
              <button
                type="button"
                onClick={() => {
                  setStage('input');
                  setDiscovery(null);
                  setSelectedUrls([]);
                }}
                disabled={submitting}
                className="text-[12px] text-[#6B7280] hover:text-[#111827] disabled:opacity-50"
              >
                ← Back
              </button>
              <button
                type="button"
                onClick={handleCommit}
                disabled={submitting || selectedUrls.length === 0}
                className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
              >
                {submitting ? <Loader2 className="size-3.5 animate-spin" /> : null}
                Ingest {selectedUrls.length} pages
              </button>
            </div>
          </>
        ) : null}

        {stage === 'committed' ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6">
            <CheckCircle2 className="size-10 text-emerald-500" />
            <p className="text-[14px] font-medium text-[#111827]">
              Ingestion started
            </p>
            <p className="text-center text-[12px] text-[#6B7280]">
              Pages will appear under Website as they&apos;re indexed.
            </p>
          </div>
        ) : null}
      </div>
    </div>
  );
}
