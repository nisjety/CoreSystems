'use client';

import { useState, useEffect, type ReactElement, type FormEvent } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import {
  BookOpen,
  Plus,
  Search,
  Loader2,
  AlertCircle,
  ExternalLink,
  X,
} from 'lucide-react';

interface PageBookmark {
  page_id: string;
  title: string;
  path: string;
  added_at: number;
}

const BOOKMARK_KEY = 'velion.knowledge.wiki.recent';

function loadBookmarks(): PageBookmark[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = window.localStorage.getItem(BOOKMARK_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as PageBookmark[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function saveBookmarks(items: PageBookmark[]): void {
  if (typeof window === 'undefined') return;
  window.localStorage.setItem(BOOKMARK_KEY, JSON.stringify(items.slice(0, 50)));
}

/**
 * Wave 11.C-a — wiki page index.
 *
 * Since wiki-store-go has no "list all pages" endpoint, the sidebar
 * shows bookmarks (operator-pinned paths from localStorage). The
 * search bar resolves any `/path` against the by-path endpoint and
 * adds it to the bookmark list on first open. A future Wave 11.C-b
 * adds a real list endpoint and migrates this client over.
 */
export function WikiListClient(): ReactElement {
  const router = useRouter();
  const [bookmarks, setBookmarks] = useState<PageBookmark[]>([]);
  const [openPath, setOpenPath] = useState<string>('');
  const [lookup, setLookup] = useState<'idle' | 'busy' | 'error'>('idle');
  const [lookupError, setLookupError] = useState<string | null>(null);

  // Create modal state
  const [createOpen, setCreateOpen] = useState<boolean>(false);
  const [newTitle, setNewTitle] = useState<string>('');
  const [newPath, setNewPath] = useState<string>('');
  const [newContent, setNewContent] = useState<string>('');
  const [creating, setCreating] = useState<boolean>(false);
  const [createError, setCreateError] = useState<string | null>(null);

  useEffect(() => {
    setBookmarks(loadBookmarks());
  }, []);

  const handleOpenByPath = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    setLookupError(null);
    const trimmed = openPath.trim();
    if (!trimmed) return;
    const normalized = trimmed.startsWith('/') ? trimmed : `/${trimmed}`;
    setLookup('busy');
    try {
      const response = await fetch(
        `/api/knowledge/wiki?path=${encodeURIComponent(normalized)}`,
      );
      if (response.status === 404) {
        setLookup('error');
        setLookupError(`No wiki page at "${normalized}". Create it below.`);
        return;
      }
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
      const data = (await response.json()) as { page: { page_id: string; title: string; path: string } };
      const entry: PageBookmark = {
        page_id: data.page.page_id,
        title: data.page.title,
        path: data.page.path,
        added_at: Date.now(),
      };
      const next = [entry, ...bookmarks.filter((b) => b.page_id !== entry.page_id)];
      setBookmarks(next);
      saveBookmarks(next);
      router.push(`/knowledge/wiki/${entry.page_id}`);
    } catch (err: unknown) {
      setLookup('error');
      setLookupError(err instanceof Error ? err.message : 'Lookup failed');
    } finally {
      setLookup('idle');
    }
  };

  const handleCreate = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    setCreateError(null);
    setCreating(true);
    try {
      const path = newPath.trim().startsWith('/') ? newPath.trim() : `/${newPath.trim()}`;
      const response = await fetch('/api/knowledge/wiki', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          title: newTitle.trim(),
          path,
          initial_content: newContent.trim() || `- ${newTitle.trim()}`,
        }),
      });
      if (!response.ok) {
        const payload = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(payload?.error ?? `HTTP ${response.status}`);
      }
      const data = (await response.json()) as { page: { page_id: string; title: string; path: string } };
      const entry: PageBookmark = {
        page_id: data.page.page_id,
        title: data.page.title,
        path: data.page.path,
        added_at: Date.now(),
      };
      const next = [entry, ...bookmarks.filter((b) => b.page_id !== entry.page_id)];
      setBookmarks(next);
      saveBookmarks(next);
      setCreateOpen(false);
      setNewTitle('');
      setNewPath('');
      setNewContent('');
      router.push(`/knowledge/wiki/${entry.page_id}`);
    } catch (err: unknown) {
      setCreateError(err instanceof Error ? err.message : 'Create failed');
    } finally {
      setCreating(false);
    }
  };

  const handleRemoveBookmark = (pageId: string): void => {
    const next = bookmarks.filter((b) => b.page_id !== pageId);
    setBookmarks(next);
    saveBookmarks(next);
  };

  return (
    <div className="px-6 py-6">
      <header className="mb-5 flex items-start justify-between gap-4">
        <div>
          <h1 className="flex items-center gap-2 text-[20px] font-semibold tracking-[-0.02em] text-[#111827]">
            <BookOpen className="size-4 text-[#6B7280]" />
            LLM Wiki
          </h1>
          <p className="mt-0.5 max-w-[64ch] text-[12px] text-[#6B7280]">
            Durable, versioned wiki pages synthesized from your sources. Edit in a
            Logseq-style block outline; agents read from the published version.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setCreateOpen(true)}
          className="inline-flex items-center gap-1.5 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B]"
        >
          <Plus className="size-3.5" />
          New page
        </button>
      </header>

      <section className="mb-6 rounded-xl border border-[#E5E7EB] bg-white p-4">
        <h2 className="text-[12px] font-medium text-[#374151]">Open page by path</h2>
        <p className="mt-0.5 text-[11px] text-[#6B7280]">
          wiki-store-go has no list-all endpoint yet — paste a known path to open it.
        </p>
        <form onSubmit={handleOpenByPath} className="mt-3 flex gap-2">
          <div className="relative flex-1">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-[#9CA3AF]" />
            <input
              type="text"
              value={openPath}
              onChange={(e) => setOpenPath(e.target.value)}
              placeholder="/engineering/onboarding"
              className="block w-full rounded-md border border-[#E5E7EB] bg-white py-1.5 pl-8 pr-3 text-[12px] text-[#111827] outline-none focus:border-[#111111]"
            />
          </div>
          <button
            type="submit"
            disabled={lookup === 'busy' || !openPath.trim()}
            className="inline-flex items-center gap-1 rounded-md bg-[#111111] px-3 py-1.5 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:opacity-50"
          >
            {lookup === 'busy' ? <Loader2 className="size-3.5 animate-spin" /> : null}
            Open
          </button>
        </form>
        {lookupError ? (
          <div className="mt-2 flex items-center gap-2 rounded-md border border-amber-200 bg-amber-50 px-2 py-1.5 text-[11px] text-amber-800">
            <AlertCircle className="size-3" />
            {lookupError}
          </div>
        ) : null}
      </section>

      <section>
        <h2 className="text-[12px] font-medium text-[#374151]">Recently opened</h2>
        {bookmarks.length === 0 ? (
          <div className="mt-3 flex min-h-[160px] flex-col items-center justify-center rounded-xl border border-dashed border-[#E5E7EB] bg-[#FAFAFA] px-6 py-8 text-center">
            <p className="text-[13px] font-medium text-[#111827]">No pages yet</p>
            <p className="mt-1 max-w-[40ch] text-[12px] leading-5 text-[#6B7280]">
              Create your first wiki page, or paste a path above if you already
              have one.
            </p>
          </div>
        ) : (
          <ul className="mt-3 grid gap-2 md:grid-cols-2">
            {bookmarks.map((b) => (
              <li
                key={b.page_id}
                className="flex items-start justify-between gap-2 rounded-xl border border-[#E5E7EB] bg-white p-3"
              >
                <Link
                  href={`/knowledge/wiki/${b.page_id}`}
                  className="min-w-0 flex-1 hover:text-[#111827]"
                >
                  <div className="flex items-center gap-1 text-[13px] font-medium text-[#111827]">
                    {b.title}
                    <ExternalLink className="size-3 text-[#9CA3AF]" />
                  </div>
                  <div className="mt-0.5 truncate font-mono text-[11px] text-[#6B7280]">
                    {b.path}
                  </div>
                </Link>
                <button
                  type="button"
                  onClick={() => handleRemoveBookmark(b.page_id)}
                  aria-label="Remove bookmark"
                  className="rounded-md p-1 text-[#9CA3AF] hover:bg-[#F3F4F6] hover:text-[#111827]"
                >
                  <X className="size-3" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {createOpen ? (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
          role="dialog"
          aria-modal="true"
          onClick={(event) => {
            if (event.target === event.currentTarget && !creating) setCreateOpen(false);
          }}
        >
          <form
            onSubmit={handleCreate}
            className="w-full max-w-[520px] rounded-2xl bg-white p-6 shadow-2xl"
          >
            <h2 className="text-[16px] font-semibold text-[#111827]">New wiki page</h2>
            <p className="mt-0.5 text-[12px] text-[#6B7280]">
              The page starts as a draft. Publish it from the editor when you&apos;re ready.
            </p>

            <div className="mt-4 space-y-3">
              <label className="block">
                <span className="block text-[12px] font-medium text-[#374151]">
                  Title <span className="text-red-500">*</span>
                </span>
                <input
                  type="text"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  required
                  placeholder="Engineering onboarding"
                  className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 text-[13px] text-[#111827] outline-none focus:border-[#111111]"
                />
              </label>
              <label className="block">
                <span className="block text-[12px] font-medium text-[#374151]">
                  Path <span className="text-red-500">*</span>
                </span>
                <input
                  type="text"
                  value={newPath}
                  onChange={(e) => setNewPath(e.target.value)}
                  required
                  placeholder="/engineering/onboarding"
                  className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 font-mono text-[12px] text-[#111827] outline-none focus:border-[#111111]"
                />
              </label>
              <label className="block">
                <span className="block text-[12px] font-medium text-[#374151]">
                  Initial content <span className="text-[#9CA3AF]">(optional)</span>
                </span>
                <textarea
                  value={newContent}
                  onChange={(e) => setNewContent(e.target.value)}
                  rows={4}
                  placeholder="- First block&#10;  - Indented block"
                  className="mt-1 block w-full rounded-md border border-[#E5E7EB] bg-white px-3 py-2 font-mono text-[12px] text-[#111827] outline-none focus:border-[#111111]"
                />
              </label>
            </div>

            {createError ? (
              <div className="mt-3 flex items-center gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[12px] text-red-700">
                <AlertCircle className="size-3.5" />
                {createError}
              </div>
            ) : null}

            <div className="mt-5 flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => setCreateOpen(false)}
                disabled={creating}
                className="rounded-full border border-[#E5E7EB] bg-white px-4 py-2 text-[12px] font-medium text-[#374151] hover:border-[#9CA3AF] disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={creating || !newTitle.trim() || !newPath.trim()}
                className="inline-flex items-center gap-2 rounded-full bg-[#111111] px-4 py-2 text-[12px] font-medium text-white hover:bg-[#2B2B2B] disabled:opacity-50"
              >
                {creating ? <Loader2 className="size-3.5 animate-spin" /> : null}
                Create page
              </button>
            </div>
          </form>
        </div>
      ) : null}
    </div>
  );
}
