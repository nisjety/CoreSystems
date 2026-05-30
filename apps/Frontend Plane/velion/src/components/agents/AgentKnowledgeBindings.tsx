'use client';

import { useState, useEffect, type ReactElement } from 'react';
import { useRouter } from 'next/navigation';
import { SlidersHorizontal, FileText, Check, Loader2, AlertCircle } from 'lucide-react';
import { ConfigureRagModal, type RetrievalConfig } from './ConfigureRagModal';

interface KnowledgeBindings {
  scope: 'all' | 'selected';
  documentIds?: string[];
  sourceIds?: string[];
  includeQnA?: boolean;
}

interface AvailableDoc {
  id: string;
  title: string;
  type: string;
  source: string;
}

interface AgentKnowledgeBindingsProps {
  agentId: string;
  initial: KnowledgeBindings;
  retrievalConfig: RetrievalConfig;
}

/**
 * Wave 11 §6 + §6.5 — per-agent knowledge scope + RAG configuration.
 *
 *   "All org knowledge"    → scope=all
 *   "Selected only"        → chip-picker of doc ids + Q&A toggle
 *   Configure RAG button   → opens ConfigureRagModal (weights, top-k…)
 */
export function AgentKnowledgeBindings({
  agentId,
  initial,
  retrievalConfig,
}: AgentKnowledgeBindingsProps): ReactElement {
  const router = useRouter();
  const [scope, setScope] = useState<'all' | 'selected'>(initial.scope);
  const [selectedIds, setSelectedIds] = useState<ReadonlyArray<string>>(initial.documentIds ?? []);
  const [includeQnA, setIncludeQnA] = useState<boolean>(initial.includeQnA ?? true);
  const [available, setAvailable] = useState<ReadonlyArray<AvailableDoc>>([]);
  const [loading, setLoading] = useState<boolean>(false);
  const [saving, setSaving] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [configOpen, setConfigOpen] = useState<boolean>(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void fetch('/api/knowledge/documents?limit=200')
      .then((r) => r.json())
      .then((data: { documents?: AvailableDoc[] }) => {
        if (cancelled) return;
        setAvailable(data.documents ?? []);
      })
      .catch(() => {
        if (!cancelled) setAvailable([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const toggleDoc = (id: string): void => {
    setSelectedIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  };

  const save = async (): Promise<void> => {
    setSaving(true);
    setError(null);
    try {
      const response = await fetch(`/api/agents/${agentId}/knowledge`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          scope,
          documentIds: scope === 'selected' ? [...selectedIds] : undefined,
          includeQnA,
        }),
      });
      if (!response.ok) {
        const detail = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(detail?.error ?? `HTTP ${response.status}`);
      }
      setSavedAt(Date.now());
      router.refresh();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-5">
      <header className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-[14px] font-semibold text-[#111827]">Knowledge access</h2>
          <p className="mt-0.5 text-[12px] text-[#6B7280]">
            Pick what this agent can retrieve from. Tune how it ranks results
            with Configure RAG.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setConfigOpen(true)}
          className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-[#E5E7EB] bg-white px-3 py-1.5 text-[12px] font-medium text-[#374151] hover:border-[#111111] hover:text-[#111827]"
        >
          <SlidersHorizontal className="size-3" />
          Configure RAG
        </button>
      </header>

      <fieldset>
        <legend className="text-[12px] font-medium text-[#374151]">Scope</legend>
        <div className="mt-2 grid gap-2 sm:grid-cols-2">
          {(['all', 'selected'] as const).map((value) => {
            const active = scope === value;
            return (
              <button
                key={value}
                type="button"
                onClick={() => setScope(value)}
                aria-pressed={active}
                className={`rounded-lg border p-3 text-left transition ${
                  active
                    ? 'border-[#111111] bg-white shadow-[0_2px_8px_rgba(17,24,39,0.06)]'
                    : 'border-[#E5E7EB] bg-white hover:border-[#9CA3AF]'
                }`}
              >
                <div className="flex items-center gap-2">
                  {active ? (
                    <span className="inline-flex size-4 items-center justify-center rounded-full bg-[#111111] text-white">
                      <Check className="size-2.5" />
                    </span>
                  ) : (
                    <span className="size-4 rounded-full border border-[#D1D5DB]" />
                  )}
                  <span className="text-[12px] font-semibold capitalize text-[#111827]">
                    {value === 'all' ? 'All org knowledge' : 'Selected only'}
                  </span>
                </div>
                <p className="mt-1 text-[11px] leading-5 text-[#6B7280]">
                  {value === 'all'
                    ? 'Every published document, Q&A, and integration the org has connected.'
                    : 'Only the documents you pick below.'}
                </p>
              </button>
            );
          })}
        </div>
      </fieldset>

      {scope === 'selected' ? (
        <section>
          <h3 className="text-[12px] font-medium text-[#374151]">
            Documents ({selectedIds.length} selected)
          </h3>
          {loading ? (
            <div className="mt-3 flex items-center gap-2 text-[12px] text-[#6B7280]">
              <Loader2 className="size-3.5 animate-spin" />
              Loading documents…
            </div>
          ) : available.length === 0 ? (
            <p className="mt-2 text-[12px] text-[#6B7280]">
              No documents in your knowledge base yet. Add some from /knowledge.
            </p>
          ) : (
            <ul className="mt-2 max-h-[360px] space-y-1 overflow-y-auto rounded-lg border border-[#E5E7EB] bg-white p-2">
              {available.map((doc) => {
                const selected = selectedIds.includes(doc.id);
                return (
                  <li key={doc.id}>
                    <button
                      type="button"
                      onClick={() => toggleDoc(doc.id)}
                      className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition ${
                        selected
                          ? 'bg-[#F3F4F6] text-[#111827]'
                          : 'text-[#374151] hover:bg-[#F9FAFB]'
                      }`}
                    >
                      <input
                        type="checkbox"
                        checked={selected}
                        readOnly
                        className="size-3.5 rounded border-[#D1D5DB] accent-[#111111]"
                      />
                      <FileText className="size-3.5 shrink-0 text-[#9CA3AF]" />
                      <span className="min-w-0 flex-1 truncate">{doc.title}</span>
                      <span className="shrink-0 font-mono text-[10px] text-[#9CA3AF]">
                        {doc.type}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      ) : null}

      <label className="flex items-center justify-between gap-3 rounded-md border border-[#E5E7EB] bg-white px-3 py-2">
        <div>
          <div className="text-[12px] font-medium text-[#111827]">Include Q&A pairs</div>
          <div className="text-[11px] text-[#6B7280]">
            Operator-curated Q&A answers will be retrieved when matched.
          </div>
        </div>
        <input
          type="checkbox"
          checked={includeQnA}
          onChange={(e) => setIncludeQnA(e.target.checked)}
          className="size-4 rounded border-[#D1D5DB] accent-[#111111]"
        />
      </label>

      {error ? (
        <div
          role="alert"
          className="flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
        >
          <AlertCircle className="size-3.5" />
          {error}
        </div>
      ) : null}

      <div className="flex items-center justify-end gap-2">
        {savedAt ? (
          <span className="text-[11px] text-emerald-600">
            Saved {new Date(savedAt).toLocaleTimeString()}
          </span>
        ) : null}
        <button
          type="button"
          onClick={save}
          disabled={saving}
          className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:opacity-50"
        >
          {saving ? <Loader2 className="size-3.5 animate-spin" /> : null}
          Save scope
        </button>
      </div>

      <ConfigureRagModal
        open={configOpen}
        agentId={agentId}
        initial={retrievalConfig}
        onClose={() => setConfigOpen(false)}
      />
    </div>
  );
}
