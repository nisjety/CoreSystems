"use client";

import Link from "next/link";
import type { Route } from "next";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import type { Easing, Transition, Variants } from "framer-motion";
import { ArrowLeft, ExternalLink, Globe, Play, Search, SendHorizontal, X } from "lucide-react";

import { streamChat } from "@/features/chat-v2/lib/chat-stream";
import { LiquidBackdrop } from "@/features/search-v2/components/LiquidBackdrop";
import { RecentSearches } from "@/features/search-v2/components/RecentSearches";
import {
  buildGroundingContent,
  dedupeSources,
  type GroundingSource,
  type ThreadTurn,
} from "@/features/search-v2/lib/answer-thread";
import {
  readCachedValue,
  writeCachedValue,
} from "@/features/search-v2/lib/search-query-cache";

// ---------------------------------------------------------------------------
// Motion presets (gentle fade/slide-in with a small stagger). All durations
// collapse to ~0 when prefers-reduced-motion is set via the global
// prefers-reduced-motion CSS rule + per-component useReducedMotion guards.
// ---------------------------------------------------------------------------

const EASE_OUT: Easing = [0.19, 1, 0.22, 1];

const listContainer: Variants = {
  hidden: {},
  show: { transition: { staggerChildren: 0.06, delayChildren: 0.04 } },
};

const listItem: Variants = {
  hidden: { opacity: 0, y: 12 },
  show: { opacity: 1, y: 0, transition: { duration: 0.42, ease: EASE_OUT } },
};

const tabPanel: Variants = {
  hidden: { opacity: 0, y: 8 },
  show: { opacity: 1, y: 0, transition: { duration: 0.32, ease: EASE_OUT } },
  exit: { opacity: 0, y: -6, transition: { duration: 0.18, ease: "easeIn" } },
};

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

// Sanitized image hit from /api/v1/search/images.
type ImageHit = {
  url: string;
  thumbnailUrl: string;
  imageUrl: string;
  title: string | null;
};

const SEARCH_IMAGE_CACHE_TTL_MS = 5 * 60 * 1000;
const SEARCH_VIDEO_CACHE_TTL_MS = 5 * 60 * 1000;
const searchImageCache = new Map<
  string,
  { expiresAt: number; value: ImageHit[] }
>();

// Status for the lazily-loaded Bilder (images) vertical. It loads
// independently of the Info (web) vertical so switching tabs is instant.
type ImagesStatus = "idle" | "loading" | "loaded" | "error";

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
  // Images vertical (Bilder tab).
  images: ImageHit[];
  imagesStatus: ImagesStatus;
  imagesError: string | null;
  // The query the loaded images correspond to, so a new search invalidates them.
  imagesQuery: string;
  // Conversational follow-up thread (turn 0 = the initial search, rendered
  // separately above). Each follow-up appends a user turn + a streaming
  // assistant turn. Reset whenever a fresh search starts.
  thread: ThreadTurn[];
  // True while an assistant turn is streaming; gates the composer send button.
  threadStreaming: boolean;
  // True while the initial (turn-0) answer is streaming in token-by-token.
  answerStreaming: boolean;
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
  | { type: "error"; message: string }
  | { type: "images-started"; query: string }
  | { type: "images-loaded"; query: string; images: ImageHit[] }
  | { type: "images-error"; query: string; message: string }
  // Initial (turn-0) answer streaming, mirroring the follow-up turn deltas.
  | { type: "answer-stream-started" }
  | { type: "answer-delta"; delta: string }
  | { type: "answer-stream-done" }
  // Follow-up thread actions. `turn-appended` adds the user question + an
  // empty streaming assistant turn in one step (ids supplied by the caller).
  | { type: "turn-appended"; userTurn: ThreadTurn; assistantTurn: ThreadTurn }
  | { type: "turn-delta"; id: string; delta: string }
  | { type: "turn-done"; id: string }
  | { type: "turn-error"; id: string; message: string };

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
  images: [],
  imagesStatus: "idle",
  imagesError: null,
  imagesQuery: "",
  thread: [],
  threadStreaming: false,
  answerStreaming: false,
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
        // A new search invalidates any previously loaded images.
        images: [],
        imagesStatus: "idle",
        imagesError: null,
        imagesQuery: "",
        // A fresh search starts a brand-new conversation.
        thread: [],
        threadStreaming: false,
        answerStreaming: false,
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
    case "images-started":
      return {
        ...state,
        imagesStatus: "loading",
        imagesError: null,
        imagesQuery: action.query,
      };
    case "images-loaded":
      // Ignore stale responses for a query the user has moved on from.
      if (action.query !== state.imagesQuery) return state;
      return { ...state, imagesStatus: "loaded", images: action.images };
    case "images-error":
      if (action.query !== state.imagesQuery) return state;
      return { ...state, imagesStatus: "error", imagesError: action.message };
    case "answer-stream-started":
      return { ...state, answer: "", answerStreaming: true };
    case "answer-delta":
      return { ...state, answer: state.answer + action.delta };
    case "answer-stream-done":
      return { ...state, answerStreaming: false };
    case "turn-appended":
      return {
        ...state,
        thread: [...state.thread, action.userTurn, action.assistantTurn],
        threadStreaming: true,
      };
    case "turn-delta":
      return {
        ...state,
        thread: state.thread.map((turn) =>
          turn.id === action.id ? { ...turn, text: turn.text + action.delta } : turn,
        ),
      };
    case "turn-done":
      return {
        ...state,
        threadStreaming: false,
        thread: state.thread.map((turn) =>
          turn.id === action.id ? { ...turn, streaming: false } : turn,
        ),
      };
    case "turn-error":
      return {
        ...state,
        threadStreaming: false,
        thread: state.thread.map((turn) =>
          turn.id === action.id
            ? { ...turn, streaming: false, error: action.message }
            : turn,
        ),
      };
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

