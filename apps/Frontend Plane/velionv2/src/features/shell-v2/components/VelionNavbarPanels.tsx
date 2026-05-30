"use client";

import Link from "next/link";
import type { Route } from "next";
import { useReducer } from "react";
import {
  Bell,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  CreditCard,
  HelpCircle,
  LogOut,
  Mic,
  Plus,
  Settings,
  User,
  Users,
  Zap,
  type LucideIcon,
} from "lucide-react";
import {
  createNavbarCalendarEvent,
  createNavbarCalendarNote,
  type CalendarEvent,
  type CalendarState,
  type NavbarNotification,
  type NavbarProfile,
} from "@/features/shell-v2/lib/navbar-data";
import type { VelionRoute } from "@/features/shell-v2/lib/shell-data";
import { cn } from "@/lib/utils";
import { useClientTodayKey } from "@/lib/use-client-today";

export function MessagesDropdown({
  configured,
  messages,
  onOpen,
}: {
  configured: boolean;
  messages: NavbarNotification[];
  onOpen: (id: string) => void;
}) {
  return (
    <Panel className="right-24 velion-floating-panel-md">
      <TabHeader tabs={["All", "Messages", "Mentions"]} />
      <div className="max-h-[380px] overflow-y-auto">
        {!configured ? <EmptyPanel text="Connect Novu to show inbox and Velion AI chat messages." /> : null}
        {configured && messages.length === 0 ? <EmptyPanel text="No messages" /> : null}
        {messages.map((message, index) => (
          <MessageRow key={message.id} item={message} bordered={index < messages.length - 1} onOpen={onOpen} />
        ))}
      </div>
      <PanelFooter href="/inbox" label="View all messages" />
    </Panel>
  );
}

export function NotificationsDropdown({
  configured,
  notifications,
  onOpen,
}: {
  configured: boolean;
  notifications: NavbarNotification[];
  onOpen: (id: string) => void;
}) {
  return (
    <Panel className="right-14 velion-floating-panel-md">
      <TabHeader tabs={["All", "Systems", "Unread"]} />
      <div className="max-h-[380px] overflow-y-auto">
        {!configured ? <EmptyPanel text="Connect Novu to show real notifications." /> : null}
        {configured && notifications.length === 0 ? <EmptyPanel text="No notifications" /> : null}
        {notifications.map((notification, index) => (
          <NotificationRow key={notification.id} item={notification} bordered={index < notifications.length - 1} onOpen={onOpen} />
        ))}
      </div>
    </Panel>
  );
}

type CalendarPanelTab = "calendar" | "notes";

type CalendarPanelState = {
  activeTab: CalendarPanelTab;
  error: string | null;
  eventTitle: string;
  expanded: boolean;
  noteText: string;
  selectedDate: Date;
};

type CalendarPanelAction =
  | { type: "event-saved" }
  | { type: "failed"; message: string }
  | { type: "note-saved" }
  | { type: "select-date"; selectedDate: Date }
  | { type: "set-event-title"; eventTitle: string }
  | { type: "set-note-text"; noteText: string }
  | { type: "set-tab"; activeTab: CalendarPanelTab }
  | { type: "toggle-expanded" };

function createInitialCalendarPanelState(): CalendarPanelState {
  return {
    activeTab: "calendar",
    error: null,
    eventTitle: "",
    expanded: false,
    noteText: "",
    selectedDate: new Date(),
  };
}

function calendarPanelReducer(state: CalendarPanelState, action: CalendarPanelAction): CalendarPanelState {
  switch (action.type) {
    case "event-saved":
      return {
        ...state,
        error: null,
        eventTitle: "",
      };
    case "failed":
      return {
        ...state,
        error: action.message,
      };
    case "note-saved":
      return {
        ...state,
        error: null,
        noteText: "",
      };
    case "select-date":
      return {
        ...state,
        selectedDate: action.selectedDate,
      };
    case "set-event-title":
      return {
        ...state,
        eventTitle: action.eventTitle,
      };
    case "set-note-text":
      return {
        ...state,
        noteText: action.noteText,
      };
    case "set-tab":
      return state.activeTab === action.activeTab ? state : { ...state, activeTab: action.activeTab };
    case "toggle-expanded":
      return {
        ...state,
        expanded: !state.expanded,
      };
  }
}

