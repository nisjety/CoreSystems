"use client";

import { useEffect, useReducer, useState } from "react";
import {
  AlertCircle,
  Bot,
  CalendarDays,
  CheckCircle2,
  ChevronDown,
  Clock3,
  ExternalLink,
  FileText,
  Link2,
  Mail,
  MessageCircle,
  MoreHorizontal,
  Play,
  Plus,
  RefreshCw,
  Search,
  Send,
  Settings,
  Sparkles,
  UserRound,
} from "lucide-react";
import {
  VerevonButton,
  VerevonIconButton,
  VerevonInput,
  VerevonTextarea,
} from "@/components/ui/verevon-ui";
import { customerName, type ZammadTicket } from "@/features/inbox-v2/lib/inbox-model";
import type { InboxModalRequest } from "@/features/inbox-v2/components/InboxWorkModal";
import { formatDateKey } from "@/features/inbox-v2/lib/calendar-format";
import { useCustomerContext, useSupportMacros } from "@/features/inbox-v2/lib/support-client-stores";
import {
  AccordionSection,
  ActivityItem,
  AsideTabButton,
  CalendarEventRow,
  CalendarNoteRow,
  EmptyAsideState,
  FieldRow,
  HealthRow,
  LinkRow,
  MiniCalendarGrid,
  SourceRow,
  type CalendarEvent,
  type CalendarNote,
} from "@/features/inbox-v2/components/InboxAsidePrimitives";
import { apiGet, apiSend } from "@/lib/api/client-envelope";
import { cn } from "@/lib/utils";

type AsideTab = "details" | "verevon" | "calendar" | "activity";

type CalendarState = {
  configured?: boolean;
  events: CalendarEvent[];
  notes: CalendarNote[];
};

const emptyCalendar: CalendarState = {
  events: [],
  notes: [],
};

type VerevonPanelState = {
  ticketId: number | null;
  quickReplies: string[];
  quickLoading: boolean;
  summary: string | null;
  summaryLoading: boolean;
  question: string;
};

type VerevonPanelAction =
  | { type: "ticket-changed"; ticketId: number | null }
  | { type: "question-changed"; value: string }
  | { type: "quick-start"; ticketId: number }
  | { type: "quick-success"; replies: string[]; ticketId: number }
  | { type: "quick-failure"; ticketId: number }
  | { type: "summary-start"; ticketId: number }
  | { type: "summary-success"; summary: string; ticketId: number }
  | { type: "summary-failure"; message: string; ticketId: number };

function createInitialVerevonPanelState(ticketId: number | null): VerevonPanelState {
  return {
    ticketId,
    quickReplies: [],
    quickLoading: false,
    summary: null,
    summaryLoading: false,
    question: "",
  };
}

function verevonPanelReducer(state: VerevonPanelState, action: VerevonPanelAction): VerevonPanelState {
  switch (action.type) {
    case "ticket-changed":
      if (state.ticketId === action.ticketId) {
        return state;
      }

      return createInitialVerevonPanelState(action.ticketId);
    case "question-changed":
      return { ...state, question: action.value };
    case "quick-start":
      return { ...state, ticketId: action.ticketId, quickLoading: true, quickReplies: [] };
    case "quick-success":
      if (state.ticketId !== action.ticketId) {
        return state;
      }

      return { ...state, quickLoading: false, quickReplies: action.replies };
    case "quick-failure":
      if (state.ticketId !== action.ticketId) {
        return state;
      }

      return { ...state, quickLoading: false, quickReplies: [] };
    case "summary-start":
      return { ...state, ticketId: action.ticketId, summaryLoading: true, summary: null };
    case "summary-success":
      if (state.ticketId !== action.ticketId) {
        return state;
      }

      return { ...state, summary: action.summary, summaryLoading: false };
    case "summary-failure":
      if (state.ticketId !== action.ticketId) {
        return state;
      }

      return { ...state, summary: action.message, summaryLoading: false };
    default:
      return state;
  }
}

type CalendarPanelState = {
  calendar: CalendarState;
  selectedDate: Date;
  eventTitle: string;
  noteText: string;
  loading: boolean;
  now: number;
  saving: boolean;
  error: string | null;
};

