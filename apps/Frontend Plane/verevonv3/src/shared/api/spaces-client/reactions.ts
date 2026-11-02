// WS12-S8: message reactions.
//
//   PUT    /api/v1/spaces/{ref}/threads/{thread_id}/messages/{message_id}/reactions/{emoji}
//   DELETE (same path)
//     -> 200 { data: { message_id, reactions: [{ emoji, count, reacted }] } }
// Errors are the typed `{error:{code,message,details}}` envelope and arrive as an ApiError
// (`space_reactions_disabled`, `space_role_cannot_react`, `space_read_not_authorized`,
// `message_not_found`, `invalid_emoji`, `reaction_limit`, `application_unavailable`, the typed
// `zdr_*` refusals). Copy for each code lives in shared/i18n/errors.ts under the WS12-S8 banner.
//
// A reaction is the caller's one emoji on one message; the transcript carries each turn's
// `reactions[]` so a refetch shows everyone's. Nothing here is text a person wrote.
import { requestJson } from '../http'

/** One emoji on a message: how many people used it and whether the viewer is one of them. */
export type SpaceReactionChip = {
  readonly emoji: string
  readonly count: number
  readonly reacted: boolean
}

/** The curated picker (24 Unicode emoji, no external provider). The first six are the quick row. */
export const SPACE_QUICK_REACTIONS: readonly string[] = [
  '\u{1F44D}', // thumbs up
  '\u{2764}\u{FE0F}', // red heart
  '\u{1F604}', // smiling face
  '\u{1F389}', // party popper
  '\u{1F440}', // eyes
  '\u{1F64F}', // folded hands
  '\u{1F44E}', // thumbs down
  '\u{1F602}', // tears of joy
  '\u{1F62E}', // open mouth
  '\u{1F622}', // crying face
  '\u{1F914}', // thinking face
  '\u{1F44F}', // clapping hands
  '\u{1F525}', // fire
  '\u{1F680}', // rocket
  '\u{1F4AF}', // hundred points
  '\u{2705}', // check mark
  '\u{274C}', // cross mark
  '\u{2753}', // question mark
  '\u{26A0}\u{FE0F}', // warning
  '\u{1F4A1}', // light bulb
  '\u{1F64C}', // raised hands
  '\u{1F4AA}', // flexed biceps
  '\u{1F31F}', // glowing star
  '\u{1F60D}', // heart eyes
]

/** Keeps only well-formed chips; anything else (absent, malformed) is "no reactions". */
export function spaceReactionsFrom(raw: unknown): readonly SpaceReactionChip[] {
  if (!Array.isArray(raw)) return []
  const chips: SpaceReactionChip[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const value = item as Record<string, unknown>
    if (
      typeof value.emoji === 'string' &&
      value.emoji !== '' &&
      typeof value.count === 'number' &&
      Number.isFinite(value.count) &&
      value.count > 0
    ) {
      chips.push({ emoji: value.emoji, count: Math.floor(value.count), reacted: value.reacted === true })
    }
  }
  return chips
}

/** Add (`present`) or remove the viewer's reaction; resolves to the message's chips afterwards. */
export async function setSpaceReaction(
  spaceRef: string,
  threadId: string,
  messageId: string,
  emoji: string,
  present: boolean,
): Promise<readonly SpaceReactionChip[]> {
  const response = await requestJson<{ reactions?: unknown }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/messages/${encodeURIComponent(messageId)}/reactions/${encodeURIComponent(emoji)}`,
    { method: present ? 'PUT' : 'DELETE' },
  )
  return spaceReactionsFrom(response?.reactions)
}
