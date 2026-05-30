"use client";

import { useState, type ReactNode } from "react";
import { ChevronDown, Plus } from "lucide-react";
import { VelionButton, VelionIconButton } from "@/components/ui/velion-ui";
import type { InboxModalRequest } from "@/features/inbox-v2/components/InboxWorkModal";
import { formatDateKey } from "@/features/inbox-v2/lib/calendar-format";
import { cn } from "@/lib/utils";
import { useClientTodayKey } from "@/lib/use-client-today";

export type CalendarEvent = {
  id: string;
  title: string;
  start: string;
  end: string;
  type: string;
  status: string;
  createdAt: string;
};

export type CalendarNote = {
  id: string;
  text: string;
  date: string;
  createdAt: string;
};

export function MiniCalendarGrid({
  events,
  onSelect,
  selectedDate,
}: {
  events: CalendarEvent[];
  onSelect: (date: Date) => void;
  selectedDate: Date;
}) {
  const days = buildCalendarDays(selectedDate);
  const eventDates = new Set(events.map((event) => formatDateKey(new Date(event.start))));
  const todayKey = useClientTodayKey();

  return (
    <section className="rounded-[12px] border border-[#E1DAD1] bg-white p-3">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-[14px] font-semibold">
          {selectedDate.toLocaleDateString("en-US", { month: "long", year: "numeric" })}
        </h3>
        <div className="flex items-center gap-1">
          <VelionIconButton size="xs" radius="sm" onClick={() => onSelect(shiftDate(selectedDate, -7))} className="hover:bg-[#F5F1EC]" aria-label="Previous week">
            <ChevronDown className="size-4 rotate-90" />
          </VelionIconButton>
          <VelionButton size="xs" radius="sm" onClick={() => selectToday(onSelect)} className="h-7 px-2 text-[11px] font-semibold hover:bg-[#F5F1EC]">
            Today
          </VelionButton>
          <VelionIconButton size="xs" radius="sm" onClick={() => onSelect(shiftDate(selectedDate, 7))} className="hover:bg-[#F5F1EC]" aria-label="Next week">
            <ChevronDown className="size-4 -rotate-90" />
          </VelionIconButton>
        </div>
      </div>
      <div className="grid grid-cols-7 gap-1">
        {days.map((day) => {
          const key = formatDateKey(day);
          const selected = key === formatDateKey(selectedDate);
          const isToday = key === todayKey;

          return (
            <button
              key={key}
              type="button"
              onClick={() => onSelect(day)}
              className={cn(
                "flex h-12 flex-col items-center justify-center rounded-[9px] text-[12px] transition-colors hover:bg-[#F5F1EC] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]",
                selected ? "bg-[#111111] text-white hover:bg-[#111111]" : "text-[#626260]",
              )}
            >
              <span className="text-[10px] font-medium uppercase opacity-70">{day.toLocaleDateString("en-US", { weekday: "short" }).slice(0, 1)}</span>
              <span className={cn("mt-0.5 font-semibold", isToday && !selected ? "text-[#006ADC]" : "")}>{day.getDate()}</span>
              <span className={cn("mt-0.5 size-1 rounded-full", eventDates.has(key) ? selected ? "bg-white" : "bg-[#DD7A1F]" : "bg-transparent")} />
            </button>
          );
        })}
      </div>
    </section>
  );
}

function selectToday(onSelect: (date: Date) => void) {
  onSelect(new Date());
}

export function CalendarEventRow({ event }: { event: CalendarEvent }) {
  const start = new Date(event.start);
  const end = new Date(event.end);

  return (
    <div className="flex items-start gap-2.5 py-2">
      <span className="mt-1.5 size-2 shrink-0 rounded-full bg-[#5E6AD2]" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[12px] font-semibold text-[#111111]">{event.title}</p>
        <p className="mt-0.5 text-[11px] text-[#7B7B78]">
          {start.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })} - {end.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}
        </p>
      </div>
    </div>
  );
}

export function CalendarNoteRow({ note }: { note: CalendarNote }) {
  return (
    <div className="flex items-start gap-2.5 border-t border-[#EEE8E0] py-2 first:border-t-0">
      <span className="mt-1.5 size-2 shrink-0 rounded-full bg-[#DD7A1F]" />
      <div className="min-w-0 flex-1">
        <p className="text-[12px] leading-5 text-[#3F3A35]">{note.text}</p>
        <p className="mt-0.5 text-[11px] text-[#9C9A96]">
          {new Date(note.createdAt).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}
        </p>
      </div>
    </div>
  );
}