export function CalendarDropdown({
  state,
  onStateChange,
  onSaved,
}: {
  state: CalendarState;
  onStateChange: (state: CalendarState) => void;
  onSaved: (message: string) => void;
}) {
  const [calendarPanel, dispatchCalendarPanel] = useReducer(calendarPanelReducer, undefined, createInitialCalendarPanelState);
  const { activeTab, error, eventTitle, expanded, noteText, selectedDate } = calendarPanel;
  const selectedKey = formatDateKey(selectedDate);
  const selectedEvents = state.events.filter((event) => formatDateKey(new Date(event.start)) === selectedKey);
  const selectedNotes = state.notes.filter((note) => note.date === selectedKey);

  const saveEvent = async () => {
    const title = eventTitle.trim();
    if (!title) {
      return;
    }

    try {
      const start = new Date(selectedDate);
      start.setHours(9, 0, 0, 0);
      const end = new Date(start);
      end.setHours(9, 30, 0, 0);
      const saved = await createNavbarCalendarEvent({
        title,
        start: start.toISOString(),
        end: end.toISOString(),
        type: "event",
      });
      onStateChange({ ...state, events: [saved.event, ...state.events] });
      dispatchCalendarPanel({ type: "event-saved" });
      onSaved("Calendar event saved to user-core.");
    } catch (saveError) {
      dispatchCalendarPanel({
        type: "failed",
        message: saveError instanceof Error ? saveError.message : "Calendar event could not be saved.",
      });
    }
  };

  const saveNote = async () => {
    const text = noteText.trim();
    if (!text) {
      return;
    }

    try {
      const saved = await createNavbarCalendarNote({
        kind: "note",
        text,
        date: selectedKey,
      });
      onStateChange({ ...state, notes: [saved.note, ...state.notes] });
      dispatchCalendarPanel({ type: "note-saved" });
      onSaved("Calendar note saved to user-core.");
    } catch (saveError) {
      dispatchCalendarPanel({
        type: "failed",
        message: saveError instanceof Error ? saveError.message : "Calendar note could not be saved.",
      });
    }
  };

  return (
    <Panel className="right-6 velion-floating-panel-md">
      <div className="flex items-center gap-1 px-4 pb-2 pt-3">
        {(["calendar", "notes"] as const).map((tab) => (
          <button
            key={tab}
            type="button"
            onClick={() => dispatchCalendarPanel({ type: "set-tab", activeTab: tab })}
            className={cn(
              "flex-1 rounded-lg py-1.5 text-xs font-semibold capitalize transition-all",
              activeTab === tab ? "bg-[#F2F2F2] text-[#1C1C1E] dark:bg-[#23252A] dark:text-white" : "text-[#AAAAAA] hover:text-[#555555] dark:hover:text-white",
            )}
          >
            {tab}
          </button>
        ))}
      </div>

      {activeTab === "calendar" ? (
        <div className="px-4 pb-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-[14px] font-semibold leading-5 text-[#1C1C1E] dark:text-white">
              {selectedDate.toLocaleDateString("en-US", { month: "short", year: "numeric" })}
            </h3>
            <div className="flex items-center gap-0.5">
              <MiniIcon label="Previous" onClick={() => dispatchCalendarPanel({ type: "select-date", selectedDate: shiftDate(selectedDate, expanded ? -30 : -7) })}>
                <ChevronLeft className="size-4" />
              </MiniIcon>
              <MiniIcon label="Next" onClick={() => dispatchCalendarPanel({ type: "select-date", selectedDate: shiftDate(selectedDate, expanded ? 30 : 7) })}>
                <ChevronRight className="size-4" />
              </MiniIcon>
              <MiniIcon label={expanded ? "Collapse calendar" : "Expand calendar"} onClick={() => dispatchCalendarPanel({ type: "toggle-expanded" })}>
                {expanded ? <ChevronUp className="size-4" /> : <ChevronDown className="size-4" />}
              </MiniIcon>
            </div>
          </div>
          <CalendarGrid
            expanded={expanded}
            selectedDate={selectedDate}
            events={state.events}
            onSelect={(nextDate) => dispatchCalendarPanel({ type: "select-date", selectedDate: nextDate })}
          />
          <div className="mt-3 border-t border-[#F2F2F2] pt-3 dark:border-[#2A2C31]">
            <div className="mb-1.5 flex items-center justify-between">
              <p className="text-[11px] font-semibold uppercase text-[#AAAAAA]">
                {selectedDate.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" })}
              </p>
            </div>
            <div className="max-h-[140px] overflow-y-auto">
              {selectedEvents.length === 0 ? <p className="py-2 text-center text-xs text-[#AAAAAA]">No events for this day</p> : null}
              {selectedEvents.map((event) => <CalendarEventRow key={event.id} event={event} />)}
            </div>
            <div className="mt-3 flex items-center gap-2 rounded-xl bg-[#F7F7F7] px-3 py-2 dark:bg-[#191A1F]">
              <button type="button" onClick={saveEvent} aria-label="Add calendar event" className="grid size-6 shrink-0 place-items-center rounded-full bg-[#1C1C1E] text-white">
                <Plus className="size-3.5" />
              </button>
              <input
                value={eventTitle}
                onChange={(event) => dispatchCalendarPanel({ type: "set-event-title", eventTitle: event.target.value })}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    void saveEvent();
                  }
                }}
                placeholder="Add event…"
                aria-label="Calendar event title"
                className="min-w-0 flex-1 bg-transparent text-xs text-[#1C1C1E] placeholder:text-[#AAAAAA] focus:outline-none dark:text-white"
              />
            </div>
          </div>
        </div>
      ) : (
        <div className="px-4 pb-4">
          <p className="text-[11px] text-[#AAAAAA]">{selectedDate.toLocaleDateString("en-US", { month: "short", day: "numeric" })} ·</p>
          <h3 className="text-[15px] font-semibold leading-5 text-[#1C1C1E] dark:text-white">Today</h3>
          <div className="mt-3 max-h-[260px] overflow-y-auto">
            {selectedNotes.length === 0 ? <p className="py-5 text-center text-xs text-[#AAAAAA]">No notes for this day</p> : null}
            {selectedNotes.map((note) => (
              <div key={note.id} className="mb-3 flex gap-3">
                <span className="h-fit rounded-full bg-[#F4F4F4] px-2 py-0.5 text-[10px] font-medium text-[#AAAAAA] dark:bg-[#23252A]">
                  {new Date(note.createdAt).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}
                </span>
                <p className="min-w-0 flex-1 text-xs leading-relaxed text-[#444444] dark:text-[#D0D6E0]">{note.text}</p>
              </div>
            ))}
          </div>
          <div className="mt-3 flex items-center gap-2 rounded-xl bg-[#F7F7F7] px-3 py-2 dark:bg-[#191A1F]">
            <button type="button" onClick={saveNote} aria-label="Add calendar note" className="grid size-6 shrink-0 place-items-center rounded-full bg-[#1C1C1E] text-white">
              <Plus className="size-3.5" />
            </button>
            <input
              value={noteText}
              onChange={(event) => dispatchCalendarPanel({ type: "set-note-text", noteText: event.target.value })}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void saveNote();
                }
              }}
              placeholder="Start typing…"
              aria-label="Calendar note text"
              className="min-w-0 flex-1 bg-transparent text-xs text-[#1C1C1E] placeholder:text-[#AAAAAA] focus:outline-none dark:text-white"
            />
            <Mic className="size-4 text-[#AAAAAA]" />
          </div>
        </div>
      )}
      {error ? <p className="border-t border-[#F2F2F2] px-4 py-2 text-xs text-[#B42318] dark:border-[#2A2C31]">{error}</p> : null}
    </Panel>
  );
}

