'use client';

import React from 'react';
import { ChevronLeft, ChevronRight, ChevronDown, ChevronUp, Plus, Mic, Check } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import type { CalendarEvent as SourceCalendarEvent } from '@/components/core/sidebar/types';

type TabType = 'calendar' | 'notes';

interface CalendarDropdownEvent {
  id: string;
  title: string;
  time: string;
  color: string;
}

interface CalendarTask {
  id: string;
  text: string;
  completed: boolean;
  badge?: string;
  progress?: string;
}

interface TimelineEntry {
  id: string;
  time: string;
  type: 'note' | 'tasks';
  content?: string;
  tasks?: CalendarTask[];
}

interface CalendarDropdownProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  onDateSelect?: (date: Date) => void;
  events?: SourceCalendarEvent[];
  isLoading?: boolean;
  trigger: React.ReactNode;
}

// ─── Constants ────────────────────────────────────────────────────────────────

const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
];

const WEEK_DAYS = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];

const INITIAL_NOTES: TimelineEntry[] = [];

// ─── Helpers ──────────────────────────────────────────────────────────────────

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

function formatEventTime(start: Date, end: Date): string {
  const formatter = new Intl.DateTimeFormat('en-US', {
    hour: 'numeric',
    minute: '2-digit',
  });
  return `${formatter.format(start)} - ${formatter.format(end)}`;
}

function eventColor(event: SourceCalendarEvent): string {
  switch (event.status) {
    case 'ongoing':
      return '#22C55E';
    case 'past':
      return '#C5C5C7';
    default:
      return event.type === 'meeting' ? '#5E6AD2' : '#3578F6';
  }
}

function groupEventsByDate(events: SourceCalendarEvent[]): Record<string, CalendarDropdownEvent[]> {
  return events.reduce<Record<string, CalendarDropdownEvent[]>>((accumulator, event) => {
    const key = fmtDate(event.start);
    const current = accumulator[key] ?? [];
    return {
      ...accumulator,
      [key]: [
        ...current,
        {
          id: event.id,
          title: event.title,
          time: formatEventTime(event.start, event.end),
          color: eventColor(event),
        },
      ],
    };
  }, {});
}

// ─── WeekStrip ────────────────────────────────────────────────────────────────

interface WeekStripProps {
  anchorDate: Date;
  selectedDate: Date | null;
  events: Record<string, CalendarDropdownEvent[]>;
  onDayClick: (d: Date) => void;
}

function WeekStrip({ anchorDate, selectedDate, events, onDayClick }: WeekStripProps) {
  const today = new Date();
  const weekStart = new Date(anchorDate);
  weekStart.setDate(anchorDate.getDate() - anchorDate.getDay());

  const days = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(weekStart);
    d.setDate(weekStart.getDate() + i);
    return d;
  });

  return (
    <div>
      <div className="grid grid-cols-7 mb-1">
        {WEEK_DAYS.map((w, i) => (
          <div key={i} className="text-center text-[11px] font-medium text-[#AAAAAA]">{w}</div>
        ))}
      </div>
      <div className="grid grid-cols-7">
        {days.map((d, i) => {
          const key = fmtDate(d);
          const isToday = fmtDate(d) === fmtDate(today);
          const isSel = selectedDate ? fmtDate(d) === fmtDate(selectedDate) : false;
          const hasEvents = (events[key]?.length ?? 0) > 0;
          return (
            <button
              key={fmtDate(d)}
              onClick={(e) => { e.stopPropagation(); onDayClick(d); }}
              className="flex flex-col items-center gap-0.5 py-1 focus:outline-none"
            >
              <div className={cn(
                'w-8 h-8 flex items-center justify-center rounded-full text-sm font-medium transition-colors',
                isSel ? 'bg-[#1C1C1E] text-white'
                  : isToday ? 'text-[#1C1C1E] font-bold'
                    : 'text-[#555555] hover:bg-[#F2F2F2]',
              )}>
                {d.getDate()}
              </div>
              <div className={cn('w-1.5 h-1.5 rounded-full', hasEvents ? 'bg-[#C5C5C7]' : 'bg-transparent')} />
            </button>
          );
        })}
      </div>
    </div>
  );
}

