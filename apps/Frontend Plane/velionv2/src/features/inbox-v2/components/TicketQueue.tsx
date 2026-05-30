"use client";

import { useEffect, useReducer, useRef, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowDown,
  ArrowDownUp,
  ArrowUp,
  Archive,
  AtSign,
  Clock3,
  Check,
  CheckCheck,
  Inbox,
  Mail,
  MailOpen,
  MessageCircle,
  MoreHorizontal,
  PanelLeft,
  Pin,
  SlidersHorizontal,
} from "lucide-react";
import {
  customerName,
  formatRelativeTime,
  type InboxTab,
  type ZammadTicket,
} from "@/features/inbox-v2/lib/inbox-model";
import type { InboxModalRequest } from "@/features/inbox-v2/components/InboxWorkModal";
import { cn } from "@/lib/utils";

const inboxTabs: Array<{ id: InboxTab; label: string }> = [
  { id: "all", label: "All" },
  { id: "open", label: "Open" },
  { id: "pending", label: "Pending" },
  { id: "solved", label: "Solved" },
];

const filterMenuItems = [
  { label: "Your inbox", href: "/inbox?view=mine", description: "Conversations assigned to you, kept inside the active inbox workspace." },
  { label: "All conversations", href: "/inbox?view=all", description: "A full team queue view for monitoring every active conversation." },
  { label: "Unassigned", href: "/inbox?view=unassigned", description: "Tickets that Velion or a human operator should route to an owner." },
  { label: "Mentions", href: "/inbox?view=mentions", description: "Conversation threads where an operator or AI workflow was mentioned." },
  { label: "Messenger", href: "/inbox?view=view-messenger", description: "Messenger-channel conversations without leaving the inbox surface." },
  { label: "Email", href: "/inbox?view=view-email", description: "Email-channel conversations without opening a separate page." },
] as const;

type SortKey =
  | "last-message-desc"
  | "last-message-asc"
  | "created-desc"
  | "created-asc"
  | "priority-desc"
  | "priority-asc";

const sortOptions: Array<{ id: SortKey; label: string; icon: typeof ArrowDown }> = [
  { id: "last-message-desc", label: "Last message", icon: ArrowDown },
  { id: "last-message-asc", label: "Last message", icon: ArrowUp },
  { id: "created-desc", label: "Created", icon: ArrowDown },
  { id: "created-asc", label: "Created", icon: ArrowUp },
  { id: "priority-desc", label: "Priority", icon: ArrowDown },
  { id: "priority-asc", label: "Priority", icon: ArrowUp },
];

type FocusLane = "focused" | "other";
type QuickFilter = "all" | "unread" | "mentions" | "pinned";

type TicketQueueState = {
  archivedIds: Set<number>;
  filtersOpen: boolean;
  focusLane: FocusLane;
  pinnedIds: Set<number>;
  quickFilter: QuickFilter;
  readIds: Set<number>;
  selectedIds: Set<number>;
  snoozedIds: Set<number>;
  sortKey: SortKey;
  sortOpen: boolean;
};

type TicketQueueAction =
  | { type: "archive-selected" }
  | { type: "archive-ticket"; ticketId: number }
  | { type: "close-filters" }
  | { type: "close-menus" }
  | { type: "mark-read"; ticketId: number }
  | { type: "mark-selected-read" }
  | { type: "retain-selected"; visibleIds: number[] }
  | { type: "set-focus-lane"; focusLane: FocusLane }
  | { type: "set-quick-filter"; quickFilter: QuickFilter }
  | { type: "set-sort-key"; sortKey: SortKey }
  | { type: "snooze-selected" }
  | { type: "snooze-ticket"; ticketId: number }
  | { type: "toggle-all-visible"; ticketIds: number[]; selected: boolean }
  | { type: "toggle-filters" }
  | { type: "toggle-pinned"; ticketId: number }
  | { type: "toggle-selected"; ticketId: number }
  | { type: "toggle-sort" };

function createInitialTicketQueueState(): TicketQueueState {
  return {
    archivedIds: new Set(),
    filtersOpen: false,
    focusLane: "focused",
    pinnedIds: new Set(),
    quickFilter: "all",
    readIds: new Set(),
    selectedIds: new Set(),
    snoozedIds: new Set(),
    sortKey: "last-message-desc",
    sortOpen: false,
  };
}

