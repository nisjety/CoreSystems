import type { RequestActor } from "@/lib/integrations/request-actor";

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

type NovuFeedItem = {
  _id?: string;
  _notificationId?: string;
  _feedId?: string | null;
  content?: string;
  subject?: string | null;
  title?: string;
  read?: boolean;
  seen?: boolean;
  archived?: boolean;
  createdAt?: string | null;
  cta?: { data?: { url?: string } };
  payload?: Record<string, unknown>;
  data?: Record<string, unknown> | null;
};

type NovuFeedResponse = {
  totalCount?: number;
  hasMore?: boolean;
  data?: NovuFeedItem[];
};

function getNovuBaseUrl() {
  return (process.env.NOVU_API_URL ?? "https://api.novu.co").replace(/\/+$/, "");
}

function getNovuSecret() {
  return process.env.NOVU_SECRET_KEY ?? process.env.NOVU_API_KEY;
}

function normalizeHref(href: unknown) {
  if (typeof href !== "string" || !href.startsWith("/")) {
    return undefined;
  }
  return href;
}

function inferSource(item: NovuFeedItem): "notification" | "message" {
  const feed = `${item._feedId ?? ""}`.toLowerCase();
  const rawSource =
    typeof item.data?.source === "string"
      ? item.data.source
      : typeof item.payload?.source === "string"
        ? item.payload.source
        : "";
  const source = rawSource.toLowerCase();

  if (
    feed.includes("message") ||
    feed.includes("inbox") ||
    feed.includes("chat") ||
    source.includes("inbox") ||
    source.includes("chat") ||
    source.includes("verevon_ai")
  ) {
    return "message";
  }

  return "notification";
}

export async function listNovuNotifications(actor: RequestActor, limit = 20) {
  const secret = getNovuSecret();

  if (!secret) {
    return {
      configured: false,
      unreadCount: 0,
      notifications: [] as NavbarNotification[],
      messages: [] as NavbarNotification[],
    };
  }

  const url = new URL(`${getNovuBaseUrl()}/v1/subscribers/${encodeURIComponent(actor.userId)}/notifications/feed`);
  url.searchParams.set("page", "0");
  url.searchParams.set("limit", String(Math.min(100, Math.max(1, limit))));

  const response = await fetch(url, {
    headers: {
      Authorization: `ApiKey ${secret}`,
      "Content-Type": "application/json",
    },
    cache: "no-store",
    signal: AbortSignal.timeout(5000),
  });

  if (!response.ok) {
    throw new Error(`Novu returned ${response.status}`);
  }

  const payload = (await response.json()) as NovuFeedResponse;
  const notifications = (payload.data ?? []).map((item) => {
    const source = inferSource(item);
    return {
      id: item._id ?? item._notificationId ?? crypto.randomUUID(),
      title: item.subject ?? item.title ?? (source === "message" ? "Message" : "Notification"),
      body: item.content ?? "",
      href: normalizeHref(item.cta?.data?.url ?? item.data?.href ?? item.payload?.href),
      createdAt: item.createdAt ?? undefined,
      read: Boolean(item.read),
      seen: Boolean(item.seen),
      archived: Boolean(item.archived),
      feed: item._feedId ?? undefined,
      source,
    } satisfies NavbarNotification;
  });

  const activeNotifications = notifications.filter((item) => !item.archived);

  return {
    configured: true,
    unreadCount: activeNotifications.filter((item) => !item.read).length,
    notifications: activeNotifications.filter((item) => item.source === "notification"),
    messages: activeNotifications.filter((item) => item.source === "message"),
  };
}

export async function markNovuNotificationRead(actor: RequestActor, notificationId: string) {
  const secret = getNovuSecret();

  if (!secret) {
    return { configured: false };
  }

  const response = await fetch(
    `${getNovuBaseUrl()}/v1/subscribers/${encodeURIComponent(actor.userId)}/messages/mark-as`,
    {
      method: "POST",
      headers: {
        Authorization: `ApiKey ${secret}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ messageId: notificationId, markAs: "read" }),
      cache: "no-store",
      signal: AbortSignal.timeout(5000),
    },
  );

  if (!response.ok) {
    throw new Error(`Novu returned ${response.status}`);
  }

  return { configured: true };
}
