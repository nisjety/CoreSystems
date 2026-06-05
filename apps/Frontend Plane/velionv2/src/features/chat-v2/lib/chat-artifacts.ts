/**
 * Artifact rendering helpers (chat-parity §2).
 */

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