type CalendarPanelAction =
  | { type: "calendar-loaded"; calendar: CalendarState }
  | { type: "calendar-failed"; error: string }
  | { type: "date-selected"; date: Date }
  | { type: "event-title-changed"; value: string }
  | { type: "note-text-changed"; value: string }
  | { type: "save-start" }
  | { type: "event-saved"; event: CalendarEvent }
  | { type: "note-saved"; note: CalendarNote }
  | { type: "save-failed"; error: string };

function createInitialCalendarPanelState(): CalendarPanelState {
  return {
    calendar: emptyCalendar,
    selectedDate: new Date(),
    eventTitle: "",
    noteText: "",
    loading: true,
    now: Date.now(),
    saving: false,
    error: null,
  };
}

function calendarPanelReducer(state: CalendarPanelState, action: CalendarPanelAction): CalendarPanelState {
  switch (action.type) {
    case "calendar-loaded":
      return {
        ...state,
        calendar: { ...emptyCalendar, ...action.calendar },
        error: null,
        loading: false,
      };
    case "calendar-failed":
      return { ...state, error: action.error, loading: false };
    case "date-selected":
      return { ...state, selectedDate: action.date };
    case "event-title-changed":
      return { ...state, eventTitle: action.value };
    case "note-text-changed":
      return { ...state, noteText: action.value };
    case "save-start":
      return { ...state, saving: true };
    case "event-saved":
      return {
        ...state,
        calendar: { ...state.calendar, events: [action.event, ...state.calendar.events] },
        eventTitle: "",
        error: null,
        saving: false,
      };
    case "note-saved":
      return {
        ...state,
        calendar: { ...state.calendar, notes: [action.note, ...state.calendar.notes] },
        noteText: "",
        error: null,
        saving: false,
      };
    case "save-failed":
      return { ...state, error: action.error, saving: false };
    default:
      return state;
  }
}

export function InboxAside({
  onInsertQuickReply,
  onMacroExecuted,
  onOpenModal,
  selectedTicket,
}: {
  onInsertQuickReply: (text: string) => void;
  onMacroExecuted: () => void;
  onOpenModal: (modal: InboxModalRequest) => void;
  selectedTicket: ZammadTicket | null;
}) {
  const [activeTab, setActiveTab] = useState<AsideTab>("details");

  return (
    <aside
      aria-label="AI and customer context"
      className="verevon-sidebar-type hidden h-full min-h-0 overflow-hidden rounded-[16px] border border-[#D8D2C8] bg-white text-[#111111] lg:flex lg:flex-col dark:border-[#2A2C31] dark:bg-[#101114] dark:text-white"
    >
      <div className="flex h-[64px] shrink-0 items-center justify-between border-b border-[#E7E1D8] px-4 dark:border-[#2A2C31]">
        <div className="flex h-full min-w-0 items-end gap-4 overflow-x-auto">
          {[
            { id: "details", label: "Details" },
            { id: "verevon", label: "Verevon" },
            { id: "calendar", label: "Calendar" },
            { id: "activity", label: "Activity" },
          ].map((tab) => (
            <AsideTabButton
              key={tab.id}
              active={activeTab === tab.id}
              onClick={() => setActiveTab(tab.id as AsideTab)}
            >
              {tab.label}
            </AsideTabButton>
          ))}
        </div>
        <VerevonIconButton
          onClick={() => onOpenModal({
            type: "work",
            title: "Inbox side panel",
            description: "Configure which AI tools, customer systems, calendar resources, and activity streams appear in this right rail.",
            primaryAction: "Save panel",
          })}
          aria-label="Open side panel settings"
          title="Open side panel settings"
        >
          <Settings className="size-4" />
        </VerevonIconButton>
      </div>

      {activeTab === "details" ? <DetailsPanel onOpenModal={onOpenModal} selectedTicket={selectedTicket} /> : null}
      {activeTab === "verevon" ? (
        <VerevonPanel
          onInsertQuickReply={onInsertQuickReply}
          onMacroExecuted={onMacroExecuted}
          onOpenModal={onOpenModal}
          selectedTicket={selectedTicket}
        />
      ) : null}
      {activeTab === "calendar" ? <CalendarPanel selectedTicket={selectedTicket} /> : null}
      {activeTab === "activity" ? <ActivityPanel selectedTicket={selectedTicket} /> : null}
    </aside>
  );
}

