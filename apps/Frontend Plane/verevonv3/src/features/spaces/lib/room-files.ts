import { registerViewer, viewerFor } from '@/shared/artifacts/viewer-registry'

import { spaceRoomNestedFeature } from './space-room-features'

/**
 * The durable room file library as the SPA reads it (WS14-S9).
 *
 * Presentation helpers only. The gateway states the rollout in `features.room_files`
 * (`{turn_attachments, durable, files_tab}`) and the bounds in `limits.room_files`; neither is an
 * authorization input. Who may upload, read or delete is decided behind the gateway, and a
 * refusal arrives as a typed error.
 */

/** Where the flags live in the context's `features` block. */
export const ROOM_FILES_FEATURE = 'room_files'

/** The extractable types the library keeps, used until the context states its own list. */
export const DEFAULT_ROOM_FILE_EXTENSIONS: readonly string[] = ['pdf', 'docx', 'xlsx', 'pptx', 'txt', 'md', 'csv', 'json']
export const DEFAULT_ROOM_FILE_MAX_FILES = 10
export const DEFAULT_ROOM_FILE_MAX_BYTES = 25 * 1024 * 1024

export type RoomFilesFlag = 'turn_attachments' | 'durable' | 'files_tab'

/** One flag of the `room_files` block, off when the room has not been read or the key is absent. */
export function roomFilesFlag(flag: RoomFilesFlag): boolean {
  return spaceRoomNestedFeature(ROOM_FILES_FEATURE, flag)
}

/** A dev build may force the library on or off with `VITE_SPACE_ROOM_FILES`; production never reads it. */
function devOverride(): boolean | undefined {
  if (import.meta.env.DEV !== true) return undefined
  const value = import.meta.env.VITE_SPACE_ROOM_FILES
  return value === 'true' ? true : value === 'false' ? false : undefined
}

/** The Files tab is shown when the gateway says so. */
export function roomFilesTabEnabled(): boolean {
  return devOverride() ?? roomFilesFlag('files_tab')
}

/** The durable "keep in room files" choice is offered when the gateway says the library is on. */
export function roomFilesDurableEnabled(): boolean {
  return devOverride() ?? roomFilesFlag('durable')
}

export interface RoomFilesLimits {
  readonly maxFiles: number
  readonly maxFileBytes: number
  readonly allowedExtensions: readonly string[]
}

/** The bounds from `limits.room_files`, each falling back to the documented default when absent. */
export function roomFilesLimits(raw: unknown): RoomFilesLimits {
  const record = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {}
  const positive = (value: unknown, fallback: number) =>
    typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : fallback
  const extensions = Array.isArray(record.allowed_extensions)
    ? record.allowed_extensions.filter((entry): entry is string => typeof entry === 'string' && entry.length > 0)
    : []
  return {
    maxFiles: positive(record.max_files, DEFAULT_ROOM_FILE_MAX_FILES),
    maxFileBytes: positive(record.max_file_bytes, DEFAULT_ROOM_FILE_MAX_BYTES),
    allowedExtensions: extensions.length > 0 ? extensions.map((entry) => entry.toLowerCase()) : DEFAULT_ROOM_FILE_EXTENSIONS,
  }
}

/** Only a role that can write in the room offers to add files. The server decides regardless. */
export function canAddRoomFiles(role: string | undefined): boolean {
  return role === 'editor' || role === 'manager' || role === 'owner'
}

export function fileExtension(name: string): string {
  const dot = name.lastIndexOf('.')
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : ''
}

type Translate = (noText: string, enText: string) => string

export interface RoomFileVerdict {
  readonly accepted: File[]
  /** One localized sentence per file turned away before any request. */
  readonly messages: string[]
}

/**
 * Splits picked files into the ones worth sending and the reasons for the rest. The server still
 * validates type, size, count and duplicates; this only avoids a round trip that is certain to fail.
 */
export function vetRoomFiles(
  incoming: readonly File[],
  limits: RoomFilesLimits,
  tr: Translate,
): RoomFileVerdict {
  const accepted: File[] = []
  const messages: string[] = []
  for (const file of incoming) {
    if (!limits.allowedExtensions.includes(fileExtension(file.name))) {
      messages.push(
        tr(
          `${file.name}: denne filtypen kan ikke lagres i rommet. Tillatt: ${limits.allowedExtensions.join(', ')}.`,
          `${file.name}: this file type cannot be kept in the room. Allowed: ${limits.allowedExtensions.join(', ')}.`,
        ),
      )
    } else if (file.size === 0) {
      messages.push(tr(`${file.name}: filen er tom.`, `${file.name}: the file is empty.`))
    } else if (file.size > limits.maxFileBytes) {
      messages.push(
        tr(
          `${file.name}: filen er større enn ${formatBytes(limits.maxFileBytes)}.`,
          `${file.name}: the file is larger than ${formatBytes(limits.maxFileBytes)}.`,
        ),
      )
    } else if (accepted.length >= limits.maxFiles) {
      messages.push(
        tr(
          `${file.name}: maks ${limits.maxFiles} filer om gangen.`,
          `${file.name}: at most ${limits.maxFiles} files at a time.`,
        ),
      )
    } else {
      accepted.push(file)
    }
  }
  return { accepted, messages }
}

/** `812 B`, `14 KB`, `1.2 MB`. */
export function formatBytes(bytes: number | null | undefined): string {
  if (typeof bytes !== 'number' || !Number.isFinite(bytes) || bytes < 0) return ''
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/**
 * The composer's chips hold an object URL, not the `File`. Read them back into `File`s for the
 * durable "keep in room files" choice, keeping only the types the library accepts. Call before the
 * chips are cleared (clearing revokes the URLs).
 */
export async function roomFilesFromChips(
  chips: readonly { readonly name: string; readonly url: string; readonly type: string }[],
  limits: RoomFilesLimits,
): Promise<File[]> {
  const out: File[] = []
  for (const chip of chips) {
    if (!chip.url || !limits.allowedExtensions.includes(fileExtension(chip.name))) continue
    try {
      const blob = await (await fetch(chip.url)).blob()
      out.push(new File([blob], chip.name, { type: chip.type || blob.type }))
    } catch {
      // A chip that cannot be read back is simply not kept; the turn itself still carries it.
    }
  }
  return out.slice(0, limits.maxFiles)
}

// ---- the `room_file_text` viewer (viewer registry, WS07) --------------------------------------

/** The render kind a room file's extracted text is drawn with. */
export const ROOM_FILE_TEXT_KIND = 'room_file_text'

let viewerRegistered = false

/**
 * Registers the room-surface viewer for extracted file text. Trust class `text`: the text is drawn
 * as inert text and never as markup, so it needs no activation. Idempotent.
 */
export function ensureRoomFileTextViewer(): void {
  if (viewerRegistered) return
  viewerRegistered = true
  registerViewer({ renderKinds: [ROOM_FILE_TEXT_KIND], surfaces: ['room'], trust: 'text' })
}

/** True when the room surface has a viewer for extracted file text. */
export function roomFileTextPreviewable(): boolean {
  ensureRoomFileTextViewer()
  return viewerFor(ROOM_FILE_TEXT_KIND, 'room') !== undefined
}