export function ProfileDropdown({
  profile,
  onSupport,
  onSignOut,
}: {
  profile: NavbarProfile | null;
  onSupport: () => void;
  onSignOut: () => void;
}) {
  return (
    <Panel className="right-0 velion-floating-panel-sm p-2">
      <div className="mb-2 rounded-xl bg-[#F7F7F8] px-3 py-2.5 dark:bg-[#191A1F]">
        <div className="truncate text-[13px] font-semibold text-[#111111] dark:text-white">{profile?.name ?? "Account"}</div>
        {profile?.email ? <div className="mt-0.5 truncate text-[12px] text-[#777777] dark:text-[#AEB4C0]">{profile.email}</div> : null}
      </div>
      <ProfileItem href="/account" icon={User} label="Profile" />
      <ProfileItem href="/settings/members" icon={Users} label="Community" />
      <ProfileItem href="/settings/billing" icon={CreditCard} label="Subscription" badge="PRO" />
      <ProfileItem href="/settings/workspace" icon={Settings} label="Settings" />
      <div className="my-1 h-px bg-[#EBEBEB] dark:bg-[#2A2C31]" />
      <button type="button" onClick={onSupport} className="velion-menu-item">
        <HelpCircle className="size-[17px] text-[#555555] dark:text-[#AEB4C0]" strokeWidth={1.7} />
        <span className="flex-1">Help center</span>
      </button>
      <button type="button" onClick={onSignOut} className="velion-menu-item">
        <LogOut className="size-[17px] text-[#555555] dark:text-[#AEB4C0]" strokeWidth={1.7} />
        <span className="flex-1">Sign out</span>
      </button>
    </Panel>
  );
}

