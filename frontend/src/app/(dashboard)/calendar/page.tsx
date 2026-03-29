'use client';

import React from 'react';
import { useSearchParams } from 'next/navigation';
import { CalendarDays, Sparkles } from 'lucide-react';
import { Calendar } from '@/components/core/sidebar/components/Calendar';
import { SidebarItemDetailDrawer, type SidebarDetailItem } from '@/components/core/sidebar/components/SidebarItemDetailDrawer';
import { useCalendarEvents } from '@/components/core/sidebar/hooks/useRealData';
import type { CalendarEvent } from '@/components/core/sidebar/types';

function normalizeEvents(data: unknown): CalendarEvent[] {
  if (Array.isArray(data)) {
    return data as CalendarEvent[];
  }

  if (data && typeof data === 'object') {
    const wrapped = data as { events?: unknown };
    if (Array.isArray(wrapped.events)) {
      return wrapped.events as CalendarEvent[];
    }
  }

  return [];
}

export default function CalendarPage() {
  const searchParams = useSearchParams();
  const { data } = useCalendarEvents({ enabled: true });
  const [selectedItem, setSelectedItem] = React.useState<SidebarDetailItem | null>(null);
  const events = React.useMemo(() => normalizeEvents(data), [data]);
  const selectedId = searchParams.get('event');

  React.useEffect(() => {
    if (!selectedId) {
      return;
    }

    const selectedEvent = events.find((event) => event.id === selectedId);
    if (selectedEvent) {
      setSelectedItem({ kind: 'event', item: selectedEvent });
    }
  }, [events, selectedId]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-[#F5F3EE]">
      <div className="w-full space-y-6 px-4 py-6 md:px-6 md:py-7 xl:px-7">
        <div className="rounded-[28px] border border-black/8 bg-[#FCFBF8] p-6 shadow-[0_22px_48px_rgba(22,20,17,0.08)]">
          <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
            <div>
              <div className="text-[11px] font-semibold uppercase tracking-[0.28em] text-[#B96618]">Planner Surface</div>
              <h1 className="mt-2 text-3xl font-semibold tracking-tight text-black">Calendar</h1>
              <p className="mt-2 max-w-2xl text-sm leading-6 text-black/54">
                Review today, upcoming meetings, and jump into the right event context from a full destination page instead of a floating utility only.
              </p>
            </div>
            <div className="flex items-center gap-2 rounded-full border border-[#E8D4BF] bg-[#FFF8EF] px-3 py-2 text-sm text-[#B96618]">
              <Sparkles className="h-4 w-4" />
              {events.length} events available
            </div>
          </div>
        </div>

        <div className="grid gap-6 xl:grid-cols-[390px_minmax(0,1fr)]">
          <div className="overflow-hidden rounded-[28px] border border-black/8 bg-[#FCFBF8] shadow-[0_18px_40px_rgba(22,20,17,0.08)]">
            <Calendar
              events={events}
              onEventClick={(event) => setSelectedItem({ kind: 'event', item: event })}
            />
          </div>

          <div className="rounded-[28px] border border-black/8 bg-[#FCFBF8] p-6 shadow-[0_18px_40px_rgba(22,20,17,0.08)]">
            <div className="flex h-full flex-col justify-between">
              <div>
                <div className="flex h-12 w-12 items-center justify-center rounded-2xl border border-[#E8D4BF] bg-[#FFF8EF] text-[#B96618]">
                  <CalendarDays className="h-5 w-5" />
                </div>
                <h2 className="mt-5 text-2xl font-semibold text-black">Meeting-first event inspection</h2>
                <p className="mt-3 max-w-xl text-sm leading-6 text-black/56">
                  Open any event to access the inspector drawer. Join the meeting when a link is available, or move to the event page and related chat from the action rail.
                </p>
              </div>

              <div className="mt-8 grid gap-3 md:grid-cols-2">
                <div className="rounded-[22px] border border-black/8 bg-[#F7F4EE] p-4">
                  <div className="text-[10px] font-semibold uppercase tracking-[0.24em] text-black/34">Join meeting</div>
                  <div className="mt-2 text-sm leading-6 text-black/62">When a live meeting link is present, the drawer promotes it as the primary action.</div>
                </div>
                <div className="rounded-[22px] border border-black/8 bg-[#F7F4EE] p-4">
                  <div className="text-[10px] font-semibold uppercase tracking-[0.24em] text-black/34">Open event page</div>
                  <div className="mt-2 text-sm leading-6 text-black/62">If there is no direct meeting URL, the primary CTA still lands on a real calendar destination.</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {selectedItem && (
        <SidebarItemDetailDrawer item={selectedItem} onClose={() => setSelectedItem(null)} />
      )}
    </div>
  );
}