function DetailsPanel({
  onOpenModal,
  selectedTicket,
}: {
  onOpenModal: (modal: InboxModalRequest) => void;
  selectedTicket: ZammadTicket | null;
}) {
  const context = useCustomerContext(selectedTicket?.customer?.id, selectedTicket?.group?.id);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="border-b border-[#E7E1D8] p-4 dark:border-[#2A2C31]">
        <div className="relative">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-[#9C9A96]" />
          <VerevonInput
            type="search"
            aria-label="Search customer context"
            placeholder="Search customers by email, order, or phone"
            variant="compact"
            className="bg-[#FAF8F5] pl-9 pr-3 dark:bg-[#17181C]"
          />
        </div>
      </div>

      {!selectedTicket ? (
        <EmptyAsideState
          icon={<UserRound className="size-6" />}
          title="No customer selected"
          body="Open a ticket to see customer fields, links, user data, and recent conversations."
        />
      ) : (
        <>
          <section className="border-b border-[#E7E1D8] px-4 py-5 dark:border-[#2A2C31]">
            <div className="flex items-start gap-3">
              <div className="grid size-10 place-items-center rounded-full bg-[#F0ECE6] text-[13px] font-semibold text-[#626260]">
                {selectedTicket.customer?.firstname?.[0] ?? "?"}
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <h2 className="truncate text-[16px] font-semibold">{selectedTicket.customer?.email ?? customerName(selectedTicket)}</h2>
                  <VerevonIconButton
                    onClick={() => onOpenModal({
                      type: "work",
                      title: "Customer actions",
                      description: "Edit customer profile fields, add notes, link orders, and let Verevon run customer-context tools in this modal.",
                      primaryAction: "Save customer action",
                    })}
                    aria-label="Customer actions"
                    className="ml-auto"
                  >
                    <MoreHorizontal className="size-4" />
                  </VerevonIconButton>
                </div>
                <p className="mt-1 text-[13px] text-[#626260]">{customerName(selectedTicket)}</p>
              </div>
            </div>

            <div className="mt-4 space-y-2 text-[13px]">
              <FieldRow label="Assignee" value={selectedTicket.owner ? `${selectedTicket.owner.firstname} ${selectedTicket.owner.lastname}` : "Unassigned"} />
              <FieldRow label="Team inbox" value={selectedTicket.group?.name ?? "Support"} />
              <FieldRow label="Customer type" value="+ Add" muted />
            </div>
          </section>

          <AccordionSection defaultOpen icon={<Link2 className="size-4" />} title="Links">
            <LinkRow label="Tracker ticket" onOpenModal={onOpenModal} />
            <LinkRow label="Back-office tickets" onOpenModal={onOpenModal} />
            <LinkRow label="Side conversations" onOpenModal={onOpenModal} />
          </AccordionSection>

          <AccordionSection defaultOpen icon={<FileText className="size-4" />} title="Conversation attributes">
            <FieldRow label="ID" value={String(selectedTicket.id)} />
            <FieldRow label="Company" value="No company" muted />
            <FieldRow label="Brand" value="Verevon" />
            <FieldRow label="Subject" value={selectedTicket.title} />
          </AccordionSection>

          <section className="border-b border-[#E7E1D8] p-4 dark:border-[#2A2C31]">
            <div className="mb-3 flex items-center justify-between gap-3">
              <h3 className="text-[14px] font-semibold">Commerce context</h3>
              <VerevonIconButton
                onClick={() => onOpenModal({
                  type: "work",
                  title: "Commerce context",
                  description: "Inspect orders, refunds, subscriptions, shipment state, and linked support evidence inside the inbox.",
                  primaryAction: "Open commerce tools",
                })}
                aria-label="Open commerce context"
                size="xs"
                radius="sm"
                className="text-[#9C9A96]"
              >
                <ExternalLink className="size-4" />
              </VerevonIconButton>
            </div>
            {context?.shopify?.orders?.length ? (
              <div className="space-y-2">
                {context.shopify.orders.slice(0, 3).map((order) => (
                  <div key={order.id} className="rounded-[9px] border border-[#E1DAD1] bg-[#FAF8F5] p-2.5 text-[12px]">
                    <div className="flex justify-between gap-2">
                      <span className="font-semibold">{order.name ?? `#${order.order_number ?? order.id}`}</span>
                      <span className="text-[#946200]">{order.fulfillment_status ?? "Unfulfilled"}</span>
                    </div>
                    <p className="mt-1 text-[#7B7B78]">{order.created_at ?? ""}{order.total_price ? ` - $${order.total_price}` : ""}</p>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-[13px] text-[#7B7B78]">No Shopify context connected.</p>
            )}
          </section>

          <AccordionSection icon={<UserRound className="size-4" />} title="User data" />
          <AccordionSection icon={<MessageCircle className="size-4" />} title="Recent conversations" />
          <AccordionSection icon={<Mail className="size-4" />} title="User notes" />
          <AccordionSection icon={<Sparkles className="size-4" />} title="User tags" />

          <section className="p-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <h3 className="text-[14px] font-semibold">Stripe</h3>
              <VerevonIconButton
                onClick={() => onOpenModal({
                  type: "work",
                  title: "Stripe context",
                  description: "Review billing state, subscription actions, and payment evidence without leaving the ticket.",
                  primaryAction: "Open billing tools",
                })}
                aria-label="Open Stripe context"
                size="xs"
                radius="sm"
                className="text-[#9C9A96]"
              >
                <ExternalLink className="size-4" />
              </VerevonIconButton>
            </div>
            {context?.stripe ? (
              <div className="space-y-2 rounded-[9px] border border-[#E1DAD1] bg-[#FAF8F5] p-2.5 text-[12px]">
                <FieldRow label="Customer" value={context.stripe.customer?.email ?? context.stripe.customer?.id ?? "-"} />
                {context.stripe.subscription ? <FieldRow label="Subscription" value={context.stripe.subscription.status ?? "Unknown"} /> : null}
                {context.stripe.subscription?.plan ? <FieldRow label="Plan" value={context.stripe.subscription.plan} /> : null}
              </div>
            ) : (
              <p className="text-[13px] text-[#7B7B78]">No Stripe context connected.</p>
            )}
          </section>
        </>
      )}
    </div>
  );
}

function VerevonPanel({
  onInsertQuickReply,
  onMacroExecuted,
  onOpenModal,
  selectedTicket,
}: {
  onInsertQuickReply: (text: string) => void;
  onMacroExecuted: () => void;
  onOpenModal: (modal: InboxModalRequest) => void;
  selectedTicket: ZammadTicket | null;
}) {
  const [panelState, dispatchPanelAction] = useReducer(
    verevonPanelReducer,
    selectedTicket?.id ?? null,
    createInitialVerevonPanelState,
  );

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      dispatchPanelAction({ type: "ticket-changed", ticketId: selectedTicket?.id ?? null });
    });

    return () => window.cancelAnimationFrame(frame);
  }, [selectedTicket?.id]);

  async function generateQuickReplies() {
    if (!selectedTicket || panelState.quickLoading) return;

    const ticketId = selectedTicket.id;
    dispatchPanelAction({ type: "quick-start", ticketId });

    try {
      const response = await fetch(`/api/support/tickets/${ticketId}/quick-replies`, { method: "POST" });
      if (!response.ok) {
        dispatchPanelAction({ type: "quick-failure", ticketId });
        return;
      }
      const payload = (await response.json()) as { options?: string[] };
      dispatchPanelAction({ type: "quick-success", replies: payload.options ?? [], ticketId });
    } catch {
      dispatchPanelAction({ type: "quick-failure", ticketId });
    }
  }

  async function generateSummary() {
    if (!selectedTicket || panelState.summaryLoading) return;

    const ticketId = selectedTicket.id;
    dispatchPanelAction({ type: "summary-start", ticketId });

    try {
      const response = await fetch(`/api/support/tickets/${ticketId}/summarize`, { method: "POST" });
      if (!response.ok) {
        dispatchPanelAction({ type: "summary-failure", message: "Unable to generate summary.", ticketId });
        return;
      }
      const payload = (await response.json()) as { summary?: string };
      dispatchPanelAction({ type: "summary-success", summary: payload.summary ?? "No summary returned.", ticketId });
    } catch {
      dispatchPanelAction({ type: "summary-failure", message: "Unable to generate summary.", ticketId });
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {!selectedTicket ? (
          <EmptyAsideState
            icon={<Bot className="size-6" />}
            title="Select a ticket"
            body="Verevon can draft replies, summarize context, and surface relevant sources once a conversation is open."
          />
        ) : (
          <div className="space-y-4">
            <section className="verevon-aside-card p-4">
              <div className="mb-3 flex items-center gap-2">
                <Bot className="size-4 text-[#DD7A1F]" />
                <h2 className="text-[14px] font-semibold">Verevon action plan</h2>
              </div>
              <div className="space-y-2">
                <ActionSuggestion
                  title="Confirm intent"
                  body="Customer is asking for resolution timing and next step clarity."
                  actionLabel="Run"
                  onRun={() => onOpenModal({ type: "verevon", prompt: "Confirm customer intent, draft the next reply, and add a private action note." })}
                />
                <ActionSuggestion
                  title="Use source-backed reply"
                  body="Insert policy excerpts only when a connected source supports the answer."
                  actionLabel="Run"
                  onRun={() => onOpenModal({ type: "verevon", prompt: "Draft a source-backed reply and keep the evidence in the audit stream." })}
                />
                <ActionSuggestion
                  title="Route if overdue"
                  body="If SLA risk is high, assign to the owning support queue before replying."
                  actionLabel="Run"
                  onRun={() => onOpenModal({ type: "verevon", prompt: "Check SLA risk, raise priority if needed, and route this conversation to the right queue." })}
                />
              </div>
            </section>

            <section className="verevon-aside-card verevon-aside-card-soft p-4">
              <div className="mb-3 flex items-center justify-between gap-3">
                <div className="flex items-center gap-2">
                  <Sparkles className="size-4 text-[#DD7A1F]" />
                  <h2 className="text-[14px] font-semibold">Reply assistance</h2>
                </div>
                <button
                  type="button"
                  disabled={panelState.quickLoading}
                  onClick={generateQuickReplies}
                  className="flex items-center gap-1.5 text-[12px] font-medium text-[#626260] disabled:opacity-60"
                >
                  <RefreshCw className={cn("size-3.5", panelState.quickLoading ? "animate-spin" : "")} />
                  Generate
                </button>
              </div>
              {panelState.quickLoading ? (
                <div className="space-y-2">
                  {[1, 2].map((item) => <div key={item} className="h-12 animate-pulse rounded-[9px] bg-[#E7E1D8]" />)}
                </div>
              ) : panelState.quickReplies.length ? (
                <div className="space-y-2">
                  {panelState.quickReplies.map((reply) => (
                    <button
                      key={reply}
                      type="button"
                      onClick={() => onInsertQuickReply(reply)}
                      className="w-full rounded-[9px] border border-[#E1DAD1] bg-white px-3 py-2 text-left text-[12px] leading-5 text-[#3F3A35] transition-colors hover:border-[#DD7A1F]/40 hover:bg-[#FFF9F2]"
                    >
                      {reply}
                    </button>
                  ))}
                </div>
              ) : (
                <p className="text-[13px] leading-5 text-[#626260]">Generate quick reply options for this conversation.</p>
              )}
            </section>

            <section className="verevon-aside-card p-4">
              <div className="mb-3 flex items-center justify-between">
                <h2 className="text-[14px] font-semibold">Conversation summary</h2>
                <button type="button" onClick={generateSummary} className="text-[12px] font-medium text-[#006ADC]">
                  Summarize
                </button>
              </div>
              <p className="text-[13px] leading-5 text-[#626260]">
                {panelState.summaryLoading ? "Generating summary…" : panelState.summary ?? "Ask Verevon to summarize the conversation and extract the customer intent."}
              </p>
            </section>

            <section className="verevon-aside-card p-4">
              <h2 className="mb-3 text-[14px] font-semibold">Relevant sources</h2>
              <div className="space-y-2 text-[13px] text-[#626260]">
                <SourceRow title="Refund policy" />
                <SourceRow title="Shipping and SLA runbook" />
                <SourceRow title="Macro: polite follow-up" />
              </div>
            </section>

            <MacrosPanel onMacroExecuted={onMacroExecuted} selectedTicket={selectedTicket} />
          </div>
        )}
      </div>

      <div className="shrink-0 border-t border-[#E7E1D8] p-4 dark:border-[#2A2C31]">
        <div className="flex h-10 items-center rounded-[10px] bg-[linear-gradient(90deg,#EE7A50,#F6AF6E,#DD7A1F)] p-px">
          <div className="flex h-full min-w-0 flex-1 items-center gap-2 rounded-[9px] bg-white px-3 dark:bg-[#101114]">
            <input
              value={panelState.question}
              onChange={(event) => dispatchPanelAction({ type: "question-changed", value: event.target.value })}
              placeholder="Ask Verevon"
              className="min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-[#9C9A96]"
              aria-label="Ask Verevon a question"
            />
            <VerevonIconButton
              onClick={() => onOpenModal({ type: "verevon", prompt: panelState.question })}
              aria-label="Send Verevon question"
              size="xs"
              radius="pill"
              className="bg-[#111111] text-white hover:bg-[#2A2A2A] hover:text-white"
            >
              <Send className="size-3.5" />
            </VerevonIconButton>
          </div>
        </div>
      </div>
    </div>
  );
}

function CalendarPanel({ selectedTicket }: { selectedTicket: ZammadTicket | null }) {
  const [calendarState, dispatchCalendarAction] = useReducer(
    calendarPanelReducer,
    undefined,
    createInitialCalendarPanelState,
  );
  const { calendar, error, eventTitle, loading, noteText, now, saving, selectedDate } = calendarState;
  const selectedKey = formatDateKey(selectedDate);

  const selectedEvents = calendar.events.filter((event) => formatDateKey(new Date(event.start)) === selectedKey);
  const selectedNotes = calendar.notes.filter((note) => note.date === selectedKey);
  const upcomingEvents = [...calendar.events]
    .filter((event) => new Date(event.end).getTime() >= now)
    .sort((a, b) => new Date(a.start).getTime() - new Date(b.start).getTime())
    .slice(0, 4);

  useEffect(() => {
    let cancelled = false;

    // Backend handoff: this intentionally uses the same navbar calendar endpoint
    // so inbox follow-ups and navbar calendar stay on one user-core calendar model.
    apiGet<CalendarState>("/api/v1/navbar/calendar")
      .then((state) => {
        if (!cancelled) {
          dispatchCalendarAction({ type: "calendar-loaded", calendar: state });
        }
      })
      .catch((loadError) => {
        if (!cancelled) {
          dispatchCalendarAction({
            type: "calendar-failed",
            error: loadError instanceof Error ? loadError.message : "Calendar could not be loaded.",
          });
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  async function saveFollowUp() {
    if (saving) return;

    const title = (eventTitle.trim() || (selectedTicket ? `Follow up: ${selectedTicket.title}` : "Inbox follow-up")).slice(0, 160);
    const start = new Date(selectedDate);
    start.setHours(9, 0, 0, 0);
    const end = new Date(start);
    end.setMinutes(end.getMinutes() + 30);

    dispatchCalendarAction({ type: "save-start" });
    try {
      const saved = await apiSend<{ event: CalendarEvent }>("/api/v1/navbar/calendar", {
        title,
        start: start.toISOString(),
        end: end.toISOString(),
        type: "inbox-follow-up",
      });
      dispatchCalendarAction({ type: "event-saved", event: saved.event });
    } catch (saveError) {
      dispatchCalendarAction({
        type: "save-failed",
        error: saveError instanceof Error ? saveError.message : "Follow-up could not be saved.",
      });
    }
  }

  async function saveNote() {
    const text = noteText.trim();
    if (!text || saving) return;

    dispatchCalendarAction({ type: "save-start" });
    try {
      const saved = await apiSend<{ note: CalendarNote }>("/api/v1/navbar/calendar", {
        kind: "note",
        text: selectedTicket ? `#${selectedTicket.number}: ${text}` : text,
        date: selectedKey,
      });
      dispatchCalendarAction({ type: "note-saved", note: saved.note });
    } catch (saveError) {
      dispatchCalendarAction({
        type: "save-failed",
        error: saveError instanceof Error ? saveError.message : "Note could not be saved.",
      });
    }
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <h2 className="text-[14px] font-semibold">Inbox calendar</h2>
          <p className="mt-1 text-[12px] text-[#7B7B78]">Follow-ups, notes, and scheduled support work.</p>
        </div>
        <CalendarDays className="size-5 text-[#DD7A1F]" />
      </div>

      <MiniCalendarGrid
        events={calendar.events}
        selectedDate={selectedDate}
        onSelect={(date) => dispatchCalendarAction({ type: "date-selected", date })}
      />

      <section className="verevon-aside-card mt-4 p-3">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-[13px] font-semibold">
            {selectedDate.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" })}
          </h3>
          {loading ? <span className="text-[11px] text-[#9C9A96]">Loading…</span> : null}
        </div>
        <div className="max-h-[138px] overflow-y-auto">
          {!selectedEvents.length ? <p className="py-3 text-center text-[12px] text-[#9C9A96]">No events for this day.</p> : null}
          {selectedEvents.map((event) => <CalendarEventRow key={event.id} event={event} />)}
          {selectedNotes.map((note) => <CalendarNoteRow key={note.id} note={note} />)}
        </div>
      </section>

      <section className="verevon-aside-card verevon-aside-card-muted mt-4 p-3">
        <h3 className="text-[13px] font-semibold">Schedule follow-up</h3>
        {/* Backend handoff: when the calendar service supports linked resources, include the ticket id and channel in the event metadata. */}
        <p className="mt-1 text-[12px] leading-5 text-[#7B7B78]">
          Create a calendar item from the selected ticket.
        </p>
        <div className="mt-3 flex items-center gap-2 rounded-[9px] bg-white px-2 py-1.5">
          <Plus className="size-4 text-[#626260]" />
          <input
            value={eventTitle}
            onChange={(event) => dispatchCalendarAction({ type: "event-title-changed", value: event.target.value })}
            onKeyDown={(event) => {
              if (event.key === "Enter") void saveFollowUp();
            }}
            placeholder={selectedTicket ? `Follow up: ${selectedTicket.title}` : "Follow-up title"}
            aria-label="Follow-up title"
            className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-[#9C9A96]"
          />
          <VerevonButton variant="primary" size="xs" radius="sm" disabled={saving} onClick={() => void saveFollowUp()} className="px-2 font-semibold disabled:opacity-50">
            Add
          </VerevonButton>
        </div>
      </section>

      <section className="verevon-aside-card mt-4 p-3">
        <h3 className="text-[13px] font-semibold">Calendar note</h3>
        <VerevonTextarea
          rows={3}
          value={noteText}
          onChange={(event) => dispatchCalendarAction({ type: "note-text-changed", value: event.target.value })}
          placeholder="Add a private follow-up note…"
          aria-label="Private follow-up note"
          variant="compact"
          className="verevon-textarea-sm mt-2 resize-none bg-[#FAF8F5] text-[12px]"
        />
        <VerevonButton variant="primary" size="xs" radius="sm" disabled={!noteText.trim() || saving} onClick={() => void saveNote()} className="mt-2 px-3 text-[12px] font-semibold disabled:opacity-40">
          Save note
        </VerevonButton>
      </section>

      <section className="verevon-aside-card mt-4 p-3">
        <h3 className="mb-2 text-[13px] font-semibold">Upcoming</h3>
        {!upcomingEvents.length ? <p className="text-[12px] text-[#7B7B78]">No upcoming calendar events.</p> : null}
        {upcomingEvents.map((event) => <CalendarEventRow key={event.id} event={event} />)}
      </section>

      {error ? <p className="mt-3 rounded-[9px] bg-[#FFF4F2] px-3 py-2 text-[12px] text-[#B42318]">{error}</p> : null}
    </div>
  );
}

function ActivityPanel({ selectedTicket }: { selectedTicket: ZammadTicket | null }) {
  if (!selectedTicket) {
    return (
      <EmptyAsideState
        icon={<Clock3 className="size-6" />}
        title="No activity selected"
        body="Open a ticket to see workflow health, collaboration state, and follow-up automation."
      />
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      <section className="rounded-[12px] border border-[#E1DAD1] bg-white p-4">
        <div className="mb-3 flex items-center gap-2">
          <CheckCircle2 className="size-4 text-[#12B76A]" />
          <h2 className="text-[14px] font-semibold">Workflow health</h2>
        </div>
        <div className="space-y-2">
          <HealthRow label="SLA status" value="On track" tone="success" />
          <HealthRow label="Ownership" value={selectedTicket.owner ? `${selectedTicket.owner.firstname} ${selectedTicket.owner.lastname}` : "Needs owner"} tone={selectedTicket.owner ? "neutral" : "warning"} />
          <HealthRow label="Queue" value={selectedTicket.group?.name ?? "Support"} tone="neutral" />
        </div>
      </section>

      <section className="mt-4 rounded-[12px] border border-[#E1DAD1] bg-white p-4">
        <div className="mb-3 flex items-center gap-2">
          <UserRound className="size-4 text-[#626260]" />
          <h2 className="text-[14px] font-semibold">Team collaboration</h2>
        </div>
        <div className="space-y-3">
          <ActivityItem title="No teammate is drafting" body="Show collision state here when another agent is viewing or replying." />
          <ActivityItem title="Internal comments" body="Add Front-style internal thread notes without changing the customer conversation." />
          <ActivityItem title="Subscribe teammate" body="Notify a teammate when customer replies or SLA changes." />
        </div>
      </section>

      <section className="mt-4 rounded-[12px] border border-[#E1DAD1] bg-[#FAF8F5] p-4">
        {/* Backend handoff: replace these placeholder rows with support-worker timeline events, lock state, audit events, reminders, and automation rule endpoints. */}
        <div className="mb-3 flex items-center gap-2">
          <AlertCircle className="size-4 text-[#DD7A1F]" />
          <h2 className="text-[14px] font-semibold">Automation hooks</h2>
        </div>
        <div className="space-y-2">
          <ActionSuggestion title="Create split rule" body="Move this sender, domain, or topic to Focused or Other." />
          <ActionSuggestion title="Auto reminder" body="Return this conversation when the follow-up date arrives." />
          <ActionSuggestion title="SLA escalation" body="Escalate when waiting time or priority exceeds policy." />
        </div>
      </section>
    </div>
  );
}

function MacrosPanel({ onMacroExecuted, selectedTicket }: { onMacroExecuted: () => void; selectedTicket: ZammadTicket | null }) {
  const [expanded, setExpanded] = useState(false);
  const [runningMacroId, setRunningMacroId] = useState<number | null>(null);
  const macros = useSupportMacros();

  async function runMacro(macroId: number) {
    if (!selectedTicket || runningMacroId !== null) return;
    setRunningMacroId(macroId);
    try {
      const response = await fetch(`/api/support/macros/${macroId}/execute`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ticketId: selectedTicket.id }),
      });
      if (response.ok) onMacroExecuted();
    } catch {
      // Macro execution is surfaced through unchanged button state and ticket refresh.
    }

    setRunningMacroId(null);
  }

  return (
    <section className="overflow-hidden rounded-[12px] border border-[#E1DAD1] bg-white">
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        className="flex h-11 w-full items-center justify-between px-4 text-left transition-colors hover:bg-[#FAF8F5]"
        aria-expanded={expanded}
      >
        <span className="truncate text-[14px] font-semibold">Macros</span>
        <ChevronDown className={cn("size-4 shrink-0 text-[#7B7B78] transition-transform", expanded ? "rotate-180" : "")} />
      </button>
      {expanded ? (
        <ul className="divide-y divide-[#EEE8E0] border-t border-[#E7E1D8]">
          {macros.length ? macros.map((macro) => (
            <li key={macro.id} className="flex items-center justify-between gap-2 px-4 py-3">
              <span className="truncate text-[13px] text-[#626260]">{macro.name}</span>
              <VerevonButton
                disabled={!selectedTicket || runningMacroId === macro.id}
                onClick={() => void runMacro(macro.id)}
                size="xs"
                radius="sm"
                className="h-7 gap-1 px-2 py-1 text-[11px] disabled:opacity-40"
              >
                <Play className={cn("size-3", runningMacroId === macro.id ? "animate-pulse" : "")} />
                {runningMacroId === macro.id ? "Running" : "Run"}
              </VerevonButton>
            </li>
          )) : (
            <li className="px-4 py-3 text-[12px] text-[#7B7B78]">No macros available.</li>
          )}
        </ul>
      ) : null}
    </section>
  );
}

function ActionSuggestion({
  actionLabel,
  body,
  onRun,
  title,
}: {
  actionLabel?: string;
  body: string;
  onRun?: () => void;
  title: string;
}) {
  return (
    <div className="rounded-[9px] border border-[#E7E1D8] bg-white px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <p className="text-[12px] font-semibold text-[#111111]">{title}</p>
        {onRun ? (
          <VerevonButton
            onClick={onRun}
            variant="primary"
            size="xs"
            radius="sm"
            className="h-7 shrink-0 px-2 py-1 text-[11px] font-semibold"
          >
            {actionLabel ?? "Open"}
          </VerevonButton>
        ) : null}
      </div>
      <p className="mt-1 text-[12px] leading-5 text-[#626260]">{body}</p>
    </div>
  );
}