function NotificationRow({
  bordered,
  item,
  onOpen,
}: {
  bordered: boolean;
  item: NavbarNotification;
  onOpen: (id: string) => void;
}) {
  return (
    <Link
      href={(item.href ?? "/inbox") as Route}
      onClick={() => onOpen(item.id)}
      className={cn("block px-4 py-3 text-left transition-colors hover:bg-[#FAFAFA] dark:hover:bg-[#191A1F]", bordered ? "border-b border-[#F0F0F0] dark:border-[#2A2C31]" : "")}
    >
      <div className="flex items-start gap-3">
        <span className="relative grid size-8 shrink-0 place-items-center rounded-full bg-[#E8E8E8] text-[11px] font-semibold text-[#555555] dark:bg-[#23252A] dark:text-[#D0D6E0]">
          N
          <span className="absolute -bottom-0.5 -right-0.5 grid size-4 place-items-center rounded-full border-2 border-white bg-[#3578F6] text-white dark:border-[#141516]">
            <Bell className="size-2" strokeWidth={2.5} />
          </span>
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-2">
            <span className="truncate text-[13px] font-semibold text-[#111111] dark:text-white">{item.title}</span>
            {!item.read ? <span className="mt-1 size-2.5 shrink-0 rounded-full bg-[#22C55E]" /> : null}
          </div>
          <p className="mt-0.5 line-clamp-2 text-[12px] leading-5 text-[#555555] dark:text-[#D0D6E0]">{item.body}</p>
          {item.createdAt ? <p className="mt-0.5 text-xs text-[#9B9B9B]">{formatRelativeTime(item.createdAt)}</p> : null}
        </div>
      </div>
    </Link>
  );
}