// ─── FullMonthGrid ────────────────────────────────────────────────────────────

interface FullMonthGridProps {
  currentDate: Date;
  selectedDate: Date | null;
  events: Record<string, CalendarDropdownEvent[]>;
  onDayClick: (d: Date) => void;
}

function FullMonthGrid({ currentDate, selectedDate, events, onDayClick }: FullMonthGridProps) {
  const today = new Date();
  const year = currentDate.getFullYear();
  const month = currentDate.getMonth();
  const firstDay = new Date(year, month, 1).getDay();
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const daysInPrevMonth = new Date(year, month, 0).getDate();

  const cells: { date: Date; current: boolean }[] = [];
  for (let i = firstDay - 1; i >= 0; i--) {
    cells.push({ date: new Date(year, month - 1, daysInPrevMonth - i), current: false });
  }
  for (let d = 1; d <= daysInMonth; d++) {
    cells.push({ date: new Date(year, month, d), current: true });
  }
  while (cells.length < 42) {
    const nextDay = cells.length - firstDay - daysInMonth + 1;
    cells.push({ date: new Date(year, month + 1, nextDay), current: false });
  }

  return (
    <div>
      <div className="grid grid-cols-7 mb-1">
        {WEEK_DAYS.map((w, i) => (
          <div key={i} className="text-center text-[11px] font-medium text-[#AAAAAA]">{w}</div>
        ))}
      </div>
      <div className="grid grid-cols-7">
        {cells.map(({ date, current }, i) => {
          const key = fmtDate(date);
          const isToday = fmtDate(date) === fmtDate(today);
          const isSel = selectedDate ? fmtDate(date) === fmtDate(selectedDate) : false;
          const hasEvents = current && (events[key]?.length ?? 0) > 0;
          return (
            <button
              key={fmtDate(date)}
              onClick={(e) => { e.stopPropagation(); onDayClick(date); }}
              className="flex flex-col items-center gap-0.5 py-0.5 focus:outline-none"
            >
              <div className={cn(
                'w-7 h-7 flex items-center justify-center rounded-full text-xs font-medium transition-colors',
                !current && 'text-[#CCCCCC]',
                current && !isSel && !isToday && 'text-[#555555] hover:bg-[#F2F2F2]',
                current && isToday && !isSel && 'text-[#1C1C1E] font-bold ring-1 ring-[#1C1C1E] ring-offset-0',
                isSel && 'bg-[#1C1C1E] text-white',
              )}>
                {date.getDate()}
              </div>
              <div className={cn('w-1 h-1 rounded-full', hasEvents ? 'bg-[#C5C5C7]' : 'bg-transparent')} />
            </button>
          );
        })}
      </div>
    </div>
  );
}

// ─── EventsList ───────────────────────────────────────────────────────────────

function EventsList({
  events,
  isLoading,
}: {
  events: CalendarDropdownEvent[];
  isLoading?: boolean;
}) {
  if (isLoading) {
    return (
      <div className="py-3 text-center">
        <p className="text-xs text-[#AAAAAA]">Loading events...</p>
      </div>
    );
  }

  if (events.length === 0) {
    return (
      <div className="py-3 text-center">
        <p className="text-xs text-[#AAAAAA]">No events for this day</p>
      </div>
    );
  }
  return (
    <div>
      {events.map((ev, i) => (
        <div key={ev.id}>
          <div className="py-2 flex items-start gap-2.5">
            <div
              className="w-2 h-2 rounded-full mt-1.5 shrink-0"
              style={{ backgroundColor: ev.color }}
            />
            <div>
              <p className="text-[13px] font-semibold text-[#1C1C1E] leading-tight">{ev.title}</p>
              <p className="text-xs text-[#8E8E93] mt-0.5">{ev.time}</p>
            </div>
          </div>
          {i < events.length - 1 && <div className="h-px bg-[#F2F2F2] ml-4" />}
        </div>
      ))}
    </div>
  );
}

