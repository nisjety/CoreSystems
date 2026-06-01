"use client";

import Image from "next/image";
import Link from "next/link";
import type { Route } from "next";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { streamChat } from "@/features/chat-v2/lib/chat-stream";
import {
  ArrowRight,
  CirclePlus,
  Search,
  Sparkles,
} from "lucide-react";
import { dashboardCards, type DashboardCard } from "@/features/dashboard-v2/lib/dashboard-surface";
import { DashboardComposer } from "@/features/composer-v2/components/DashboardComposer";
import { formatComposerTurnTime, loadComposerSettings } from "@/features/composer-v2/lib/dashboard-composer-storage";
import {
  type ComposerFile,
  type ComposerSettings,
  type ComposerTurn,
  type DashboardComposerModel,
  type ResponseMode,
} from "@/features/dashboard-v2/lib/dashboard-composer-model";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import { useControlPlaneContext } from "@/features/shell-v2/lib/control-plane-provider";
import { formatPlanLabel } from "@/features/shell-v2/lib/shell-data";
import { apiGet } from "@/lib/api/client-envelope";
import { cn } from "@/lib/utils";

type DashboardTab = "Chat" | "Søk" | "Kunnskap";

type SearchSuggestion = {
  text: string;
  source: string;
  collection: string;
  object: string;
};

type VelionHomeState = {
  activeTab: DashboardTab;
  browseWeb: boolean;
  cardPage: number;
  deepSearch: boolean;
  files: ComposerFile[];
  historyOpen: boolean;
  /** Accumulating assistant response text during streaming. */
  streamingResponse: string;
  /** True while an SSE stream is in-flight. */
  isStreaming: boolean;
  message: string;
  modelOpen: boolean;
  responseMode: ResponseMode;
  selectedModel: DashboardComposerModel;
  settings: ComposerSettings;
  settingsOpen: boolean;
  suggestionsOpen: boolean;
  turns: ComposerTurn[];
  voiceMode: boolean;
};

type VelionHomeAction =
  | { type: "active-tab-changed"; tab: DashboardTab }
  | { type: "browse-web-changed"; active: boolean }
  | { type: "card-page-next"; pageCount: number }
  | { type: "deep-search-changed"; active: boolean }
  | { type: "files-changed"; files: ComposerFile[] }
  | { type: "history-open-changed"; open: boolean }
  | { type: "message-changed"; message: string }
  | { type: "message-submitted"; turn: ComposerTurn }
  | { type: "model-changed"; model: DashboardComposerModel }
  | { type: "model-open-changed"; open: boolean }
  | { type: "open-agent-builder" }
  | { type: "response-mode-changed"; mode: ResponseMode }
  | { type: "settings-changed"; settings: ComposerSettings }
  | { type: "settings-open-changed"; open: boolean }
  | { type: "suggestions-open-changed"; open: boolean }
  | { type: "voice-mode-changed"; active: boolean }
  | { type: "card-prompt-applied"; card: DashboardCard }
  | { type: "stream-started" }
  | { type: "stream-delta"; delta: string }
  | { type: "stream-done" }
  | { type: "stream-error" };

function createInitialVelionHomeState(): VelionHomeState {
  return {
    activeTab: "Chat",
    browseWeb: true,
    cardPage: 0,
    deepSearch: false,
    files: [],
    historyOpen: false,
    streamingResponse: "",
    isStreaming: false,
    message: "",
    modelOpen: false,
    responseMode: "auto",
    selectedModel: "GPT-4o Mini",
    settings: loadComposerSettings(),
    settingsOpen: false,
    suggestionsOpen: false,
    turns: [],
    voiceMode: false,
  };
}

