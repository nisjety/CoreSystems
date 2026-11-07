/**
 * Typed context references for one chat turn (Wave 3 EP-3 S5). The composer attaches atomic chips
 * (a knowledge document, a past conversation, the current Space, a skill) that travel as a list next
 * to the text, never as text in the prompt. A reference is a selector: the gateway resolves its id
 * under the person's own authority, and the label is display text for the chip only.
 *
 * The bounds are the gateway's own (model-gateway `context_refs`, mp-contracts `context_refs`): at
 * most 20 references, at most 20 conversations, ids of 1-128 characters from [A-Za-z0-9_.:-], labels
 * cut to 120 characters, no repeat of a kind and id. Nothing here sends a list the gateway would
 * refuse for its shape.
 */
export const CONTEXT_REFS_LIMIT = 20
export const CONTEXT_REF_THREAD_LIMIT = 20
export const CONTEXT_REF_LABEL_MAX = 120

/** The kinds a chip can name. `artifact_selection` is the selection edit's (EP-3 S6), not a picker tab. */
export type ChatContextRefKind = 'document' | 'thread' | 'space' | 'skill' | 'artifact_selection'

export const CONTEXT_REF_KINDS: readonly ChatContextRefKind[] = ['document', 'thread', 'space', 'skill', 'artifact_selection']

/** The range an artifact_selection names (EP-3 S6): the revision the person saw, character offsets, and the quote's hash. */
export type ChatArtifactSelectionRef = {
  artifactId: string
  version: number
  start: number
  end: number
  quoteSha256: string
}

/** One chip as the send carries it and as `metadata.context_refs` stores it after the turn. */
export type ChatContextRef = {
  kind: ChatContextRefKind
  id: string
  label: string
  /** Present for an artifact_selection the panel sends. Never stored: the gateway keeps only kind, id and label. */
  selection?: ChatArtifactSelectionRef
}

const CONTEXT_REF_ID = /^[A-Za-z0-9_.:-]{1,128}$/

function isKind(value: unknown): value is ChatContextRefKind {
  return typeof value === 'string' && (CONTEXT_REF_KINDS as readonly string[]).includes(value)
}

const SHA256_HEX = /^[0-9a-f]{64}$/

function normalizeArtifactSelection(value: unknown, id: string): ChatArtifactSelectionRef | undefined {
  if (!value || typeof value !== 'object') return undefined
  const { artifactId, version, start, end, quoteSha256 } = value as Record<string, unknown>
  const whole = (n: unknown): n is number => typeof n === 'number' && Number.isInteger(n)
  if (artifactId !== id || !whole(version) || !whole(start) || !whole(end)) return undefined
  if (typeof quoteSha256 !== 'string' || !SHA256_HEX.test(quoteSha256)) return undefined
  if (version < 1 || start < 0 || end <= start) return undefined
  return { artifactId: id, version, start, end, quoteSha256 }
}

/** The label as the gateway keeps it: trimmed, control characters dropped, cut to the cap. */
export function capContextRefLabel(label: string): string {
  // eslint-disable-next-line no-control-regex
  return Array.from(label.trim().replace(/[\u0000-\u001f\u007f-\u009f]/g, '')).slice(0, CONTEXT_REF_LABEL_MAX).join('')
}

/**
 * Drops anything the gateway would refuse for its shape (an unknown kind, a malformed id, a repeat),
 * and keeps at most the limits. Used for the wire and for the stored chips a reload reads back.
 */
export function normalizeContextRefs(value: unknown): ChatContextRef[] {
  if (!Array.isArray(value)) return []
  const refs: ChatContextRef[] = []
  let threads = 0
  for (const item of value) {
    if (!item || typeof item !== 'object') continue
    const { kind, id, label, selection } = item as Record<string, unknown>
    if (!isKind(kind) || typeof id !== 'string') continue
    const trimmed = id.trim()
    if (!CONTEXT_REF_ID.test(trimmed)) continue
    if (refs.some((ref) => ref.kind === kind && ref.id === trimmed)) continue
    if (kind === 'thread' && threads >= CONTEXT_REF_THREAD_LIMIT) continue
    if (kind === 'thread') threads += 1
    const chip: ChatContextRef = { kind, id: trimmed, label: capContextRefLabel(typeof label === 'string' ? label : '') }
    if (kind === 'artifact_selection') {
      const range = normalizeArtifactSelection(selection, trimmed)
      if (range) chip.selection = range
    }
    refs.push(chip)
    if (refs.length === CONTEXT_REFS_LIMIT) break
  }
  return refs
}

/** The list with one reference added (a repeat or a reference past the limit is not added). */
export function addContextRef(current: readonly ChatContextRef[], ref: ChatContextRef): ChatContextRef[] {
  return normalizeContextRefs([...current, ref])
}

/** The list without one reference: a chip is removed whole, never edited. */
export function removeContextRef(current: readonly ChatContextRef[], ref: Pick<ChatContextRef, 'kind' | 'id'>): ChatContextRef[] {
  return current.filter((item) => !(item.kind === ref.kind && item.id === ref.id))
}

/**
 * The catalogue flag that says the gateway resolves references on this deployment: a chat model
 * lists it only while `MODEL_GATEWAY_CONTEXT_REFS_ENABLED` is on, so the picker and `/kontekst`
 * exist only when the server says they work. Nothing here derives it from a name.
 */
export const CONTEXT_REFS_CAPABILITY = 'context_refs'

export function catalogueAdvertisesContextRefs(
  models: readonly { modality?: string; capabilities?: readonly string[] }[],
): boolean {
  return models.some((model) => model.modality === 'chat' && (model.capabilities?.includes(CONTEXT_REFS_CAPABILITY) ?? false))
}

/**
 * Why references cannot ride a turn. A zero-retention or temporary turn is refused by the gateway
 * (`context_references`), and a subscription route carries no reference, so the chips are hidden
 * there and none is sent.
 */
export function contextRefsBlocked(turn: { zeroRetention: boolean; subscriptionBacked: boolean }): boolean {
  return turn.zeroRetention || turn.subscriptionBacked
}
