'use client';

import { useCallback, useEffect, useRef } from 'react';
import { useRouter } from 'next/navigation';

import { SearchInput } from '@/components/search/SearchInput';
import { StreamingAnswer } from '@/components/search/StreamingAnswer';
import { SearchResults } from '@/components/search/SearchResults';
import { streamSearch } from '@/lib/api/search-api';

import { useSearchState } from './use-search-state';

interface SearchPageClientProps {
  initialQuery: string | null;
}

export function SearchPageClient({ initialQuery }: SearchPageClientProps) {
  const router = useRouter();
  const [state, dispatch] = useSearchState(initialQuery);
  const { query, submittedQuery, answer, sources, isStreaming, hasSearched } = state;

  const abortRef = useRef<AbortController | null>(null);
  const initialStreamRef = useRef<string | null>(null);

  const runSearch = useCallback(async (q: string) => {
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;

    dispatch({ type: 'START_SEARCH', payload: q });
    router.replace(`/search?q=${encodeURIComponent(q)}`, { scroll: false });

    for await (const chunk of streamSearch(q, controller.signal)) {
      if (chunk.type === 'answer_chunk' && chunk.content) {
        dispatch({ type: 'APPEND_ANSWER', payload: chunk.content });
      } else if (chunk.type === 'sources' && chunk.sources) {
        dispatch({ type: 'SET_SOURCES', payload: chunk.sources });
      } else if (chunk.type === 'done' || chunk.type === 'error') {
        dispatch({ type: 'STOP_STREAMING' });
      }
    }
  }, [dispatch, router]);

  useEffect(() => {
    if (!initialQuery || initialStreamRef.current === initialQuery) {
      return;
    }

    initialStreamRef.current = initialQuery;
    void runSearch(initialQuery);
  }, [initialQuery, runSearch]);

  const isEmpty = !hasSearched;
  const hasAnswer = Boolean(answer);

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-y-auto bg-[#F4F1EB]">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 -z-10"
        style={{
          background:
            'radial-gradient(ellipse 60% 40% at 20% 0%, rgba(230,220,255,0.18) 0%, transparent 65%),' +
            'radial-gradient(ellipse 50% 40% at 90% 100%, rgba(180,200,240,0.15) 0%, transparent 65%)',
        }}
      />

      <div className="border-b border-[#D8D2C6] px-6 py-5 md:px-10">
        <div className="mx-auto max-w-3xl">
          <SearchInput
            value={query}
            onChange={(value: string) => dispatch({ type: 'SET_QUERY', payload: value })}
            onSubmit={runSearch}
            isLoading={isStreaming}
          />
        </div>
      </div>

      <div className="flex-1 px-6 py-10 md:px-10">
        <div className="mx-auto max-w-3xl space-y-10">
          {isEmpty && (
            <div className="py-20 text-center">
              <h2
                className="mb-3 text-[32px] font-normal text-[#2B2B2B]"
                style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
              >
                Søk i selskapets kunnskap
              </h2>
              <p className="mx-auto max-w-[44ch] font-inter text-[14px] leading-relaxed text-[#A09890]">
                Søk på tvers av nettsider, dokumenter, SharePoint og Teams —
                og få et svar med kildehenvisninger.
              </p>

              <div className="mt-10 flex flex-col items-center gap-2.5">
                {[
                  'Hva er selskapets prisingsstrategi?',
                  'Hvem er ansvarlig for GDPR-compliance?',
                  'Hva er onboarding-prosessen for nye ansatte?',
                ].map((example) => (
                  <button
                    key={example}
                    onClick={() => {
                      dispatch({ type: 'SET_QUERY', payload: example });
                      void runSearch(example);
                    }}
                    className="border border-[#D8D2C6] bg-transparent px-5 py-2.5 font-inter text-[13px] text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
                  >
                    {example}
                  </button>
                ))}
              </div>
            </div>
          )}

          {submittedQuery && (
            <>
              <div>
                <p className="mb-1 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
                  Spørsmål
                </p>
                <h2
                  className="text-[26px] font-normal leading-tight text-[#2B2B2B]"
                  style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
                >
                  {submittedQuery}
                </h2>
              </div>

              {(hasAnswer || isStreaming) && (
                <div>
                  <p className="mb-3 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
                    Svar
                  </p>
                  <StreamingAnswer text={answer} isStreaming={isStreaming} />
                </div>
              )}

              {sources.length > 0 && <SearchResults sources={sources} />}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
