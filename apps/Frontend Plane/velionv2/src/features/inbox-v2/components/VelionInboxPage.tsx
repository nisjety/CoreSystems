"use client";

import { Suspense, useEffect, useReducer } from "react";
import { useSearchParams } from "next/navigation";
import { ConversationPanel } from "@/features/inbox-v2/components/ConversationPanel";
import { InboxAside } from "@/features/inbox-v2/components/InboxAside";
import { InboxWorkModal, type InboxModalRequest } from "@/features/inbox-v2/components/InboxWorkModal";
import { TicketQueue } from "@/features/inbox-v2/components/TicketQueue";
import { useSupportReferenceData } from "@/features/inbox-v2/lib/support-client-stores";
import {
  customerName,
  resolveInboxRouteFilter,
  searchParamsFromInboxSlug,
  type Agent,
  type Group,
  type InboxTab,
  type TicketSentiment,
  type ZammadArticle,
  type ZammadTicket,
} from "@/features/inbox-v2/lib/inbox-model";

type TicketPayload = {
  tickets?: ZammadTicket[];
  total?: number;
};

type TicketQueryInput = {
  activeTab: InboxTab;
  agentState?: string;
  assigned?: string;
  channel?: string;
  queue?: string;
};

const EMPTY_ROUTE_SLUG: string[] = [];

type InboxWorkspaceLayoutProps = {
  activeTab: InboxTab;
  agents: Agent[];
  articles: ZammadArticle[];
  articlesLoading: boolean;
  filteredTickets: ZammadTicket[];
  groups: Group[];
  modal: InboxModalRequest | null;
  notice: string | null;
  onActiveTabChange: (tab: InboxTab) => void;
  onAddTag: (tag: string) => void;
  onCloseModal: () => void;
  onInsertReply: (text: string) => void;
  onMacroExecuted: () => void;
  onOpenModal: (modal: InboxModalRequest | null) => void;
  onPatchTicket: (patch: Record<string, unknown>) => void;
  onRefreshTicket: () => void;
  onRemoveTag: (tag: string) => void;
  onSearchChange: (query: string) => void;
  onSelectTicket: (ticket: ZammadTicket) => void;
  onSendReply: (text: string, internal: boolean) => void;
  onSuggestReply: () => void;
  replySending: boolean;
  replyText: string;
  routeLabel: string;
  searchQuery: string;
  selectedTicket: ZammadTicket | null;
  sentiment: TicketSentiment | null;
  ticketsError: string | null;
  ticketsLoading: boolean;
};

type InboxPageState = {
  articles: ZammadArticle[];
  articlesLoading: boolean;
  modal: InboxModalRequest | null;
  notice: string | null;
  replySending: boolean;
  replyText: string;
  searchQuery: string;
  selectedTicket: ZammadTicket | null;
  sentiment: TicketSentiment | null;
  tabState: { routeKey: string; tab: InboxTab };
  tickets: ZammadTicket[];
  ticketsError: string | null;
  ticketsLoading: boolean;
};

type InboxPageAction =
  | { type: "route-changed"; routeKey: string; tab: InboxTab }
  | { type: "tab-changed"; routeKey: string; tab: InboxTab }
  | { type: "search-changed"; query: string }
  | { type: "tickets-load-start" }
  | { type: "tickets-loaded"; tickets: ZammadTicket[] }
  | { type: "tickets-failed"; error: string }
  | { type: "ticket-open-start"; ticket: ZammadTicket }
  | { type: "ticket-details-loaded"; articles: ZammadArticle[]; sentiment: TicketSentiment | null; ticketId: number }
  | { type: "ticket-details-failed"; ticketId: number }
  | { type: "ticket-updated"; notice?: string; ticket: ZammadTicket }
  | { type: "notice-set"; notice: string | null }
  | { type: "reply-text-changed"; text: string }
  | { type: "reply-send-start"; article: ZammadArticle; ticketId: number }
  | { type: "reply-send-succeeded"; article?: ZammadArticle; optimisticArticleId: number; notice: string; ticketId: number }
  | { type: "reply-send-failed"; body: string; optimisticArticleId: number; ticketId: number }
  | { type: "modal-set"; modal: InboxModalRequest | null };

