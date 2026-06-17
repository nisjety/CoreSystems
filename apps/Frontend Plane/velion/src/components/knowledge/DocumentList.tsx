'use client';

import { useState, useMemo, type ReactElement } from 'react';
import { Search, MoreHorizontal, Trash2, RotateCw } from 'lucide-react';

export interface DocumentRow {
  id: string;
  title: string;
  /** Subtitle line under the title — usually source URL, file name, etc. */
  subtitle?: string;
  status: 'active' | 'pending' | 'indexing' | 'error' | 'deprecated';
  /** Optional bytes-on-disk for the size column. */
  sizeBytes?: number;
  updatedAt: string;
  /** Chatbase "New" pill — true for rows created in the last 7 days. */
  isFresh?: boolean;
}

interface DocumentListProps {
  documents: DocumentRow[];
  searchPlaceholder?: string;
  /** Title shown above the search bar (per-mode label, e.g. "File sources"). */
  emptyStateTitle?: string;
  emptyStateBody?: string;
  /** Called when the operator clicks a row to open the drawer. */
  onOpen: (id: string) => void;
  /** Called when bulk-delete is confirmed. */
  onBulkDelete?: (ids: string[]) => Promise<void>;
  /** Called when bulk-reindex is confirmed. */
  onBulkReindex?: (ids: string[]) => Promise<void>;
}

const STATUS_STYLES: Record<DocumentRow['status'], string> = {
  active: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
  pending: 'bg-amber-50 text-amber-700 ring-amber-200',
  indexing: 'bg-blue-50 text-blue-700 ring-blue-200',
  error: 'bg-red-50 text-red-700 ring-red-200',
  deprecated: 'bg-zinc-100 text-zinc-600 ring-zinc-200',
};