function ticketQueueReducer(state: TicketQueueState, action: TicketQueueAction): TicketQueueState {
  switch (action.type) {
    case "archive-selected":
      return state.selectedIds.size
        ? {
            ...state,
            archivedIds: addManyToSet(state.archivedIds, state.selectedIds),
            selectedIds: new Set(),
          }
        : state;
    case "archive-ticket":
      return {
        ...state,
        archivedIds: addToSet(state.archivedIds, action.ticketId),
        selectedIds: removeFromSet(state.selectedIds, action.ticketId),
      };
    case "close-filters":
      return state.filtersOpen ? { ...state, filtersOpen: false } : state;
    case "close-menus":
      return state.filtersOpen || state.sortOpen ? { ...state, filtersOpen: false, sortOpen: false } : state;
    case "mark-read":
      return {
        ...state,
        readIds: addToSet(state.readIds, action.ticketId),
      };
    case "mark-selected-read":
      return state.selectedIds.size
        ? {
            ...state,
            readIds: addManyToSet(state.readIds, state.selectedIds),
          }
        : state;
    case "retain-selected": {
      const nextSelectedIds = retainSetValues(state.selectedIds, new Set(action.visibleIds));
      return nextSelectedIds === state.selectedIds ? state : { ...state, selectedIds: nextSelectedIds };
    }
    case "set-focus-lane":
      return state.focusLane === action.focusLane ? state : { ...state, focusLane: action.focusLane };
    case "set-quick-filter":
      return state.quickFilter === action.quickFilter ? state : { ...state, quickFilter: action.quickFilter };
    case "set-sort-key":
      return {
        ...state,
        sortKey: action.sortKey,
        sortOpen: false,
      };
    case "snooze-selected":
      return state.selectedIds.size
        ? {
            ...state,
            selectedIds: new Set(),
            snoozedIds: addManyToSet(state.snoozedIds, state.selectedIds),
          }
        : state;
    case "snooze-ticket":
      return {
        ...state,
        selectedIds: removeFromSet(state.selectedIds, action.ticketId),
        snoozedIds: addToSet(state.snoozedIds, action.ticketId),
      };
    case "toggle-all-visible":
      return {
        ...state,
        selectedIds: action.selected
          ? removeManyFromSet(state.selectedIds, action.ticketIds)
          : addManyToSet(state.selectedIds, action.ticketIds),
      };
    case "toggle-filters":
      return {
        ...state,
        filtersOpen: !state.filtersOpen,
        sortOpen: false,
      };
    case "toggle-pinned":
      return {
        ...state,
        pinnedIds: toggleSetValue(state.pinnedIds, action.ticketId),
      };
    case "toggle-selected":
      return {
        ...state,
        selectedIds: toggleSetValue(state.selectedIds, action.ticketId),
      };
    case "toggle-sort":
      return {
        ...state,
        filtersOpen: false,
        sortOpen: !state.sortOpen,
      };
  }
}

const quickFilters: Array<{ id: QuickFilter; label: string; icon?: typeof MailOpen }> = [
  { id: "all", label: "All" },
  { id: "unread", label: "Unread", icon: MailOpen },
  { id: "mentions", label: "Mentions", icon: AtSign },
  { id: "pinned", label: "Pinned", icon: Pin },
];

