"use client";

import { useEffect, useState } from "react";
import { apiGet, apiSend } from "@/lib/api/client-envelope";

export type NavbarProfile = {
  id: string;
  name: string;
  email?: string;
  avatar?: string;
  status: string;
};

export type NavbarNotification = {
  id: string;
  title: string;
  body: string;
  href?: string;
  createdAt?: string;
  read: boolean;
  seen: boolean;
  archived: boolean;
  feed?: string;
  source: "notification" | "message";
};

export type NotificationPayload = {
  configured: boolean;
  unreadCount: number;
  notifications: NavbarNotification[];
  messages: NavbarNotification[];
};

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

export type CalendarState = {
  events: CalendarEvent[];
  notes: CalendarNote[];
};

export type ThemePayload = {
  configured?: boolean;
  theme: "light" | "dark" | "system";
};

export type SearchResult = {
  id: string;
  label: string;
  excerpt: string;
  href: string;
  source: string;
};

type NavbarPayload = {
  calendar: CalendarState;
  notifications: NotificationPayload;
  profile: NavbarProfile | null;
  theme: ThemePayload | null;
};

type UseNavbarDataOptions = {
  onProfileChange: (profile: NavbarProfile) => void;
  onThemeChange: (theme: ThemePayload["theme"]) => void;
};

const emptyNotifications: NotificationPayload = {
  configured: false,
  unreadCount: 0,
  notifications: [],
  messages: [],
};

const emptyCalendar: CalendarState = {
  events: [],
  notes: [],
};

export function useNavbarData({ onProfileChange, onThemeChange }: UseNavbarDataOptions) {
  const [notifications, setNotifications] = useState<NotificationPayload>(emptyNotifications);
  const [calendar, setCalendar] = useState<CalendarState>(emptyCalendar);

  useEffect(() => {
    let cancelled = false;
    let refreshTimer: number | undefined;
    let controller: AbortController | null = null;

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

      setNotifications(payload.notifications);
      setCalendar(payload.calendar);

      if (payload.theme?.configured !== false && payload.theme?.theme) {
        onThemeChange(payload.theme.theme);
      }
    };

    async function loadNavbarData() {
      controller?.abort();
      controller = new AbortController();

      try {
        const payload = await getNavbarData(controller.signal);
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
      controller?.abort();
      window.clearTimeout(refreshTimer);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [onProfileChange, onThemeChange]);

  return {
    calendar,
    notifications,
    setCalendar,
    setNotifications,
  };
}

async function getNavbarData(signal?: AbortSignal): Promise<NavbarPayload> {
  const [profileResult, notificationResult, calendarResult, themeResult] = await Promise.allSettled([
    apiGet<NavbarProfile | null>("/api/v1/navbar/profile", { signal }),
    apiGet<NotificationPayload>("/api/v1/navbar/notifications", { signal }),
    apiGet<CalendarState>("/api/v1/navbar/calendar", { signal }),
    apiGet<ThemePayload>("/api/v1/navbar/theme", { signal }),
  ]);

  return {
    profile: profileResult.status === "fulfilled" ? profileResult.value : null,
    notifications: notificationResult.status === "fulfilled" ? notificationResult.value : emptyNotifications,
    calendar: calendarResult.status === "fulfilled" ? calendarResult.value : emptyCalendar,
    theme: themeResult.status === "fulfilled" ? themeResult.value : null,
  };
}

export function saveNavbarTheme(theme: ThemePayload["theme"]) {
  return apiSend("/api/v1/navbar/theme", { theme }, "PUT");
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
    `/api/v1/navbar/search?q=${encodeURIComponent(query)}`,
    { signal },
  );
}
