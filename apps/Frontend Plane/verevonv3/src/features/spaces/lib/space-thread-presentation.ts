import type { SpaceThread } from '@/shared/api/spaces-client'

/**
 * Presentation vocabulary for a Space thread, shared by the room timeline,
 * the page header, and the pulse rail so a status never renders with two
 * different words in two corners of the same room.
 */

export const ACTIVE_RUN_STATUSES = new Set(['queued', 'running', 'awaiting_approval'])

export const FAILED_RUN_STATUSES = new Set(['failed'])

export function threadTitle(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  return thread.title?.trim() || thread.preview?.trim() || tr('Samtale uten tittel', 'Untitled conversation')
}

export function threadStatus(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  const status = thread.latest_run_status
  if (!status) return tr('Samtale åpen', 'Conversation open')
  if (status === 'awaiting_approval') return tr('Venter på godkjenning', 'Needs approval')
  if (status === 'running') return tr('Arbeider', 'Working')
  if (status === 'queued') return tr('I kø', 'Queued')
  if (status === 'completed') return tr('Fullført', 'Completed')
  if (status === 'failed') return tr('Feilet', 'Failed')
  return formatLabel(status)
}

// Humanizes a raw server enum token that has no translated dictionary here.
export function formatLabel(value: string): string {
  return value
    .trim()
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase())
}

// Strips the markdown decoration a stored assistant reply carries (**bold**,
// `code`, #headings, list/quote markers) so a one-line preview reads as plain
// prose instead of showing literal punctuation. The full reply still renders
// as real markdown wherever it's read in full.
export function stripMarkdownPreview(content: string): string {
  return content
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/^[-*+]\s+/gm, '')
    .replace(/^>\s?/gm, '')
    .replace(/(\*\*|__)(.*?)\1/g, '$2')
    .replace(/(\*|_)(.*?)\1/g, '$2')
    .replace(/`([^`]*)`/g, '$1')
    .replace(/\s+/g, ' ')
    .trim()
}

export function formatWhen(value: string): string {
  try {
    return new Date(value).toLocaleDateString('nb-NO', { day: 'numeric', month: 'short' })
  } catch {
    return value
  }
}
