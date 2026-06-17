'use client';

import { useState, type ReactElement, type FormEvent } from 'react';
import { X, SlidersHorizontal, Loader2, AlertCircle } from 'lucide-react';

export interface RetrievalConfig {
  weights?: { dense: number; bm25: number; graph: number; wiki: number };
  chunkSize?: number;
  topK?: number;
  rerank?: boolean;
  graphHops?: number;
}

interface ConfigureRagModalProps {
  open: boolean;
  agentId: string;
  initial: RetrievalConfig;
  onClose: () => void;
  onSaved?: (config: RetrievalConfig) => void;
}

const DEFAULTS: Required<NonNullable<RetrievalConfig['weights']>> = {
  dense: 0.5,
  bm25: 0.2,
  graph: 0.2,
  wiki: 0.1,
};

/**
 * Wave 11 §6.5 — ElevenLabs "Configure RAG" modal (Mobbin
 * `94abd75b…b2e4`). Per-agent knobs for the hybrid retrieval blend +
 * top-k + chunking + re-rank toggle + graph hops.
 *
 * Persists to Convex `agents.retrievalConfig`. Empty fields stay empty
 * so the retrieval engine's server-side defaults remain authoritative.
 */
export function ConfigureRagModal({
  open,
  agentId,
  initial,
  onClose,
  onSaved,
}: ConfigureRagModalProps): ReactElement | null {
  const [weights, setWeights] = useState({
    ...DEFAULTS,
    ...(initial.weights ?? {}),
  });
  const [topK, setTopK] = useState<number>(initial.topK ?? 10);
  const [chunkSize, setChunkSize] = useState<number>(initial.chunkSize ?? 512);
  const [rerank, setRerank] = useState<boolean>(initial.rerank ?? true);
  const [graphHops, setGraphHops] = useState<number>(initial.graphHops ?? 2);
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const weightSum = weights.dense + weights.bm25 + weights.graph + weights.wiki;

  const normalizeWeights = (): RetrievalConfig['weights'] => {
    if (weightSum === 0) return DEFAULTS;
    const factor = 1 / weightSum;
    return {
      dense: Number((weights.dense * factor).toFixed(2)),
      bm25: Number((weights.bm25 * factor).toFixed(2)),
      graph: Number((weights.graph * factor).toFixed(2)),
      wiki: Number((weights.wiki * factor).toFixed(2)),
    };
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const payload: RetrievalConfig = {
        weights: normalizeWeights(),
        topK,
        chunkSize,
        rerank,
        graphHops,
      };
      const response = await fetch(`/api/agents/${agentId}/retrieval-config`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!response.ok) {
        const detail = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(detail?.error ?? `HTTP ${response.status}`);
      }
      onSaved?.(payload);
      onClose();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Save failed');
    } finally {
      setSubmitting(false);
    }
  };

  const updateWeight = (key: keyof typeof weights, value: number): void => {
    setWeights((prev) => ({ ...prev, [key]: Math.max(0, Math.min(1, value)) }));
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      onClick={(event) => {
        if (event.target === event.currentTarget && !submitting) onClose();
      }}
    >
      <form
        onSubmit={handleSubmit}
        className="flex max-h-[88vh] w-full max-w-[560px] flex-col overflow-hidden rounded-2xl bg-white shadow-2xl"
      >
        <header className="flex shrink-0 items-start justify-between gap-4 border-b border-[#E5E7EB] p-6">
          <div className="flex items-center gap-2">
            <span className="inline-flex size-8 items-center justify-center rounded-md bg-[#F3F4F6] text-[#111827]">
              <SlidersHorizontal className="size-4" strokeWidth={1.8} />
            </span>
            <div>
              <h2 className="text-[16px] font-semibold text-[#111827]">Configure RAG</h2>
              <p className="mt-0.5 text-[12px] text-[#6B7280]">
                Tune how this agent retrieves and ranks knowledge.
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            aria-label="Close"
            className="rounded-md p-1 text-[#6B7280] hover:bg-[#F3F4F6] hover:text-[#111827] disabled:opacity-50"
          >
            <X className="size-4" />
          </button>
        </header>

        <div className="flex-1 space-y-5 overflow-y-auto px-6 py-5">
          <section>
            <h3 className="text-[12px] font-semibold uppercase tracking-wide text-[#6B7280]">
              Blend weights
            </h3>
            <p className="mt-0.5 text-[11px] text-[#6B7280]">
              Higher weight = that signal contributes more to the final ranking.
              We&apos;ll renormalize automatically on save.
            </p>

            <div className="mt-3 space-y-3">
              {(['dense', 'bm25', 'graph', 'wiki'] as const).map((key) => (
                <label key={key} className="block">
                  <div className="flex items-baseline justify-between text-[11px]">
                    <span className="capitalize text-[#374151]">
                      {key === 'bm25' ? 'BM25 (keyword)' : key === 'dense' ? 'Dense (semantic)' : key === 'graph' ? 'Graph (entity)' : 'Wiki (curated)'}
                    </span>
                    <span className="font-mono text-[#6B7280]">{weights[key].toFixed(2)}</span>
                  </div>
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    value={weights[key]}
                    onChange={(e) => updateWeight(key, Number(e.target.value))}
                    className="mt-1 w-full accent-[#111111]"
                  />
                </label>
              ))}
            </div>
            <p className="mt-2 text-[10px] text-[#9CA3AF]">
              Sum: <span className="font-mono">{weightSum.toFixed(2)}</span> · normalized to 1.00 on save
            </p>
          </section>

          <section>
            <h3 className="text-[12px] font-semibold uppercase tracking-wide text-[#6B7280]">
              Retrieval shape
            </h3>
            <div className="mt-3 grid grid-cols-2 gap-3">
              <label className="block">
                <span className="block text-[11px] text-[#374151]">
                  Top-K: <span className="font-mono">{topK}</span>
                </span>
                <input
                  type="range"
                  min={1}
                  max={50}
                  step={1}
                  value={topK}
                  onChange={(e) => setTopK(Number(e.target.value))}
                  className="mt-1 w-full accent-[#111111]"
                />
              </label>
              <label className="block">
                <span className="block text-[11px] text-[#374151]">
                  Chunk size: <span className="font-mono">{chunkSize}</span>
                </span>
                <input
                  type="range"
                  min={64}
                  max={2048}
                  step={32}
                  value={chunkSize}
                  onChange={(e) => setChunkSize(Number(e.target.value))}
                  className="mt-1 w-full accent-[#111111]"
                />
              </label>
              <label className="col-span-2 flex items-center justify-between gap-3 rounded-md border border-[#E5E7EB] bg-white px-3 py-2">
                <div>
                  <div className="text-[12px] font-medium text-[#111827]">Re-rank</div>
                  <div className="text-[11px] text-[#6B7280]">
                    Cross-encoder pass for ~2× precision at ~3× latency.
                  </div>
                </div>
                <input
                  type="checkbox"
                  checked={rerank}
                  onChange={(e) => setRerank(e.target.checked)}
                  className="size-4 rounded border-[#D1D5DB] accent-[#111111]"
                />
              </label>
              <label className="col-span-2 block">
                <span className="block text-[11px] text-[#374151]">
                  Graph hops: <span className="font-mono">{graphHops}</span>
                </span>
                <input
                  type="range"
                  min={0}
                  max={3}
                  step={1}
                  value={graphHops}
                  onChange={(e) => setGraphHops(Number(e.target.value))}
                  className="mt-1 w-full accent-[#111111]"
                />
                <span className="block text-[10px] text-[#9CA3AF]">
                  0 disables graph expansion; 2 is a safe default.
                </span>
              </label>
            </div>
          </section>
        </div>

        {error ? (
          <div
            role="alert"
            className="mx-6 mb-3 flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
          >
            <AlertCircle className="size-3.5" />
            {error}
          </div>
        ) : null}

        <footer className="flex shrink-0 items-center justify-end gap-2 border-t border-[#E5E7EB] px-6 py-3">
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            className="rounded-full border border-[#E5E7EB] bg-white px-4 py-2 text-[12px] font-medium text-[#374151] hover:border-[#9CA3AF] disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={submitting}
            className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
          >
            {submitting ? <Loader2 className="size-3.5 animate-spin" /> : null}
            Save configuration
          </button>
        </footer>
      </form>
    </div>
  );
}