// ─── NotesTimeline ────────────────────────────────────────────────────────────

interface NotesTimelineProps {
  notes: TimelineEntry[];
  onToggleTask: (noteId: string, taskId: string) => void;
}

function NotesTimeline({ notes, onToggleTask }: NotesTimelineProps) {
  return (
    <div>
      {notes.map((entry) => (
        <div key={entry.id} className="flex gap-3 mb-4">
          <div className="shrink-0 pt-0.5">
            <span className="inline-block text-[10px] font-medium text-[#AAAAAA] bg-[#F4F4F4] px-2 py-0.5 rounded-full whitespace-nowrap">
              {entry.time}
            </span>
          </div>
          <div className="flex-1 min-w-0">
            {entry.type === 'note' ? (
              <p className="text-xs text-[#444444] leading-relaxed mt-0.5">{entry.content}</p>
            ) : (
              <div className="border border-[#EFEFEF] rounded-xl overflow-hidden">
                {entry.tasks?.map((task, ti) => (
                  <div key={task.id}>
                    <div className="flex items-start gap-2.5 px-3 py-2.5">
                      <button
                        onClick={(e) => { e.stopPropagation(); onToggleTask(entry.id, task.id); }}
                        className={cn(
                          'w-4 h-4 rounded border flex items-center justify-center shrink-0 mt-0.5 transition-colors focus:outline-none',
                          task.completed
                            ? 'bg-[#1C1C1E] border-[#1C1C1E]'
                            : 'border-[#CCCCCC] hover:border-[#888888]'
                        )}
                      >
                        {task.completed && <Check className="w-2.5 h-2.5 text-white" strokeWidth={3} />}
                      </button>
                      <div className="flex-1 min-w-0">
                        <p className={cn(
                          'text-xs font-medium',
                          task.completed ? 'line-through text-[#AAAAAA]' : 'text-[#1C1C1E]'
                        )}>
                          {task.text}
                        </p>
                        {(task.badge ?? task.progress) && (
                          <div className="flex items-center gap-1.5 mt-1">
                            {task.badge && (
                              <span className="text-[10px] font-semibold bg-[#1C1C1E] text-white px-2 py-0.5 rounded-full">
                                {task.badge}
                              </span>
                            )}
                            {task.progress && (
                              <span className="text-[10px] text-[#AAAAAA]">{task.progress}</span>
                            )}
                          </div>
                        )}
                      </div>
                    </div>
                    {ti < (entry.tasks?.length ?? 0) - 1 && <div className="h-px bg-[#F4F4F4]" />}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

// ─── Main Component ───────────────────────────────────────────────────────────

export function CalendarDropdown({
  isOpen,
  onOpenChange,
  onDateSelect,
  events = [],
  isLoading = false,
  trigger,
}: CalendarDropdownProps) {
  const today = React.useMemo(() => new Date(), []);
  const eventsByDate = React.useMemo(() => groupEventsByDate(events), [events]);

  const [currentDate, setCurrentDate] = React.useState(() => new Date());
  const [selectedDate, setSelectedDate] = React.useState<Date | null>(() => new Date());
  const [hoveredParent, setHoveredParent] = React.useState(false);
  const [activeTab, setActiveTab] = React.useState<TabType>('calendar');
  const [isExpanded, setIsExpanded] = React.useState(false);
  const [notes, setNotes] = React.useState<TimelineEntry[]>(INITIAL_NOTES);
  const [noteInput, setNoteInput] = React.useState('');

  React.useEffect(() => {
    if (hoveredParent && !isOpen) { onOpenChange(true); }
    else if (!hoveredParent && isOpen) { onOpenChange(false); }
  }, [hoveredParent, isOpen, onOpenChange]);

  const handleDayClick = React.useCallback((date: Date) => {
    setSelectedDate(date);
    setCurrentDate(date);
    onDateSelect?.(date);
  }, [onDateSelect]);

  const handleToggleTask = React.useCallback((noteId: string, taskId: string) => {
    setNotes(prev => prev.map(n => {
      if (n.id !== noteId || !n.tasks) return n;
      return { ...n, tasks: n.tasks.map(t => t.id === taskId ? { ...t, completed: !t.completed } : t) };
    }));
  }, []);

  const handleAddNote = React.useCallback((e: React.MouseEvent | React.KeyboardEvent) => {
    e.stopPropagation();
    const text = noteInput.trim();
    if (!text) return;
    const now = new Date();
    const timeStr = now.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
    setNotes(prev => [...prev, { id: `n${Date.now()}`, time: timeStr, type: 'note', content: text }]);
    setNoteInput('');
  }, [noteInput]);

  // Week view: navigate by one week; Month view: navigate by one month
  const prevPeriod = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isExpanded) {
      setCurrentDate(d => new Date(d.getFullYear(), d.getMonth() - 1, 1));
    } else {
      const anchor = selectedDate ?? currentDate;
      const shifted = new Date(anchor);
      shifted.setDate(anchor.getDate() - 7);
      setSelectedDate(shifted);
      setCurrentDate(shifted);
    }
  };

  const nextPeriod = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isExpanded) {
      setCurrentDate(d => new Date(d.getFullYear(), d.getMonth() + 1, 1));
    } else {
      const anchor = selectedDate ?? currentDate;
      const shifted = new Date(anchor);
      shifted.setDate(anchor.getDate() + 7);
      setSelectedDate(shifted);
      setCurrentDate(shifted);
    }
  };

  const selectedKey = selectedDate ? fmtDate(selectedDate) : '';
  const selectedEvents = eventsByDate[selectedKey] ?? [];
  const todayLabel = today.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  const headerDate = isExpanded ? currentDate : (selectedDate ?? currentDate);

  return (
    <div onMouseEnter={() => setHoveredParent(true)} onMouseLeave={() => setHoveredParent(false)}>
      <DropdownMenu open={isOpen} onOpenChange={onOpenChange}>
        <DropdownMenuTrigger asChild id="dashboard-calendar-trigger">
          {trigger}
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          side="bottom"
          sideOffset={8}
          className={cn(
            'w-[360px] overflow-hidden rounded-2xl',
            'border border-[#E9EBF2] bg-white shadow-[0_20px_80px_rgba(17,17,17,0.15)]',
            'p-0'
          )}
          onClick={(e) => e.stopPropagation()}
        >
          {/* ── Tab bar ────────────────────────────────────────────── */}
          <div className="flex items-center gap-1 px-4 pt-3 pb-2">
            {(['calendar', 'notes'] as TabType[]).map((tab) => (
              <button
                key={tab}
                onClick={(e) => { e.stopPropagation(); setActiveTab(tab); }}
                className={cn(
                  'flex-1 py-1.5 text-xs font-semibold rounded-lg capitalize transition-all',
                  activeTab === tab
                    ? 'bg-[#F2F2F2] text-[#1C1C1E]'
                    : 'text-[#AAAAAA] hover:text-[#555555]'
                )}
              >
                {tab}
              </button>
            ))}
          </div>

          {/* ── Calendar tab ──────────────────────────────────────── */}
          {activeTab === 'calendar' && (
            <div className="px-4 pb-4">
              {/* Month / nav header */}
              <div className="flex items-center justify-between mb-3">
                <h3 className="text-lg font-bold text-[#1C1C1E]">
                  {MONTH_NAMES[headerDate.getMonth()].slice(0, 3)} {headerDate.getFullYear()}
                </h3>
                <div className="flex items-center gap-0.5">
                  <button
                    onClick={prevPeriod}
                    className="w-7 h-7 flex items-center justify-center rounded-lg text-[#8E8E93] hover:bg-[#F2F2F2] transition-colors focus:outline-none"
                    aria-label="Previous"
                  >
                    <ChevronLeft className="w-4 h-4" />
                  </button>
                  <button
                    onClick={nextPeriod}
                    className="w-7 h-7 flex items-center justify-center rounded-lg text-[#8E8E93] hover:bg-[#F2F2F2] transition-colors focus:outline-none"
                    aria-label="Next"
                  >
                    <ChevronRight className="w-4 h-4" />
                  </button>
                  <button
                    onClick={(e) => { e.stopPropagation(); setIsExpanded(v => !v); }}
                    aria-label={isExpanded ? 'Collapse calendar' : 'Expand calendar'}
                    className="w-7 h-7 flex items-center justify-center rounded-lg text-[#8E8E93] hover:bg-[#F2F2F2] transition-colors focus:outline-none ml-1"
                  >
                    {isExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
                  </button>
                </div>
              </div>

              {/* Calendar grid */}
              {isExpanded ? (
                <FullMonthGrid
                  currentDate={currentDate}
                  selectedDate={selectedDate}
                  events={eventsByDate}
                  onDayClick={handleDayClick}
                />
              ) : (
                <WeekStrip
                  anchorDate={selectedDate ?? currentDate}
                  selectedDate={selectedDate}
                  events={eventsByDate}
                  onDayClick={handleDayClick}
                />
              )}

              {/* Events section */}
              <div className="mt-3 pt-3 border-t border-[#F2F2F2]">
                <div className="flex items-center justify-between mb-1.5">
                  <p className="text-[11px] font-semibold text-[#AAAAAA] uppercase tracking-wide">
                    {selectedDate
                      ? selectedDate.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })
                      : 'Events'}
                  </p>
                  <button
                    onClick={(e) => e.stopPropagation()}
                    className="w-6 h-6 flex items-center justify-center rounded-md text-[#8E8E93] hover:bg-[#F2F2F2] transition-colors focus:outline-none"
                    aria-label="Add event"
                  >
                    <Plus className="w-3.5 h-3.5" />
                  </button>
                </div>
                <div className="max-h-[160px] overflow-y-auto">
                  <EventsList events={selectedEvents} isLoading={isLoading} />
                </div>
              </div>
            </div>
          )}

          {/* ── Notes tab ─────────────────────────────────────────── */}
          {activeTab === 'notes' && (
            <div className="flex flex-col">
              {/* Notes header */}
              <div className="px-4 pb-2">
                <p className="text-[11px] text-[#AAAAAA] font-medium">{todayLabel} •</p>
                <h3 className="text-xl font-bold text-[#1C1C1E] leading-tight">Today</h3>
              </div>

              {/* Timeline */}
              <div className="overflow-y-auto max-h-[280px] px-4 pb-2">
                <NotesTimeline notes={notes} onToggleTask={handleToggleTask} />
              </div>

              {/* Add note input */}
              <div className="px-4 pb-4 pt-2 border-t border-[#F2F2F2]">
                <div className="flex items-center gap-2 bg-[#F7F7F7] rounded-xl px-3 py-2">
                  <button
                    onClick={handleAddNote}
                    className="w-6 h-6 flex items-center justify-center rounded-full bg-[#1C1C1E] text-white shrink-0 hover:bg-[#333333] transition-colors focus:outline-none"
                    aria-label="Add note"
                  >
                    <Plus className="w-3.5 h-3.5" />
                  </button>
                  <input
                    type="text"
                    value={noteInput}
                    onChange={(e) => setNoteInput(e.target.value)}
                    onKeyDown={(e) => { if (e.key === 'Enter') handleAddNote(e); }}
                    onClick={(e) => e.stopPropagation()}
                    placeholder="Start typing..."
                    className="flex-1 text-xs bg-transparent text-[#1C1C1E] placeholder:text-[#AAAAAA] focus:outline-none"
                  />
                  <button
                    onClick={(e) => e.stopPropagation()}
                    className="text-[#AAAAAA] hover:text-[#555555] transition-colors focus:outline-none shrink-0"
                    aria-label="Voice note"
                  >
                    <Mic className="w-4 h-4" />
                  </button>
                </div>
              </div>
            </div>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
