'use client';

import { useState, type ReactElement } from 'react';
import Link from 'next/link';
import {
  ArrowLeft,
  BookOpen,
  Link2,
  History,
  FileText,
  ChevronRight,
} from 'lucide-react';
import { BlockOutlineEditor } from './BlockOutlineEditor';
import type {
  WikiPage,
  WikiPageVersion,
  WikiSourceLog,
} from '@/types/data-plane/wiki_v1';

interface WikiPageEditorProps {
  page: WikiPage;
  version: WikiPageVersion | null;
  backlinks: ReadonlyArray<{ page_id: string; title: string; path: string }>;
  sourceLog: WikiSourceLog | null;
}

type SidePanel = 'backlinks' | 'sources' | 'history';

const STATUS_STYLES: Record<string, string> = {
  draft: 'bg-amber-50 text-amber-700 ring-amber-200',
  published: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
  deprecated: 'bg-zinc-100 text-zinc-600 ring-zinc-200',
  approved: 'bg-sky-50 text-sky-700 ring-sky-200',
};

/**
 * Wave 11.C-a — Logseq-style wiki editor (Mobbin Reflect reference).
 *
 *   Left  : Back to wiki + page title + status
 *   Main  : block-outline editor (BlockOutlineEditor, debounced save)
 *   Right : 3-tab side panel — Backlinks / Sources / History
 */
