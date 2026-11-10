// The origins the chat library lists, by kind (F-t3-080, W5-090-b). The gateway fixes these in
// apps/gateway/src/domains/chat/library.rs: the browser names a kind, never an origin. This file mirrors that list for the
// SPA's own filter and row kind, so it must change together with the gateway's.

/** A saved copy of a document artifact (`chat-artifact`). */
export const CHAT_ARTIFACT_LIBRARY_ORIGIN = 'chat-artifact'
/** Text the person uploaded into a chat (`chat-attachment`). */
export const CHAT_ATTACHMENT_LIBRARY_ORIGIN = 'chat-attachment'
/** A transcript made from an audio upload (`chat-transcript`, W5-090-b). */
export const CHAT_TRANSCRIPT_LIBRARY_ORIGIN = 'chat-transcript'

export type ChatLibraryOrigin =
  | typeof CHAT_ARTIFACT_LIBRARY_ORIGIN
  | typeof CHAT_ATTACHMENT_LIBRARY_ORIGIN
  | typeof CHAT_TRANSCRIPT_LIBRARY_ORIGIN

/** The origins each library kind lists. `uploaded` is the uploaded text and the transcripts; `all` is both kinds. */
export const CHAT_LIBRARY_ORIGINS: Readonly<Record<'generated' | 'uploaded' | 'all', readonly ChatLibraryOrigin[]>> = {
  generated: [CHAT_ARTIFACT_LIBRARY_ORIGIN],
  uploaded: [CHAT_ATTACHMENT_LIBRARY_ORIGIN, CHAT_TRANSCRIPT_LIBRARY_ORIGIN],
  all: [CHAT_ARTIFACT_LIBRARY_ORIGIN, CHAT_ATTACHMENT_LIBRARY_ORIGIN, CHAT_TRANSCRIPT_LIBRARY_ORIGIN],
}

/**
 * The kind a document's origin is listed under, or null for an origin the library does not list. The caller decides what
 * an unlisted origin shows as.
 */
export function chatLibraryKindOf(origin: unknown): 'generated' | 'uploaded' | null {
  const listed = (kind: 'generated' | 'uploaded'): boolean =>
    (CHAT_LIBRARY_ORIGINS[kind] as readonly unknown[]).includes(origin)
  if (listed('generated')) return 'generated'
  return listed('uploaded') ? 'uploaded' : null
}
