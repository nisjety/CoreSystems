import type { VoiceReply } from './VoiceModeDialog'

type TurnLike = { id: string; role: string; content: string; streaming: boolean }

/**
 * What Voice mode waits on in chat: the latest assistant turn, and nothing
 * else of it (only the id, text and streaming flag cross over).
 *
 * A message sent while a reply streams is either taken by that run, which
 * then answers it, or not yet: it may still be on its way to the run, or the
 * run could not take it and it goes out as the next turn once the run ends.
 * Until the chat knows which (`followUpPending`), the turn counts as still
 * streaming, so the dialog never takes the earlier reply for the answer and
 * reads it aloud while the real one streams in unseen.
 */
export function latestVoiceReply(turns: readonly TurnLike[], followUpPending: boolean): VoiceReply | null {
  const turn = turns.findLast((candidate) => candidate.role === 'assistant')
  return turn ? { id: turn.id, text: turn.content, streaming: turn.streaming || followUpPending } : null
}
