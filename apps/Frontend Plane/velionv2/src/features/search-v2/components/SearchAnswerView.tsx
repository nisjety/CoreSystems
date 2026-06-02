"use client";

import Link from "next/link";
import type { Route } from "next";
import { useEffect, useReducer, useRef } from "react";
import { useRouter } from "next/navigation";
import { ArrowLeft, ExternalLink, Search } from "lucide-react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type WebSearchResult = {
  url: string;
  title?: string;
  snippet?: string;
};

type WebSearchCitation = {
  url: string;
  title?: string;
};

type FetchedPage = {
  url: string;
  title: string;
  description: string | null;
  excerpt: string | null;
};

type SearchState = {
  query: string;
  submittedQuery: string;
  mode: "search" | "fetch" | null;
  results: WebSearchResult[];
  answer: string;
  citations: WebSearchCitation[];
  fetchedPage: FetchedPage | null;
  loading: boolean;
  error: string | null;
};

type SearchAction =
  | { type: "query-changed"; query: string }
  | { type: "submitted"; query: string }
  | { type: "search-started" }
  | {
      type: "results-loaded";
      results: WebSearchResult[];
      answer: string;
      citations: WebSearchCitation[];
    }
  | { type: "fetch-loaded"; page: FetchedPage }
  | { type: "error"; message: string };

// ---------------------------------------------------------------------------
// Reducer
// ---------------------------------------------------------------------------

const initialState: SearchState = {
  query: "",
  submittedQuery: "",
  mode: null,
  results: [],
  answer: "",
  citations: [],
  fetchedPage: null,
  loading: false,
  error: null,
};