function formatBytes(bytes?: number): string {
  if (typeof bytes !== 'number' || bytes <= 0) return '';
  const units = ['B', 'KB', 'MB', 'GB'];
  let unit = 0;
  let value = bytes;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

function formatRelative(iso: string): string {
  const ms = Date.now() - Date.parse(iso);
  if (Number.isNaN(ms)) return '';
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return 'Just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return new Date(iso).toLocaleDateString();
}

/**
 * Chatbase-style document list. Includes search, bulk-select with
 * floating action bar (Mobbin screen `591d4424…b936`), and per-row
 * status pill. Rows are clickable and route the operator into the
 * Document drawer (Phase 5).
 */
export function DocumentList({
  documents,
  searchPlaceholder = 'Search…',
  emptyStateTitle = 'Nothing here yet',
  emptyStateBody = 'Add a source from the picker above.',
  onOpen,
  onBulkDelete,
  onBulkReindex,
}: DocumentListProps): ReactElement {
  const [query, setQuery] = useState<string>('');
  const [selected, setSelected] = useState<ReadonlyArray<string>>([]);
  const [busy, setBusy] = useState<boolean>(false);

  const filtered = useMemo(() => {
    if (!query.trim()) return documents;
    const needle = query.trim().toLowerCase();
    return documents.filter(
      (doc) =>
        doc.title.toLowerCase().includes(needle) ||
        (doc.subtitle ?? '').toLowerCase().includes(needle),
    );
  }, [documents, query]);

  const allSelected = filtered.length > 0 && selected.length === filtered.length;
  const toggleSelectAll = (): void => {
    setSelected(allSelected ? [] : filtered.map((d) => d.id));
  };
  const toggleOne = (id: string): void => {
    setSelected((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  };

  const handleBulk = async (
    action: 'delete' | 'reindex',
  ): Promise<void> => {
    if (selected.length === 0) return;
    setBusy(true);
    try {
      const ids = [...selected];
      if (action === 'delete' && onBulkDelete) await onBulkDelete(ids);
      if (action === 'reindex' && onBulkReindex) await onBulkReindex(ids);
      setSelected([]);
    } finally {
      setBusy(false);
    }
  };

  if (documents.length === 0) {
    return (
      <div className="flex min-h-[200px] flex-col items-center justify-center rounded-xl border border-dashed border-[#E5E7EB] bg-[#FAFAFA] px-6 py-10 text-center">
        <p className="text-[14px] font-medium text-[#111827]">{emptyStateTitle}</p>
        <p className="mt-1 max-w-[40ch] text-[12px] leading-5 text-[#6B7280]">
          {emptyStateBody}
        </p>
      </div>
    );
  }

  return (
    <div className="relative">
      <div className="mb-3 flex items-center gap-3">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-[#9CA3AF]" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={searchPlaceholder}
            className="block w-full rounded-md border border-[#E5E7EB] bg-white py-1.5 pl-8 pr-3 text-[12px] text-[#111827] outline-none placeholder:text-[#9CA3AF] focus:border-[#111111]"
          />
        </div>
        <label className="flex shrink-0 items-center gap-1.5 text-[11px] text-[#6B7280]">
          <input
            type="checkbox"
            checked={allSelected}
            onChange={toggleSelectAll}
            className="size-3.5 rounded border-[#D1D5DB] accent-[#111111]"
          />
          Select all
        </label>
      </div>

      <ul className="divide-y divide-[#F3F4F6] rounded-xl border border-[#E5E7EB] bg-white">
        {filtered.map((doc) => {
          const isSelected = selected.includes(doc.id);
          return (
            <li
              key={doc.id}
              className={`flex items-center gap-3 px-3 py-2.5 transition ${
                isSelected ? 'bg-[#F9FAFB]' : 'hover:bg-[#FAFAFA]'
              }`}
            >
              <input
                type="checkbox"
                checked={isSelected}
                onChange={() => toggleOne(doc.id)}
                onClick={(e) => e.stopPropagation()}
                aria-label={`Select ${doc.title}`}
                className="size-3.5 shrink-0 rounded border-[#D1D5DB] accent-[#111111]"
              />
              <button
                type="button"
                onClick={() => onOpen(doc.id)}
                className="flex min-w-0 flex-1 items-center gap-3 text-left"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-[13px] font-medium text-[#111827]">
                      {doc.title}
                    </span>
                    {doc.isFresh ? (
                      <span className="rounded-full bg-emerald-50 px-1.5 py-0 text-[9px] font-semibold uppercase tracking-wide text-emerald-700 ring-1 ring-inset ring-emerald-200">
                        New
                      </span>
                    ) : null}
                  </div>
                  {doc.subtitle ? (
                    <div className="mt-0.5 truncate text-[11px] text-[#6B7280]">
                      {doc.subtitle}
                    </div>
                  ) : null}
                </div>
                <span className="hidden shrink-0 font-mono text-[10px] text-[#9CA3AF] md:inline">
                  {formatBytes(doc.sizeBytes)}
                </span>
                <span
                  className={`inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-[10px] font-medium capitalize ring-1 ring-inset ${
                    STATUS_STYLES[doc.status]
                  }`}
                >
                  {doc.status}
                </span>
                <span className="hidden w-[80px] shrink-0 text-right text-[11px] text-[#9CA3AF] md:inline">
                  {formatRelative(doc.updatedAt)}
                </span>
              </button>
              <button
                type="button"
                aria-label="More actions"
                className="rounded-md p-1 text-[#9CA3AF] hover:bg-[#F3F4F6] hover:text-[#111827]"
              >
                <MoreHorizontal className="size-3.5" />
              </button>
            </li>
          );
        })}
      </ul>

      {selected.length > 0 ? (
        <div className="fixed bottom-6 left-1/2 z-40 flex -translate-x-1/2 items-center gap-3 rounded-full bg-[#111111] px-4 py-2 text-white shadow-xl">
          <span className="text-[12px]">{selected.length} selected</span>
          {onBulkReindex ? (
            <button
              type="button"
              onClick={() => handleBulk('reindex')}
              disabled={busy}
              className="inline-flex items-center gap-1 rounded-full bg-white/10 px-2.5 py-1 text-[12px] hover:bg-white/20 disabled:opacity-50"
            >
              <RotateCw className={`size-3 ${busy ? 'animate-spin' : ''}`} />
              Reindex
            </button>
          ) : null}
          {onBulkDelete ? (
            <button
              type="button"
              onClick={() => handleBulk('delete')}
              disabled={busy}
              className="inline-flex items-center gap-1 rounded-full bg-red-500/90 px-2.5 py-1 text-[12px] hover:bg-red-500 disabled:opacity-50"
            >
              <Trash2 className="size-3" />
              Delete
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => setSelected([])}
            className="text-[12px] text-white/60 hover:text-white"
          >
            Cancel
          </button>
        </div>
      ) : null}
    </div>
  );
}