function velionHomeReducer(state: VelionHomeState, action: VelionHomeAction): VelionHomeState {
  switch (action.type) {
    case "active-tab-changed":
      return { ...state, activeTab: action.tab };
    case "browse-web-changed":
      return { ...state, browseWeb: action.active };
    case "card-page-next":
      return { ...state, cardPage: (state.cardPage + 1) % action.pageCount };
    case "deep-search-changed":
      return { ...state, deepSearch: action.active };
    case "files-changed":
      return { ...state, files: action.files };
    case "history-open-changed":
      return { ...state, historyOpen: action.open };
    case "message-changed":
      return { ...state, message: action.message };
    case "message-submitted":
      return {
        ...state,
        files: [],
        message: "",
        suggestionsOpen: false,
        turns: [action.turn, ...state.turns].slice(0, 6),
      };
    case "model-changed":
      return { ...state, selectedModel: action.model };
    case "model-open-changed":
      return { ...state, modelOpen: action.open };
    case "open-agent-builder":
      return {
        ...state,
        message: "Opprett en agent som håndterer kundesamtaler med kunnskapsbase, tone og eskaleringer.",
        suggestionsOpen: true,
      };
    case "response-mode-changed":
      return { ...state, responseMode: action.mode };
    case "settings-changed":
      return { ...state, settings: action.settings };
    case "settings-open-changed":
      return { ...state, settingsOpen: action.open };
    case "suggestions-open-changed":
      return { ...state, suggestionsOpen: action.open };
    case "voice-mode-changed":
      return { ...state, voiceMode: action.active };
    case "card-prompt-applied":
      return {
        ...state,
        activeTab: "Chat",
        browseWeb: action.card.id === "search" || action.card.id === "knowledge" ? true : state.browseWeb,
        deepSearch: action.card.id === "knowledge" ? true : state.deepSearch,
        message: action.card.prompt,
      };
    case "stream-started":
      return { ...state, isStreaming: true, streamingResponse: "" };
    case "stream-delta":
      return { ...state, streamingResponse: state.streamingResponse + action.delta };
    case "stream-done":
      return { ...state, isStreaming: false };
    case "stream-error":
      return { ...state, isStreaming: false };
    default:
      return state;
  }
}

type SearchPanelState = {
  query: string;
  submittedQuery: string;
  suggestions: SearchSuggestion[];
  suggestionsQuery: string;
};

type SearchPanelAction =
  | { type: "query-changed"; query: string }
  | { type: "submitted"; query: string }
  | { type: "suggestions-cleared" }
  | { type: "suggestions-loaded"; query: string; suggestions: SearchSuggestion[] };

const initialSearchPanelState: SearchPanelState = {
  query: "",
  submittedQuery: "",
  suggestions: [],
  suggestionsQuery: "",
};

function searchPanelReducer(state: SearchPanelState, action: SearchPanelAction): SearchPanelState {
  switch (action.type) {
    case "query-changed":
      return { ...state, query: action.query };
    case "submitted":
      return { ...state, submittedQuery: action.query };
    case "suggestions-cleared":
      return { ...state, suggestions: [], suggestionsQuery: "" };
    case "suggestions-loaded":
      return { ...state, suggestions: action.suggestions, suggestionsQuery: action.query };
    default:
      return state;
  }
}


const tabs: DashboardTab[] = ["Chat", "Søk", "Kunnskap"];
const aboveFoldDashboardCardIds = new Set(["search", "chat", "knowledge"]);

function getNorwegianGreeting() {
  const hour = new Date().getHours();

  if (hour < 11) {
    return "God morgen";
  }

  if (hour < 17) {
    return "God ettermiddag";
  }

  return "God kveld";
}

function firstName(value?: string | null) {
  const trimmed = value?.trim();
  if (!trimmed) return "";
  if (trimmed.includes("@")) return trimmed.split("@")[0] || "";
  return trimmed.split(/\s+/)[0] || "";
}

/** Streaming assistant response bubble shown above the composer while tokens arrive. */
function StreamingResponseBubble({
  text,
  isStreaming,
}: {
  text: string;
  isStreaming: boolean;
}) {
  return (
    <div className="velion-fade-up mb-3 rounded-[18px] border border-black/[0.05] bg-white/90 p-4 text-[14px] text-[#1A1A1A] shadow-[0_10px_28px_rgba(0,0,0,0.04)] backdrop-blur-sm dark:border-[#2A2C31] dark:bg-[#141516]/90 dark:text-[#F0F1F3]">
      <p className="whitespace-pre-wrap leading-relaxed">
        {text}
        {isStreaming ? (
          <span
            aria-hidden="true"
            className="ml-0.5 inline-block h-[1em] w-[2px] animate-pulse bg-current align-middle opacity-70"
          />
        ) : null}
      </p>
    </div>
  );
}