function createInitialInboxPageState(routeKey: string, tab: InboxTab): InboxPageState {
  return {
    articles: [],
    articlesLoading: false,
    modal: null,
    notice: null,
    replySending: false,
    replyText: "",
    searchQuery: "",
    selectedTicket: null,
    sentiment: null,
    tabState: { routeKey, tab },
    tickets: [],
    ticketsError: null,
    ticketsLoading: true,
  };
}

function updateTicketInList(tickets: ZammadTicket[], updated: ZammadTicket) {
  return tickets.map((ticket) => ticket.id === updated.id ? updated : ticket);
}

function inboxPageReducer(state: InboxPageState, action: InboxPageAction): InboxPageState {
  switch (action.type) {
    case "route-changed":
      if (state.tabState.routeKey === action.routeKey && state.tabState.tab === action.tab) {
        return state;
      }

      return {
        ...state,
        notice: null,
        replyText: "",
        selectedTicket: null,
        sentiment: null,
        tabState: { routeKey: action.routeKey, tab: action.tab },
      };
    case "tab-changed":
      return { ...state, tabState: { routeKey: action.routeKey, tab: action.tab } };
    case "search-changed":
      return { ...state, searchQuery: action.query };
    case "tickets-load-start":
      return { ...state, ticketsError: null, ticketsLoading: true };
    case "tickets-loaded":
      return { ...state, tickets: action.tickets, ticketsError: null, ticketsLoading: false };
    case "tickets-failed":
      return { ...state, tickets: [], ticketsError: action.error, ticketsLoading: false };
    case "ticket-open-start":
      return {
        ...state,
        articles: [],
        articlesLoading: true,
        notice: null,
        replyText: "",
        selectedTicket: action.ticket,
        sentiment: null,
      };
    case "ticket-details-loaded":
      if (state.selectedTicket?.id !== action.ticketId) {
        return state;
      }

      return {
        ...state,
        articles: action.articles,
        articlesLoading: false,
        sentiment: action.sentiment,
      };
    case "ticket-details-failed":
      if (state.selectedTicket?.id !== action.ticketId) {
        return state;
      }

      return { ...state, articles: [], articlesLoading: false };
    case "ticket-updated":
      return {
        ...state,
        notice: action.notice ?? state.notice,
        selectedTicket: state.selectedTicket?.id === action.ticket.id ? action.ticket : state.selectedTicket,
        tickets: updateTicketInList(state.tickets, action.ticket),
      };
    case "notice-set":
      return { ...state, notice: action.notice };
    case "reply-text-changed":
      return { ...state, replyText: action.text };
    case "reply-send-start":
      return {
        ...state,
        articles: state.selectedTicket?.id === action.ticketId ? [...state.articles, action.article] : state.articles,
        replySending: true,
        replyText: state.selectedTicket?.id === action.ticketId ? "" : state.replyText,
      };
    case "reply-send-succeeded":
      if (state.selectedTicket?.id !== action.ticketId) {
        return { ...state, replySending: false };
      }

      return {
        ...state,
        articles: action.article
          ? state.articles.map((article) => article.id === action.optimisticArticleId ? action.article as ZammadArticle : article)
          : state.articles,
        notice: action.notice,
        replySending: false,
      };
    case "reply-send-failed":
      if (state.selectedTicket?.id !== action.ticketId) {
        return { ...state, replySending: false };
      }

      return {
        ...state,
        articles: state.articles.filter((article) => article.id !== action.optimisticArticleId),
        notice: "Reply failed.",
        replySending: false,
        replyText: action.body,
      };
    case "modal-set":
      return { ...state, modal: action.modal };
    default:
      return state;
  }
}

function createTicketQuery({
  activeTab,
  agentState,
  assigned,
  channel,
  queue,
}: TicketQueryInput) {
  const params = new URLSearchParams({ page: "1", limit: "50" });
  if (activeTab !== "all") params.set("state", activeTab);
  if (assigned) params.set("assigned", assigned);
  if (channel) params.set("channel", channel);
  if (queue) params.set("queue", queue);
  if (agentState) params.set("agentState", agentState);
  return params;
}