export function TicketQueue({
  activeTab,
  error,
  loading,
  onActiveTabChange,
  onOpenModal,
  onSearchChange,
  onSelectTicket,
  searchQuery,
  selectedTicketId,
  tickets,
}: {
  activeTab: InboxTab;
  error: string | null;
  label: string;
  loading: boolean;
  onActiveTabChange: (tab: InboxTab) => void;
  onOpenModal: (modal: InboxModalRequest) => void;
  onSearchChange: (query: string) => void;
  onSelectTicket: (ticket: ZammadTicket) => void;
  searchQuery: string;
  selectedTicketId: number | null;
  tickets: ZammadTicket[];
}) {
  // Backend handoff: these local triage sets should become persisted ticket
  // state via support endpoints for archive, snooze, pin, read, and bulk actions.
  const [queueState, dispatchQueue] = useReducer(ticketQueueReducer, undefined, createInitialTicketQueueState);
  const {
    archivedIds,
    filtersOpen,
    focusLane,
    pinnedIds,
    quickFilter,
    readIds,
    selectedIds,
    snoozedIds,
    sortKey,
    sortOpen,
  } = queueState;
  const filtersRef = useRef<HTMLDivElement>(null);
  const sortRef = useRef<HTMLDivElement>(null);

  const laneCounts = {
    focused: tickets.filter((ticket) => !archivedIds.has(ticket.id) && !snoozedIds.has(ticket.id) && isFocusedTicket(ticket)).length,
    other: tickets.filter((ticket) => !archivedIds.has(ticket.id) && !snoozedIds.has(ticket.id) && !isFocusedTicket(ticket)).length,
  };

  const visibleTickets = tickets.filter((ticket) => {
    if (archivedIds.has(ticket.id) || snoozedIds.has(ticket.id)) return false;
    if (focusLane === "focused" && !isFocusedTicket(ticket)) return false;
    if (focusLane === "other" && isFocusedTicket(ticket)) return false;
    if (quickFilter === "unread" && readIds.has(ticket.id)) return false;
    if (quickFilter === "mentions" && !isMentionedTicket(ticket)) return false;
    if (quickFilter === "pinned" && !pinnedIds.has(ticket.id)) return false;
    return true;
  });

  const sortedTickets = sortTickets(visibleTickets, sortKey, pinnedIds);
  const allVisibleSelected = sortedTickets.length > 0 && sortedTickets.every((ticket) => selectedIds.has(ticket.id));
  const selectedCount = sortedTickets.filter((ticket) => selectedIds.has(ticket.id)).length;

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      const visibleIds = new Set(sortedTickets.map((ticket) => ticket.id));
      dispatchQueue({ type: "retain-selected", visibleIds: [...visibleIds] });
    }, 0);

    return () => window.clearTimeout(timeout);
  }, [sortedTickets]);

  useEffect(() => {
    if (!filtersOpen && !sortOpen) return;

    function closeOnOutside(event: MouseEvent) {
      const target = event.target as Node;
      if (filtersRef.current?.contains(target) || sortRef.current?.contains(target)) return;
      dispatchQueue({ type: "close-menus" });
    }

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        dispatchQueue({ type: "close-menus" });
      }
    }

    document.addEventListener("mousedown", closeOnOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("mousedown", closeOnOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [filtersOpen, sortOpen]);

  function toggleTicketSelection(ticketId: number) {
    dispatchQueue({ type: "toggle-selected", ticketId });
  }

  function markRead(ticketId: number) {
    dispatchQueue({ type: "mark-read", ticketId });
  }

  function archiveTicket(ticketId: number) {
    dispatchQueue({ type: "archive-ticket", ticketId });
  }

  function snoozeTicket(ticketId: number) {
    dispatchQueue({ type: "snooze-ticket", ticketId });
  }

  function togglePinned(ticketId: number) {
    dispatchQueue({ type: "toggle-pinned", ticketId });
  }

  function archiveSelected() {
    dispatchQueue({ type: "archive-selected" });
  }

  function markSelectedRead() {
    dispatchQueue({ type: "mark-selected-read" });
  }

  function snoozeSelected() {
    dispatchQueue({ type: "snooze-selected" });
  }

  function toggleAllVisible() {
    dispatchQueue({
      type: "toggle-all-visible",
      selected: allVisibleSelected,
      ticketIds: sortedTickets.map((ticket) => ticket.id),
    });
  }

  return (
    <section className="flex h-[520px] min-h-0 flex-col overflow-hidden rounded-[10px] border border-[#E1E4EA] bg-white text-[#1D1D1F] shadow-[0_1px_2px_rgba(16,24,40,0.03)] xl:h-full dark:border-[#2A2C31] dark:bg-[#101114] dark:text-[#F7F8F8]">
      <div className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-[#ECEEF2] px-3 dark:border-[#2A2C31]">
        <div className="flex min-w-0 items-center gap-2.5">
          <PanelLeft className="size-4 shrink-0 text-[#5F6673]" strokeWidth={2} />
          <Inbox className="size-4 shrink-0 text-[#1D1D1F]" strokeWidth={2} />
          <h1 className="truncate text-[16px] font-semibold leading-[22px] tracking-normal">
            Inbox
          </h1>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <div ref={filtersRef} className="relative">
            <button
              type="button"
              onClick={() => dispatchQueue({ type: "toggle-filters" })}
              className={cn(
                "grid size-7 place-items-center rounded-[7px] text-[#5F6673] transition-colors hover:bg-[#F3F5F8] hover:text-[#1D1D1F] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB] dark:text-[#B3BAC6] dark:hover:bg-white/5 dark:hover:text-white",
                filtersOpen ? "bg-[#F3F5F8] text-[#1D1D1F] dark:bg-white/10 dark:text-white" : "",
              )}
              aria-expanded={filtersOpen}
              aria-label="Open inbox filters"
              title="Open inbox filters"
            >
              <SlidersHorizontal className="size-4" strokeWidth={1.9} />
            </button>
            {filtersOpen ? (
              <FiltersMenu
                activeTab={activeTab}
                onActiveTabChange={onActiveTabChange}
                onClose={() => dispatchQueue({ type: "close-filters" })}
                onOpenModal={onOpenModal}
                onSearchChange={onSearchChange}
                searchQuery={searchQuery}
              />
            ) : null}
          </div>
          <div ref={sortRef} className="relative">
            <button
              type="button"
              onClick={() => dispatchQueue({ type: "toggle-sort" })}
              className={cn(
                "grid size-7 place-items-center rounded-[7px] text-[#5F6673] transition-colors hover:bg-[#F3F5F8] hover:text-[#1D1D1F] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB] dark:text-[#B3BAC6] dark:hover:bg-white/5 dark:hover:text-white",
                sortOpen ? "bg-[#F3F5F8] text-[#1D1D1F] dark:bg-white/10 dark:text-white" : "",
              )}
              aria-expanded={sortOpen}
              aria-label="Sort conversations"
              title="Sort conversations"
            >
              <ArrowDownUp className="size-4" strokeWidth={1.9} />
            </button>
            {sortOpen ? (
              <SortMenu
                activeSort={sortKey}
                onSelect={(nextSort) => {
                  dispatchQueue({ type: "set-sort-key", sortKey: nextSort });
                }}
              />
            ) : null}
          </div>
        </div>
      </div>

      <div className="flex h-10 shrink-0 items-center justify-between gap-2 overflow-hidden border-b border-[#F2F3F5] px-3 dark:border-[#2A2C31]">
        <div className="inline-flex h-7 shrink-0 rounded-[8px] bg-[#F4F6F8] p-0.5 text-[12px] font-medium text-[#596171] dark:bg-white/5">
          <FocusLaneButton active={focusLane === "focused"} count={laneCounts.focused} label="Focused" onClick={() => dispatchQueue({ type: "set-focus-lane", focusLane: "focused" })} />
          <FocusLaneButton active={focusLane === "other"} count={laneCounts.other} label="Other" onClick={() => dispatchQueue({ type: "set-focus-lane", focusLane: "other" })} />
        </div>
        <div className="flex min-w-0 flex-1 items-center justify-end gap-1 overflow-hidden">
          {quickFilters.map((filter) => {
            const Icon = filter.icon;
            const textFilter = filter.id === "all";
            return (
              <button
                key={filter.id}
                type="button"
                onClick={() => dispatchQueue({ type: "set-quick-filter", quickFilter: filter.id })}
                className={cn(
                  "flex h-7 shrink-0 items-center gap-1 rounded-[7px] text-[12px] font-medium transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]",
                  textFilter ? "min-w-10 px-2" : "w-7 justify-center px-0",
                  quickFilter === filter.id
                    ? "bg-white text-[#1D1D1F] shadow-[0_1px_2px_rgba(16,24,40,0.08)] dark:bg-[#202229] dark:text-white"
                    : "text-[#626A78] hover:bg-white/70 hover:text-[#1D1D1F] dark:text-[#AEB4C0] dark:hover:bg-white/10 dark:hover:text-white",
                )}
                aria-pressed={quickFilter === filter.id}
                aria-label={`Show ${filter.label.toLowerCase()} conversations`}
                title={`Show ${filter.label.toLowerCase()} conversations`}
              >
                {Icon ? <Icon className="size-3.5" strokeWidth={1.9} /> : null}
                <span className={cn(textFilter ? "block" : "sr-only")}>{filter.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex h-10 shrink-0 items-center justify-between border-b border-[#F2F3F5] px-3 text-[12px] text-[#6F7786] dark:border-[#2A2C31]">
        <label className="flex items-center gap-2 text-[14px] font-semibold text-[#1D1D1F] dark:text-white">
          <input
            type="checkbox"
            checked={allVisibleSelected}
            onChange={toggleAllVisible}
            className="size-4 rounded-[4px] border-[#CDD2DC] text-[#1D1D1F] accent-[#2563EB]"
          />
          {selectedCount ? `${selectedCount} selected` : "Select all"}
        </label>
        <div className={cn("flex items-center gap-1 transition-opacity", selectedCount ? "opacity-100" : "opacity-35")}>
          <BulkActionButton disabled={!selectedCount} label="Mark selected as read" onClick={markSelectedRead}><CheckCheck className="size-3.5" /></BulkActionButton>
          <BulkActionButton disabled={!selectedCount} label="Snooze selected" onClick={snoozeSelected}><Clock3 className="size-3.5" /></BulkActionButton>
          <BulkActionButton disabled={!selectedCount} label="Archive selected" onClick={archiveSelected}><Archive className="size-3.5" /></BulkActionButton>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        {loading ? <LoadingRows /> : null}
        {!loading && error ? <QueueEmptyState tone="error" title="Support is not connected" body={error} /> : null}
        {!loading && !error && sortedTickets.length === 0 ? (
          <QueueEmptyState title="No items" body="This view is clear for now. Change filters or switch lanes to see more conversations." />
        ) : null}
        {!loading && !error && sortedTickets.length > 0 ? (
          <VirtualTicketList
            onArchive={archiveTicket}
            onMarkRead={markRead}
            onSelectTicket={onSelectTicket}
            onSnooze={snoozeTicket}
            onTogglePinned={togglePinned}
            onToggleSelected={toggleTicketSelection}
            pinnedIds={pinnedIds}
            readIds={readIds}
            selectedIds={selectedIds}
            selectedTicketId={selectedTicketId}
            tickets={sortedTickets}
          />
        ) : null}
      </div>
    </section>
  );
}

function FiltersMenu({
  activeTab,
  onActiveTabChange,
  onClose,
  onOpenModal,
  onSearchChange,
  searchQuery,
}: {
  activeTab: InboxTab;
  onActiveTabChange: (tab: InboxTab) => void;
  onClose: () => void;
  onOpenModal: (modal: InboxModalRequest) => void;
  onSearchChange: (query: string) => void;
  searchQuery: string;
}) {
  return (
    <div className="velion-popover absolute right-0 top-9 z-30 w-[238px] py-2">
      <div className="px-3 pb-1 text-[12px] font-semibold text-[#626260]">Status</div>
      <div className="px-1.5">
        {inboxTabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => {
              onActiveTabChange(tab.id);
              onClose();
            }}
            className={cn(
              "flex h-8 w-full items-center justify-between rounded-[8px] px-2 text-left text-[13px] transition-colors hover:bg-[#F5F1EC]",
              activeTab === tab.id ? "font-semibold text-[#111111] dark:text-white" : "text-[#626260] dark:text-[#B7BEC9]",
            )}
          >
            {tab.label}
            {activeTab === tab.id ? <Check className="size-3.5" /> : null}
          </button>
        ))}
      </div>
      <div className="my-2 border-t border-[#EEE8E0] dark:border-[#2A2C31]" />
      <div className="px-3 pb-1 text-[12px] font-semibold text-[#626260]">Views</div>
      <div className="px-1.5">
        {filterMenuItems.map((item) => (
          <button
            key={item.href}
            type="button"
            onClick={() => {
              onOpenModal({
                type: "view",
                title: item.label,
                description: item.description,
                sourceHref: item.href,
              });
              onClose();
            }}
            className="flex h-8 w-full items-center rounded-[8px] px-2 text-left text-[13px] text-[#626260] transition-colors hover:bg-[#F5F1EC] hover:text-[#111111] dark:text-[#B7BEC9] dark:hover:bg-white/5 dark:hover:text-white"
          >
            {item.label}
          </button>
        ))}
      </div>
      {searchQuery ? (
        <>
          <div className="my-2 border-t border-[#EEE8E0] dark:border-[#2A2C31]" />
          <button
            type="button"
            onClick={() => {
              onSearchChange("");
              onClose();
            }}
            className="mx-1.5 flex h-8 w-[calc(100%-12px)] items-center rounded-[8px] px-2 text-left text-[13px] text-[#626260] transition-colors hover:bg-[#F5F1EC] hover:text-[#111111]"
          >
            Clear search
          </button>
        </>
      ) : null}
    </div>
  );
}

function SortMenu({ activeSort, onSelect }: { activeSort: SortKey; onSelect: (sort: SortKey) => void }) {
  return (
    <div className="velion-popover absolute right-0 top-9 z-30 w-[258px] py-2">
      {sortOptions.map((option) => {
        const Icon = option.icon;
        return (
          <button
            key={option.id}
            type="button"
            onClick={() => onSelect(option.id)}
            className="flex h-10 w-full items-center gap-3 px-4 text-left text-[14px] text-[#111111] transition-colors hover:bg-[#F5F1EC] dark:text-white dark:hover:bg-white/5"
          >
            <Icon className="size-4 text-[#626260]" />
            <span className="min-w-0 flex-1">{option.label}</span>
            {activeSort === option.id ? <Check className="size-4 text-[#8A5CF6]" /> : null}
          </button>
        );
      })}
    </div>
  );
}

function LoadingRows() {
  return (
    <div className="space-y-1 p-2">
      {[1, 2, 3].map((item) => (
        <div key={item} className="rounded-[9px] p-3">
          <div className="flex gap-2.5">
            <div className="size-4 shrink-0 animate-pulse rounded-[4px] bg-[#EEE8E0]" />
            <div className="min-w-0 flex-1 space-y-2">
              <div className="h-3 w-2/3 animate-pulse rounded bg-[#EEE8E0]" />
              <div className="h-3 w-full animate-pulse rounded bg-[#EEE8E0]" />
              <div className="h-3 w-1/2 animate-pulse rounded bg-[#EEE8E0]" />
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function QueueEmptyState({ body, title, tone = "neutral" }: { body: string; title: string; tone?: "neutral" | "error" }) {
  return (
    <div className="grid h-full min-h-[280px] place-items-center px-8 text-center">
      <div>
        <div className="mx-auto grid size-11 place-items-center rounded-[12px] border border-[#D8D2C8] bg-[#F8F5F1] text-[#9C9A96]">
          <MessageCircle className="size-5" strokeWidth={1.45} />
        </div>
        <h2 className="mt-3 text-[14px] font-semibold text-[#111111] dark:text-white">{title}</h2>
        <p className={cn("mt-1 max-w-[260px] text-[12px] leading-5", tone === "error" ? "text-[#C41C1C]" : "text-[#7B7B78]")}>{body}</p>
      </div>
    </div>
  );
}

function VirtualTicketList({
  onArchive,
  onMarkRead,
  onSelectTicket,
  onSnooze,
  onTogglePinned,
  onToggleSelected,
  pinnedIds,
  readIds,
  selectedIds,
  selectedTicketId,
  tickets,
}: {
  onArchive: (ticketId: number) => void;
  onMarkRead: (ticketId: number) => void;
  onSelectTicket: (ticket: ZammadTicket) => void;
  onSnooze: (ticketId: number) => void;
  onTogglePinned: (ticketId: number) => void;
  onToggleSelected: (ticketId: number) => void;
  pinnedIds: Set<number>;
  readIds: Set<number>;
  selectedIds: Set<number>;
  selectedTicketId: number | null;
  tickets: ZammadTicket[];
}) {
  "use no memo";

  const listRef = useRef<HTMLDivElement>(null);
  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual owns scroll measurement; this component opts out with "use no memo".
  const ticketVirtualizer = useVirtualizer({
    count: tickets.length,
    estimateSize: () => 112,
    getScrollElement: () => listRef.current,
    initialRect: { height: 520, width: 360 },
    overscan: 8,
  });
  const virtualTickets = ticketVirtualizer.getVirtualItems();
  const renderedTickets = virtualTickets.length > 0
    ? virtualTickets
    : tickets.slice(0, Math.min(tickets.length, 12)).map((ticket, index) => ({
        index,
        key: ticket.id,
        start: index * 112,
      }));
  const totalSize = Math.max(ticketVirtualizer.getTotalSize(), tickets.length * 112);

  return (
    <div ref={listRef} className="h-full min-h-0 overflow-y-auto">
      <ul
        aria-label="Tickets"
        className="relative"
        style={{ height: `${totalSize}px` }}
      >
        {renderedTickets.map((virtualTicket) => {
          const ticket = tickets[virtualTicket.index];
          if (!ticket) {
            return null;
          }

          return (
            <li
              key={virtualTicket.key}
              ref={virtualTickets.length > 0 ? ticketVirtualizer.measureElement : undefined}
              data-index={virtualTicket.index}
              className="absolute left-0 top-0 w-full border-b border-[#F2F2F2] dark:border-[#24262C]"
              style={{ transform: `translateY(${virtualTicket.start}px)` }}
            >
              <TicketRow
                active={selectedTicketId === ticket.id}
                checked={selectedIds.has(ticket.id)}
                pinned={pinnedIds.has(ticket.id)}
                unread={!readIds.has(ticket.id)}
                ticket={ticket}
                onArchive={() => onArchive(ticket.id)}
                onClick={() => {
                  onMarkRead(ticket.id);
                  onSelectTicket(ticket);
                }}
                onSnooze={() => onSnooze(ticket.id)}
                onTogglePinned={() => onTogglePinned(ticket.id)}
                onToggleSelected={() => onToggleSelected(ticket.id)}
              />
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function TicketRow({
  active,
  checked,
  pinned,
  onClick,
  onArchive,
  onSnooze,
  onTogglePinned,
  onToggleSelected,
  ticket,
  unread,
}: {
  active: boolean;
  checked: boolean;
  pinned: boolean;
  onClick: () => void;
  onArchive: () => void;
  onSnooze: () => void;
  onTogglePinned: () => void;
  onToggleSelected: () => void;
  ticket: ZammadTicket;
  unread: boolean;
}) {
  return (
    <div
      className={cn(
        "group relative flex items-start gap-2.5 border-l-[3px] border-transparent p-3 transition-colors",
        active
          ? "border-l-[#2563EB] bg-[#F3F8FF] text-[#1D1D1F] dark:bg-[#202229] dark:text-white"
          : "text-[#303030] hover:bg-[#FAFAFA] dark:text-[#D8DDE6] dark:hover:bg-white/5",
      )}
    >
      <input
        type="checkbox"
        checked={checked}
        onChange={onToggleSelected}
        aria-label={`Select ${ticket.title}`}
        className="mt-0.5 size-4 shrink-0 rounded-[4px] border-[#CDD2DC] accent-[#2563EB]"
      />
      <button
        type="button"
        aria-label={ticket.title}
        aria-pressed={active}
        onClick={onClick}
        className="min-w-0 flex-1 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]"
      >
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-1.5 text-[12px] leading-4 text-[#5F6673]">
            {unread ? <span className="size-1.5 shrink-0 rounded-full bg-[#2563EB]" aria-label="Unread conversation" /> : null}
            <span className={cn("min-w-0 flex-1 truncate", unread ? "font-semibold text-[#30343B]" : "")}>
              {ticket.customer?.email ?? customerName(ticket)}
            </span>
            {pinned ? <Pin className="size-3 shrink-0 fill-[#2563EB] text-[#2563EB]" strokeWidth={1.8} /> : null}
            <Mail className="size-3 shrink-0 fill-[#5F6673] text-[#5F6673]" strokeWidth={1.7} />
            <span className="grid size-4 shrink-0 place-items-center rounded-full bg-[#E3E6EB] text-[#6F7786]">
              <MoreHorizontal className="size-3" strokeWidth={1.8} />
            </span>
            <span className="shrink-0 text-[12px] text-[#6F7786]">{formatRelativeTime(ticket.updated_at)} ago</span>
          </div>
          <div className={cn("mt-1 truncate text-[14px] leading-5 text-[#23262D] dark:text-white", unread ? "font-semibold" : "font-medium")}>
            {ticket.title.startsWith("Re:") ? ticket.title : `Re: ${ticket.title}`}
          </div>
          <div className="line-clamp-2 text-[13px] leading-[18px] text-[#747B88] dark:text-[#AEB4C0]">
            {ticketPreview(ticket)}
          </div>
        </div>
      </button>
      <div className="absolute bottom-2 right-2 flex translate-y-1 items-center gap-0.5 rounded-[8px] border border-[#E4E7EC] bg-white/95 p-0.5 opacity-0 shadow-[0_6px_16px_rgba(16,24,40,0.12)] transition group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:translate-y-0 group-focus-within:opacity-100 dark:border-[#303238] dark:bg-[#17181C]">
        <RowActionButton label={pinned ? "Unpin conversation" : "Pin conversation"} onClick={onTogglePinned}>
          <Pin className={cn("size-3.5", pinned ? "fill-[#2563EB] text-[#2563EB]" : "")} />
        </RowActionButton>
        <RowActionButton label="Snooze conversation" onClick={onSnooze}>
          <Clock3 className="size-3.5" />
        </RowActionButton>
        <RowActionButton label="Archive conversation" onClick={onArchive}>
          <Archive className="size-3.5" />
        </RowActionButton>
      </div>
    </div>
  );
}

function BulkActionButton({
  children,
  disabled,
  label,
  onClick,
}: {
  children: ReactNode;
  disabled?: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      aria-label={label}
      title={label}
      className="grid size-7 place-items-center rounded-[8px] text-[#626260] transition-colors hover:bg-[#F5F1EC] hover:text-[#111111] disabled:pointer-events-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]"
    >
      {children}
    </button>
  );
}

function RowActionButton({ children, label, onClick }: { children: ReactNode; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
      aria-label={label}
      title={label}
      className="grid size-6 place-items-center rounded-[6px] text-[#626A78] transition-colors hover:bg-[#F3F5F8] hover:text-[#1D1D1F] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[#2563EB] dark:text-[#AEB4C0] dark:hover:bg-white/10 dark:hover:text-white"
    >
      {children}
    </button>
  );
}

function FocusLaneButton({ active, count, label, onClick }: { active: boolean; count: number; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1 rounded-[7px] px-2 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]",
        active
          ? "bg-white text-[#1D1D1F] shadow-[0_1px_2px_rgba(16,24,40,0.08)] dark:bg-[#202229] dark:text-white"
          : "text-[#626A78] hover:bg-white/70 hover:text-[#1D1D1F] dark:text-[#AEB4C0] dark:hover:bg-white/10 dark:hover:text-white",
      )}
      aria-pressed={active}
    >
      <span>{label}</span>
      <span className="text-[11px] text-[#8A92A0]">{count}</span>
    </button>
  );
}

function addToSet(values: Set<number>, value: number) {
  if (values.has(value)) {
    return values;
  }

  return new Set([...values, value]);
}

function addManyToSet(values: Set<number>, nextValues: Iterable<number>) {
  const next = new Set(values);
  let changed = false;

  for (const value of nextValues) {
    if (!next.has(value)) {
      next.add(value);
      changed = true;
    }
  }

  return changed ? next : values;
}

function removeFromSet(values: Set<number>, value: number) {
  if (!values.has(value)) {
    return values;
  }

  const next = new Set(values);
  next.delete(value);
  return next;
}

function removeManyFromSet(values: Set<number>, nextValues: Iterable<number>) {
  const next = new Set(values);
  let changed = false;

  for (const value of nextValues) {
    if (next.delete(value)) {
      changed = true;
    }
  }

  return changed ? next : values;
}

function retainSetValues(values: Set<number>, allowedValues: Set<number>) {
  const next = new Set<number>();

  for (const value of values) {
    if (allowedValues.has(value)) {
      next.add(value);
    }
  }

  return next.size === values.size ? values : next;
}

function toggleSetValue(values: Set<number>, value: number) {
  const next = new Set(values);
  if (next.has(value)) {
    next.delete(value);
  } else {
    next.add(value);
  }

  return next;
}

function sortTickets(tickets: ZammadTicket[], sortKey: SortKey, pinnedIds: Set<number>) {
  const sorted = [...tickets];

  return sorted.sort((a, b) => {
    const pinnedDelta = Number(pinnedIds.has(b.id)) - Number(pinnedIds.has(a.id));
    if (pinnedDelta !== 0) return pinnedDelta;
    if (sortKey === "created-desc") return compareDates(b.created_at, a.created_at);
    if (sortKey === "created-asc") return compareDates(a.created_at, b.created_at);
    if (sortKey === "priority-desc") return priorityScore(b) - priorityScore(a);
    if (sortKey === "priority-asc") return priorityScore(a) - priorityScore(b);
    if (sortKey === "last-message-asc") return compareDates(a.updated_at, b.updated_at);
    return compareDates(b.updated_at, a.updated_at);
  });
}

function compareDates(a: string, b: string) {
  return new Date(a).getTime() - new Date(b).getTime();
}

function priorityScore(ticket: ZammadTicket) {
  const value = ticket.priority?.name?.toLowerCase() ?? "";
  if (value.includes("high") || value === "3") return 3;
  if (value.includes("normal") || value === "2") return 2;
  return 1;
}

function isFocusedTicket(ticket: ZammadTicket) {
  const state = ticket.state?.name?.toLowerCase() ?? "";
  const priority = ticket.priority?.name?.toLowerCase() ?? "";
  if (state.includes("spam") || state.includes("closed") || state.includes("solved")) return false;
  return priority.includes("high") || priority.includes("normal") || priority === "2" || priority === "3" || state.includes("open");
}

function isMentionedTicket(ticket: ZammadTicket) {
  const text = `${ticket.title} ${ticket.tags?.join(" ") ?? ""}`.toLowerCase();
  return text.includes("@") || text.includes("mention") || text.includes("urgent") || text.includes("vip");
}

function ticketPreview(ticket: ZammadTicket) {
  if (ticket.tags?.length) {
    return `Regarding your ${ticket.tags.join(", ")} request, we are checking the details and will follow up shortly.`;
  }

  if (ticket.group?.name) {
    return `Could you please confirm the status with ${ticket.group.name} and let me know when I can expect an update?`;
  }

  return "Could you please confirm the status and let me know when I can expect an update?";
}