export function HealthRow({ label, tone, value }: { label: string; tone: "neutral" | "success" | "warning"; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 text-[13px]">
      <span className="text-[#7B7B78]">{label}</span>
      <span
        className={cn(
          "rounded-full px-2 py-0.5 text-[11px] font-semibold",
          tone === "success" ? "bg-[#ECFDF3] text-[#067647]" : "",
          tone === "warning" ? "bg-[#FFF7E8] text-[#9A6300]" : "",
          tone === "neutral" ? "bg-[#F5F1EC] text-[#3F3A35]" : "",
        )}
      >
        {value}
      </span>
    </div>
  );
}

export function ActivityItem({ body, title }: { body: string; title: string }) {
  return (
    <div className="flex gap-2.5">
      <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-[#C8C1B7]" />
      <div>
        <p className="text-[12px] font-semibold text-[#111111]">{title}</p>
        <p className="mt-0.5 text-[12px] leading-5 text-[#626260]">{body}</p>
      </div>
    </div>
  );
}

export function AsideTabButton({ active, children, onClick }: { active: boolean; children: ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "h-full border-b-2 text-[14px] font-semibold transition-colors",
        active ? "border-[#DD7A1F] text-[#111111] dark:text-white" : "border-transparent text-[#626260] hover:text-[#111111]",
      )}
    >
      {children}
    </button>
  );
}

export function AccordionSection({
  children,
  defaultOpen = false,
  icon,
  title,
}: {
  children?: ReactNode;
  defaultOpen?: boolean;
  icon: ReactNode;
  title: string;
}) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <section className="border-b border-[#E7E1D8] dark:border-[#2A2C31]">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex h-12 w-full items-center gap-2 px-4 text-left text-[14px] font-semibold transition-colors hover:bg-[#FAF8F5]"
        aria-expanded={open}
      >
        {icon}
        <span className="min-w-0 flex-1 truncate">{title}</span>
        <ChevronDown className={cn("size-4 text-[#626260] transition-transform", open ? "rotate-180" : "")} />
      </button>
      {open ? <div className="space-y-3 px-4 pb-4 text-[13px]">{children ?? <p className="text-[#7B7B78]">No data yet.</p>}</div> : null}
    </section>
  );
}

export function LinkRow({ label, onOpenModal }: { label: string; onOpenModal: (modal: InboxModalRequest) => void }) {
  return (
    <div className="flex items-center justify-between gap-3 text-[#626260]">
      <span>{label}</span>
      <VelionIconButton
        onClick={() => onOpenModal({
          type: "work",
          title: label,
          description: `Create or attach ${label.toLowerCase()} from this ticket without navigating away from the inbox.`,
          primaryAction: "Attach link",
        })}
        aria-label={`Add ${label}`}
        size="xs"
        radius="pill"
        className="bg-[#F5F1EC] text-[#111111]"
      >
        <Plus className="size-4" />
      </VelionIconButton>
    </div>
  );
}

export function FieldRow({ label, muted = false, value }: { label: string; muted?: boolean; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="shrink-0 text-[#7B7B78]">{label}</span>
      <span className={cn("min-w-0 truncate text-right", muted ? "text-[#9C9A96]" : "text-[#111111] dark:text-white")}>{value}</span>
    </div>
  );
}

export function SourceRow({ title }: { title: string }) {
  return (
    <div className="flex items-center gap-2">
      <span className="grid size-5 place-items-center rounded-full bg-[#F5F1EC] text-[10px]">i</span>
      <span className="min-w-0 truncate">{title}</span>
    </div>
  );
}

export function EmptyAsideState({ body, icon, title }: { body: string; icon: ReactNode; title: string }) {
  return (
    <div className="grid min-h-[340px] place-items-center px-8 text-center">
      <div>
        <div className="mx-auto grid size-14 place-items-center rounded-[16px] border border-[#D8D2C8] bg-[#F8F5F1] text-[#9C9A96]">
          {icon}
        </div>
        <h2 className="mt-4 text-[15px] font-semibold">{title}</h2>
        <p className="mt-2 text-[13px] leading-5 text-[#7B7B78]">{body}</p>
      </div>
    </div>
  );
}

function buildCalendarDays(anchor: Date) {
  const start = new Date(anchor);
  start.setDate(anchor.getDate() - anchor.getDay());
  return Array.from({ length: 7 }, (_, index) => shiftDate(start, index));
}

function shiftDate(date: Date, days: number) {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}