export function WikiPageEditor({
  page,
  version,
  backlinks,
  sourceLog,
}: WikiPageEditorProps): ReactElement {
  const [tab, setTab] = useState<SidePanel>('backlinks');

  const handleSave = async (content: string): Promise<{ ok: boolean; error?: string }> => {
    try {
      const response = await fetch(`/api/knowledge/wiki/${encodeURIComponent(page.page_id)}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content, edit_reason: 'manual_edit' }),
      });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        return { ok: false, error: payload?.error ?? `HTTP ${response.status}` };
      }
      return { ok: true };
    } catch (err) {
      return { ok: false, error: err instanceof Error ? err.message : 'Network error' };
    }
  };

  return (
    <div className="flex h-full min-h-0">
      {/* Center — editor */}
      <main className="min-w-0 flex-1 overflow-y-auto px-8 py-6">
        <Link
          href="/knowledge/wiki"
          className="inline-flex items-center gap-1.5 text-[12px] text-[#6B7280] hover:text-[#111827]"
        >
          <ArrowLeft className="size-3.5" />
          Back to wiki
        </Link>

        <header className="mt-4 mb-6">
          <div className="flex items-center gap-2">
            <BookOpen className="size-4 text-[#6B7280]" />
            <h1 className="text-[22px] font-semibold tracking-[-0.02em] text-[#111827]">
              {page.title}
            </h1>
            <span
              className={`rounded-full px-1.5 py-0 text-[9px] font-semibold uppercase tracking-wide ring-1 ring-inset ${
                STATUS_STYLES[page.status] ?? STATUS_STYLES.draft
              }`}
            >
              {page.status}
            </span>
          </div>
          <div className="mt-1 flex items-center gap-1 font-mono text-[11px] text-[#6B7280]">
            <span>{page.path}</span>
            {version ? (
              <>
                <ChevronRight className="size-3" />
                <span>v{version.version_id.slice(0, 8)}</span>
              </>
            ) : null}
          </div>
        </header>

        {version ? (
          <BlockOutlineEditor initialContent={version.content} onSave={handleSave} />
        ) : (
          <p className="text-[12px] text-[#6B7280]">No version available yet.</p>
        )}
      </main>

      {/* Right — side panel */}
      <aside className="hidden w-[320px] shrink-0 flex-col border-l border-[#E9EBF2] bg-white lg:flex">
        <nav className="flex shrink-0 border-b border-[#E9EBF2]">
          {([
            { key: 'backlinks', label: 'Backlinks', icon: Link2, count: backlinks.length },
            { key: 'sources', label: 'Sources', icon: FileText, count: sourceLog?.original_chunks.length ?? 0 },
            { key: 'history', label: 'History', icon: History, count: null },
          ] as const).map(({ key, label, icon: Icon, count }) => (
            <button
              key={key}
              type="button"
              onClick={() => setTab(key)}
              className={`flex flex-1 items-center justify-center gap-1.5 border-b-2 px-2 py-2.5 text-[12px] font-medium ${
                tab === key
                  ? 'border-[#111111] text-[#111827]'
                  : 'border-transparent text-[#6B7280] hover:text-[#111827]'
              }`}
            >
              <Icon className="size-3.5" />
              {label}
              {count !== null && count > 0 ? (
                <span className="rounded-full bg-[#F3F4F6] px-1.5 py-0 text-[9px] font-medium text-[#6B7280]">
                  {count}
                </span>
              ) : null}
            </button>
          ))}
        </nav>

        <div className="flex-1 overflow-y-auto px-4 py-4">
          {tab === 'backlinks' ? (
            backlinks.length === 0 ? (
              <p className="text-[12px] text-[#6B7280]">No pages link here yet.</p>
            ) : (
              <ul className="space-y-1.5">
                {backlinks.map((b) => (
                  <li key={b.page_id}>
                    <Link
                      href={`/knowledge/wiki/${b.page_id}`}
                      className="flex flex-col rounded-md border border-[#E5E7EB] bg-white px-2.5 py-1.5 hover:border-[#111111]"
                    >
                      <span className="text-[12px] font-medium text-[#111827]">
                        {b.title}
                      </span>
                      <span className="truncate font-mono text-[10px] text-[#6B7280]">
                        {b.path}
                      </span>
                    </Link>
                  </li>
                ))}
              </ul>
            )
          ) : null}

          {tab === 'sources' ? (
            sourceLog ? (
              <div>
                <h3 className="text-[10px] font-semibold uppercase tracking-wide text-[#6B7280]">
                  Synthesis model
                </h3>
                <p className="mt-1 font-mono text-[11px] text-[#374151]">
                  {sourceLog.processing_model}
                </p>
                <h3 className="mt-3 text-[10px] font-semibold uppercase tracking-wide text-[#6B7280]">
                  Source chunks ({sourceLog.original_chunks.length})
                </h3>
                <ul className="mt-1 space-y-1">
                  {sourceLog.original_chunks.slice(0, 50).map((chunk, idx) => (
                    <li
                      key={`${chunk}-${idx}`}
                      className="truncate rounded-md bg-[#F9FAFB] px-2 py-1 font-mono text-[10px] text-[#374151]"
                    >
                      {chunk}
                    </li>
                  ))}
                </ul>
                <h3 className="mt-3 text-[10px] font-semibold uppercase tracking-wide text-[#6B7280]">
                  Prompt hash
                </h3>
                <p className="mt-1 truncate font-mono text-[10px] text-[#6B7280]">
                  {sourceLog.synthesis_prompt_hash}
                </p>
              </div>
            ) : (
              <p className="text-[12px] text-[#6B7280]">
                No synthesis source log yet. Pages added manually don&apos;t have
                one — the orchestrator populates it when a wiki page is
                synthesized from documents.
              </p>
            )
          ) : null}

          {tab === 'history' ? (
            <div>
              <p className="text-[12px] text-[#6B7280]">
                Version timeline. Loading list-versions in this side panel is a
                Wave 11.C-b follow-up — for now the current version&apos;s metadata is
                shown below.
              </p>
              {version ? (
                <dl className="mt-3 space-y-2 text-[11px]">
                  <div>
                    <dt className="font-semibold text-[#6B7280]">Reason</dt>
                    <dd className="mt-0.5 text-[#374151]">{version.edit_reason}</dd>
                  </div>
                  <div>
                    <dt className="font-semibold text-[#6B7280]">Status</dt>
                    <dd className="mt-0.5 text-[#374151]">{version.status}</dd>
                  </div>
                  {version.proposed_by_user ? (
                    <div>
                      <dt className="font-semibold text-[#6B7280]">Author</dt>
                      <dd className="mt-0.5 truncate font-mono text-[#374151]">
                        {version.proposed_by_user}
                      </dd>
                    </div>
                  ) : null}
                  {version.proposed_by_agent ? (
                    <div>
                      <dt className="font-semibold text-[#6B7280]">Agent</dt>
                      <dd className="mt-0.5 truncate font-mono text-[#374151]">
                        {version.proposed_by_agent}
                      </dd>
                    </div>
                  ) : null}
                </dl>
              ) : null}
            </div>
          ) : null}
        </div>
      </aside>
    </div>
  );
}
