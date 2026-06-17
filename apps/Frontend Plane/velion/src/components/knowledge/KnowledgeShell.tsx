'use client';

import { type ReactNode, type ReactElement, useState } from 'react';
import {
  Plus,
  RefreshCcw,
  AlertCircle,
  Loader2,
} from 'lucide-react';
import { AddKnowledgeModal } from './AddKnowledgeModal';
import type { KnowledgeStats } from './types';

/**
 * Wave 11 follow-up: the global Sidebar
 * (`components/core/sidebar/config/nav-items.ts`) owns the knowledge
 * section nav. This shell no longer renders a duplicate sub-nav — it
 * is a 2-column "main + right rail" only.
 *
 * `KnowledgeFeatureFlags` is preserved on the props for callers that
 * still pass it (`layout.tsx`), but the shell itself doesn't render
 * sub-nav items, so the value is currently unused. Pages and the
 * global Sidebar continue to honour the flag via `feature-flags.ts`.
 */
export interface KnowledgeFeatureFlags {
  graph: boolean;
  wiki: boolean;
}

interface KnowledgeShellProps {
  children: ReactNode;
  stats: KnowledgeStats;
  flags?: KnowledgeFeatureFlags;
}

function formatBytes(bytes: number): string {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let unitIndex = 0;
  let value = bytes;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}

/**
 * Wave 11 §2 (ui-ux-velion-gap.md wave11-knowledge):
 *
 * Chatbase IA shell. Three columns:
 *   Left   — sub-nav of ingestion modes (each is its own page).
 *   Center — the current mode's content (Children).
 *   Right  — sticky status rail with totals + Retrain CTA + dirty chip.
 *
 * The top-right "Add knowledge" button opens the Lindy 8-tile picker.
 * Each tile deep-links to a sub-nav child or kicks an OAuth flow.
 */
export function KnowledgeShell({
  children,
  stats,
  // `flags` retained on the props for back-compat; the shell delegates
  // sub-nav gating to the global Sidebar config now.
  flags: _flags,
}: KnowledgeShellProps): ReactElement {
  void _flags;
  const [pickerOpen, setPickerOpen] = useState<boolean>(false);
  const [retraining, setRetraining] = useState<boolean>(false);
  const [retrainError, setRetrainError] = useState<string | null>(null);

  const handleRetrain = async (): Promise<void> => {
    setRetraining(true);
    setRetrainError(null);
    try {
      const response = await fetch('/api/knowledge/retrain', { method: 'POST' });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as
          | { error?: string }
          | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
    } catch (error) {
      setRetrainError(error instanceof Error ? error.message : 'Retrain failed');
    } finally {
      setRetraining(false);
    }
  };

  const storageRatio = stats.storageQuotaBytes
    ? Math.min(100, Math.round((stats.totalSizeBytes / stats.storageQuotaBytes) * 100))
    : 0;

  return (
    <div className="flex h-full overflow-hidden bg-white text-[#23252f]">
      {/* CENTER: per-mode content.
       *
       * Wave 11 follow-up: the global Sidebar (`components/core/sidebar/`)
       * already renders the knowledge section nav with the new IA
       * (Files / Text / Website / Q&A / Graph / Wiki / Integrations).
       * The previous duplicate left sub-nav has been removed; the
       * "Add knowledge" CTA + ingestion banner now live as a sticky
       * top bar above each pane's own header.
       */}
      <main className="min-w-0 flex-1 overflow-y-auto">
        <div className="sticky top-0 z-20 flex items-center justify-between gap-3 border-b border-[#E9EBF2] bg-white/95 px-6 py-2 backdrop-blur">
          <span className="text-[11px] uppercase tracking-[0.18em] text-[#9CA3AF]">
            Knowledge
          </span>
          <button
            type="button"
            onClick={() => setPickerOpen(true)}
            className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-3 py-1.5 text-[12px] font-medium text-white transition hover:bg-[#2B2B2B]"
          >
            <Plus className="size-3.5" />
            Add knowledge
          </button>
        </div>
        {stats.ingestionInProgress.active ? (
          <div className="sticky top-[42px] z-10 flex items-center gap-2 border-b border-blue-200 bg-blue-50 px-6 py-2 text-[12px] text-blue-900">
            <Loader2 className="size-3.5 animate-spin" />
            <span>
              Your content is currently being ingested
              <span className="ml-1 text-blue-700">
                ({stats.ingestionInProgress.pendingCount} pending) — you can keep working,
                we&apos;ll notify you when it&apos;s ready.
              </span>
            </span>
          </div>
        ) : null}
        {children}
      </main>

      {/* RIGHT: sticky status rail */}
      <aside className="hidden w-[280px] shrink-0 flex-col border-l border-[#E9EBF2] bg-[#FCFBF8]/60 px-5 py-5 lg:flex">
        <h2 className="text-[12px] font-semibold uppercase tracking-[0.1em] text-[#6B7280]">
          Data sources
        </h2>
        <div className="mt-3 grid grid-cols-2 gap-2">
          <div className="rounded-lg border border-[#E5E7EB] bg-white px-3 py-2">
            <div className="text-[11px] text-[#6B7280]">Sources</div>
            <div className="mt-0.5 text-[16px] font-semibold text-[#111827]">
              {stats.totalSources}
            </div>
          </div>
          <div className="rounded-lg border border-[#E5E7EB] bg-white px-3 py-2">
            <div className="text-[11px] text-[#6B7280]">Documents</div>
            <div className="mt-0.5 text-[16px] font-semibold text-[#111827]">
              {stats.totalDocuments}
            </div>
          </div>
        </div>

        {stats.storageQuotaBytes ? (
          <div className="mt-3 rounded-lg border border-[#E5E7EB] bg-white px-3 py-2">
            <div className="flex items-baseline justify-between text-[11px] text-[#6B7280]">
              <span>Total size</span>
              <span className="font-mono text-[#374151]">
                {formatBytes(stats.totalSizeBytes)} / {formatBytes(stats.storageQuotaBytes)}
              </span>
            </div>
            <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-[#F3F4F6]">
              <div
                className="h-full bg-[#111111] transition-all"
                style={{ width: `${storageRatio}%` }}
              />
            </div>
          </div>
        ) : null}

        <button
          type="button"
          onClick={handleRetrain}
          disabled={retraining || !stats.dirty}
          className="mt-4 inline-flex items-center justify-center gap-1.5 rounded-lg bg-[#111111] px-3 py-2 text-[12px] font-medium text-white transition hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:bg-[#9CA3AF]"
        >
          <RefreshCcw className={`size-3.5 ${retraining ? 'animate-spin' : ''}`} />
          {retraining ? 'Retraining…' : 'Retrain agents'}
        </button>

        {stats.dirty ? (
          <div className="mt-2 inline-flex items-center gap-1.5 rounded-md bg-amber-50 px-2 py-1.5 text-[11px] text-amber-800 ring-1 ring-inset ring-amber-200">
            <AlertCircle className="size-3" />
            Retraining required for changes to apply
          </div>
        ) : null}

        {retrainError ? (
          <div
            role="alert"
            className="mt-2 rounded-md border border-red-200 bg-red-50 px-2 py-1.5 text-[11px] text-red-700"
          >
            {retrainError}
          </div>
        ) : null}
      </aside>

      <AddKnowledgeModal open={pickerOpen} onClose={() => setPickerOpen(false)} />
    </div>
  );
}
