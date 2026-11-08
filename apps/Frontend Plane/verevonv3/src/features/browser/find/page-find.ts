/**
 * Page find over the browser session's already-loaded DOM nodes (slice 6
 * presentation slice, Codex "find in page" parity). Pure client-side search:
 * no server call, no Quarry action, no persistence — it only reads the
 * `DomNodeList` data the chrome already holds, so ZDR posture is unaffected.
 * Matching is case-insensitive on the node `text`. The frame itself is an
 * image; "matches" are listed by kind + text (never a fake in-image highlight).
 */
export type FindableNode = {
  kind: string
  text: string
}

export type FindMatch = {
  index: number
  kind: string
  text: string
}

/** All nodes whose text contains `query` (case-insensitive). Empty query → []. */
export function findInDomNodes(nodes: FindableNode[], query: string): FindMatch[] {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return []
  return nodes
    .map((node, index) => ({ index, kind: node.kind, text: node.text }))
    .filter((node) => node.text.toLowerCase().includes(normalized))
}

/** Wrap-around step: `stepFindIndex(0, 3, 1) === 1`, `stepFindIndex(2, 3, 1) === 0`. */
export function stepFindIndex(current: number, total: number, direction: 1 | -1): number {
  if (total <= 0) return 0
  const next = current + direction
  if (next >= total) return 0
  if (next < 0) return total - 1
  return next
}
