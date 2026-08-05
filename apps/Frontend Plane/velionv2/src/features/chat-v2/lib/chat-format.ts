import type { ComposerToolId } from "@/features/chat-v2/components/VerevonComposer";

export const toolLabels: Record<ComposerToolId, string> = {
  search: "Search",
  reason: "Reason",
  research: "Deep research",
  image: "Create image",
};

const relativeDateFormat = new Intl.DateTimeFormat("en", { month: "short", day: "numeric" });
const messageTimeFormat = new Intl.DateTimeFormat("en", { hour: "2-digit", minute: "2-digit" });

export function formatRelative(value: string) {
  const timestamp = new Date(value).getTime();
  const delta = Date.now() - timestamp;
  const minutes = Math.max(0, Math.round(delta / 60000));

  if (minutes < 1) {
    return "Just now";
  }

  if (minutes < 60) {
    return `${minutes}m ago`;
  }

  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }

  return relativeDateFormat.format(new Date(value));
}

export function formatTime(value: string) {
  return messageTimeFormat.format(new Date(value));
}
