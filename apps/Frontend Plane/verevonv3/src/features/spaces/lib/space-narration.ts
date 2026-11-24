import type { SpaceThread } from '@/shared/api/spaces-client'
import type { AnnounceOptions } from '@/shared/a11y/announcer'

import { mentionPresent } from './mention-model'
import type { Tr } from './space-copy'
import { ACTIVE_RUN_STATUSES, AWAITING_APPROVAL_RUN_STATUS, FAILED_RUN_STATUSES, threadTitle } from './space-thread-presentation'

/**
 * What a screen-reader user hears when the room changes under them (WS16-S4).
 *
 * Pure: each narrator compares the previous listing with the next one and returns
 * the announcements to make; the caller hands them to `announce()`. Nothing here
 * touches the DOM, storage or the network, and no id is ever put into a message.
 *
 * A first listing (no previous) is a baseline, never news, so opening a room does
 * not read it out. An empty previous listing is a room that was empty when it was
 * read, so its first post IS news. The viewer's own posts are never narrated,
 * and when the viewer is unknown nothing about posts is said (an unknown author
 * is not "you", and an own post must not be announced back).
 */

export interface Announcement {
  readonly message: string
  readonly options: AnnounceOptions
}

export interface NarrationContext {
  readonly roomName: string
  /** The display name for a Control subject id (the caller supplies the roster lookup). */
  readonly nameOf: (subjectId: string | undefined) => string
  /**
   * The viewer's own display name, the label a mention of them is typed with (`@Kari`). Absent when the
   * roster does not know it, and then no post is narrated as a mention.
   */
  readonly viewerLabel?: string
}

function postsGroup(roomName: string): string {
  return `posts:${roomName}`
}

/**
 * Whether the newest message of a listed post names the viewer as `@label`. The listing carries no mention ids,
 * so this reads the preview, the text the composer wrote, which is the message that just arrived. The title is
 * not read: it is set once and would make every later reply look like a mention.
 */
function mentionsViewer(thread: SpaceThread, viewerLabel: string | undefined): boolean {
  const label = viewerLabel?.trim()
  if (!label || !thread.preview) return false
  return mentionPresent(thread.preview, label)
}

/** Posts and replies by other members that appeared between two listings, and the posts that name the viewer. */
export function narrateNewPosts(
  prev: readonly SpaceThread[] | undefined,
  next: readonly SpaceThread[],
  viewerSubjectId: string | undefined,
  tr: Tr,
  context: NarrationContext,
): Announcement[] {
  const viewer = viewerSubjectId?.trim()
  if (!viewer || !prev) return []
  const before = new Map(prev.map((thread) => [thread.thread_id, thread]))
  const out: Announcement[] = []
  const group = postsGroup(context.roomName)
  const summarise = (count: number) =>
    tr(`${count} nye meldinger i ${context.roomName}`, `${count} new messages in ${context.roomName}`)
  for (const thread of next) {
    const old = before.get(thread.thread_id)
    const title = threadTitle(thread, tr)
    if (!old) {
      const author = thread.owner_subject_id?.trim()
      if (!author || author === viewer) continue
      if (mentionsViewer(thread, context.viewerLabel)) {
        // A mention is said on its own, never folded into the room's summary, so it cannot be lost in a burst.
        out.push({
          message: tr(`${context.nameOf(author)} nevnte deg i ${title}`, `${context.nameOf(author)} mentioned you in ${title}`),
          options: { politeness: 'polite', key: `mention:${thread.thread_id}:${thread.reply_count ?? 0}`, kind: 'mention' },
        })
        continue
      }
      out.push({
        message: tr(`Nytt innlegg fra ${context.nameOf(author)}: ${title}`, `New post from ${context.nameOf(author)}: ${title}`),
        options: { politeness: 'polite', key: `post:${thread.thread_id}`, kind: 'post', group, summarise },
      })
      continue
    }
    const replied = (thread.reply_count ?? 0) > (old.reply_count ?? 0)
    const author = thread.last_author_subject_id?.trim()
    if (replied && author && author !== viewer) {
      if (mentionsViewer(thread, context.viewerLabel)) {
        out.push({
          message: tr(`${context.nameOf(author)} nevnte deg i ${title}`, `${context.nameOf(author)} mentioned you in ${title}`),
          options: { politeness: 'polite', key: `mention:${thread.thread_id}:${thread.reply_count ?? 0}`, kind: 'mention' },
        })
        continue
      }
      out.push({
        message: tr(`${context.nameOf(author)} svarte i ${title}`, `${context.nameOf(author)} replied in ${title}`),
        options: {
          politeness: 'polite',
          key: `reply:${thread.thread_id}:${thread.reply_count ?? 0}`,
          kind: 'post',
          group,
          summarise,
        },
      })
    }
  }
  return out
}

/** A run that now needs approval (assertive), failed, or completed (polite), once per change. */
export function narrateRunChange(
  prev: readonly SpaceThread[] | undefined,
  next: readonly SpaceThread[],
  tr: Tr,
): Announcement[] {
  if (!prev || prev.length === 0) return []
  const before = new Map(prev.map((thread) => [thread.thread_id, thread]))
  const out: Announcement[] = []
  for (const thread of next) {
    const old = before.get(thread.thread_id)
    const status = thread.latest_run_status
    if (!old || !status) continue
    const oldStatus = old.latest_run_status
    if (oldStatus === status && old.latest_run_id === thread.latest_run_id) continue
    const title = threadTitle(thread, tr)
    const runKey = thread.latest_run_id ?? thread.thread_id
    if (status === AWAITING_APPROVAL_RUN_STATUS) {
      out.push({
        message: tr(`Venter på godkjenning: ${title}`, `Waiting for approval: ${title}`),
        options: { politeness: 'assertive', key: `approval:${runKey}`, coalesceMs: 10_000 },
      })
    } else if (FAILED_RUN_STATUSES.has(status)) {
      out.push({
        message: tr(`Kjøringen feilet: ${title}`, `Run failed: ${title}`),
        options: { politeness: 'polite', key: `run:${runKey}:failed`, coalesceMs: 10_000, kind: 'status' },
      })
    } else if (status === 'completed' && oldStatus !== undefined && ACTIVE_RUN_STATUSES.has(oldStatus)) {
      out.push({
        message: tr(`Kjøringen er fullført: ${title}`, `Run completed: ${title}`),
        options: { politeness: 'polite', key: `run:${runKey}:completed`, coalesceMs: 10_000, kind: 'status' },
      })
    }
  }
  return out
}

/** A room moved in the sidebar (star, section, hide), said once the move has happened. */
export function narrateSidebarMove(tr: Tr, roomName: string, destination: string): Announcement {
  return {
    message: tr(`${roomName} flyttet til ${destination}`, `${roomName} moved to ${destination}`),
    options: { politeness: 'polite', kind: 'status' },
  }
}