export function VelionHome() {
  const composerRef = useRef<HTMLDivElement>(null);
  const controlPlane = useControlPlaneContext();
  const greeting = getNorwegianGreeting();
  const [state, dispatch] = useReducer(velionHomeReducer, undefined, createInitialVelionHomeState);
  const {
    activeTab,
    browseWeb,
    cardPage,
    deepSearch,
    files,
    historyOpen,
    isStreaming,
    streamingResponse,
    message,
    modelOpen,
    responseMode,
    selectedModel,
    settings,
    settingsOpen,
    suggestionsOpen,
    turns,
    voiceMode,
  } = state;

  // Ref so the async stream loop can read the latest abort controller
  const streamAbortRef = useRef<AbortController | null>(null);

  const pageCount = Math.ceil(dashboardCards.length / 3);
  const visibleCards = dashboardCards.slice(cardPage * 3, cardPage * 3 + 3);
  const displayName = firstName(controlPlane.user?.name ?? controlPlane.user?.email);
  const planLabel = formatPlanLabel(controlPlane.entitlements?.plan ?? controlPlane.organization?.plan);

  const submitMessage = useCallback(() => {
    const body = message.trim();
    if (!body && files.length === 0) return;
    if (isStreaming) return; // prevent double-submit while streaming

    const now = new Date();
    const nextTurn: ComposerTurn = {
      id: `turn-${now.getTime()}`,
      body: body || "Vedlegg sendt til Velion.",
      model: selectedModel,
      responseMode,
      browseWeb,
      deepSearch,
      files: files.map((file) => file.name),
      createdAt: formatComposerTurnTime(now),
      createdAtIso: now.toISOString(),
    };

    dispatch({ type: "message-submitted", turn: nextTurn });

    // Abort any in-flight stream before starting a new one
    streamAbortRef.current?.abort();
    const abortController = new AbortController();
    streamAbortRef.current = abortController;

    dispatch({ type: "stream-started" });

    void (async () => {
      try {
        for await (const chunk of streamChat({
          content: body,
          model: selectedModel,
          browseWeb,
          signal: abortController.signal,
        })) {
          if (chunk.type === "delta") {
            dispatch({ type: "stream-delta", delta: chunk.delta });
          } else if (chunk.type === "done") {
            dispatch({ type: "stream-done" });
          }
        }
      } catch (err: unknown) {
        if (err instanceof Error && err.name === "AbortError") return;
        dispatch({ type: "stream-error" });
      }
    })();
  }, [browseWeb, deepSearch, files, isStreaming, message, responseMode, selectedModel]);

  const applyCardPrompt = (card: DashboardCard) => {
    dispatch({ type: "card-prompt-applied", card });
    window.requestAnimationFrame(() => {
      composerRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    });
  };

  const openAgentBuilder = () => {
    dispatch({ type: "open-agent-builder" });
  };

  return (
    <div className="velion-dashboard-surface relative h-full overflow-hidden bg-transparent text-[#1A1A1A] transition-colors dark:text-[#F7F8F8]">
      <div className="pointer-events-none absolute inset-0 dashboard-home-grid" aria-hidden="true" />

      <div className="velion-home-stage relative flex h-full min-h-0 flex-col overflow-hidden">
        <DashboardTabs activeTab={activeTab} onTabChange={(tab) => dispatch({ type: "active-tab-changed", tab })} />

        <section className="velion-home-header velion-fade-up shrink-0 px-4">
          <div className="mx-auto flex w-full max-w-5xl flex-col items-center">
            <div className="velion-home-plan w-full max-w-[720px]">
              <PlanBadge planLabel={planLabel} />
            </div>

            <h1
              suppressHydrationWarning
              className="velion-home-title w-full max-w-[720px] font-[450] leading-none tracking-tight text-[#1A1A1A] transition-colors dark:text-[#F7F8F8]"
            >
              {displayName ? `${greeting}, ${displayName}` : greeting}
            </h1>
          </div>
        </section>

        <section className="velion-home-composer-section shrink-0 px-4 pb-0">
          <div className="relative mx-auto w-full px-0 lg:w-[60%]">
            <div ref={composerRef} className="velion-home-composer velion-fade-up velion-stagger-1 mx-auto w-full max-w-[720px]">
              {activeTab === "Chat" && (isStreaming || streamingResponse) ? (
                <StreamingResponseBubble
                  text={streamingResponse}
                  isStreaming={isStreaming}
                />
              ) : null}
              {activeTab === "Chat" ? (
                <DashboardComposer
                  browseWeb={browseWeb}
                  deepSearch={deepSearch}
                  files={files}
                  historyOpen={historyOpen}
                  message={message}
                  modelOpen={modelOpen}
                  responseMode={responseMode}
                  selectedModel={selectedModel}
                  settings={settings}
                  settingsOpen={settingsOpen}
                  suggestionsOpen={suggestionsOpen}
                  turns={turns}
                  voiceMode={voiceMode}
                  onBrowseWebChange={(active) => dispatch({ type: "browse-web-changed", active })}
                  onDeepSearchChange={(active) => dispatch({ type: "deep-search-changed", active })}
                  onFilesChange={(nextFiles) => dispatch({ type: "files-changed", files: nextFiles })}
                  onHistoryOpenChange={(open) => dispatch({ type: "history-open-changed", open })}
                  onMessageChange={(nextMessage) => dispatch({ type: "message-changed", message: nextMessage })}
                  onModelChange={(model) => dispatch({ type: "model-changed", model })}
                  onModelOpenChange={(open) => dispatch({ type: "model-open-changed", open })}
                  onOpenAgentBuilder={openAgentBuilder}
                  onResponseModeChange={(mode) => dispatch({ type: "response-mode-changed", mode })}
                  onSettingsChange={(nextSettings) => dispatch({ type: "settings-changed", settings: nextSettings })}
                  onSettingsOpenChange={(open) => dispatch({ type: "settings-open-changed", open })}
                  onSubmit={submitMessage}
                  onSuggestionsOpenChange={(open) => dispatch({ type: "suggestions-open-changed", open })}
                  onVoiceModeChange={(active) => dispatch({ type: "voice-mode-changed", active })}
                />
              ) : activeTab === "Søk" ? (
                <SearchPanel />
              ) : (
                <KnowledgePanel />
              )}
            </div>
          </div>
        </section>

        <section className="velion-home-cards mx-auto min-h-0 w-full max-w-5xl shrink-0 px-4 pt-0">
          <div key={cardPage} className="velion-home-card-grid velion-card-page grid grid-cols-1 gap-4 lg:grid-cols-3">
            {visibleCards.map((card) => (
              <DashboardImageCard key={card.id} card={card} onPrompt={applyCardPrompt} />
            ))}
          </div>

          <button
            type="button"
            onClick={() => dispatch({ type: "card-page-next", pageCount })}
            className="velion-home-next mx-auto flex size-12 items-center justify-center transition-transform duration-300 hover:scale-105 active:scale-95"
            aria-label="Vis neste kortside"
            title="Vis neste kortside"
          >
            <span className="relative block h-5 w-8" aria-hidden="true">
              <span className="absolute -left-1 top-1/2 h-[3px] w-[25px] -translate-y-1/2 rotate-45 rounded-full bg-[#171D18]" />
              <span className="absolute -right-1 top-2/5 h-[3px] w-[22px] -translate-y-1/2 -rotate-45 rounded-full bg-[#171D18]" />
            </span>
          </button>
        </section>
      </div>
    </div>
  );
}

