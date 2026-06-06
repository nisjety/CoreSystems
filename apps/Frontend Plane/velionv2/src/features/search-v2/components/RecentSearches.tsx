"use client";

import { useEffect, useState } from "react";
import { useQuery } from "convex/react";
import { makeFunctionReference } from "convex/server";

// Decoupled reference to convex-core's `searches.listThreads` query — velionv2
// doesn't carry convex-core's generated `api`, so we name the function by path.
// Args/return are untyped here; we narrow the row shape locally.
const listThreadsRef = makeFunctionReference<"query">("searches:listThreads");

type SearchThread = {
  _id: string;
  query: string;
  answer: string;
  createdAt: number;
  updatedAt: number;
};

type SearchContext = { userId: string; orgId: string | null };

/**
 * RecentSearches — live recent search history for the signed-in user, scoped to
 * their active org. Reads reactively from Convex (convex-core `searchThreads`),
 * so a search persisted by the BFF appears here without a refresh. Self-hides
 * when the user has no context or no history yet.
 */
export function RecentSearches({
  onPick,
}: {
  onPick?: (query: string) => void;
}) {
  const [ctx, setCtx] = useState<SearchContext | null>(null);

  // Resolve { userId, orgId } once — the same scoping the persist route writes
  // under, so the list matches what was stored.
  useEffect(() => {
    let cancelled = false;
    void fetch("/api/v1/search/context", {
      credentials: "include",
      cache: "no-store",
    })
      .then((r) => (r.ok ? r.json() : null))
      .then((j) => {
        const data = (j as { data?: SearchContext } | null)?.data;
        if (!cancelled && data?.userId) setCtx(data);
      })
      .catch(() => {
        /* not signed in / context unavailable — stay hidden */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const threads = useQuery(
    listThreadsRef,
    ctx?.userId
      ? {
          externalUserId: ctx.userId,
          limit: 8,
          ...(ctx.orgId ? { externalOrgId: ctx.orgId } : {}),
        }
      : "skip",
  ) as SearchThread[] | undefined;

  if (!ctx?.userId) return null;
  if (!threads || threads.length === 0) return null;

  return (
    <section
      aria-label="Nylige søk"
      className="velion-glass-soft rounded-3xl px-4 py-3.5"
    >
      <h3 className="text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#9A9EA8]">
        Nylige søk
      </h3>
      <ul className="mt-2 flex flex-col gap-0.5">
        {threads.map((t) => (
          <li key={t._id}>
            <button
              type="button"
              onClick={() => onPick?.(t.query)}
              className="group flex w-full items-center gap-2 rounded-xl px-2 py-1.5 text-left transition-colors hover:bg-[#EE7A50]/8"
            >
              <svg
                aria-hidden="true"
                viewBox="0 0 24 24"
                className="h-3.5 w-3.5 shrink-0 text-[#9A9188] group-hover:text-[#EE7A50] dark:text-[#9A9EA8]"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <circle cx="12" cy="12" r="9" />
                <path d="M12 7v5l3 2" />
              </svg>
              <span className="truncate text-[13px] text-[#5F5A54] group-hover:text-[#1A1A1A] dark:text-[#B6BAC4] dark:group-hover:text-[#F7F8F8]">
                {t.query}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
