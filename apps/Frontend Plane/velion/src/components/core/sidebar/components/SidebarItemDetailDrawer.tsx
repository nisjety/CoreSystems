'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import {
  ArrowUpRight,
  Bell,
  Calendar as CalendarIcon,
  Clock,
  ExternalLink,
  Mail,
  MapPin,
  MessageCircleMore,
  MessageSquare,
  Settings,
  Users,
  X,
} from 'lucide-react';
import type { CalendarEvent, Message, Notification } from '../types';
import { cn } from '../utils';

export type SidebarDetailItem =
  | { kind: 'message'; item: Message }
  | { kind: 'notification'; item: Notification }
  | { kind: 'event'; item: CalendarEvent };

interface SidebarItemDetailDrawerProps {
  item: SidebarDetailItem;
  onClose: () => void;
}

type DrawerAction = {
  label: string;
  href: string;
  external?: boolean;
  emphasis?: 'primary' | 'secondary';
};

const PRIORITY_STYLES: Record<'low' | 'normal' | 'medium' | 'high', string> = {
  low: 'border-emerald-500/18 bg-emerald-500/10 text-emerald-800',
  normal: 'border-slate-400/18 bg-slate-500/10 text-slate-700',
  medium: 'border-amber-500/18 bg-amber-500/10 text-amber-800',
  high: 'border-rose-500/18 bg-rose-500/10 text-rose-800',
};

const STATUS_STYLES: Record<string, string> = {
  upcoming: 'border-sky-500/18 bg-sky-500/10 text-sky-800',
  ongoing: 'border-emerald-500/18 bg-emerald-500/10 text-emerald-800',
  past: 'border-slate-400/18 bg-slate-500/10 text-slate-700',
};

