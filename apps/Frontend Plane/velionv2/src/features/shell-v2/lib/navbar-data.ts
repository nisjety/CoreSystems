"use client";

import { startTransition, useEffect, useState } from "react";
import { apiGet, apiSend } from "@/lib/api/client-envelope";
import {
  emptyCalendar,
  emptyNotifications,
  type CalendarEvent,
  type CalendarNote,
  type CalendarState,
  type NavbarPayload,
  type NavbarProfile,
  type NotificationPayload,
  type ThemePayload,
} from "@/features/shell-v2/lib/navbar-types";

export type {
  CalendarEvent,
  CalendarNote,
  CalendarState,
  NavbarNotification,
  NavbarPayload,
  NavbarProfile,
  NotificationPayload,
  ThemePayload,
} from "@/features/shell-v2/lib/navbar-types";

export type SearchResult = {
  id: string;
  label: string;
  excerpt: string;
  href: string;
  source: string;
};

type UseNavbarDataOptions = {
  initialData?: NavbarPayload | null;
  onProfileChange: (profile: NavbarProfile) => void;
  onThemeChange: (theme: ThemePayload["theme"]) => void;
};

let pendingNavbarLoad: Promise<NavbarPayload> | null = null;

export function useNavbarData({ initialData, onProfileChange, onThemeChange }: UseNavbarDataOptions) {
  const [notifications, setNotifications] = useState<NotificationPayload>(initialData?.notifications ?? emptyNotifications);
  const [calendar, setCalendar] = useState<CalendarState>(initialData?.calendar ?? emptyCalendar);

  useEffect(() => {
    let cancelled = false;
    let refreshTimer: number | undefined;

    if (initialData?.profile) {
      onProfileChange(initialData.profile);
    }

    if (initialData?.theme?.configured !== false && initialData?.theme?.theme) {
      onThemeChange(initialData.theme.theme);
    }

    const scheduleRefresh = () => {
      if (cancelled) {
        return;
      }

      window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(loadNavbarData, document.hidden ? 180_000 : 60_000);
    };

    const applyPayload = (payload: NavbarPayload) => {
      if (payload.profile) {
        onProfileChange(payload.profile);
      }

      startTransition(() => {
        setNotifications(payload.notifications);
        setCalendar(payload.calendar);
      });

      if (payload.theme?.configured !== false && payload.theme?.theme) {
        onThemeChange(payload.theme.theme);
      }
    };

    async function loadNavbarData() {
      try {
        const payload = await getNavbarData();
        if (!cancelled) {
          applyPayload(payload);
        }
      } catch {
        // Missing local integration services should not stall shell rendering.
      }

      scheduleRefresh();
    }

    const handleVisibilityChange = () => {
      if (!document.hidden) {
        void loadNavbarData();
      }
    };

    void loadNavbarData();
    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      cancelled = true;
      window.clearTimeout(refreshTimer);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [initialData, onProfileChange, onThemeChange]);

  return {
    calendar,
    notifications,
    setCalendar,
    setNotifications,
  };
}

async function getNavbarData(): Promise<NavbarPayload> {
  pendingNavbarLoad ??= apiGet<NavbarPayload>("/api/v1/navbar")
    .finally(() => {
      pendingNavbarLoad = null;
    });

  return pendingNavbarLoad;
}

export function saveNavbarTheme(theme: ThemePayload["theme"], colorScheme?: string | null) {
  return apiSend("/api/v1/navbar/theme", { colorScheme, theme }, "PUT");
}

export function markNavbarNotificationRead(notificationId: string) {
  return apiSend("/api/v1/navbar/notifications", { notificationId });
}

export function createNavbarCalendarEvent(input: {
  end: string;
  start: string;
  title: string;
  type: string;
}) {
  return apiSend<{ event: CalendarEvent }>("/api/v1/navbar/calendar", input);
}

export function createNavbarCalendarNote(input: {
  date: string;
  kind: "note";
  text: string;
}) {
  return apiSend<{ note: CalendarNote }>("/api/v1/navbar/calendar", input);
}

export function submitNavbarSupportRequest(input: {
  context: string;
  message: string;
  subject: string;
}) {
  return apiSend("/api/v1/navbar/support", input);
}

export function searchNavbar(query: string, signal?: AbortSignal) {
  return apiGet<{ results: SearchResult[] }>(
    `/api/v1/navbar/search?q=${encodeURIComponent(query)}&scope=knowledge`,
    { signal },
  );
}
