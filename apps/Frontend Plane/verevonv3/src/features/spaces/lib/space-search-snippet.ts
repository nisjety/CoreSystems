/**
 * Snippet segments for one search hit (WS13-S5).
 *
 * The server sends a snippet as `[{ text, matched }]` segments, never HTML, and the room renders each
 * segment as text (a `<mark>` for a matched one). These helpers only tidy the list: they never build
 * markup, so there is nothing to escape and nothing a hostile message can inject.
 */

import type { SpaceSearchSnippetSegment } from '@/shared/api/spaces-client'

/** Joins neighbouring segments of the same kind and drops empty ones. */
export function normalizeSnippet(
  segments: readonly SpaceSearchSnippetSegment[],
): readonly SpaceSearchSnippetSegment[] {
  const out: SpaceSearchSnippetSegment[] = []
  for (const segment of segments) {
    if (!segment.text) continue
    const last = out[out.length - 1]
    if (last && last.matched === segment.matched) {
      out[out.length - 1] = { text: last.text + segment.text, matched: last.matched }
    } else {
      out.push({ text: segment.text, matched: segment.matched })
    }
  }
  return out
}

/** The snippet as one plain string, for an accessible name. Newlines collapse to spaces. */
export function snippetPlainText(segments: readonly SpaceSearchSnippetSegment[]): string {
  return segments
    .map((segment) => segment.text)
    .join('')
    .replace(/\s+/g, ' ')
    .trim()
}