function searchReducer(state: SearchState, action: SearchAction): SearchState {
  switch (action.type) {
    case "query-changed":
      return { ...state, query: action.query };
    case "submitted":
      return { ...state, submittedQuery: action.query };
    case "search-started":
      return {
        ...state,
        loading: true,
        error: null,
        mode: null,
        results: [],
        answer: "",
        citations: [],
        fetchedPage: null,
      };
    case "results-loaded":
      return {
        ...state,
        loading: false,
        mode: "search",
        results: action.results,
        answer: action.answer,
        citations: action.citations,
      };
    case "fetch-loaded":
      return { ...state, loading: false, mode: "fetch", fetchedPage: action.page };
    case "error":
      return { ...state, loading: false, error: action.message };
    default:
      return state;
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function safeHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

// ---------------------------------------------------------------------------
// Tab bar
// ---------------------------------------------------------------------------

const TABS = ["Info", "Videos", "Kart", "Bilder", "Shopping"] as const;
type Tab = (typeof TABS)[number];

function TabBar({ active }: { active: Tab }) {
  return (
    <div
      role="tablist"
      aria-label="Søkekategorier"
      className="flex items-center gap-1 border-b border-black/[0.06] dark:border-white/[0.06]"
    >
      {TABS.map((tab) => {
        const isActive = tab === active;
        const isDisabled = tab !== "Info";
        return (
          <button
            key={tab}
            role="tab"
            aria-selected={isActive}
            aria-disabled={isDisabled}
            disabled={isDisabled}
            title={isDisabled ? `${tab} — Kommer snart` : tab}
            className={[
              "relative px-4 py-2.5 text-[13px] font-medium transition-colors focus:outline-none",
              isActive
                ? "text-[#EE7A50] after:absolute after:inset-x-0 after:bottom-0 after:h-[2px] after:rounded-full after:bg-[#EE7A50]"
                : isDisabled
                  ? "cursor-not-allowed text-[#B0A899] dark:text-[#5A5D65]"
                  : "text-[#6B6560] hover:text-[#1A1A1A] dark:text-[#AEB4C0] dark:hover:text-white",
            ].join(" ")}
          >
            {tab}
            {isDisabled && tab !== active ? (
              <span className="ml-1.5 rounded-full bg-[#F4F1EB] px-1.5 py-0.5 text-[10px] font-medium text-[#A09890] dark:bg-[#2A2C31] dark:text-[#6B7080]">
                Snart
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Loading skeleton
// ---------------------------------------------------------------------------

function LoadingSkeleton() {
  return (
    <div className="flex flex-col gap-3" aria-busy="true" aria-label="Søker på nettet…">
      {/* Answer card skeleton */}
      <div className="h-24 animate-pulse rounded-[16px] bg-[#F4F1EB] dark:bg-[#1D1A17]" />
      {/* Sources skeleton */}
      <div className="flex gap-2">
        {[1, 2, 3].map((i) => (
          <div key={i} className="h-6 w-20 animate-pulse rounded-full bg-[#F4F1EB] dark:bg-[#1D1A17]" />
        ))}
      </div>
      {/* Results skeleton */}
      {[1, 2, 3, 4].map((i) => (
        <div key={i} className="h-16 animate-pulse rounded-[14px] bg-[#F4F1EB] dark:bg-[#1D1A17]" />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

export function SearchAnswerView({ initialQuery }: { initialQuery: string }) {
  const router = useRouter();
  const abortRef = useRef<AbortController | null>(null);
  const followUpAbortRef = useRef<AbortController | null>(null);

  const [state, dispatch] = useReducer(searchReducer, {
    ...initialState,
    query: initialQuery,
    submittedQuery: initialQuery,
  });

  const {
    query,
    submittedQuery,
    mode,
    results,
    answer,
    citations,
    fetchedPage,
    loading,
    error,
  } = state;

  // -------------------------------------------------------------------------
  // Fetch logic
  // -------------------------------------------------------------------------

  function runSearch(q: string) {
    if (!q.trim()) return;

    // Update URL
    router.replace(`/search?q=${encodeURIComponent(q.trim())}` as Route);

    abortRef.current?.abort();
    const abort = new AbortController();
    abortRef.current = abort;

    dispatch({ type: "submitted", query: q.trim() });
    dispatch({ type: "search-started" });

    void (async () => {
      try {
        const response = await fetch("/api/v1/search/web", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ query: q.trim() }),
          signal: abort.signal,
        });

        type FetchPayloadData = {
          mode: "fetch";
          url: string;
          title: string;
          description: string | null;
          excerpt: string | null;
        };
        type SearchPayloadData = {
          mode: "search";
          results: WebSearchResult[];
          answer: string | null;
          citations: WebSearchCitation[];
        };

        const payload = (await response.json().catch(() => null)) as
          | { data?: FetchPayloadData | SearchPayloadData; error?: { code?: string; message: string } }
          | null;

        if (!response.ok || !payload || !payload.data) {
          dispatch({
            type: "error",
            message: payload?.error?.message ?? "Web search could not be completed.",
          });
          return;
        }

        if (payload.data.mode === "fetch") {
          dispatch({
            type: "fetch-loaded",
            page: {
              url: payload.data.url,
              title: payload.data.title,
              description: payload.data.description,
              excerpt: payload.data.excerpt,
            },
          });
        } else {
          dispatch({
            type: "results-loaded",
            results: payload.data.results ?? [],
            answer: payload.data.answer ?? "",
            citations: payload.data.citations ?? [],
          });
        }
      } catch (err: unknown) {
        if (err instanceof DOMException && err.name === "AbortError") return;
        dispatch({ type: "error", message: "Web search could not be completed." });
      }
    })();
  }

  // Run on mount if we have an initial query
  useEffect(() => {
    if (initialQuery.trim()) {
      runSearch(initialQuery);
    }
    return () => {
      abortRef.current?.abort();
      followUpAbortRef.current?.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // -------------------------------------------------------------------------
  // Header submit
  // -------------------------------------------------------------------------

  const handleHeaderSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = query.trim();
    if (!trimmed || trimmed === submittedQuery) return;
    runSearch(trimmed);
  };

  // -------------------------------------------------------------------------
  // Follow-up composer
  // -------------------------------------------------------------------------

  const [followUp, setFollowUp] = useLocalState("");

  const handleFollowUp = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = followUp.trim();
    if (!trimmed) return;
    setFollowUp("");
    // TODO(P4): wire to /api/chat/stream for conversational follow-up
    dispatch({ type: "query-changed", query: trimmed });
    runSearch(trimmed);
  };

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* ------------------------------------------------------------------ */}
      {/* Header / query bar */}
      {/* ------------------------------------------------------------------ */}
      <header className="flex shrink-0 items-center gap-3 border-b border-black/[0.06] bg-[#FCFCFD] px-4 py-3 dark:border-white/[0.06] dark:bg-[#1C1E24]">
        <Link
          href={"/dashboard" as Route}
          aria-label="Tilbake til dashboard"
          title="Tilbake til dashboard"
          className="grid size-8 shrink-0 place-items-center rounded-[10px] text-[#6B6560] transition hover:bg-black/[0.05] dark:text-[#AEB4C0] dark:hover:bg-white/[0.08]"
        >
          <ArrowLeft className="size-4" />
        </Link>

        <form onSubmit={handleHeaderSubmit} className="flex min-w-0 flex-1 items-center">
          <label className="sr-only" htmlFor="answer-search-query">
            Søk
          </label>
          <div className="flex h-10 min-w-0 flex-1 items-center rounded-full bg-white/88 px-4 shadow-[0_2px_8px_rgba(20,21,24,0.08)] ring-1 ring-[#EFE7DC] backdrop-blur-sm dark:bg-[#1A1B20]/92 dark:ring-[#2A2C31]">
            <Search className="mr-2 size-4 shrink-0 text-[#9A9188]" />
            <input
              id="answer-search-query"
              type="search"
              value={query}
              onChange={(e) => dispatch({ type: "query-changed", query: e.target.value })}
              className="h-full min-w-0 flex-1 bg-transparent text-[14px] font-medium text-[#24262D] placeholder:text-[#AAA198] focus:outline-none dark:text-white dark:placeholder:text-[#737780]"
              placeholder="Søk på nytt…"
              autoComplete="off"
            />
            <button
              type="submit"
              aria-label="Søk"
              disabled={!query.trim() || loading}
              className="ml-2 grid size-7 shrink-0 place-items-center rounded-full bg-[#111111] text-white transition hover:bg-[#2A2A2A] disabled:bg-[#E9E4DD] disabled:text-[#A99E93] dark:bg-white dark:text-[#111111] dark:disabled:bg-[#35373D] dark:disabled:text-[#5A5D65]"
            >
              <Search className="size-3.5" />
            </button>
          </div>
        </form>
      </header>

      {/* ------------------------------------------------------------------ */}
      {/* Tab bar */}
      {/* ------------------------------------------------------------------ */}
      <div className="shrink-0 bg-[#FCFCFD] px-4 dark:bg-[#1C1E24]">
        <TabBar active="Info" />
      </div>

      {/* ------------------------------------------------------------------ */}
      {/* Scrollable results area */}
      {/* ------------------------------------------------------------------ */}
      <main
        className="min-h-0 flex-1 overflow-y-auto px-4 py-5"
        aria-label="Søkeresultater"
      >
        <div className="mx-auto flex max-w-3xl flex-col gap-4">
          {/* Submitted query header */}
          {submittedQuery ? (
            <h1 className="text-[20px] font-[540] tracking-[-0.02em] text-[#1A1A1A] dark:text-[#F7F8F8]">
              {submittedQuery}
            </h1>
          ) : null}

          {/* Loading skeleton */}
          {loading ? <LoadingSkeleton /> : null}

          {/* Error state */}
          {!loading && error ? (
            <div className="rounded-[14px] bg-[#FDF2F0] px-4 py-3 text-[13px] text-[#B04020] dark:bg-[#2A1A17] dark:text-[#E8A090]">
              {error}
            </div>
          ) : null}

          {/* ---- mode: "fetch" ---- */}
          {!loading && !error && mode === "fetch" && fetchedPage ? (
            <div className="rounded-[16px] bg-white px-5 py-4 shadow-[0_2px_8px_rgba(20,21,24,0.06)] ring-1 ring-black/[0.04] dark:bg-[#1A1B20] dark:ring-white/[0.06]">
              <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#737780]">
                Hentet side
              </p>
              <a
                href={fetchedPage.url}
                target="_blank"
                rel="noopener noreferrer"
                className="text-[15px] font-semibold text-[#EE7A50] underline-offset-2 hover:underline"
              >
                {fetchedPage.title}
              </a>
              <p className="mt-0.5 truncate text-[11px] text-[#9A9188] dark:text-[#737780]">
                {fetchedPage.url}
              </p>
              {fetchedPage.description ? (
                <p className="mt-2 text-[13px] leading-relaxed text-[#5F5A54] dark:text-[#AEB4C0]">
                  {fetchedPage.description}
                </p>
              ) : null}
              {fetchedPage.excerpt ? (
                <p className="mt-2 border-t border-black/[0.06] pt-2 text-[12px] leading-relaxed text-[#3A3530] dark:border-white/[0.06] dark:text-[#D4D6DC]">
                  {fetchedPage.excerpt}
                </p>
              ) : null}
              <a
                href={fetchedPage.url}
                target="_blank"
                rel="noopener noreferrer"
                className="mt-3 inline-flex items-center gap-1 rounded-full bg-[#F4F1EB] px-3 py-1 text-[12px] font-medium text-[#504A43] transition hover:bg-[#EBE6DD] dark:bg-[#2A2C31] dark:text-[#D4D6DC] dark:hover:bg-[#35373D]"
              >
                Åpne side
                <ExternalLink className="size-3" />
              </a>
            </div>
          ) : null}

          {/* ---- mode: "search" — AI summary card ---- */}
          {!loading && !error && mode === "search" && answer ? (
            <section
              aria-labelledby="ai-summary-heading"
              className="rounded-[16px] bg-[#F4F1EB] px-5 py-4 dark:bg-[#1D1A17]"
            >
              <h2
                id="ai-summary-heading"
                className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#737780]"
              >
                AI-sammendrag
              </h2>
              <p className="text-[13px] leading-relaxed text-[#3A3530] dark:text-[#D4D6DC]">
                {answer}
              </p>
              {citations.length > 0 ? (
                <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-black/[0.06] pt-3 dark:border-white/[0.06]">
                  <span className="text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#737780]">
                    Kilder
                  </span>
                  {citations.map((citation, idx) => (
                    <a
                      key={idx}
                      href={citation.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-1 rounded-full bg-white px-2.5 py-1 text-[11px] font-medium text-[#504A43] shadow-sm ring-1 ring-black/[0.06] transition hover:bg-[#EBE6DD] dark:bg-[#2A2C31] dark:text-[#D4D6DC] dark:ring-white/[0.06] dark:hover:bg-[#35373D]"
                    >
                      {citation.title ?? safeHostname(citation.url)}
                      <ExternalLink className="size-2.5 shrink-0 opacity-60" />
                    </a>
                  ))}
                </div>
              ) : null}
            </section>
          ) : null}

          {/* ---- mode: "search" — web results list ---- */}
          {!loading && !error && mode === "search" && results.length > 0 ? (
            <section aria-labelledby="web-results-heading">
              <h2
                id="web-results-heading"
                className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#737780]"
              >
                Webresultater
              </h2>
              <ul className="flex flex-col gap-2">
                {results.map((result, idx) => (
                  <li key={idx}>
                    <a
                      href={result.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="group block rounded-[14px] bg-white px-4 py-3 shadow-[0_2px_8px_rgba(20,21,24,0.06)] ring-1 ring-black/[0.04] transition hover:shadow-[0_4px_16px_rgba(20,21,24,0.10)] dark:bg-[#1A1B20] dark:ring-white/[0.06]"
                    >
                      {result.title ? (
                        <p className="text-[13px] font-semibold text-[#1A1A1A] group-hover:text-[#EE7A50] dark:text-[#F7F8F8]">
                          {result.title}
                        </p>
                      ) : null}
                      <p className="mt-0.5 truncate text-[11px] text-[#9A9188] dark:text-[#737780]">
                        {safeHostname(result.url)}
                      </p>
                      {result.snippet ? (
                        <p className="mt-1 text-[12px] leading-relaxed text-[#5F5A54] dark:text-[#AEB4C0]">
                          {result.snippet}
                        </p>
                      ) : null}
                    </a>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          {/* Empty state */}
          {!loading && !error && mode === "search" && results.length === 0 && !answer ? (
            <div className="rounded-[14px] bg-[#F4F1EB] px-4 py-3 text-[13px] text-[#7A756F] dark:bg-[#1D1A17] dark:text-[#AEB4C0]">
              Ingen webresultater funnet for{" "}
              <span className="font-medium text-[#1A1A1A] dark:text-white">{submittedQuery}</span>.
            </div>
          ) : null}
        </div>
      </main>

      {/* ------------------------------------------------------------------ */}
      {/* Pinned follow-up composer */}
      {/* ------------------------------------------------------------------ */}
      <div className="shrink-0 border-t border-black/[0.06] bg-[#FCFCFD] px-4 py-3 dark:border-white/[0.06] dark:bg-[#1C1E24]">
        <form onSubmit={handleFollowUp} className="mx-auto flex max-w-3xl items-center gap-2">
          <label className="sr-only" htmlFor="search-follow-up">
            Stille oppfølgingsspørsmål
          </label>
          <div className="flex h-11 min-w-0 flex-1 items-center rounded-full bg-white/88 px-4 shadow-[0_2px_8px_rgba(20,21,24,0.06)] ring-1 ring-[#EFE7DC] backdrop-blur-sm dark:bg-[#1A1B20]/92 dark:ring-[#2A2C31]">
            <input
              id="search-follow-up"
              type="text"
              value={followUp}
              onChange={(e) => setFollowUp(e.target.value)}
              className="h-full min-w-0 flex-1 bg-transparent text-[13px] text-[#24262D] placeholder:text-[#AAA198] focus:outline-none dark:text-white dark:placeholder:text-[#737780]"
              placeholder="Spør mer om dette…"
              autoComplete="off"
            />
          </div>
          <button
            type="submit"
            aria-label="Send oppfølgingsspørsmål"
            disabled={!followUp.trim() || loading}
            className="grid size-11 shrink-0 place-items-center rounded-full bg-[#111111] text-white transition hover:bg-[#2A2A2A] disabled:bg-[#E9E4DD] disabled:text-[#A99E93] dark:bg-white dark:text-[#111111] dark:disabled:bg-[#35373D] dark:disabled:text-[#5A5D65]"
          >
            <Search className="size-4" />
          </button>
        </form>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Micro-hook: local state for the follow-up input (avoids extra import)
// ---------------------------------------------------------------------------

function useLocalState(initial: string): [string, (v: string) => void] {
  const [value, setValue] = useReducer((_: string, next: string) => next, initial);
  return [value, setValue];
}