const TABS = ["Info", "Bilder", "Videos", "Kart", "Shopping"] as const;
type Tab = (typeof TABS)[number];

// Tabs backed by a real provider today. Info → web search/answer; Bilder →
// SearXNG image vertical; Videos → SearXNG video vertical (inline-play cards).
// Kart/Shopping have no provider yet and stay honestly disabled with a "Snart"
// (soon) badge.
const ENABLED_TABS: ReadonlySet<Tab> = new Set<Tab>(["Info", "Bilder", "Videos"]);

function TabBar({ active, onSelect }: { active: Tab; onSelect: (tab: Tab) => void }) {
  return (
    <div
      role="tablist"
      aria-label="Søkekategorier"
      className="flex items-center gap-1 border-b border-black/[0.06] dark:border-white/[0.06]"
    >
      {TABS.map((tab) => {
        const isActive = tab === active;
        const isDisabled = !ENABLED_TABS.has(tab);
        return (
          <button
            key={tab}
            type="button"
            role="tab"
            aria-selected={isActive}
            aria-disabled={isDisabled}
            disabled={isDisabled}
            onClick={() => {
              if (!isDisabled && !isActive) onSelect(tab);
            }}
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
      <div className="h-24 animate-pulse rounded-3xl bg-white/45 dark:bg-white/[0.06]" />
      {/* Sources skeleton */}
      <div className="flex gap-2">
        {[1, 2, 3].map((i) => (
          <div key={i} className="h-6 w-20 animate-pulse rounded-full bg-white/45 dark:bg-white/[0.06]" />
        ))}
      </div>
      {/* Results skeleton */}
      {[1, 2, 3, 4].map((i) => (
        <div key={i} className="h-16 animate-pulse rounded-3xl bg-white/45 dark:bg-white/[0.06]" />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Image grid (Bilder tab)
// ---------------------------------------------------------------------------

function ImageGridSkeleton() {
  return (
    <div
      aria-busy="true"
      aria-label="Laster bilder…"
      className="[column-fill:_balance] gap-3 [column-count:2] sm:[column-count:3]"
    >
      {[36, 28, 44, 32, 40, 30, 38, 34, 42].map((h, i) => (
        <div
          key={i}
          style={{ height: `${h * 4}px` }}
          className="mb-3 w-full animate-pulse rounded-[18px] bg-white/45 dark:bg-white/[0.06]"
        />
      ))}
    </div>
  );
}

function ImageGallery({ images }: { images: ImageHit[] }) {
  // Two-step interaction (per product spec): the first click on a thumbnail
  // EXPANDS the image in an in-page lightbox (no navigation); clicking the
  // expanded image then opens the source page. Backdrop / ✕ / Escape dismiss.
  const [expanded, setExpanded] = useState<ImageHit | null>(null);
  const reduceMotion = useReducedMotion();

  useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setExpanded(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [expanded]);

  return (
    <>
      <div className="[column-fill:_balance] gap-3 [column-count:2] sm:[column-count:3]">
        {images.map((image, idx) => (
          <button
            key={`${image.url}-${idx}`}
            type="button"
            onClick={() => setExpanded(image)}
            title={image.title ?? safeHostname(image.url)}
            aria-label={`Forstørr bilde — ${image.title ?? safeHostname(image.url)}`}
            className="verevon-glass-soft group mb-3 block w-full break-inside-avoid cursor-zoom-in overflow-hidden rounded-[18px] text-left transition hover:shadow-[0_14px_36px_rgba(76,60,92,0.16)]"
          >
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={image.thumbnailUrl}
              alt={image.title ?? ""}
              loading="lazy"
              decoding="async"
              referrerPolicy="no-referrer"
              className="block h-auto w-full object-cover transition group-hover:opacity-95"
            />
            <div className="flex items-center gap-1 px-2.5 py-1.5">
              <span className="truncate text-[10px] text-[#9A9188] dark:text-[#737780]">
                {safeHostname(image.url)}
              </span>
            </div>
          </button>
        ))}
      </div>

      <AnimatePresence>
        {expanded ? (
          <motion.div
            key="image-lightbox"
            className="fixed inset-0 z-[60] grid place-items-center bg-black/70 p-4 backdrop-blur-sm"
            initial={reduceMotion ? false : { opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            onClick={() => setExpanded(null)}
            role="dialog"
            aria-modal="true"
            aria-label={expanded.title ?? "Forstørret bilde"}
          >
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                setExpanded(null);
              }}
              aria-label="Lukk"
              className="absolute right-4 top-4 grid size-9 place-items-center rounded-full bg-white/15 text-white backdrop-blur transition hover:bg-white/25"
            >
              <X className="size-5" />
            </button>
            <motion.figure
              className="flex max-h-[88vh] max-w-[92vw] flex-col items-center gap-3"
              initial={reduceMotion ? false : { scale: 0.96, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.97, opacity: 0 }}
              transition={{ duration: 0.22, ease: EASE_OUT }}
              onClick={(e) => e.stopPropagation()}
            >
              {/* Clicking the expanded image opens the source page. */}
              <a
                href={expanded.url}
                target="_blank"
                rel="noopener noreferrer"
                title={`Åpne kilde — ${safeHostname(expanded.url)}`}
                className="block cursor-zoom-in overflow-hidden rounded-[18px]"
              >
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img
                  src={expanded.imageUrl}
                  alt={expanded.title ?? ""}
                  decoding="async"
                  referrerPolicy="no-referrer"
                  className="max-h-[80vh] w-auto max-w-full object-contain"
                />
              </a>
              <figcaption className="flex max-w-full items-center gap-1.5 text-[12px] text-white/80">
                <span className="truncate">{expanded.title ?? safeHostname(expanded.url)}</span>
                <ExternalLink className="size-3 shrink-0 opacity-70" />
                <span className="shrink-0 opacity-60">{safeHostname(expanded.url)}</span>
              </figcaption>
            </motion.figure>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </>
  );
}

// ---------------------------------------------------------------------------
// Video grid (Videos tab) — SearXNG video vertical
// ---------------------------------------------------------------------------

// Sanitized video hit from /api/v1/search/videos. `url` = source page (card
// click); `embedUrl` = inline-playable iframe (video click).
type VideoHit = {
  url: string;
  title: string | null;
  thumbnailUrl: string | null;
  embedUrl: string | null;
  author: string | null;
  length: string | null;
};

type VideosStatus = "idle" | "loading" | "loaded" | "error";
const searchVideoCache = new Map<
  string,
  { expiresAt: number; value: VideoHit[] }
>();

type VideosState = {
  error: string | null;
  status: VideosStatus;
  videos: VideoHit[];
};

type VideosAction =
  | { type: "loading" }
  | { type: "loaded"; videos: VideoHit[] }
  | { type: "error"; message: string };

function videosReducer(state: VideosState, action: VideosAction): VideosState {
  switch (action.type) {
    case "loading":
      return { ...state, error: null, status: "loading" };
    case "loaded":
      return { error: null, status: "loaded", videos: action.videos };
    case "error":
      return { ...state, error: action.message, status: "error" };
    default:
      return state;
  }
}

function VideoGridSkeleton() {
  return (
    <div
      aria-busy="true"
      aria-label="Laster videoer…"
      className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3"
    >
      {Array.from({ length: 6 }).map((_, i) => (
        <div key={i} className="verevon-glass-soft overflow-hidden rounded-[18px]">
          <div className="aspect-video w-full animate-pulse bg-white/45 dark:bg-white/[0.06]" />
          <div className="space-y-1.5 px-3 py-2">
            <div className="h-3 w-4/5 animate-pulse rounded bg-white/45 dark:bg-white/[0.06]" />
            <div className="h-2.5 w-2/5 animate-pulse rounded bg-white/45 dark:bg-white/[0.06]" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** A single video result. Per spec: clicking the *video* (thumbnail / play
 *  button) plays it inline via the embed iframe; clicking anywhere else on the
 *  *card* opens the source page in a new tab. */
function VideoCard({ video }: { video: VideoHit }) {
  const [playing, setPlaying] = useState(false);

  const openSource = () => {
    if (typeof window !== "undefined") {
      window.open(video.url, "_blank", "noopener,noreferrer");
    }
  };

  const embedSrc = video.embedUrl
    ? `${video.embedUrl}${video.embedUrl.includes("?") ? "&" : "?"}autoplay=1`
    : null;

  return (
    <div
      role="link"
      tabIndex={0}
      onClick={openSource}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          openSource();
        }
      }}
      title={video.title ?? safeHostname(video.url)}
      className="verevon-glass-soft group flex cursor-pointer flex-col overflow-hidden rounded-[18px] transition hover:shadow-[0_14px_36px_rgba(76,60,92,0.16)]"
    >
      <div className="relative aspect-video w-full overflow-hidden bg-black/[0.06] dark:bg-white/[0.04]">
        {playing && embedSrc ? (
          // The iframe captures its own clicks, so playback never bubbles to
          // the card's navigation handler.
          <iframe
            src={embedSrc}
            title={video.title ?? "Video"}
            allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
            allowFullScreen
            referrerPolicy="no-referrer"
            sandbox="allow-same-origin allow-scripts allow-presentation"
            className="absolute inset-0 h-full w-full"
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation(); // pressing the video must NOT navigate
              if (embedSrc) setPlaying(true);
              else openSource(); // no embeddable player → fall back to source
            }}
            aria-label={`Spill av${video.title ? ` — ${video.title}` : " video"}`}
            className="absolute inset-0 grid place-items-center"
          >
            {video.thumbnailUrl ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                src={video.thumbnailUrl}
                alt=""
                loading="lazy"
                decoding="async"
                referrerPolicy="no-referrer"
                className="absolute inset-0 h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
              />
            ) : (
              <div className="absolute inset-0 bg-gradient-to-br from-[#EE7A50]/12 to-[#A06CD5]/12" />
            )}
            <span className="relative grid size-12 place-items-center rounded-full bg-black/55 text-white shadow-lg backdrop-blur transition group-hover:scale-110 group-hover:bg-black/70">
              <Play className="size-5 translate-x-[1px]" fill="currentColor" />
            </span>
            {video.length ? (
              <span className="absolute bottom-2 right-2 rounded-md bg-black/70 px-1.5 py-0.5 text-[10px] font-medium tabular-nums text-white">
                {video.length}
              </span>
            ) : null}
          </button>
        )}
      </div>
      <div className="flex flex-col gap-0.5 px-3 py-2">
        <span className="line-clamp-2 text-[13px] font-medium leading-snug text-[#24262D] dark:text-white">
          {video.title ?? safeHostname(video.url)}
        </span>
        <span className="flex items-center gap-1 truncate text-[11px] text-[#9A9188] dark:text-[#737780]">
          {video.author ? <span className="truncate">{video.author}</span> : null}
          {video.author ? <span aria-hidden>·</span> : null}
          <span className="truncate">{safeHostname(video.url)}</span>
        </span>
      </div>
    </div>
  );
}

/** Self-contained Videos vertical: lazily fetches SearXNG video results when
 *  the tab is first viewed for a query. Independent of the reducer-backed Info
 *  / Bilder verticals. */
function VideosTab({ query }: { query: string }) {
  const [{ error, status, videos }, dispatchVideos] = useReducer(
    videosReducer,
    {
      error: null,
      status: "idle",
      videos: [],
    },
  );
  const loadedQueryRef = useRef("");

  useEffect(() => {
    const q = query.trim();
    if (!q || q === loadedQueryRef.current) return;
    const cachedVideos = readCachedValue(searchVideoCache, q);
    if (cachedVideos) {
      loadedQueryRef.current = q;
      dispatchVideos({ type: "loaded", videos: cachedVideos });
      return;
    }

    const abort = new AbortController();
    dispatchVideos({ type: "loading" });
    void (async () => {
      try {
        const res = await fetch("/api/v1/search/videos", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ query: q, limit: 24 }),
          signal: abort.signal,
        });
        const payload = (await res.json().catch(() => null)) as
          | { data?: { videos?: VideoHit[] }; error?: { message: string } }
          | null;
        if (!res.ok || !payload?.data) {
          dispatchVideos({
            type: "error",
            message: payload?.error?.message ?? "Videosøk kunne ikke fullføres.",
          });
          return;
        }
        const nextVideos = Array.isArray(payload.data.videos)
          ? payload.data.videos
          : [];
        writeCachedValue(
          searchVideoCache,
          q,
          nextVideos,
          SEARCH_VIDEO_CACHE_TTL_MS,
        );
        loadedQueryRef.current = q;
        dispatchVideos({ type: "loaded", videos: nextVideos });
      } catch (err: unknown) {
        if (err instanceof DOMException && err.name === "AbortError") return;
        dispatchVideos({
          type: "error",
          message: "Videosøk kunne ikke fullføres.",
        });
      }
    })();
    return () => abort.abort();
  }, [query]);

  if (status === "loading" || status === "idle") return <VideoGridSkeleton />;
  if (status === "error") {
    return (
      <div className="verevon-glass-soft rounded-3xl px-4 py-3 text-[13px] text-[#B04020] dark:text-[#E8A090]">
        {error ?? "Videosøk kunne ikke fullføres."}
      </div>
    );
  }
  if (videos.length === 0) {
    return (
      <div className="verevon-glass-soft rounded-3xl px-4 py-3 text-[13px] text-[#7A756F] dark:text-[#B6BAC4]">
        Ingen videoer funnet for{" "}
        <span className="font-medium text-[#1A1A1A] dark:text-white">{query}</span>.
      </div>
    );
  }
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
      {videos.map((v, i) => (
        <VideoCard key={`${v.url}-${i}`} video={v} />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Follow-up conversation thread
// ---------------------------------------------------------------------------

/** A single rendered turn (question bubble or streaming answer) in the thread. */
function ThreadTurnView({ turn }: { turn: ThreadTurn }) {
  const reduceMotion = useReducedMotion();
  const bubbleMotion = reduceMotion
    ? {}
    : {
        initial: { opacity: 0, y: 8 },
        animate: { opacity: 1, y: 0 },
        transition: { duration: 0.32, ease: EASE_OUT } as Transition,
      };

  if (turn.role === "user") {
    return (
      <motion.div className="flex justify-end" {...bubbleMotion}>
        <div className="max-w-[85%] rounded-[18px] rounded-br-[6px] bg-[#111111] px-4 py-2.5 text-[13px] leading-relaxed text-white dark:bg-white dark:text-[#111111]">
          <p className="whitespace-pre-wrap">{turn.text}</p>
        </div>
      </motion.div>
    );
  }

  const showCursor = turn.streaming;
  const isEmptyStreaming = turn.streaming && turn.text.length === 0;

  return (
    <motion.div className="flex justify-start" {...bubbleMotion}>
      <div className="verevon-glass-soft max-w-[92%] rounded-[18px] rounded-bl-[6px] px-4 py-3">
        <p className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#737780]">
          Verevon
        </p>
        {turn.error ? (
          <p
            role="alert"
            className="text-[13px] leading-relaxed text-[#B04020] dark:text-[#E8A090]"
          >
            {turn.error}
          </p>
        ) : isEmptyStreaming ? (
          <p
            className="flex items-center gap-1 text-[13px] text-[#9A9188] dark:text-[#737780]"
            aria-label="Verevon skriver…"
          >
            <span className="size-1.5 animate-bounce rounded-full bg-current [animation-delay:-0.2s]" />
            <span className="size-1.5 animate-bounce rounded-full bg-current [animation-delay:-0.1s]" />
            <span className="size-1.5 animate-bounce rounded-full bg-current" />
          </p>
        ) : (
          <p className="whitespace-pre-wrap text-[13px] leading-relaxed text-[#3A3530] dark:text-[#D4D6DC]">
            {turn.text}
            {showCursor ? (
              <span
                aria-hidden="true"
                className="ml-0.5 inline-block h-[1em] w-[2px] animate-pulse bg-current align-middle opacity-70"
              />
            ) : null}
          </p>
        )}
      </div>
    </motion.div>
  );
}

/** The full conversation thread below the initial search answer. */
function FollowUpThread({ turns }: { turns: ThreadTurn[] }) {
  if (turns.length === 0) return null;
  return (
    <section
      aria-label="Oppfølgingssamtale"
      aria-live="polite"
      className="flex flex-col gap-3 border-t border-black/[0.06] pt-4 dark:border-white/[0.06]"
    >
      {turns.map((turn) => (
        <ThreadTurnView key={turn.id} turn={turn} />
      ))}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

export function SearchAnswerView({ initialQuery }: { initialQuery: string }) {
  const router = useRouter();
  const abortRef = useRef<AbortController | null>(null);
  const followUpAbortRef = useRef<AbortController | null>(null);
  const imagesAbortRef = useRef<AbortController | null>(null);
  // Convex thread id for the current search, set once turn-0 persists. Lets
  // follow-up turns append onto the same thread (and the recent list link back).
  const persistedThreadIdRef = useRef<string | null>(null);
  // Anchor at the end of the thread, scrolled into view as turns/tokens arrive.
  const threadEndRef = useRef<HTMLDivElement | null>(null);

  const [activeTab, setActiveTab] = useState<Tab>("Info");
  // Optional "search the web again for this follow-up" toggle. Off by default:
  // the original search already grounded the conversation.
  const [browseFollowUp, setBrowseFollowUp] = useState(false);

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
    images,
    imagesStatus,
    imagesError,
    imagesQuery,
    thread,
    threadStreaming,
    answerStreaming,
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
    persistedThreadIdRef.current = null;

    void (async () => {
      try {
        const response = await fetch("/api/v1/search/web", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          // includeAnswer:false → the edge skips its (blocking) synthesis and
          // returns results fast; the answer is streamed client-side below.
          body: JSON.stringify({ query: q.trim(), includeAnswer: false }),
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
          const resultList = payload.data.results ?? [];
          // With include_answer=false the edge returns no citations, so derive
          // source chips from the top results (the answer itself streams below).
          const citationList =
            payload.data.citations && payload.data.citations.length > 0
              ? payload.data.citations
              : resultList.slice(0, 8).map((r) => ({ url: r.url, title: r.title ?? undefined }));
          dispatch({
            type: "results-loaded",
            results: resultList,
            answer: "",
            citations: citationList,
          });

          // Phase 3: stream the initial answer token-by-token, grounded on the
          // results — the same path the follow-up thread uses. Reuses the search
          // AbortController, so starting a new search cancels an in-flight stream.
          if (resultList.length > 0) {
            // Edge-side streaming: the high-quality answer (full-page grounded +
            // cached) streams from quarry-edge /v1/answer/stream via the BFF SSE
            // proxy. Reuses the search AbortController so a new search cancels an
            // in-flight stream. The `citations` SSE event is ignored — the UI
            // already shows source chips derived from resultList above.
            dispatch({ type: "answer-stream-started" });
            // Accumulate the answer locally (alongside the reducer) so we can
            // persist the final text once the stream completes.
            let answerText = "";
            try {
              const sres = await fetch("/api/v1/search/answer/stream", {
                method: "POST",
                credentials: "include",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ query: q.trim() }),
                signal: abort.signal,
              });
              if (sres.ok && sres.body) {
                const reader = sres.body.getReader();
                const decoder = new TextDecoder();
                let buf = "";
                let finished = false;
                while (!finished) {
                  const { value, done: readDone } = await reader.read();
                  if (readDone) break;
                  buf += decoder.decode(value, { stream: true });
                  // Parse complete SSE frames (blank-line-delimited).
                  let sep = buf.indexOf("\n\n");
                  while (sep !== -1) {
                    const frame = buf.slice(0, sep);
                    buf = buf.slice(sep + 2);
                    let event = "message";
                    const dataLines: string[] = [];
                    for (const line of frame.split("\n")) {
                      if (line.startsWith("event:")) event = line.slice(6).trim();
                      else if (line.startsWith("data:")) dataLines.push(line.slice(5).replace(/^ /, ""));
                    }
                    const dataStr = dataLines.join("\n");
                    if (event === "delta") {
                      try {
                        const parsed = JSON.parse(dataStr) as { delta?: string };
                        if (parsed.delta) {
                          answerText += parsed.delta;
                          dispatch({ type: "answer-delta", delta: parsed.delta });
                        }
                      } catch {
                        /* ignore a malformed frame */
                      }
                    } else if (event === "done" || event === "error") {
                      finished = true;
                      break;
                    }
                    sep = buf.indexOf("\n\n");
                  }
                }
              }
              dispatch({ type: "answer-stream-done" });
              // Persist the completed search to Convex (server-side, best-effort).
              // The BFF resolves org/user from the session and writes with the
              // service key; we keep the returned thread id for follow-up turns.
              if (answerText.trim()) {
                void fetch("/api/v1/search/persist", {
                  method: "POST",
                  credentials: "include",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    query: q.trim(),
                    answer: answerText,
                    citations: citationList,
                  }),
                })
                  .then((r) => (r.ok ? r.json() : null))
                  .then((j) => {
                    const tid = (j as { data?: { threadId?: unknown } } | null)?.data
                      ?.threadId;
                    if (typeof tid === "string") persistedThreadIdRef.current = tid;
                  })
                  .catch(() => {
                    /* best-effort: persistence must never break the search UX */
                  });
              }
            } catch (streamErr: unknown) {
              if (!(streamErr instanceof DOMException && streamErr.name === "AbortError")) {
                dispatch({ type: "answer-stream-done" });
              }
            }
          }
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
      imagesAbortRef.current?.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // -------------------------------------------------------------------------
  // Images (Bilder) fetch — lazy, independent of the web vertical
  // -------------------------------------------------------------------------

  const fetchImages = useCallback((q: string) => {
    const trimmed = q.trim();
    if (!trimmed) return;
    const cachedImages = readCachedValue(searchImageCache, trimmed);
    if (cachedImages) {
      dispatch({
        type: "images-loaded",
        query: trimmed,
        images: cachedImages,
      });
      return;
    }

    imagesAbortRef.current?.abort();
    const abort = new AbortController();
    imagesAbortRef.current = abort;

    dispatch({ type: "images-started", query: trimmed });

    void (async () => {
      try {
        const response = await fetch("/api/v1/search/images", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ query: trimmed, limit: 24 }),
          signal: abort.signal,
        });

        const payload = (await response.json().catch(() => null)) as
          | { data?: { images?: ImageHit[] }; error?: { code?: string; message: string } }
          | null;

        if (!response.ok || !payload || !payload.data) {
          dispatch({
            type: "images-error",
            query: trimmed,
            message: payload?.error?.message ?? "Bildesøk kunne ikke fullføres.",
          });
          return;
        }

        const nextImages = Array.isArray(payload.data.images)
          ? payload.data.images
          : [];
        writeCachedValue(
          searchImageCache,
          trimmed,
          nextImages,
          SEARCH_IMAGE_CACHE_TTL_MS,
        );
        dispatch({
          type: "images-loaded",
          query: trimmed,
          images: nextImages,
        });
      } catch (err: unknown) {
        if (err instanceof DOMException && err.name === "AbortError") return;
        dispatch({ type: "images-error", query: trimmed, message: "Bildesøk kunne ikke fullføres." });
      }
    })();
  }, []);

  // When the Bilder tab is active and we have a fresh query, load images once.
  // `imagesStatus === "idle"` is reset on every new search, so each query
  // triggers exactly one fetch the first time the tab is viewed.
  useEffect(() => {
    if (
      activeTab === "Bilder" &&
      submittedQuery.trim() &&
      imagesStatus === "idle" &&
      imagesQuery !== submittedQuery.trim()
    ) {
      fetchImages(submittedQuery);
    }
  }, [activeTab, submittedQuery, imagesStatus, imagesQuery, fetchImages]);

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

  // Send a follow-up: build a bounded, source-grounded context block from the
  // current answer + prior thread, append a user turn and a streaming
  // assistant turn, then stream tokens from the chat BFF into the assistant
  // turn. One AbortController per in-flight stream; a new submit cancels the
  // previous (the button is disabled while streaming, but this is belt-and-
  // suspenders for Enter-key races and unmount).
  const sendFollowUp = useCallback(
    (raw: string) => {
      const question = raw.trim();
      if (!question || state.threadStreaming) return;

      // Top sources: prefer citations, fall back to / augment with results.
      const sources: GroundingSource[] = dedupeSources([
        ...state.citations.map((c) => ({ url: c.url, title: c.title })),
        ...state.results.map((r) => ({ url: r.url, title: r.title })),
      ]);

      const content = buildGroundingContent({
        query: state.submittedQuery || state.query,
        answer: state.answer,
        sources,
        priorTurns: state.thread,
        question,
      });

      const baseId = `t-${Date.now()}`;
      const userTurn: ThreadTurn = { id: `${baseId}-u`, role: "user", text: question };
      const assistantId = `${baseId}-a`;
      const assistantTurn: ThreadTurn = {
        id: assistantId,
        role: "assistant",
        text: "",
        streaming: true,
        error: null,
      };

      setFollowUp("");
      dispatch({ type: "turn-appended", userTurn, assistantTurn });

      // Persist follow-up turns onto the Convex thread (best-effort). Needs the
      // thread id from turn 0; if it's absent (turn-0 write failed / not signed
      // in) we skip silently rather than break the chat.
      const persistTurn = (role: "user" | "assistant", text: string) => {
        const threadId = persistedThreadIdRef.current;
        if (!threadId || !text.trim()) return;
        void fetch("/api/v1/search/persist", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ threadId, role, text }),
        }).catch(() => {
          /* best-effort: persistence must never break the chat UX */
        });
      };
      persistTurn("user", question);

      followUpAbortRef.current?.abort();
      const abort = new AbortController();
      followUpAbortRef.current = abort;

      void (async () => {
        try {
          let assistantText = "";
          for await (const chunk of streamChat({
            content,
            browseWeb: browseFollowUp,
            signal: abort.signal,
          })) {
            if (chunk.type === "delta") {
              assistantText += chunk.delta;
              dispatch({ type: "turn-delta", id: assistantId, delta: chunk.delta });
            } else if (chunk.type === "error") {
              dispatch({ type: "turn-error", id: assistantId, message: chunk.message });
              return;
            } else if (chunk.type === "done") {
              dispatch({ type: "turn-done", id: assistantId });
            }
          }
          // Generator can complete without an explicit "done" event; ensure the
          // turn is marked finished so the typing indicator stops.
          dispatch({ type: "turn-done", id: assistantId });
          persistTurn("assistant", assistantText);
        } catch (err: unknown) {
          if (err instanceof DOMException && err.name === "AbortError") return;
          if (err instanceof Error && err.name === "AbortError") return;
          dispatch({
            type: "turn-error",
            id: assistantId,
            message: "Kunne ikke hente svar. Prøv igjen.",
          });
        }
      })();
    },
    [
      browseFollowUp,
      setFollowUp,
      state.answer,
      state.citations,
      state.query,
      state.results,
      state.submittedQuery,
      state.thread,
      state.threadStreaming,
    ],
  );

  const handleFollowUp = (e: React.FormEvent) => {
    e.preventDefault();
    sendFollowUp(followUp);
  };

  // Enter submits, Shift+Enter inserts a newline.
  const handleFollowUpKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendFollowUp(followUp);
    }
  };

  // Auto-scroll the thread to the latest content as turns append and tokens
  // stream in.
  useEffect(() => {
    if (thread.length === 0) return;
    threadEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [thread]);

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------

  return (
    <div className="relative flex h-full flex-col overflow-hidden">
      {/* Animated liquid-glass backdrop behind everything. */}
      <LiquidBackdrop variant="answer" />

      {/* ------------------------------------------------------------------ */}
      {/* Header / query bar */}
      {/* ------------------------------------------------------------------ */}
      <header className="relative z-[1] flex shrink-0 items-center gap-3 border-b border-white/40 bg-white/40 px-4 py-3 backdrop-blur-xl dark:border-white/[0.06] dark:bg-[#1C1E24]/55">
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
          <div className="verevon-glass-input flex h-10 min-w-0 flex-1 items-center rounded-full px-4">
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
      <div className="relative z-[1] shrink-0 bg-white/30 px-4 backdrop-blur-xl dark:bg-[#1C1E24]/45">
        <TabBar active={activeTab} onSelect={setActiveTab} />
      </div>

      {/* ------------------------------------------------------------------ */}
      {/* Scrollable results area */}
      {/* ------------------------------------------------------------------ */}
      <main
        className="relative z-[1] min-h-0 flex-1 overflow-y-auto px-4 py-5"
        aria-label="Søkeresultater"
      >
        <div className="mx-auto flex max-w-3xl flex-col gap-4">
          {/* Submitted query header */}
          {submittedQuery ? (
            <h1 className="text-[20px] font-[540] tracking-[-0.02em] text-[#1A1A1A] dark:text-[#F7F8F8]">
              {submittedQuery}
            </h1>
          ) : null}

          {/* ================================================================ */}
          {/* Tab content — animated cross-fade between verticals */}
          {/* ================================================================ */}
          <AnimatePresence mode="wait" initial={false}>
          {activeTab === "Info" ? (
            <motion.div
              key="tab-info"
              className="flex flex-col gap-4"
              variants={tabPanel}
              initial="hidden"
              animate="show"
              exit="exit"
            >
          {/* Loading skeleton */}
          {loading ? <LoadingSkeleton /> : null}

          {/* Error state */}
          {!loading && error ? (
            <div className="verevon-glass-soft rounded-3xl px-4 py-3 text-[13px] text-[#B04020] dark:text-[#E8A090]">
              {error}
            </div>
          ) : null}

          {/* ---- mode: "fetch" ---- */}
          {!loading && !error && mode === "fetch" && fetchedPage ? (
            <div className="verevon-glass rounded-3xl px-5 py-4">
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

          {/* ---- mode: "search" — AI summary card (streams in token-by-token) ---- */}
          {!loading && !error && mode === "search" && (answer || answerStreaming) ? (
            <motion.section
              aria-labelledby="ai-summary-heading"
              className="verevon-glass rounded-3xl px-5 py-4"
              variants={listItem}
              initial="hidden"
              animate="show"
            >
              <h2
                id="ai-summary-heading"
                className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#EE7A50] dark:text-[#F6AF6E]"
              >
                AI-sammendrag
              </h2>
              <p className="text-[13.5px] leading-relaxed text-[#3A3530] dark:text-[#D4D6DC]">
                {answer}
                {answerStreaming ? (
                  <span
                    aria-hidden
                    className="ml-0.5 inline-block h-[1.05em] w-[2px] translate-y-[2px] animate-pulse bg-[#EE7A50] align-text-bottom dark:bg-[#F6AF6E]"
                  />
                ) : null}
              </p>
              {citations.length > 0 ? (
                <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-black/[0.06] pt-3 dark:border-white/[0.08]">
                  <span className="text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#9A9EA8]">
                    Kilder
                  </span>
                  {citations.map((citation) => (
                    <a
                      key={citation.url}
                      href={citation.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="verevon-glass-soft inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium text-[#504A43] transition hover:shadow-[0_8px_20px_rgba(76,60,92,0.14)] dark:text-[#D4D6DC]"
                    >
                      {citation.title ?? safeHostname(citation.url)}
                      <ExternalLink className="size-2.5 shrink-0 opacity-60" />
                    </a>
                  ))}
                </div>
              ) : null}
            </motion.section>
          ) : null}

          {/* ---- mode: "search" — web results list ---- */}
          {!loading && !error && mode === "search" && results.length > 0 ? (
            <section aria-labelledby="web-results-heading">
              <h2
                id="web-results-heading"
                className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[#9A9188] dark:text-[#9A9EA8]"
              >
                Webresultater
              </h2>
              <motion.ul
                className="flex flex-col gap-2"
                variants={listContainer}
                initial="hidden"
                animate="show"
              >
                {results.map((result) => (
                  <motion.li key={result.url} variants={listItem}>
                    <a
                      href={result.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="verevon-glass-soft group block rounded-3xl px-4 py-3 transition hover:shadow-[0_14px_34px_rgba(76,60,92,0.14)]"
                    >
                      {result.title ? (
                        <p className="text-[13px] font-semibold text-[#1A1A1A] group-hover:text-[#EE7A50] dark:text-[#F7F8F8]">
                          {result.title}
                        </p>
                      ) : null}
                      <p className="mt-0.5 truncate text-[11px] text-[#9A9188] dark:text-[#9A9EA8]">
                        {safeHostname(result.url)}
                      </p>
                      {result.snippet ? (
                        <p className="mt-1 text-[12px] leading-relaxed text-[#5F5A54] dark:text-[#B6BAC4]">
                          {result.snippet}
                        </p>
                      ) : null}
                    </a>
                  </motion.li>
                ))}
              </motion.ul>
            </section>
          ) : null}

          {/* Empty state. A 200 with zero results, no answer and no citations
              can mean either a genuinely empty search OR a search provider that
              returned nothing because it is down/unconfigured — the route can't
              always tell the two apart. So we keep the headline calm and add a
              muted hint pointing at the provider, rather than a scary error. */}
          {!loading && !error && mode === "search" && results.length === 0 && !answer && citations.length === 0 ? (
            <div
              role="status"
              className="verevon-glass-soft rounded-3xl px-4 py-3.5 text-[13px] text-[#7A756F] dark:text-[#B6BAC4]"
            >
              <p className="font-medium text-[#1A1A1A] dark:text-white">Ingen webresultater</p>
              <p className="mt-1 text-[12px] leading-relaxed text-[#9A9188] dark:text-[#9A9EA8]">
                Ingen treff for{" "}
                <span className="font-medium text-[#5F5A54] dark:text-[#D4D6DC]">«{submittedQuery}»</span>{" "}
                — sjekk at søkeleverandøren (SearXNG) kjører hvis dette er uventet.
              </p>
            </div>
          ) : null}

          {/* Recent searches — live from Convex (convex-core searchThreads).
              Shown on the landing / before results arrive; self-hides when the
              user has no history. Clicking one re-runs that query. */}
          {!loading && !answer && results.length === 0 && mode === "search" ? (
            <RecentSearches onPick={runSearch} />
          ) : null}

          {/* ---- Conversational follow-up thread ---- */}
          <FollowUpThread turns={thread} />
            </motion.div>
          ) : null}

          {/* ================================================================ */}
          {/* Bilder tab — SearXNG image vertical */}
          {/* ================================================================ */}
          {activeTab === "Bilder" ? (
            <motion.section
              key="tab-bilder"
              aria-label="Bilderesultater"
              variants={tabPanel}
              initial="hidden"
              animate="show"
              exit="exit"
            >
              {/* Loading skeleton */}
              {imagesStatus === "loading" || imagesStatus === "idle" ? (
                <ImageGridSkeleton />
              ) : null}

              {/* Error state */}
              {imagesStatus === "error" ? (
                <div className="verevon-glass-soft rounded-3xl px-4 py-3 text-[13px] text-[#B04020] dark:text-[#E8A090]">
                  {imagesError ?? "Bildesøk kunne ikke fullføres."}
                </div>
              ) : null}

              {/* Results */}
              {imagesStatus === "loaded" && images.length > 0 ? (
                <ImageGallery images={images} />
              ) : null}

              {/* Empty state */}
              {imagesStatus === "loaded" && images.length === 0 ? (
                <div className="verevon-glass-soft rounded-3xl px-4 py-3 text-[13px] text-[#7A756F] dark:text-[#B6BAC4]">
                  Ingen bilder funnet for{" "}
                  <span className="font-medium text-[#1A1A1A] dark:text-white">{submittedQuery}</span>.
                </div>
              ) : null}
            </motion.section>
          ) : null}

          {/* ================================================================ */}
          {/* Videos tab — SearXNG video vertical (inline-play cards) */}
          {/* ================================================================ */}
          {activeTab === "Videos" ? (
            <motion.section
              key="tab-videos"
              aria-label="Videoresultater"
              variants={tabPanel}
              initial="hidden"
              animate="show"
              exit="exit"
            >
              <VideosTab query={submittedQuery} />
            </motion.section>
          ) : null}
          </AnimatePresence>

          {/* Scroll anchor: keeps the latest thread turn in view. */}
          <div ref={threadEndRef} aria-hidden="true" />
        </div>
      </main>

      {/* ------------------------------------------------------------------ */}
      {/* Pinned follow-up composer */}
      {/* ------------------------------------------------------------------ */}
      <div className="relative z-[1] shrink-0 border-t border-white/40 bg-white/40 px-4 py-3 backdrop-blur-xl dark:border-white/[0.06] dark:bg-[#1C1E24]/55">
        <form onSubmit={handleFollowUp} className="mx-auto flex max-w-3xl items-end gap-2">
          <label className="sr-only" htmlFor="search-follow-up">
            Stille oppfølgingsspørsmål
          </label>
          <div className="verevon-glass-input flex min-w-0 flex-1 items-end rounded-[22px] px-4 py-2">
            <textarea
              id="search-follow-up"
              rows={1}
              value={followUp}
              onChange={(e) => setFollowUp(e.target.value)}
              onKeyDown={handleFollowUpKeyDown}
              className="max-h-32 min-h-[24px] min-w-0 flex-1 resize-none bg-transparent py-0.5 text-[13px] leading-relaxed text-[#24262D] placeholder:text-[#AAA198] focus:outline-none dark:text-white dark:placeholder:text-[#737780]"
              placeholder="Spør mer om dette…"
              autoComplete="off"
              aria-describedby="search-follow-up-hint"
            />
            {/* Optional: re-ground this follow-up with a fresh web search. */}
            <button
              type="button"
              role="switch"
              aria-checked={browseFollowUp}
              aria-label="Søk på nettet for dette oppfølgingsspørsmålet"
              title={browseFollowUp ? "Nettsøk på (klikk for å slå av)" : "Slå på nettsøk for oppfølging"}
              onClick={() => setBrowseFollowUp((v) => !v)}
              className={[
                "ml-2 grid size-7 shrink-0 place-items-center rounded-full transition",
                browseFollowUp
                  ? "bg-[#EE7A50]/15 text-[#EE7A50]"
                  : "text-[#9A9188] hover:bg-black/[0.05] dark:text-[#737780] dark:hover:bg-white/[0.08]",
              ].join(" ")}
            >
              <Globe className="size-4" />
            </button>
          </div>
          <span id="search-follow-up-hint" className="sr-only">
            Trykk Enter for å sende, Shift+Enter for ny linje.
          </span>
          <button
            type="submit"
            aria-label="Send oppfølgingsspørsmål"
            disabled={!followUp.trim() || threadStreaming}
            className="grid size-11 shrink-0 place-items-center rounded-full bg-[#111111] text-white transition hover:bg-[#2A2A2A] disabled:bg-[#E9E4DD] disabled:text-[#A99E93] dark:bg-white dark:text-[#111111] dark:disabled:bg-[#35373D] dark:disabled:text-[#5A5D65]"
          >
            <SendHorizontal className="size-4" />
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