function MessageRow({
  bordered,
  item,
  onOpen,
}: {
  bordered: boolean;
  item: NavbarNotification;
  onOpen: (id: string) => void;
}) {
  return (
    <Link
      href={(item.href ?? "/inbox") as Route}
      onClick={() => onOpen(item.id)}
      className={cn("block px-4 py-3 text-left transition-colors hover:bg-[#FAFAFA] dark:hover:bg-[#191A1F]", bordered ? "border-b border-[#F0F0F0] dark:border-[#2A2C31]" : "")}
    >
      <div className="flex items-start gap-3">
        <span className="grid size-8 shrink-0 place-items-center rounded-full bg-[#DBEAFE] text-[11px] font-semibold text-[#2563EB] dark:bg-[#1B2B49] dark:text-[#8DB3FF]">
          {item.title.charAt(0).toUpperCase()}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-2">
            <span className="truncate text-[13px] font-semibold text-[#111111] dark:text-white">{item.title}</span>
            {item.createdAt ? <span className="shrink-0 text-xs text-[#9B9B9B]">{formatRelativeTime(item.createdAt)}</span> : null}
          </div>
          <p className="mt-0.5 truncate text-xs text-[#9B9B9B]">{item.body}</p>
        </div>
        {!item.read ? <span className="mt-1 size-2.5 shrink-0 rounded-full bg-[#22C55E]" /> : null}
      </div>
    </Link>
  );
}

function CalendarGrid({
  expanded,
  selectedDate,
  events,
  onSelect,
}: {
  expanded: boolean;
  selectedDate: Date;
  events: CalendarEvent[];
  onSelect: (date: Date) => void;
}) {
  const days = buildCalendarDays(selectedDate, expanded);
  const eventDates = new Set(events.map((event) => formatDateKey(new Date(event.start))));
  const todayKey = useClientTodayKey();

  return (
    <div>
      <div className="mb-1 grid grid-cols-7">
        {["S", "M", "T", "W", "T", "F", "S"].map((day, index) => (
          <div key={`${day}-${index}`} className="text-center text-[11px] font-medium text-[#AAAAAA]">{day}</div>
        ))}
      </div>
      <div className="grid grid-cols-7">
        {days.map((day) => {
          const key = formatDateKey(day.date);
          const selected = key === formatDateKey(selectedDate);
          const isToday = key === todayKey;
          return (
            <button key={key} type="button" onClick={() => onSelect(day.date)} className="flex flex-col items-center gap-0.5 py-1 focus:outline-none">
              <span
                className={cn(
                  "grid size-7 place-items-center rounded-full text-[12px] font-medium transition-colors",
                  !day.currentMonth && expanded ? "text-[#CCCCCC]" : "text-[#555555] hover:bg-[#F2F2F2] dark:text-[#D0D6E0] dark:hover:bg-[#23252A]",
                  isToday && !selected ? "font-bold text-[#1C1C1E] ring-1 ring-[#1C1C1E] dark:text-white dark:ring-white" : "",
                  selected ? "bg-[#1C1C1E] text-white dark:bg-white dark:text-[#111]" : "",
                )}
              >
                {day.date.getDate()}
              </span>
              <span className={cn("size-1.5 rounded-full", eventDates.has(key) ? "bg-[#C5C5C7]" : "bg-transparent")} />
            </button>
          );
        })}
      </div>
    </div>
  );
}

function CalendarEventRow({ event }: { event: CalendarEvent }) {
  const start = new Date(event.start);
  const end = new Date(event.end);
  return (
    <div className="flex items-start gap-2.5 py-2">
      <span className="mt-1.5 size-2 shrink-0 rounded-full bg-[#5E6AD2]" />
      <div>
        <p className="text-[13px] font-semibold leading-tight text-[#1C1C1E] dark:text-white">{event.title}</p>
        <p className="mt-0.5 text-xs text-[#8E8E93]">
          {start.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })} - {end.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}
        </p>
      </div>
    </div>
  );
}

function ProfileItem({
  badge,
  href,
  icon: Icon,
  label,
}: {
  badge?: string;
  href: VelionRoute;
  icon: LucideIcon;
  label: string;
}) {
  return (
    <Link href={href as Route} className="velion-menu-item">
      <Icon className="size-[17px] text-[#555555] dark:text-[#AEB4C0]" strokeWidth={1.7} />
      <span className="flex-1">{label}</span>
      {badge ? (
        <span className="inline-flex items-center gap-0.5 rounded-md bg-[#E9D5FF] px-2 py-0.5 text-[10px] font-bold text-[#7C3AED]">
          <Zap className="size-2.5 fill-[#7C3AED] stroke-none" />
          {badge}
        </span>
      ) : null}
    </Link>
  );
}

