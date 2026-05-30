'use client';

import { useState, useMemo, type ReactElement } from 'react';
import { Wand2, Loader2, AlertCircle, Bot, User } from 'lucide-react';
import { ROUTER_ROUTES, type RouterRoute } from '@/lib/knowledge/router-schema';

export interface RoutingMatrixDoc {
  id: string;
  title: string;
  type: string;
  /** Server-supplied current routing (absent for never-classified docs). */
  route?: RouterRoute;
  routedBy?: 'ai' | 'manual';
  confidence?: number;
  reason?: string;
}

interface RoutingMatrixProps {
  documents: RoutingMatrixDoc[];
  onRefetch?: () => void;
}

const ROUTE_STYLES: Record<RouterRoute, string> = {
  rag: 'bg-blue-50 text-blue-700 ring-blue-200',
  graphrag: 'bg-violet-50 text-violet-700 ring-violet-200',
  wiki: 'bg-cyan-50 text-cyan-700 ring-cyan-200',
  finetune: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
  prompt: 'bg-amber-50 text-amber-700 ring-amber-200',
  skip: 'bg-zinc-100 text-zinc-600 ring-zinc-200',
};

const ROUTE_DESCRIPTIONS: Record<RouterRoute, string> = {
  rag: 'Vector retrieval over chunks.',
  graphrag: 'Entity graph + relations.',
  wiki: 'Named-entity / glossary lookup.',
  finetune: 'Style/format exemplar for fine-tuning.',
  prompt: 'Rule appended to system prompt.',
  skip: 'Excluded from retrieval.',
};

/**
 * Wave 11 §7 — per-document routing matrix.
 *
 * Operator can:
 *   - Click "Auto-route all" → POST /api/knowledge/route-classify.
 *   - Override any row via the dropdown → PATCH /…/routing.
 *   - Manual overrides are flagged with the User icon; AI assignments
 *     show the Bot icon. The next auto-route sweep won't clobber
 *     manual overrides unless `force=true`.
 */
