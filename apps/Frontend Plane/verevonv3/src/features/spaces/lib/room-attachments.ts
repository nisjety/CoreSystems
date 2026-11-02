import {
  appendInlinedAttachments,
  inlineTextAttachments,
  splitInlinedAttachments,
  toStreamAttachments,
  type InlinedAttachment,
} from '@/features/chat/components/chat-normalizers'
import type { ComposerAttachment } from '@/features/chat/components/chat-types'
import {
  type AttachmentFileLike,
  type AttachmentPolicyContext,
  type AttachmentRefusal,
  attachmentRefusalMessage,
  evaluateAttachments,
  isImageAttachment,
} from '@/features/chat/lib/attachment-policy'
import { prepareChatAttachments } from '@/features/chat/lib/prepare-chat-attachments'
import type { ChatAttachment } from '@/shared/api/chat-client'
import { zdrRefusalPresentation } from '@/shared/api/zdr-refusals'

/**
 * Per-turn attachments for the room composer (WS14-S8).
 *
 * Phase 1 of room files: nothing here is stored. A file is read in the browser, its text rides
 * inside the member's message under the labelled fence (`appendInlinedAttachments`), and an image
 * rides the request's `attachments` field. The durable room file library is WS14-S3/S9 and is not
 * touched. Presentation and preparation only: the room, Control and the model gateway still decide
 * what is accepted, and a zero-retention turn is refused by the server whatever this module says.
 */

/** A paste longer than this (in UTF-16 code units) becomes a chip instead of landing in the box. */
export const ROOM_PASTE_ATTACHMENT_THRESHOLD = 4000

/** Files in one turn. Mirrors the durable upload's 1..10 so the chip row never outgrows a later Files tab. */
export const ROOM_TURN_MAX_FILES = 10

/** A chip in the composer: the same record the main composer carries, with an in-memory `url`. */
export type RoomAttachment = ComposerAttachment

export interface RoomAttachmentVerdict<T extends AttachmentFileLike = File> {
  readonly accepted: T[]
  /** One localized sentence per refused file, ready to show. */
  readonly messages: string[]
}

type Translate = (noText: string, enText: string) => string

/**
 * Splits picked files into the ones this turn can carry and the reasons for the rest. The shared
 * attachment policy decides type, size, image count and route; this adds the per-turn file ceiling
 * and states a zero-retention refusal in the typed ZDR vocabulary instead of the generic line.
 */
export function vetRoomAttachments<T extends AttachmentFileLike>(
  incoming: readonly T[],
  context: AttachmentPolicyContext,
  currentFileCount: number,
  tr: Translate,
): RoomAttachmentVerdict<T> {
  const room = Math.max(0, ROOM_TURN_MAX_FILES - currentFileCount)
  const policy = evaluateAttachments(incoming, context)
  const accepted = policy.accepted.slice(0, room)
  const overflow = policy.accepted.slice(room)
  const messages = policy.refused.map((entry) => refusalMessage(entry, tr))
  if (overflow.length > 0) {
    messages.push(
      tr(
        `Maks. ${ROOM_TURN_MAX_FILES} vedlegg per melding. ${overflow.length} ble ikke lagt ved.`,
        `At most ${ROOM_TURN_MAX_FILES} attachments per message. ${overflow.length} were not attached.`,
      ),
    )
  }
  return { accepted, messages }
}

function refusalMessage(entry: AttachmentRefusal, tr: Translate): string {
  if (entry.reason === 'zdr_refused') {
    const shown = zdrRefusalPresentation('zdr_feature_not_supported', 'attachments', tr)
    return shown.next ? `${shown.message} ${shown.next}` : shown.message
  }
  return attachmentRefusalMessage(entry, tr)
}

/** A paste long enough to become an attachment chip rather than text in the box. */
export function pasteBecomesAttachment(text: string): boolean {
  return text.length > ROOM_PASTE_ATTACHMENT_THRESHOLD
}

/** The pasted text as a `.txt` file, named from its opening words. */
export function pastedTextFile(text: string, label: string): File {
  const preview = text.trim().replace(/\s+/g, ' ').slice(0, 60)
  const name = preview ? `${label}: ${preview}${text.trim().length > preview.length ? '…' : ''}.txt` : `${label}.txt`
  return new File([text], name, { type: 'text/plain' })
}

/** The image files on a paste event, if any. */
export function pastedImageFiles(data: DataTransfer | null | undefined): File[] {
  return Array.from(data?.files ?? []).filter((file) => file.type.startsWith('image/'))
}

/** The chip record for a picked file; its `url` is an object URL the composer revokes on removal. */
export function toRoomAttachment(file: File, index: number): RoomAttachment {
  return {
    id: `${file.name}-${file.size}-${file.lastModified}-${Date.now()}-${index}`,
    name: file.name,
    size: file.size,
    type: file.type || 'application/octet-stream',
    url: typeof URL !== 'undefined' && typeof URL.createObjectURL === 'function' ? URL.createObjectURL(file) : '',
  }
}

export function revokeRoomAttachment(attachment: RoomAttachment): void {
  if (attachment.url && typeof URL !== 'undefined' && typeof URL.revokeObjectURL === 'function') {
    URL.revokeObjectURL(attachment.url)
  }
}

export function roomAttachmentIsImage(attachment: RoomAttachment): boolean {
  return isImageAttachment(attachment)
}

/** A short size for a chip: `812 B`, `14 KB`, `1.2 MB`. */
export function formatChipSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** What the send path needs for one turn that carries files. */
export interface PreparedRoomTurn {
  /** The member's words followed by each document's text under the labelled fence. */
  readonly content: string
  /** Images (and, where the server stages them, sandbox files) for the request's `attachments` field. */
  readonly attachments: readonly ChatAttachment[]
}

/**
 * Reads every chip before the box is cleared or anything is sent. Throws with a sentence for the
 * member (the draft stays) when a file cannot be read whole; nothing is sent in that case.
 */
export async function prepareRoomTurn(text: string, files: readonly RoomAttachment[]): Promise<PreparedRoomTurn> {
  if (files.length === 0) return { content: text, attachments: [] }
  const prepared = await prepareChatAttachments([...files])
  const inlined = await inlineTextAttachments(prepared)
  const attachments = await toStreamAttachments(prepared)
  return { content: inlined.length > 0 ? appendInlinedAttachments(text, inlined) : text, attachments }
}

/** A stored or streamed user message split into its words and the files it carried. */
export function splitRoomMessage(content: string): { text: string; files: readonly InlinedAttachment[] } {
  const split = splitInlinedAttachments(content)
  return { text: split.text, files: split.attachments }
}
