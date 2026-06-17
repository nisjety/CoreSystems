'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import { ArrowRight, Search } from 'lucide-react';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { SearchInput } from '@/components/search/SearchInput';
import { SearchResults } from '@/components/search/SearchResults';
import { StreamingAnswer } from '@/components/search/StreamingAnswer';
import { streamSearch, type SearchSource } from '@/lib/api/search-api';
import { useDashboardSearch } from './DashboardSearchContext';

const EXAMPLE_QUERIES = [
  'What is our pricing strategy?',
  'Who owns GDPR compliance?',
  'Show me the onboarding process for new hires.',
];

function ShortcutBadge({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center rounded-xl border border-[#D8D2C6] bg-white px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-[#9A9387] shadow-[inset_0_0_0_1px_rgba(0,0,0,0.04)]">
      {label}
    </span>
  );
}

export function GlobalSearchModal() {
  const router = useRouter();
  const {
    isGlobalSearchOpen,
    globalSearchQuery,
    recentQueries,
    registerRecentQuery,
    setGlobalSearchQuery,
    closeGlobalSearch,
  } = useDashboardSearch();
  const [submittedQuery, setSubmittedQuery] = React.useState<string | null>(null);
  const [answer, setAnswer] = React.useState('');
  const [sources, setSources] = React.useState<SearchSource[]>([]);
  const [isStreaming, setIsStreaming] = React.useState(false);
  const [hasSearched, setHasSearched] = React.useState(false);
  const abortRef = React.useRef<AbortController | null>(null);

  const runSearch = React.useCallback(async (query: string) => {
    const normalizedQuery = query.trim();
    if (!normalizedQuery) {
      return;
    }

    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;

    setSubmittedQuery(normalizedQuery);
    setAnswer('');
    setSources([]);
    setIsStreaming(true);
    setHasSearched(true);
    registerRecentQuery(normalizedQuery);

    for await (const chunk of streamSearch(normalizedQuery, controller.signal)) {
      if (chunk.type === 'answer_chunk' && chunk.content) {
        setAnswer((prev) => prev + chunk.content);
      } else if (chunk.type === 'sources' && chunk.sources) {
        setSources(chunk.sources);
      } else if (chunk.type === 'done' || chunk.type === 'error') {
        setIsStreaming(false);
      }
    }
  }, [registerRecentQuery]);

  React.useEffect(() => () => {
    abortRef.current?.abort();
  }, []);

  React.useEffect(() => {
    if (!isGlobalSearchOpen) {
      abortRef.current?.abort();
      setIsStreaming(false);
    }
  }, [isGlobalSearchOpen]);

  const openFullSearchPage = React.useCallback(() => {
    const query = (submittedQuery ?? globalSearchQuery).trim();
    closeGlobalSearch();
    router.push(query ? `/search?q=${encodeURIComponent(query)}` : '/search');
  }, [closeGlobalSearch, globalSearchQuery, router, submittedQuery]);

  const hasAnswer = answer.length > 0;

  return (
    <Dialog open={isGlobalSearchOpen} onOpenChange={(open) => {
      if (!open) {
        closeGlobalSearch();
      }
    }}>
      <DialogContent className="w-[min(92vw,860px)] max-w-none border-[#DED8CC] bg-[#F4F1EB] p-0 shadow-[0_32px_120px_rgba(17,17,17,0.18)] sm:rounded-3xl">
        <DialogHeader className="border-b border-[#D8D2C6] px-6 py-5 sm:px-8">
          <div className="flex items-start justify-between gap-4">
            <DialogTitle className="flex items-center gap-2 text-[20px] font-medium text-[#2B2B2B]">
              <Search className="h-5 w-5 text-[#8C8478]" strokeWidth={1.8} />
              Search The Whole System
            </DialogTitle>
            <div className="hidden shrink-0 items-center gap-2 sm:flex">
              <ShortcutBadge label="/" />
              <ShortcutBadge label="Cmd+K" />
            </div>
          </div>
          <DialogDescription className="text-[13px] text-[#8B857B]">
            Use the navbar or minimized sidebar search for system-wide answers. The expanded sidebar search only filters the current section.
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[min(78dvh,760px)] overflow-y-auto px-6 py-6 sm:px-8 sm:py-7">
          <SearchInput
            value={globalSearchQuery}
            onChange={setGlobalSearchQuery}
            onSubmit={runSearch}
            isLoading={isStreaming}
            placeholder="Search across documents, teams, people, and internal knowledge..."
          />

          {!hasSearched ? (
            <div className="space-y-6 py-8">
              <div>
                <p className="text-[28px] leading-tight text-[#2B2B2B]" style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}>
                  Ask once, search everywhere.
                </p>
                <p className="mt-2 max-w-[52ch] text-[14px] leading-6 text-[#8B857B]">
                  Search across workspace documents, conversations, sites, and internal references from a single command surface.
                </p>
              </div>

              {recentQueries.length > 0 ? (
                <div className="space-y-3">
                  <p className="text-[11px] uppercase tracking-[0.24em] text-[#B4AD9F]">Recent Queries</p>
                  <div className="flex flex-wrap gap-2">
                    {recentQueries.map((query) => (
                      <button
                        key={query}
                        type="button"
                        onClick={() => {
                          setGlobalSearchQuery(query);
                          runSearch(query);
                        }}
                        className="rounded-full border border-[#D8D2C6] bg-white px-4 py-2 text-[12px] text-[#5C564E] transition-colors hover:border-[#2B2B2B] hover:bg-[#F2ECE3]"
                      >
                        {query}
                      </button>
                    ))}
                  </div>
                </div>
              ) : null}

              <div className="grid gap-2 sm:grid-cols-3">
                {EXAMPLE_QUERIES.map((query) => (
                  <button
                    key={query}
                    type="button"
                    onClick={() => {
                      setGlobalSearchQuery(query);
                      runSearch(query);
                    }}
                    className="border border-[#D8D2C6] bg-[#FBF8F3] px-4 py-3 text-left text-[13px] leading-5 text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#ECE6DD]"
                  >
                    {query}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="space-y-8 py-7">
              {submittedQuery ? (
                <div>
                  <p className="mb-1 text-[11px] uppercase tracking-[0.26em] text-[#B4AD9F]">Question</p>
                  <h2 className="text-[28px] leading-tight text-[#2B2B2B]" style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}>
                    {submittedQuery}
                  </h2>
                </div>
              ) : null}

              {(hasAnswer || isStreaming) ? (
                <div>
                  <p className="mb-3 text-[11px] uppercase tracking-[0.26em] text-[#B4AD9F]">Answer</p>
                  <StreamingAnswer text={answer} isStreaming={isStreaming} />
                </div>
              ) : null}

              {sources.length > 0 ? <SearchResults sources={sources} /> : null}
            </div>
          )}
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-[#D8D2C6] bg-[#F8F5EF] px-6 py-4 sm:px-8">
          <div className="flex flex-wrap items-center gap-2 text-[12px] text-[#8B857B]">
            <span>Enter runs a global search. Escape closes the modal.</span>
            <div className="flex items-center gap-2">
              <ShortcutBadge label="/" />
              <ShortcutBadge label="Cmd+K" />
            </div>
          </div>
          <button
            type="button"
            onClick={openFullSearchPage}
            className="inline-flex items-center gap-2 border border-[#D8D2C6] bg-white px-4 py-2 text-[12px] uppercase tracking-[0.18em] text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#2B2B2B] hover:text-white"
          >
            Open Search Page
            <ArrowRight className="h-3.5 w-3.5" strokeWidth={1.8} />
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}