function Panel({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "velion-popover velion-floating-panel absolute top-12 z-[var(--velion-z-popover)] p-0",
        className,
      )}
    >
      {children}
    </div>
  );
}

function TabHeader({ tabs }: { tabs: string[] }) {
  return (
    <div className="flex border-b border-[#EBEBEB] dark:border-[#2A2C31]">
      {tabs.map((tab, index) => (
        <button
          key={tab}
          type="button"
          className={cn(
            "relative flex-1 py-3.5 text-[11px] font-medium whitespace-nowrap transition-colors focus:outline-none",
            index === 0 ? "font-bold text-[#111111] dark:text-white" : "text-[#AAAAAA] hover:text-[#666666] dark:hover:text-white",
          )}
        >
          {tab}
          {index === 0 ? <span className="absolute inset-x-0 bottom-0 h-0.5 rounded-sm bg-[#111111] dark:bg-white" /> : null}
        </button>
      ))}
    </div>
  );
}

function EmptyPanel({ text }: { text: string }) {
  return (
    <div className="px-5 py-10 text-center">
        <p className="text-[13px] text-[#888888] dark:text-[#AEB4C0]">{text}</p>
    </div>
  );
}

function PanelFooter({ href, label }: { href: VelionRoute; label: string }) {
  return (
    <div className="border-t border-[#EBEBEB] px-4 py-2.5 dark:border-[#2A2C31]">
      <Link href={href as Route} className="block w-full py-1 text-center text-xs font-semibold text-[#555555] transition-colors hover:text-[#111111] dark:text-[#AEB4C0] dark:hover:text-white">
        {label}
      </Link>
    </div>
  );
}

function MiniIcon({
  children,
  label,
  onClick,
}: {
  children: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className="flex size-7 items-center justify-center rounded-lg text-[#8E8E93] transition-colors hover:bg-[#F2F2F2] dark:hover:bg-[#23252A]"
    >
      {children}
    </button>
  );
}

function shiftDate(date: Date, days: number) {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

function formatDateKey(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function buildCalendarDays(anchor: Date, expanded: boolean) {
  if (!expanded) {
    const weekStart = new Date(anchor);
    weekStart.setDate(anchor.getDate() - anchor.getDay());
    return Array.from({ length: 7 }, (_, index) => {
      const date = new Date(weekStart);
      date.setDate(weekStart.getDate() + index);
      return { date, currentMonth: true };
    });
  }

  const year = anchor.getFullYear();
  const month = anchor.getMonth();
  const firstDay = new Date(year, month, 1).getDay();
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const daysInPreviousMonth = new Date(year, month, 0).getDate();
  const cells: Array<{ date: Date; currentMonth: boolean }> = [];

  for (let index = firstDay - 1; index >= 0; index -= 1) {
    cells.push({ date: new Date(year, month - 1, daysInPreviousMonth - index), currentMonth: false });
  }
  for (let day = 1; day <= daysInMonth; day += 1) {
    cells.push({ date: new Date(year, month, day), currentMonth: true });
  }
  while (cells.length < 42) {
    const nextDay = cells.length - firstDay - daysInMonth + 1;
    cells.push({ date: new Date(year, month + 1, nextDay), currentMonth: false });
  }

  return cells;
}

function formatRelativeTime(timestamp: string) {
  const diffMinutes = Math.max(0, Math.floor((Date.now() - new Date(timestamp).getTime()) / 60_000));

  if (diffMinutes < 1) {
    return "now";
  }
  if (diffMinutes < 60) {
    return `${diffMinutes}m ago`;
  }

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 24) {
    return `${diffHours}h ago`;
  }

  return `${Math.floor(diffHours / 24)}d ago`;
}