function formatDateTime(value: Date) {
  if (!(value instanceof Date) || Number.isNaN(value.getTime())) {
    return 'Date unavailable';
  }

  return value.toLocaleString([], {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}

function formatEventRange(start: Date, end: Date) {
  if (
    !(start instanceof Date) ||
    !(end instanceof Date) ||
    Number.isNaN(start.getTime()) ||
    Number.isNaN(end.getTime())
  ) {
    return 'Schedule unavailable';
  }

  const startDate = start.toLocaleDateString([], { dateStyle: 'medium' });
  const startTime = start.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  const endTime = end.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  if (start.toDateString() === end.toDateString()) {
    return `${startDate} • ${startTime} - ${endTime}`;
  }

  return `${formatDateTime(start)} - ${formatDateTime(end)}`;
}

function getMessageTypeLabel(type: Message['type']) {
  switch (type) {
    case 'teams':
      return 'Teams';
    case 'email':
      return 'Email';
    case 'chat':
      return 'Chat';
    default:
      return 'Message';
  }
}

function getNotificationTypeLabel(type: Notification['type']) {
  switch (type) {
    case 'email':
      return 'Email';
    case 'teams':
      return 'Teams';
    case 'calendar':
      return 'Calendar';
    case 'system':
      return 'System';
    default:
      return 'Notification';
  }
}

function getMessageIcon(type: Message['type']) {
  switch (type) {
    case 'teams':
      return Users;
    case 'email':
      return Mail;
    case 'chat':
      return MessageSquare;
    default:
      return MessageSquare;
  }
}

function getNotificationIcon(type: Notification['type']) {
  switch (type) {
    case 'email':
      return Mail;
    case 'teams':
      return Users;
    case 'calendar':
      return CalendarIcon;
    case 'system':
      return Settings;
    default:
      return Bell;
  }
}

function getExternalLinkCandidate(value?: string) {
  if (!value) return null;
  if (value.startsWith('http://') || value.startsWith('https://')) {
    return value;
  }
  return null;
}

function buildDrawerActions(item: SidebarDetailItem): DrawerAction[] {
  if (item.kind === 'message') {
    const relatedChatId = item.item.relatedChatId ?? (item.item.type === 'chat' ? item.item.id : undefined);

    if (relatedChatId) {
      return [
        { label: 'Open conversation', href: `/chat/${relatedChatId}`, emphasis: 'primary' },
      ];
    }

    if (item.item.relatedRoute) {
      return [
        {
          label: item.item.relatedRouteLabel ?? 'Open source page',
          href: item.item.relatedRoute,
          emphasis: 'primary',
        },
      ];
    }

    if (item.item.sourceHref) {
      return [
        {
          label: item.item.sourceLabel ?? 'Open source page',
          href: item.item.sourceHref,
          external: !!getExternalLinkCandidate(item.item.sourceHref),
          emphasis: 'primary',
        },
      ];
    }

    return [
      { label: 'Open messages', href: '/chat', emphasis: 'primary' },
    ];
  }

  if (item.kind === 'notification') {
    const actions: DrawerAction[] = [];

    if (item.item.sourceHref) {
      actions.push({
        label: item.item.sourceLabel ?? 'Open source page',
        href: item.item.sourceHref,
        external: !!getExternalLinkCandidate(item.item.sourceHref),
        emphasis: 'primary',
      });
    } else if (item.item.relatedRoute) {
      actions.push({
        label: item.item.relatedRouteLabel ?? 'Open source page',
        href: item.item.relatedRoute,
        emphasis: 'primary',
      });
    } else {
      actions.push({ label: 'Open source page', href: `/notifications?notification=${encodeURIComponent(item.item.id)}`, emphasis: 'primary' });
    }

    if (item.item.relatedChatId) {
      actions.push({ label: 'View related chat', href: `/chat/${item.item.relatedChatId}`, emphasis: 'secondary' });
    } else {
      actions.push({ label: 'View related chat', href: '/chat', emphasis: 'secondary' });
    }

    return actions;
  }

  const actions: DrawerAction[] = [];

  if (item.item.meetingUrl) {
    actions.push({ label: 'Join meeting', href: item.item.meetingUrl, external: true, emphasis: 'primary' });
  } else if (item.item.sourceHref) {
    actions.push({
      label: item.item.sourceLabel ?? 'Open source page',
      href: item.item.sourceHref,
      external: !!getExternalLinkCandidate(item.item.sourceHref),
      emphasis: 'primary',
    });
  } else {
    actions.push({ label: 'View event page', href: `/calendar?event=${encodeURIComponent(item.item.id)}`, emphasis: 'primary' });
  }

  if (item.item.relatedChatId) {
    actions.push({ label: 'View related chat', href: `/chat/${item.item.relatedChatId}`, emphasis: 'secondary' });
  } else {
    actions.push({ label: 'Open calendar page', href: `/calendar?event=${encodeURIComponent(item.item.id)}`, emphasis: 'secondary' });
  }

  return actions;
}

function DrawerContent({ item }: { item: SidebarDetailItem }) {
  if (item.kind === 'message') {
    const Icon = getMessageIcon(item.item.type);

    return (
      <>
        <div className="flex items-start gap-4">
          <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-black/8 bg-[#F2EBDD] text-black/65">
            <Icon className="h-5 w-5" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/36">{getMessageTypeLabel(item.item.type)}</div>
            <h3 id="sidebar-detail-title" className="mt-1 text-xl font-semibold text-black">{item.item.from.name}</h3>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-black/48">
              <span>{formatDateTime(item.item.timestamp)}</span>
              <span className={cn('rounded-full border px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.12em]', PRIORITY_STYLES[item.item.priority])}>
                {item.item.priority}
              </span>
            </div>
          </div>
        </div>

        {item.item.from.email?.trim() && (
          <div className="mt-6 rounded-[20px] border border-black/8 bg-[#F8F3E8] px-4 py-3">
            <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Sender</div>
            <div className="mt-2 text-sm text-black/72">{item.item.from.email}</div>
          </div>
        )}

        <div className="mt-6 rounded-3xl border border-black/8 bg-white/80 p-5 shadow-[inset_0_1px_0_rgba(255,255,255,0.76)]">
          <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Message</div>
          <p className="mt-3 whitespace-pre-wrap text-[15px] leading-7 text-black/76">{item.item.content}</p>
        </div>

        <div className="mt-6 rounded-[22px] border border-[#E6DDD1] bg-[#F6F1E8] px-4 py-4">
          <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Action</div>
          <div className="mt-3 text-sm text-black/62">Use the conversation route to continue this thread in context.</div>
        </div>
      </>
    );
  }

  if (item.kind === 'notification') {
    const Icon = getNotificationIcon(item.item.type);

    return (
      <>
        <div className="flex items-start gap-4">
          <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-black/8 bg-[#F2EBDD] text-black/65">
            <Icon className="h-5 w-5" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/36">{getNotificationTypeLabel(item.item.type)}</div>
            <h3 id="sidebar-detail-title" className="mt-1 text-xl font-semibold text-black">{item.item.title}</h3>
            <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-black/48">
              <span>{formatDateTime(item.item.timestamp)}</span>
              <span className={cn('rounded-full border px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.12em]', PRIORITY_STYLES[item.item.priority])}>
                {item.item.priority}
              </span>
              {!item.item.read && (
                <span className="rounded-full border border-[#FF2E63]/18 bg-[#FF2E63]/10 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.12em] text-[#B21846]">
                  unread
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="mt-6 rounded-3xl border border-black/8 bg-white/80 p-5 shadow-[inset_0_1px_0_rgba(255,255,255,0.76)]">
          <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Details</div>
          <p className="mt-3 whitespace-pre-wrap text-[15px] leading-7 text-black/76">{item.item.description}</p>
        </div>

        <div className="mt-6 rounded-[22px] border border-[#E6DDD1] bg-[#F6F1E8] px-4 py-4">
          <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Links</div>
          <div className="mt-3 flex items-center gap-2 text-sm text-black/62">
            <ExternalLink className="h-4 w-4 text-[#DD7A1F]" />
            Open the source or jump directly to the related conversation.
          </div>
        </div>
      </>
    );
  }

  const statusStyle = STATUS_STYLES[item.item.status] ?? STATUS_STYLES.past;

  return (
    <>
      <div className="flex items-start gap-4">
        <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-black/8 bg-[#F2EBDD] text-black/65">
          <CalendarIcon className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/36">Calendar Event</div>
          <h3 id="sidebar-detail-title" className="mt-1 text-xl font-semibold text-black">{item.item.title}</h3>
          <div className="mt-2 flex flex-wrap items-center gap-2 text-sm text-black/48">
            <span>{formatEventRange(item.item.start, item.item.end)}</span>
            <span className={cn('rounded-full border px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.12em]', statusStyle)}>
              {item.item.status}
            </span>
          </div>
        </div>
      </div>

      <div className="mt-6 grid gap-3">
        <div className="rounded-[20px] border border-black/8 bg-[#F8F3E8] px-4 py-3">
          <div className="flex items-start gap-3 text-sm text-black/72">
            <Clock className="mt-0.5 h-4 w-4 shrink-0 text-black/45" />
            <div>
              <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">When</div>
              <div className="mt-1 leading-6">{formatEventRange(item.item.start, item.item.end)}</div>
            </div>
          </div>
        </div>

        {item.item.location?.trim() && (
          <div className="rounded-[20px] border border-black/8 bg-[#F8F3E8] px-4 py-3">
            <div className="flex items-start gap-3 text-sm text-black/72">
              <MapPin className="mt-0.5 h-4 w-4 shrink-0 text-black/45" />
              <div>
                <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Location</div>
                <div className="mt-1 leading-6">{item.item.location}</div>
              </div>
            </div>
          </div>
        )}

        <div className="rounded-[20px] border border-black/8 bg-[#F8F3E8] px-4 py-3">
          <div className="flex items-start gap-3 text-sm text-black/72">
            <Users className="mt-0.5 h-4 w-4 shrink-0 text-black/45" />
            <div className="min-w-0 flex-1">
              <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Attendees</div>
              <div className="mt-2 flex flex-wrap gap-2">
                {item.item.attendees.length > 0 ? item.item.attendees.map((attendee) => (
                  <span
                    key={attendee}
                    className="rounded-full border border-black/8 bg-white/75 px-3 py-1 text-xs text-black/72"
                  >
                    {attendee}
                  </span>
                )) : (
                  <span className="text-sm text-black/56">No attendees listed</span>
                )}
              </div>
            </div>
          </div>
        </div>

        <div className="rounded-[20px] border border-[#E6DDD1] bg-[#F6F1E8] px-4 py-4">
          <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-black/34">Action</div>
          <div className="mt-3 flex items-center gap-2 text-sm text-black/62">
            <ArrowUpRight className="h-4 w-4 text-[#DD7A1F]" />
            Join the meeting when available, or open the event page for the full context.
          </div>
        </div>
      </div>
    </>
  );
}

export function SidebarItemDetailDrawer({ item, onClose }: SidebarItemDetailDrawerProps) {
  const router = useRouter();
  const dialogRef = React.useRef<HTMLDivElement>(null);
  const actions = React.useMemo(() => buildDrawerActions(item), [item]);

  React.useEffect(() => {
    dialogRef.current?.focus();
  }, []);

  React.useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  const handleActionClick = React.useCallback((action: DrawerAction) => {
    onClose();

    if (action.external && typeof window !== 'undefined') {
      window.open(action.href, '_blank', 'noopener,noreferrer');
      return;
    }

    router.push(action.href);
  }, [onClose, router]);

  return (
    <div
      className="fixed inset-0 z-210 flex items-center justify-end bg-[rgba(17,14,10,0.34)] backdrop-blur-[2px]"
      onClick={onClose}
      role="presentation"
      onKeyDown={(e) => { if (e.key === 'Escape') onClose(); }}
    >
      <div
        ref={dialogRef}
        className="relative flex h-full w-full max-w-[480px] flex-col border-l border-black/8 bg-[#FCF8F0] shadow-[-24px_0_60px_rgba(24,19,12,0.18)]"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="sidebar-detail-title"
        tabIndex={-1}
      >
        <div className="border-b border-black/8 bg-[#F7F4EE] px-5 py-5">
          <div className="mb-4 flex items-center gap-4 border-b border-black/8 pb-3">
            <button className="border-b-2 border-[#DD7A1F] pb-2 text-[12px] font-semibold text-black">Details</button>
            <div className="pb-2 text-[12px] font-medium text-black/40">Actions</div>
          </div>
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-[10px] font-semibold uppercase tracking-[0.26em] text-black/34">Inspector</div>
              <div className="mt-1 text-sm leading-6 text-black/52">Review the selected item, then jump to the most relevant destination from the action rail below.</div>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="rounded-full border border-black/8 bg-white/80 p-2 text-black/60 transition-colors hover:bg-white hover:text-black"
              aria-label="Close detail drawer"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        <div className="border-b border-black/8 bg-white px-5 py-4">
          <div className="grid gap-2">
            {actions.map((action, index) => (
              <button
                key={action.label}
                type="button"
                onClick={() => handleActionClick(action)}
                className={cn(
                  'flex w-full items-center justify-between rounded-[18px] border px-4 py-3 text-left transition-colors',
                  action.emphasis === 'primary'
                    ? 'border-[#161210] bg-[#161210] text-white hover:bg-[#26211D]'
                    : 'border-black/8 bg-[#FAF8F4] text-black hover:bg-[#F1ECE4]'
                )}
              >
                <div className="flex items-center gap-3">
                  <span className={cn(
                    'flex h-9 w-9 items-center justify-center rounded-xl border',
                    action.emphasis === 'primary'
                      ? 'border-white/10 bg-white/10 text-white'
                      : 'border-black/8 bg-white text-[#DD7A1F]'
                  )}>
                    {action.label.toLowerCase().includes('chat') || action.label.toLowerCase().includes('conversation') ? (
                      <MessageCircleMore className="h-4 w-4" />
                    ) : action.label.toLowerCase().includes('join') ? (
                      <ArrowUpRight className="h-4 w-4" />
                    ) : (
                      <ExternalLink className="h-4 w-4" />
                    )}
                  </span>
                  <div>
                    <div className="text-sm font-semibold">{action.label}</div>
                    <div className={cn('text-xs', action.emphasis === 'primary' ? 'text-white/62' : 'text-black/44')}>
                      {action.external ? 'Opens in a new tab' : 'Open in workspace'}
                    </div>
                  </div>
                </div>
                <ArrowUpRight className={cn('h-4 w-4', action.emphasis === 'primary' ? 'text-white/72' : 'text-black/35')} />
              </button>
            ))}
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-6 bg-[#FCF8F0]">
          <DrawerContent item={item} />
        </div>

        <div className="border-t border-black/8 bg-[#F7F4EE] px-5 py-4">
          <button
            type="button"
            onClick={onClose}
            className="inline-flex items-center justify-center rounded-full border border-black/10 bg-white px-4 py-2 text-sm font-medium text-black transition-colors hover:bg-[#F1ECE4]"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}