function DashboardTabs({
  activeTab,
  onTabChange,
}: {
  activeTab: DashboardTab;
  onTabChange: (tab: DashboardTab) => void;
}) {
  return (
    <div className="velion-home-tabs sticky top-0 z-10 px-4">
      <div className="mx-auto flex w-full max-w-5xl justify-center">
        <div className="inline-flex items-center gap-0.5 rounded-full bg-black/6 p-1 dark:bg-white/10">
          {tabs.map((tab) => (
            <button
              key={tab}
              type="button"
              onClick={() => onTabChange(tab)}
              title={`Vis ${tab.toLowerCase()}`}
              className={cn(
                "rounded-full px-5 py-1.5 text-[13px] font-medium transition-all duration-150",
                activeTab === tab
                  ? "bg-white text-[#1A1A1A] shadow-sm dark:bg-[#23252A] dark:text-white"
                  : "text-[#6B6560] hover:text-[#1A1A1A] dark:text-[#AEB4C0] dark:hover:text-white",
              )}
              aria-pressed={activeTab === tab}
            >
              {tab}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function PlanBadge({ planLabel }: { planLabel: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full border border-[#E5E0D8] bg-white px-3.5 py-1 text-[12.5px] font-medium text-[#6B6560] transition-colors dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-[#AEB4C0]">
      {planLabel} Plan
      <span className="text-[#D4C9BF]">·</span>
      <button type="button" className="font-semibold text-[#E8853D] hover:underline" title="Oppgrader plan">
        Upgrade
      </button>
    </span>
  );
}


function SearchPanel() {
  const [searchState, dispatchSearch] = useReducer(searchPanelReducer, initialSearchPanelState);
  const { query, submittedQuery, suggestions, suggestionsQuery } = searchState;
  const activeQuery = query.trim();
  const isTyping = activeQuery.length > 0 && submittedQuery !== activeQuery;
  const visibleSuggestions = isTyping && suggestionsQuery === activeQuery ? suggestions : [];

  useEffect(() => {
    if (!isTyping || activeQuery.length < 2) {
      return;
    }

    const controller = new AbortController();
    const timeout = window.setTimeout(() => {
      apiGet<{ suggestions?: SearchSuggestion[] }>(
        `/api/v1/search/suggestions?q=${encodeURIComponent(activeQuery)}&scope=queries&limit=6`,
        { credentials: "include", signal: controller.signal },
      )
        .then((payload) => {
          if (!payload.suggestions) {
            dispatchSearch({ type: "suggestions-cleared" });
            return;
          }

          dispatchSearch({
            type: "suggestions-loaded",
            query: activeQuery,
            suggestions: payload.suggestions.filter(
              (suggestion) => typeof suggestion.text === "string" && suggestion.text.trim().length > 0,
            ),
          });
        })
        .catch((error: unknown) => {
          if (error instanceof DOMException && error.name === "AbortError") {
            return;
          }
          dispatchSearch({ type: "suggestions-cleared" });
        });
    }, 120);

    return () => {
      controller.abort();
      window.clearTimeout(timeout);
    };
  }, [activeQuery, isTyping]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const trimmed = query.trim();

    if (!trimmed) {
      return;
    }

    dispatchSearch({ type: "submitted", query: trimmed });
  };

  return (
    <div className="velion-panel-in">
      <h2 className="mb-4 text-center text-[20px] font-[520] tracking-[-0.03em] text-[#273038] dark:text-[#F7F8F8]">
        What <span className="text-[#EE7A50]">do you want</span> to know?
      </h2>

      <form onSubmit={submit} className="relative w-full">
        <label className="sr-only" htmlFor="dashboard-search">
          Søk i selskapets kunnskap
        </label>
        <div className="flex items-center gap-2">
          <TopLayerTooltip label="Add context" placement="top">
            <button
              type="button"
              aria-label="Add search context"
              className="grid size-11 shrink-0 place-items-center rounded-full bg-white/78 text-[#34363D] shadow-[0_12px_28px_rgba(72,55,41,0.08)] transition hover:bg-white dark:bg-[#1D1E22] dark:text-white"
            >
              <CirclePlus className="size-4" />
            </button>
          </TopLayerTooltip>
          <div className="flex h-12 min-w-0 flex-1 items-center rounded-full bg-white/88 px-4 shadow-[0_16px_42px_rgba(72,55,41,0.10)] ring-1 ring-[#EFE7DC] backdrop-blur-xl dark:bg-[#1A1B20]/92 dark:ring-[#2A2C31]">
            <Search className="mr-2 size-4 shrink-0 text-[#9A9188]" />
            <input
              id="dashboard-search"
              aria-label="Søk i selskapets kunnskap"
              value={query}
              onChange={(event) => {
                dispatchSearch({ type: "query-changed", query: event.target.value });
              }}
              className="h-full min-w-0 flex-1 bg-transparent text-[14px] font-medium text-[#24262D] placeholder:text-[#AAA198] focus:outline-none dark:text-white dark:placeholder:text-[#737780]"
              placeholder="Ask anything…"
            />
            <button
              type="submit"
              aria-label="Søk"
              disabled={!query.trim()}
              className="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white transition hover:bg-[#2A2A2A] disabled:bg-[#E9E4DD] disabled:text-[#A99E93] dark:bg-white dark:text-[#111111] dark:disabled:bg-[#2A2C31] dark:disabled:text-[#737780]"
              title="Søk"
            >
              <ArrowRight className="size-4" />
            </button>
          </div>
        </div>

        {visibleSuggestions.length > 0 ? (
          <div className="velion-fade-up ml-[52px] mt-2 overflow-hidden rounded-[22px] bg-white/96 p-2 shadow-[0_24px_58px_rgba(72,55,41,0.16)] ring-1 ring-[#EFE7DC] backdrop-blur-xl dark:bg-[#1A1B20]/96 dark:ring-[#2A2C31]">
            <p className="px-3 pb-1.5 pt-1 text-[12px] font-semibold text-[#504A43] dark:text-[#D4D6DC]">Find me</p>
            {visibleSuggestions.map((suggestion) => (
              <button
                key={`${suggestion.collection}:${suggestion.object}`}
                type="button"
                onClick={() => {
                  dispatchSearch({ type: "query-changed", query: suggestion.text });
                }}
                className="flex w-full items-center gap-3 rounded-[12px] px-3 py-2.5 text-left text-[13px] font-medium text-[#2E3137] transition hover:bg-[#F4F1EB] dark:text-white dark:hover:bg-white/10"
              >
                <Search className="size-4 shrink-0 text-[#7E776F]" />
                <span>{suggestion.text}</span>
              </button>
            ))}
          </div>
        ) : null}

        {submittedQuery && !isTyping ? (
          <div className="velion-fade-up mt-4 rounded-[16px] bg-[#F4F1EB] p-3 text-[13px] text-[#5F5A54] dark:bg-[#1D1A17] dark:text-[#AEB4C0]">
            Søket er klart: <span className="font-medium text-[#1A1A1A] dark:text-white">{submittedQuery}</span>. Koble en kilde i kunnskapsbasen for å hente live resultater.
          </div>
        ) : null}
      </form>
    </div>
  );
}

function KnowledgePanel() {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="velion-panel-in velion-dashboard-composer-card rounded-[28px] bg-white p-5 shadow-[0_20px_60px_rgba(20,21,24,0.08)] ring-1 ring-black/[0.03] dark:bg-[#141516] dark:ring-white/[0.06]">
      <div className="flex items-center gap-3">
        <span className="grid size-10 place-items-center rounded-[12px] bg-[#F4F5F1] text-[#6B6560]">
          <Sparkles className="size-4" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[14px] font-semibold text-[#1A1A1A]">Kunnskapsbase</p>
          <p className="text-[13px] text-[#7A756F]">Se status for indeksert innhold og koblede kilder.</p>
        </div>
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          title={expanded ? "Skjul kunnskapsbase" : "Vis kunnskapsbase"}
          className="h-10 rounded-[12px] border border-black/[0.08] px-4 text-[13px] font-medium text-[#333] transition-colors hover:bg-black/[0.04]"
        >
          {expanded ? "Skjul" : "Vis"}
        </button>
      </div>
      {expanded ? (
        <div className="velion-fade-up mt-4 grid gap-2 sm:grid-cols-3">
          <Link href={"/knowledge" as Route} className="rounded-[14px] bg-[#F7F7F8] p-3 text-[12px] font-medium text-[#333] transition-colors hover:bg-[#EFEFF1]">
            Koble kilde
          </Link>
          <Link href={"/knowledge" as Route} className="rounded-[14px] bg-[#F7F7F8] p-3 text-[12px] font-medium text-[#333] transition-colors hover:bg-[#EFEFF1]">
            Importer dokumenter
          </Link>
          <Link href={"/settings" as Route} className="rounded-[14px] bg-[#F7F7F8] p-3 text-[12px] font-medium text-[#333] transition-colors hover:bg-[#EFEFF1]">
            Tilganger
          </Link>
        </div>
      ) : null}
    </div>
  );
}

function DashboardImageCard({
  card,
  onPrompt,
}: {
  card: DashboardCard;
  onPrompt: (card: DashboardCard) => void;
}) {
  const aboveFold = aboveFoldDashboardCardIds.has(card.id);

  return (
    <div className="velion-dashboard-card group relative h-full overflow-hidden rounded-[18px] bg-white p-2.5 shadow-[0_2px_10px_rgba(0,0,0,0.05)] transition-transform duration-300 hover:-translate-y-0.5 dark:bg-[#141516]">
      <div className="velion-dashboard-card-label pointer-events-none absolute left-0 top-0 z-30 bg-white px-5 pb-4 pt-5 text-[11px] font-semibold tracking-wide text-[#1A1A1A] dark:bg-[#141516] dark:text-white">
        {card.category}
      </div>

      <Link href={card.href as Route} className="block" prefetch>
        <div className="velion-dashboard-card-media relative aspect-[4/3] overflow-hidden rounded-[15px]">
          <Image
            src={card.image}
            alt={card.title}
            fill
            priority={aboveFold}
            loading={aboveFold ? "eager" : "lazy"}
            sizes="(max-width: 640px) 100vw, (max-width: 1024px) 70vw, 31vw"
            className="object-cover transition-transform duration-700 group-hover:scale-[1.02]"
          />
          <div className="absolute inset-x-0 bottom-0 z-10 h-3/5 bg-gradient-to-t from-black/70 via-black/30 to-transparent" />
          <div className="velion-dashboard-card-copy pointer-events-none absolute inset-x-0 bottom-0 z-20 p-4 pb-16">
            <h3 className="text-[16px] font-semibold leading-snug text-white">{card.title}</h3>
            <p className="mt-1 line-clamp-2 text-[12px] leading-relaxed text-white/70">{card.description}</p>
          </div>
        </div>
      </Link>

      <button
        type="button"
        onClick={() => onPrompt(card)}
        className="velion-dashboard-card-action liquid-action absolute -bottom-px right-2.5 z-40 h-[86px] w-40"
        aria-label={`Start chat for ${card.title}`}
        title={`Start chat for ${card.title}`}
      >
        <LiquidCorner cardId={card.id} className="size-full" />
      </button>
    </div>
  );
}


function LiquidCorner({
  cardId,
  className,
}: {
  cardId: string;
  className?: string;
}) {
  const gradientId = `buttonGradient-${cardId}`;

  return (
    <svg className={className} viewBox="0 0 420 300" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id={gradientId} x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stopColor="#FFFFFF" />
          <stop offset="100%" stopColor="#FAFAFA" />
        </linearGradient>
      </defs>

      <path
        d="M 0 260 C 39 250, 52 200, 78 160 C 98 120, 128 95, 170 90 L 280 90 C 335 90, 370 85, 395 70 C 410 55, 420 20, 420 0 L 420 270 L 0 270 Z"
        fill="white"
      />
      <rect x="96" y="150" width="300" height="90" rx="45" fill="rgba(0, 0, 0, 0.04)" />
      <rect x="101" y="150" width="290" height="88" rx="44" fill={`url(#${gradientId})`} stroke="#E8853D" strokeWidth="2" />
      <text
        x="243"
        y="210"
        textAnchor="middle"
        fontSize="31"
        fontWeight="700"
        letterSpacing="0.22em"
        fill="#E8853D"
        style={{ pointerEvents: "none" }}
      >
        CHAT
      </text>
    </svg>
  );
}
