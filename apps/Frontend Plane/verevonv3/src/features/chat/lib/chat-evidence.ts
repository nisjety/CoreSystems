import type {
  ChatArtifact,
  ChatArtifactVersion,
  ChatKnowledgeGrounding,
  Citation,
} from '@/features/chat/components/chat-types'

/**
 * The evidence a stored turn carries (citations, artifacts and grounding) and the checks each piece passes before it is
 * shown. One set of checks serves every read that brings evidence to the client: the transcript route's turns (chat-normalizers)
 * and the messages route's messages, where the owner read flattens the same evidence onto the message (F-f01-24).
 *
 * A leaf module: it imports types only, so the API client can use these checks without a cycle back into the chat components.
 */

export function isCitation(value: unknown): value is Citation {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.title === 'string' &&
    typeof record.url === 'string' &&
    typeof record.snippet === 'string'
  )
}

/** One revision of an artifact, as its history holds it. The same check the artifact helpers apply to a history entry. */
export function isChatArtifactVersion(value: unknown): value is ChatArtifactVersion {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.content === 'string' &&
    typeof record.title === 'string' &&
    typeof record.version === 'number'
  )
}

export function isChatArtifact(value: unknown): value is ChatArtifact {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.kind === 'string' &&
    typeof record.content === 'string' &&
    typeof record.title === 'string' &&
    typeof record.version === 'number' &&
    // `history` rides along in the thread snapshot as untyped JSON; a snapshot
    // written by an older build simply has none.
    (record.history === undefined ||
      (Array.isArray(record.history) && record.history.every(isChatArtifactVersion))) &&
    // A deck's content is its manifest. One that is not a JSON object is
    // refused rather than shown as a deck.
    (record.kind !== 'deck' ||
      (typeof record.content === 'string' && deckManifestVersion(record.content) !== null))
  )
}

/** The deck manifest version this client reads (EP-5 S2/S3). */
export const DECK_MANIFEST_VERSION = 1

/**
 * The `version` a deck manifest states: `null` when the content is not a JSON
 * object, `undefined` when the object states none. Never throws.
 */
export function deckManifestVersion(content: string): number | undefined | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(content)
  } catch {
    return null
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
  const version = (parsed as Record<string, unknown>).version
  return typeof version === 'number' ? version : undefined
}

/**
 * A deck this client cannot read (a manifest version other than
 * {@link DECK_MANIFEST_VERSION}) is shown as its JSON in a plain code block
 * rather than handed to a deck view that would misread it. Any other artifact
 * is returned as it is.
 */
export function deckArtifactForDisplay(artifact: ChatArtifact): ChatArtifact {
  if (artifact.kind !== 'deck' || deckManifestVersion(artifact.content) === DECK_MANIFEST_VERSION) {
    return artifact
  }
  return { ...artifact, kind: 'code' }
}

export function isChatKnowledgeGrounding(value: unknown): value is ChatKnowledgeGrounding {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    (record.mode === 'retrieve' || record.mode === 'hybrid') &&
    typeof record.query === 'string' &&
    typeof record.lowConfidence === 'boolean' &&
    typeof record.factCount === 'number' &&
    typeof record.sourceCount === 'number' &&
    Array.isArray(record.facts) &&
    Array.isArray(record.sources)
  )
}
