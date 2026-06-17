import 'server-only';

import { listNotifications } from '@/lib/notifications/client';
import {
  normalizeSidebarCalendarEvents,
  normalizeSidebarNotifications,
} from '@/components/core/sidebar/lib/adapters';

import type { CalendarEvent, Notification } from '@/components/core/sidebar/types';

const USER_SERVICE_URL = process.env.USER_SERVICE_URL ?? 'http://user-core:3012';

interface SidebarActor {
  userId: string;
  email?: string;
  name?: string;
  cookieHeader?: string;
}

function getInternalApiKey(): string | null {
  return process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? null;
}

export async function getServerSidebarNotifications(userId: string): Promise<Notification[]> {
  try {
    const feed = await listNotifications(userId, { limit: 50 });
    return normalizeSidebarNotifications(feed);
  } catch {
    return [];
  }
}

export async function getServerSidebarCalendarEvents(
  actor: SidebarActor,
): Promise<CalendarEvent[]> {
  const internalApiKey = getInternalApiKey();

  if (!internalApiKey) {
    return [];
  }

  const headers = new Headers({
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': internalApiKey,
    'X-User-Id': actor.userId,
  });

  if (actor.email) {
    headers.set('X-User-Email', actor.email);
  }

  if (actor.name) {
    headers.set('X-User-Name', actor.name);
  }

  if (actor.cookieHeader) {
    headers.set('Cookie', actor.cookieHeader);
  }

  try {
    const response = await fetch(`${USER_SERVICE_URL}/api/v1/calendar/events`, {
      headers,
      cache: 'no-store',
      signal: AbortSignal.timeout(5000),
    });

    if (!response.ok) {
      return [];
    }

    const payload = await response.json();
    return normalizeSidebarCalendarEvents(payload);
  } catch {
    return [];
  }
}
