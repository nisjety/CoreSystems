/**
 * Viewer items for the artifacts in one room thread (WS07-S11).
 *
 * Builds stub `ChatTurn`s from the room's turns and runs the chat module's own
 * `collectArtifactItems` over them, so version history and
 * `mergeArtifactVersion` behave exactly as they do in Chat (revisions of one
 * artifact `id` across turns become one item with a `‹ v1/2 ›` stepper). The
 * stubs carry no content, no files and no createdAt: the room reads none of
 * those from them. `chat-types.ts` is not edited.
 */
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from '@/features/chat/components/chat-types'
import { collectArtifactItems } from '@/features/chat/components/chat-media-markdown'

export type RoomItemSource = {
  readonly messageId?: string
  readonly artifacts?: readonly ChatArtifact[]
}

export function roomItems(turns: readonly RoomItemSource[]): ArtifactPanelItem[] {
  const stubs: ChatTurn[] = []
  turns.forEach((turn, index) => {
    if (!turn.artifacts || turn.artifacts.length === 0) return
    stubs.push({
      id: turn.messageId?.trim() || `room-turn-${index}`,
      role: 'assistant',
      content: '',
      createdAt: '',
      streaming: false,
      tools: [],
      attachments: [],
      artifacts: [...turn.artifacts],
    } as ChatTurn)
  })
  return collectArtifactItems(stubs)
}
