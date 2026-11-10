import { z } from 'zod'

// The client action contract for transcribing an audio recording into a document (W5-090-b, F-t3-090). The browser
// parses the request here before it sends anything, and the gateway's POST /api/v1/chat/transcripts reads the same
// shape (apps/gateway/src/domains/chat/transcripts.rs). The recording is sent once and never kept: the answer names the
// saved document and nothing else. A transcript is durable content, so a temporary or zero-retention chat never offers
// one.

/**
 * The largest recording the browser sends, in bytes. It is the voice recorder's own cap (`VOICE_MAX_BYTES` in
 * features/voice/voice-recorder.ts), so any take the recorder keeps can be transcribed, and it sits well inside the
 * gateway's audio body limit after base64.
 */
export const TRANSCRIPT_MAX_RECORDING_BYTES = 1_400_000

/** The longest title the gateway keeps, in characters. */
export const TRANSCRIPT_MAX_TITLE_CHARS = 120

/** The longest thread id the gateway's transcript metadata may name, in characters. */
export const TRANSCRIPT_MAX_THREAD_ID_CHARS = 256

/** The audio types the gateway takes, by base type. A codec parameter (`audio/webm;codecs=opus`) is ignored. */
export const TRANSCRIPT_AUDIO_TYPES = ['audio/mpeg', 'audio/wav', 'audio/webm', 'audio/mp4', 'audio/ogg'] as const

export type TranscriptAudioType = (typeof TRANSCRIPT_AUDIO_TYPES)[number]

/** The base type of a recording's MIME type when the gateway takes it, or null for any other type. */
export function transcriptAudioType(mime: string | null | undefined): TranscriptAudioType | null {
  const base = ((mime ?? '').split(';')[0] ?? '').trim().toLowerCase()
  return (TRANSCRIPT_AUDIO_TYPES as readonly string[]).includes(base) ? (base as TranscriptAudioType) : null
}

/** The longest base64 text that carries `bytes` bytes. */
const base64Length = (bytes: number): number => Math.ceil(bytes / 3) * 4

const transcriptCreateFields = {
  audio_base64: z.string().min(1).max(base64Length(TRANSCRIPT_MAX_RECORDING_BYTES)),
  mime: z.enum(TRANSCRIPT_AUDIO_TYPES),
  title: z.string().trim().min(1).max(TRANSCRIPT_MAX_TITLE_CHARS),
  thread_id: z.string().trim().min(1).max(TRANSCRIPT_MAX_THREAD_ID_CHARS).optional(),
}

/** `transcript.create`: one recording, its audio type, a title and, when there is one, the thread it was made in. */
export const chatTranscriptCreateInput = z.object(transcriptCreateFields).strict()

export type ChatTranscriptCreateInput = z.infer<typeof chatTranscriptCreateInput>

/**
 * A transcript request the contract refuses. Nothing is sent when it is thrown. `action` names the operation and never
 * carries the recording or the title, so a refusal can be logged without the person's data.
 */
export class ChatTranscriptContractError extends Error {
  readonly code = 'chat_transcript_contract_refused'
  readonly action: string

  constructor(action: string) {
    super(`The transcript ${action} request is refused by its action contract.`)
    this.name = 'ChatTranscriptContractError'
    this.action = action
  }
}

/** Parses one transcript request through the contract and returns the parsed input, or throws a typed refusal. */
export function parseChatTranscriptCreate(input: Record<string, unknown>): ChatTranscriptCreateInput {
  const parsed = chatTranscriptCreateInput.safeParse(input)
  if (!parsed.success) throw new ChatTranscriptContractError('transcript.create')
  return parsed.data
}