async function requestTickets(params: URLSearchParams) {
  const response = await fetch(`/api/support/tickets?${params}`, { cache: "no-store" });

  if (!response.ok) {
    throw new Error(`Failed to load tickets: ${response.status}`);
  }

  const payload = (await response.json()) as TicketPayload | ZammadTicket[];
  return Array.isArray(payload) ? payload : payload.tickets ?? [];
}

export function VerevonInboxPage({ routeSlug = EMPTY_ROUTE_SLUG }: { routeSlug?: string[] }) {
  return (
    <Suspense fallback={null}>
      <VerevonInboxPageContent routeSlug={routeSlug} />
    </Suspense>
  );
}

function VerevonInboxPageContent({ routeSlug }: { routeSlug: string[] }) {
  const searchParams = useSearchParams();
  const { agents, groups } = useSupportReferenceData();
  const queryString = searchParams?.toString() ?? "";
  const searchParamString = queryString || searchParamsFromInboxSlug(routeSlug).toString();
  const routeFilter = resolveInboxRouteFilter(new URLSearchParams(searchParamString));
  const [state, dispatch] = useReducer(
    inboxPageReducer,
    { routeKey: searchParamString, tab: routeFilter.activeTab },
    ({ routeKey, tab }) => createInitialInboxPageState(routeKey, tab),
  );
  const {
    articles,
    articlesLoading,
    modal,
    notice,
    replySending,
    replyText,
    searchQuery,
    selectedTicket,
    sentiment,
    tabState,
    tickets,
    ticketsError,
    ticketsLoading,
  } = state;
  const activeTab = tabState.routeKey === searchParamString ? tabState.tab : routeFilter.activeTab;

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      dispatch({ type: "route-changed", routeKey: searchParamString, tab: routeFilter.activeTab });
    });

    return () => window.cancelAnimationFrame(frame);
  }, [routeFilter.activeTab, searchParamString]);

  const handleActiveTabChange = (tab: InboxTab) => {
    dispatch({ type: "tab-changed", routeKey: searchParamString, tab });
  };

  const loadTickets = async () => {
    dispatch({ type: "tickets-load-start" });
    const params = createTicketQuery({
      activeTab,
      agentState: routeFilter.agentState,
      assigned: routeFilter.assigned,
      channel: routeFilter.channel,
      queue: routeFilter.queue,
    });

    try {
      dispatch({ type: "tickets-loaded", tickets: await requestTickets(params) });
    } catch (error) {
      dispatch({ type: "tickets-failed", error: error instanceof Error ? error.message : "Failed to load tickets" });
    }
  };

  useEffect(() => {
    let cancelled = false;
    const timeout = window.setTimeout(() => {
      const params = createTicketQuery({
        activeTab,
        agentState: routeFilter.agentState,
        assigned: routeFilter.assigned,
        channel: routeFilter.channel,
        queue: routeFilter.queue,
      });
      dispatch({ type: "tickets-load-start" });

      requestTickets(params)
        .then((nextTickets) => {
          if (!cancelled) {
            dispatch({ type: "tickets-loaded", tickets: nextTickets });
          }
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            dispatch({ type: "tickets-failed", error: error instanceof Error ? error.message : "Failed to load tickets" });
          }
        });
    }, 0);

    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
    };
  }, [
    activeTab,
    routeFilter.agentState,
    routeFilter.assigned,
    routeFilter.channel,
    routeFilter.queue,
  ]);

  const query = searchQuery.trim().toLowerCase();
  const filteredTickets = query
    ? tickets.filter((ticket) => (
      ticket.title.toLowerCase().includes(query) ||
      ticket.number.includes(query) ||
      customerName(ticket).toLowerCase().includes(query) ||
      ticket.customer?.email?.toLowerCase().includes(query) ||
      ticket.tags?.join(" ").toLowerCase().includes(query)
    ))
    : tickets;

  const loadTicketDetails = async (ticket: ZammadTicket) => {
    dispatch({ type: "ticket-open-start", ticket });

    try {
      const [articlesResult, sentimentResult] = await Promise.allSettled([
        fetch(`/api/support/tickets/${ticket.id}/articles`, { cache: "no-store" }),
        fetch(`/api/support/tickets/${ticket.id}/sentiment`, { method: "POST" }),
      ]);

      let nextArticles: ZammadArticle[] = [];
      let nextSentiment: TicketSentiment | null = null;

      if (articlesResult.status === "fulfilled" && articlesResult.value.ok) {
        const payload = (await articlesResult.value.json()) as { articles?: ZammadArticle[] } | ZammadArticle[];
        nextArticles = Array.isArray(payload) ? payload : payload.articles ?? [];
      }

      if (sentimentResult.status === "fulfilled" && sentimentResult.value.ok) {
        const payload = (await sentimentResult.value.json()) as Partial<TicketSentiment>;
        if (payload.sentiment && typeof payload.score === "number") {
          nextSentiment = { sentiment: payload.sentiment, score: payload.score };
        }
      }

      dispatch({
        type: "ticket-details-loaded",
        articles: nextArticles,
        sentiment: nextSentiment,
        ticketId: ticket.id,
      });
    } catch {
      dispatch({ type: "ticket-details-failed", ticketId: ticket.id });
    }
  };

  const patchSelectedTicket = async (patch: Record<string, unknown>) => {
    if (!selectedTicket) return;
    try {
      const response = await fetch(`/api/support/tickets/${selectedTicket.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(patch),
      });
      if (!response.ok) {
        dispatch({ type: "notice-set", notice: "Ticket update failed." });
        return;
      }
      const updated = (await response.json()) as ZammadTicket;
      dispatch({ type: "ticket-updated", notice: "Conversation details updated.", ticket: updated });
    } catch {
      dispatch({ type: "notice-set", notice: "Ticket update failed." });
    }
  };

  const refreshSelectedTicket = async () => {
    if (!selectedTicket) return;

    try {
      const response = await fetch(`/api/support/tickets/${selectedTicket.id}`, { cache: "no-store" });
      if (!response.ok) return;
      const updated = (await response.json()) as ZammadTicket;
      dispatch({ type: "ticket-updated", ticket: updated });
    } catch {
      // Best-effort refresh after macro execution.
    }
  };

  const addTag = (tag: string) => {
    if (!selectedTicket) return;
    const normalized = tag.trim();
    if (!normalized || selectedTicket.tags?.includes(normalized)) return;
    void patchSelectedTicket({ tags: [...(selectedTicket.tags ?? []), normalized] });
  };

  const removeTag = (tag: string) => {
    if (!selectedTicket) return;
    void patchSelectedTicket({ tags: (selectedTicket.tags ?? []).filter((current) => current !== tag) });
  };

  const sendReply = async (text: string, internal: boolean) => {
    const body = text.trim();
    if (!body || !selectedTicket || replySending) return;

    const optimisticArticle: ZammadArticle = {
      id: -Date.now(),
      ticket_id: selectedTicket.id,
      body,
      internal,
      sender: "Agent",
      from: "You",
      created_at: new Date().toISOString(),
    };

    dispatch({ type: "reply-send-start", article: optimisticArticle, ticketId: selectedTicket.id });

    try {
      const response = await fetch(`/api/support/tickets/${selectedTicket.id}/articles`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ body, internal }),
      });
      if (!response.ok) {
        dispatch({
          type: "reply-send-failed",
          body,
          optimisticArticleId: optimisticArticle.id,
          ticketId: selectedTicket.id,
        });
        return;
      }
      const payload = (await response.json()) as { article?: ZammadArticle };
      dispatch({
        type: "reply-send-succeeded",
        article: payload.article,
        optimisticArticleId: optimisticArticle.id,
        notice: internal ? "Internal note added." : "Reply sent.",
        ticketId: selectedTicket.id,
      });
    } catch {
      dispatch({
        type: "reply-send-failed",
        body,
        optimisticArticleId: optimisticArticle.id,
        ticketId: selectedTicket.id,
      });
    }
  };

  const suggestReply = async () => {
    if (!selectedTicket) return;
    try {
      const response = await fetch(`/api/support/tickets/${selectedTicket.id}/quick-replies`, { method: "POST" });
      if (!response.ok) {
        dispatch({ type: "notice-set", notice: "AI suggestion failed." });
        return;
      }
      const payload = (await response.json()) as { options?: string[] };
      dispatch({ type: "reply-text-changed", text: payload.options?.[0] ?? "" });
    } catch {
      dispatch({ type: "notice-set", notice: "AI suggestion failed." });
    }
  };

  const handleMacroExecuted = () => {
    void loadTickets();
    void refreshSelectedTicket();
  };

  return (
    <InboxWorkspaceLayout
      activeTab={activeTab}
      agents={agents}
      articles={articles}
      articlesLoading={articlesLoading}
      filteredTickets={filteredTickets}
      groups={groups}
      modal={modal}
      notice={notice}
      onActiveTabChange={handleActiveTabChange}
      onAddTag={addTag}
      onCloseModal={() => dispatch({ type: "modal-set", modal: null })}
      onInsertReply={(text) => dispatch({ type: "reply-text-changed", text })}
      onMacroExecuted={handleMacroExecuted}
      onOpenModal={(nextModal) => dispatch({ type: "modal-set", modal: nextModal })}
      onPatchTicket={patchSelectedTicket}
      onRefreshTicket={refreshSelectedTicket}
      onRemoveTag={removeTag}
      onSearchChange={(nextQuery) => dispatch({ type: "search-changed", query: nextQuery })}
      onSelectTicket={loadTicketDetails}
      onSendReply={sendReply}
      onSuggestReply={suggestReply}
      replySending={replySending}
      replyText={replyText}
      routeLabel={routeFilter.label}
      searchQuery={searchQuery}
      selectedTicket={selectedTicket}
      sentiment={sentiment}
      ticketsError={ticketsError}
      ticketsLoading={ticketsLoading}
    />
  );
}

function InboxWorkspaceLayout({
  activeTab,
  agents,
  articles,
  articlesLoading,
  filteredTickets,
  groups,
  modal,
  notice,
  onActiveTabChange,
  onAddTag,
  onCloseModal,
  onInsertReply,
  onMacroExecuted,
  onOpenModal,
  onPatchTicket,
  onRefreshTicket,
  onRemoveTag,
  onSearchChange,
  onSelectTicket,
  onSendReply,
  onSuggestReply,
  replySending,
  replyText,
  routeLabel,
  searchQuery,
  selectedTicket,
  sentiment,
  ticketsError,
  ticketsLoading,
}: InboxWorkspaceLayoutProps) {
  return (
    <div className="h-full min-h-0 overflow-y-auto bg-[#F5F1EC] text-[#111111] xl:overflow-hidden dark:bg-[#101114] dark:text-[#F7F8F8]">
      <div className="grid min-h-full grid-cols-1 gap-2 p-2 xl:h-full xl:grid-cols-[340px_minmax(0,1fr)]">
        <TicketQueue
          activeTab={activeTab}
          error={ticketsError}
          label={routeLabel}
          loading={ticketsLoading}
          onActiveTabChange={onActiveTabChange}
          onOpenModal={onOpenModal}
          onSearchChange={onSearchChange}
          onSelectTicket={onSelectTicket}
          searchQuery={searchQuery}
          selectedTicketId={selectedTicket?.id ?? null}
          tickets={filteredTickets}
        />

        <div className="grid min-h-[720px] min-w-0 gap-2 xl:h-full xl:min-h-0 lg:grid-cols-[minmax(0,1fr)_360px]">
          <ConversationPanel
            agents={agents}
            articles={articles}
            articlesLoading={articlesLoading}
            groups={groups}
            notice={notice}
            onAddTag={onAddTag}
            onOpenModal={onOpenModal}
            onPatchTicket={onPatchTicket}
            onRemoveTag={onRemoveTag}
            onSendReply={onSendReply}
            onSuggestReply={onSuggestReply}
            replyText={replyText}
            replySending={replySending}
            selectedTicket={selectedTicket}
            sentiment={sentiment}
            setReplyText={onInsertReply}
          />
          <InboxAside
            onInsertQuickReply={onInsertReply}
            onMacroExecuted={onMacroExecuted}
            onOpenModal={onOpenModal}
            selectedTicket={selectedTicket}
          />
        </div>
      </div>
      <InboxWorkModal
        agents={agents}
        groups={groups}
        modal={modal}
        onClose={onCloseModal}
        onInsertReply={onInsertReply}
        onPatchTicket={onPatchTicket}
        onRefreshTicket={onRefreshTicket}
        onSendReply={onSendReply}
        selectedTicket={selectedTicket}
      />
    </div>
  );
}