export function RoutingMatrix({
  documents,
  onRefetch,
}: RoutingMatrixProps): ReactElement {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [bulkBusy, setBulkBusy] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<RouterRoute | 'all'>('all');

  const filtered = useMemo(() => {
    if (filter === 'all') return documents;
    return documents.filter((doc) => doc.route === filter);
  }, [documents, filter]);

  const counts = useMemo(() => {
    const out: Record<RouterRoute | 'unclassified', number> = {
      rag: 0,
      graphrag: 0,
      wiki: 0,
      finetune: 0,
      prompt: 0,
      skip: 0,
      unclassified: 0,
    };
    for (const doc of documents) {
      if (doc.route) out[doc.route] += 1;
      else out.unclassified += 1;
    }
    return out;
  }, [documents]);

  const handleAutoRouteAll = async (): Promise<void> => {
    setBulkBusy(true);
    setError(null);
    try {
      const response = await fetch('/api/knowledge/route-classify', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({}),
      });
      if (!response.ok) {
        const detail = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(detail?.error ?? `HTTP ${response.status}`);
      }
      onRefetch?.();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Auto-route failed');
    } finally {
      setBulkBusy(false);
    }
  };

  const handleOverride = async (docId: string, route: RouterRoute): Promise<void> => {
    setBusyId(docId);
    setError(null);
    try {
      const response = await fetch(`/api/knowledge/documents/${docId}/routing`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ route }),
      });
      if (!response.ok) {
        const detail = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(detail?.error ?? `HTTP ${response.status}`);
      }
      onRefetch?.();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Override failed');
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="space-y-4">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-[14px] font-semibold text-[#111827]">Training data routing</h2>
          <p className="mt-0.5 text-[12px] text-[#6B7280]">
            How each piece of knowledge is used. AI decides by default; you
            can override any row.
          </p>
        </div>
        <button
          type="button"
          onClick={handleAutoRouteAll}
          disabled={bulkBusy}
          className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:opacity-50"
        >
          {bulkBusy ? <Loader2 className="size-3.5 animate-spin" /> : <Wand2 className="size-3.5" />}
          Auto-route all
        </button>
      </header>

      <div className="flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={() => setFilter('all')}
          className={`rounded-full px-2.5 py-0.5 text-[11px] font-medium ring-1 ring-inset ${
            filter === 'all'
              ? 'bg-[#111111] text-white ring-[#111111]'
              : 'bg-white text-[#374151] ring-[#E5E7EB] hover:border-[#9CA3AF]'
          }`}
        >
          All <span className="font-mono">{documents.length}</span>
        </button>
        {ROUTER_ROUTES.map((route) => (
          <button
            key={route}
            type="button"
            onClick={() => setFilter(route)}
            className={`inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[11px] font-medium ring-1 ring-inset ${
              filter === route
                ? `${ROUTE_STYLES[route]} ring-2`
                : 'bg-white text-[#374151] ring-[#E5E7EB] hover:border-[#9CA3AF]'
            }`}
            title={ROUTE_DESCRIPTIONS[route]}
          >
            <span className="capitalize">{route}</span>
            <span className="font-mono">{counts[route]}</span>
          </button>
        ))}
        {counts.unclassified > 0 ? (
          <span className="inline-flex items-center gap-1 rounded-full bg-white px-2.5 py-0.5 text-[11px] text-[#6B7280] ring-1 ring-inset ring-[#E5E7EB]">
            Unclassified <span className="font-mono">{counts.unclassified}</span>
          </span>
        ) : null}
      </div>

      {error ? (
        <div
          role="alert"
          className="flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
        >
          <AlertCircle className="size-3.5" />
          {error}
        </div>
      ) : null}

      {filtered.length === 0 ? (
        <div className="rounded-xl border border-dashed border-[#E5E7EB] bg-[#FAFAFA] px-6 py-10 text-center">
          <p className="text-[13px] font-medium text-[#111827]">No documents to route</p>
          <p className="mt-1 text-[12px] text-[#6B7280]">
            Add some sources from /knowledge, then come back here to classify them.
          </p>
        </div>
      ) : (
        <ul className="divide-y divide-[#F3F4F6] rounded-xl border border-[#E5E7EB] bg-white">
          {filtered.map((doc) => (
            <li
              key={doc.id}
              className="flex items-center gap-3 px-3 py-2.5"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate text-[13px] font-medium text-[#111827]">
                    {doc.title}
                  </span>
                  <span className="rounded-full bg-[#F3F4F6] px-1.5 py-0 font-mono text-[9px] text-[#6B7280]">
                    {doc.type}
                  </span>
                </div>
                {doc.reason ? (
                  <p className="mt-0.5 truncate text-[11px] text-[#6B7280]">
                    {doc.reason}
                  </p>
                ) : null}
              </div>

              {doc.routedBy ? (
                <span
                  className="inline-flex items-center gap-1 text-[10px] text-[#9CA3AF]"
                  title={
                    doc.routedBy === 'manual'
                      ? 'Manual override — AI auto-route won\'t overwrite this.'
                      : 'AI-classified.'
                  }
                >
                  {doc.routedBy === 'manual' ? (
                    <User className="size-3" />
                  ) : (
                    <Bot className="size-3" />
                  )}
                </span>
              ) : null}

              <select
                value={doc.route ?? ''}
                onChange={(e) => handleOverride(doc.id, e.target.value as RouterRoute)}
                disabled={busyId === doc.id}
                className={`shrink-0 rounded-full border-0 py-0.5 pl-2.5 pr-7 text-[11px] font-medium capitalize ring-1 ring-inset focus:ring-2 disabled:opacity-50 ${
                  doc.route
                    ? `${ROUTE_STYLES[doc.route]} ring-1`
                    : 'bg-white text-[#6B7280] ring-[#E5E7EB]'
                }`}
              >
                {!doc.route ? <option value="">— route —</option> : null}
                {ROUTER_ROUTES.map((route) => (
                  <option key={route} value={route}>
                    {route}
                  </option>
                ))}
              </select>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
