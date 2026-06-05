/**
 * Artifact rendering helpers (chat-parity §2).
 */

import type { ChatArtifact, ChatMessage } from "@/features/chat-v2/lib/chat-workspace";

/**
 * Select the most recent image artifact across a thread's messages — the
 * "current screen" for a live agent/computer-use view (chat-parity Phase 3).
 * Later messages and later artifacts within a message win. Returns null when
 * the run has produced no screenshot yet.
 */
export function selectLatestImageArtifact(messages: ChatMessage[]): ChatArtifact | null {
  let latest: ChatArtifact | null = null;
  for (const message of messages) {
    for (const artifact of message.artifacts ?? []) {
      if (artifact.kind.toLowerCase() === "image") {
        latest = artifact;
      }
    }
  }
  return latest;
}

/**
 * Resolve an image artifact's `content` to a usable `<img>` src. Passes through
 * http(s) and data URLs; wraps bare base64 as a PNG data URL. Image-generation
 * (`GenerateImage` → `artifact` event) emits one of these forms.
 */
export function imageArtifactSrc(content: string): string {
  const trimmed = content.trim();
  if (
    trimmed.startsWith("http://") ||
    trimmed.startsWith("https://") ||
    trimmed.startsWith("data:")
  ) {
    return trimmed;
  }
  return `data:image/png;base64,${trimmed}`;
}
