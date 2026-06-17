'use client';

import { useState, useEffect, type ReactElement } from 'react';
import { useRouter } from 'next/navigation';
import {
  GraduationCap,
  Wand2,
  Loader2,
  AlertCircle,
  CheckCircle2,
  Info,
} from 'lucide-react';
import { RoutingMatrix, type RoutingMatrixDoc } from '@/components/knowledge/RoutingMatrix';

interface AgentTrainTabProps {
  agentId: string;
  agentModel: string;
}

interface KnowledgeRow {
  id: string;
  title: string;
  type: string;
}

/**
 * Wave 11 §8 — Train tab for the agent workspace.
 *
 * Operator surface:
 *   1. Master "Enable fine-tune for this agent" switch.
 *   2. "Let AI choose" toggle (recommended; default ON).
 *   3. Routing matrix below, scoped to this org's docs.
 *   4. Big "Start training run" CTA.
 *
 * Wired to:
 *   - POST /api/agents/{id}/train   (kicks the run)
 *   - GET /api/agents/{id}/finetune (polls status; existing Wave-7 path)
 */
export function AgentTrainTab({
  agentId,
  agentModel,
}: AgentTrainTabProps): ReactElement {
  const router = useRouter();
  const [enabled, setEnabled] = useState<boolean>(true);
  const [aiSelect, setAiSelect] = useState<boolean>(true);
  const [manualIds, setManualIds] = useState<ReadonlyArray<string>>([]);
  const [docs, setDocs] = useState<ReadonlyArray<RoutingMatrixDoc>>([]);
  const [loadingDocs, setLoadingDocs] = useState<boolean>(false);
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const fetchDocs = async (): Promise<void> => {
    setLoadingDocs(true);
    try {
      const response = await fetch('/api/knowledge/documents?limit=500');
      if (!response.ok) return;
      const data = (await response.json()) as { documents: KnowledgeRow[] };
      // The list endpoint doesn't include routing fields — that's OK; the
      // RoutingMatrix renders an "unclassified" row count until the
      // operator triggers Auto-route.
      setDocs(
        data.documents.map((d) => ({
          id: d.id,
          title: d.title,
          type: d.type,
        })),
      );
    } finally {
      setLoadingDocs(false);
    }
  };

  useEffect(() => {
    void fetchDocs();
  }, []);

  const handleStartTraining = async (): Promise<void> => {
    setSubmitting(true);
    setError(null);
    setStatusMessage(null);
    try {
      const response = await fetch(`/api/agents/${agentId}/train`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          aiSelect,
          documentIds: aiSelect ? undefined : [...manualIds],
          baseModel: agentModel,
        }),
      });
      const payload = (await response.json().catch(() => null)) as
        | { error?: string; jobsCount?: number }
        | null;
      if (!response.ok) {
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
      setStatusMessage(
        `Training started with ${payload?.jobsCount ?? '?'} examples. Track progress under the Fine-tune tab.`,
      );
      router.refresh();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Training kick failed');
    } finally {
      setSubmitting(false);
    }
  };

  const fineTuneCount = docs.filter((d) => d.route === 'finetune').length;

  return (
    <div className="space-y-6 px-6 py-6">
      <header className="flex items-start gap-3">
        <span className="inline-flex size-9 items-center justify-center rounded-md bg-[#F3F4F6] text-[#111827]">
          <GraduationCap className="size-4.5" strokeWidth={1.8} />
        </span>
        <div className="min-w-0 flex-1">
          <h1 className="text-[18px] font-semibold tracking-[-0.02em] text-[#111827]">
            Train this agent
          </h1>
          <p className="mt-0.5 max-w-[64ch] text-[12px] leading-6 text-[#6B7280]">
            Fine-tune the base model with a slice of your org knowledge.
            AI-routed picks include only docs the classifier flagged as good
            fine-tune material (style, voice, format exemplars).
          </p>
        </div>
      </header>

      <section className="rounded-xl border border-[#E5E7EB] bg-white p-4">
        <label className="flex items-start justify-between gap-3">
          <div>
            <div className="text-[13px] font-medium text-[#111827]">
              Enable fine-tune for this agent
            </div>
            <p className="mt-0.5 text-[11px] text-[#6B7280]">
              When off, this agent uses the base model exclusively. Retrieval
              from the knowledge base still works.
            </p>
          </div>
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="mt-0.5 size-4 rounded border-[#D1D5DB] accent-[#111111]"
          />
        </label>
      </section>

      {enabled ? (
        <>
          <section className="rounded-xl border border-[#E5E7EB] bg-white p-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="text-[13px] font-medium text-[#111827]">
                  Let AI choose what to fine-tune{' '}
                  <span className="text-[11px] font-normal text-emerald-600">
                    (recommended)
                  </span>
                </div>
                <p className="mt-0.5 max-w-[60ch] text-[11px] text-[#6B7280]">
                  The classifier scans every doc in your knowledge base and
                  picks only style-exemplars, format templates, and brand-voice
                  samples. The rest stay in RAG retrieval — they don&apos;t
                  belong in training data.
                </p>
              </div>
              <input
                type="checkbox"
                checked={aiSelect}
                onChange={(e) => setAiSelect(e.target.checked)}
                className="mt-0.5 size-4 rounded border-[#D1D5DB] accent-[#111111]"
              />
            </div>

            {aiSelect ? (
              <div className="mt-3 flex items-center gap-2 rounded-md bg-blue-50 px-3 py-2 text-[11px] text-blue-900">
                <Info className="size-3.5 shrink-0" />
                <span>
                  Currently {fineTuneCount} of {docs.length} docs are classified
                  as fine-tune. Run Auto-route below to refresh.
                </span>
              </div>
            ) : null}
          </section>

          <section className="rounded-xl border border-[#E5E7EB] bg-white p-4">
            {loadingDocs ? (
              <div className="flex items-center gap-2 text-[12px] text-[#6B7280]">
                <Loader2 className="size-3.5 animate-spin" />
                Loading docs…
              </div>
            ) : (
              <RoutingMatrix
                documents={docs as RoutingMatrixDoc[]}
                onRefetch={fetchDocs}
              />
            )}
          </section>

          {statusMessage ? (
            <div className="flex items-center gap-2 rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-[12px] text-emerald-700">
              <CheckCircle2 className="size-3.5" />
              {statusMessage}
            </div>
          ) : null}

          {error ? (
            <div
              role="alert"
              className="flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700"
            >
              <AlertCircle className="size-3.5" />
              {error}
            </div>
          ) : null}

          <div className="flex items-center justify-between gap-3 rounded-xl border border-[#E5E7EB] bg-[#FAFAFA] p-4">
            <div>
              <div className="text-[13px] font-medium text-[#111827]">
                Start a training run
              </div>
              <p className="mt-0.5 text-[11px] text-[#6B7280]">
                Uses base model <span className="font-mono text-[#374151]">{agentModel}</span>.
                Costs are billed by your provider — runs take 10–30 min.
              </p>
            </div>
            <button
              type="button"
              onClick={handleStartTraining}
              disabled={submitting}
              className="inline-flex shrink-0 items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:opacity-50"
            >
              {submitting ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <Wand2 className="size-3.5" />
              )}
              Start training run
            </button>
          </div>
        </>
      ) : null}
    </div>
  );
}
