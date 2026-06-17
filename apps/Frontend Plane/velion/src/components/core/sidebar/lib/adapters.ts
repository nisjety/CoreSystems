import type { Notification as ApiNotification, NotificationFeed } from '@/lib/notifications/types';

import type { CalendarEvent, Notification } from '../types';

function parseDate(value: unknown, fallback = new Date()): Date {
  if (value instanceof Date && !Number.isNaN(value.getTime())) {
    return value;
  }

  if (typeof value === 'string' || typeof value === 'number') {
    const parsed = new Date(value);
    if (!Number.isNaN(parsed.getTime())) {
      return parsed;
    }
  }

  return fallback;
}

function mapEventTypeToUIType(
  eventType: ApiNotification['event_type'],
): Notification['type'] {
  switch (eventType) {
    case 'user_mentioned':
      return 'teams';
    case 'crawl_completed':
    case 'document_indexed':
    case 'team_invite_sent':
    default:
      return 'system';
  }
}

function mapPriority(_eventType: ApiNotification['event_type']): Notification['priority'] {
  return 'normal';
}

export function mapNotificationToSidebarNotification(
  notification: ApiNotification,
): Notification {
  return {
    id: notification.id,
    title: notification.title,
    description: notification.body,
    timestamp: parseDate(notification.created_at),
    read: notification.read,
    priority: mapPriority(notification.event_type),
    type: mapEventTypeToUIType(notification.event_type),
    sourceHref: notification.action_url,
  };
}

export function normalizeSidebarNotifications(data: unknown): Notification[] {
  const items = Array.isArray(data)
    ? data
    : data && typeof data === 'object' && Array.isArray((data as NotificationFeed).notifications)
      ? (data as NotificationFeed).notifications
      : [];

  return items
    .filter((item): item is ApiNotification => Boolean(item && typeof item === 'object'))
    .map(mapNotificationToSidebarNotification);
}

function normalizeAttendees(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value
    .map((attendee) => {
      if (typeof attendee === 'string') {
        return attendee;
      }

      if (attendee && typeof attendee === 'object') {
        const record = attendee as Record<string, unknown>;
        return (
          (typeof record.email === 'string' && record.email) ||
          (typeof record.name === 'string' && record.name) ||
          (typeof record.displayName === 'string' && record.displayName) ||
          ''
        );
      }

      return '';
    })
    .filter(Boolean);
}

function deriveCalendarStatus(
  start: Date,
  end: Date,
): CalendarEvent['status'] {
  const now = Date.now();

  if (end.getTime() < now) {
    return 'past';
  }

  if (start.getTime() <= now && end.getTime() >= now) {
    return 'ongoing';
  }

  return 'upcoming';
}

function mapCalendarEvent(value: unknown): CalendarEvent | null {
  if (!value || typeof value !== 'object') {
    return null;
  }

  const record = value as Record<string, unknown>;
  const start = parseDate(
    record.start ?? record.startTime ?? record.start_at ?? record.begin,
    new Date(),
  );
  const end = parseDate(
    record.end ?? record.endTime ?? record.end_at ?? record.finish,
    start,
  );

  return {
    id:
      (typeof record.id === 'string' && record.id) ||
      (typeof record.eventId === 'string' && record.eventId) ||
      `${start.getTime()}-${String(record.title ?? record.subject ?? 'event')}`,
    title:
      (typeof record.title === 'string' && record.title) ||
      (typeof record.subject === 'string' && record.subject) ||
      'Untitled event',
    start,
    end,
    location:
      (typeof record.location === 'string' && record.location) ||
      (record.location && typeof record.location === 'object'
        ? (record.location as Record<string, unknown>).displayName as string | undefined
        : undefined),
    description:
      (typeof record.description === 'string' && record.description) ||
      (typeof record.body === 'string' && record.body) ||
      undefined,
    attendees: normalizeAttendees(record.attendees ?? record.participants),
    status: deriveCalendarStatus(start, end),
    type:
      typeof record.type === 'string' &&
      ['meeting', 'event', 'reminder'].includes(record.type)
        ? (record.type as CalendarEvent['type'])
        : ((record.meetingUrl || record.onlineMeetingUrl || record.joinUrl)
            ? 'meeting'
            : 'event'),
    meetingUrl:
      (typeof record.meetingUrl === 'string' && record.meetingUrl) ||
      (typeof record.onlineMeetingUrl === 'string' && record.onlineMeetingUrl) ||
      (typeof record.joinUrl === 'string' && record.joinUrl) ||
      undefined,
    sourceHref:
      (typeof record.sourceHref === 'string' && record.sourceHref) ||
      (typeof record.webLink === 'string' && record.webLink) ||
      (typeof record.url === 'string' && record.url) ||
      undefined,
    sourceLabel:
      (typeof record.sourceLabel === 'string' && record.sourceLabel) ||
      (typeof record.provider === 'string' && record.provider) ||
      undefined,
    relatedChatId:
      (typeof record.relatedChatId === 'string' && record.relatedChatId) ||
      undefined,
    relatedRoute:
      (typeof record.relatedRoute === 'string' && record.relatedRoute) ||
      undefined,
    relatedRouteLabel:
      (typeof record.relatedRouteLabel === 'string' && record.relatedRouteLabel) ||
      undefined,
  };
}

export function normalizeSidebarCalendarEvents(data: unknown): CalendarEvent[] {
  const items: unknown[] = Array.isArray(data)
    ? data
    : data && typeof data === 'object'
      ? (
          (Array.isArray((data as Record<string, unknown>).events) &&
            (data as Record<string, unknown>).events) ||
          (Array.isArray((data as Record<string, unknown>).items) &&
            (data as Record<string, unknown>).items) ||
          (Array.isArray((data as Record<string, unknown>).data) &&
            (data as Record<string, unknown>).data) ||
          []
        ) as unknown[]
      : [];

  return items
    .map(mapCalendarEvent)
    .filter((event): event is CalendarEvent => event !== null);
}
