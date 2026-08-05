import { blobToDataUrl, parseDataUrl } from '@/shared/lib/blob-data'
import { requestJson } from './http'

export type SpeechTranscription = {
  confidence?: number
  detected_language?: string
  id?: string
  model_used?: string
  provider_used?: string
  text?: string
  transcript?: string
}

export async function transcribeAudioBlob(blob: Blob, language: string): Promise<string> {
  const dataUrl = await blobToDataUrl(blob)
  const parsed = parseDataUrl(dataUrl)
  if (!parsed) throw new Error('Unable to encode voice recording.')

  const payload = await requestJson<SpeechTranscription>('/api/v1/ai/speech', {
    method: 'POST',
    body: JSON.stringify({
      audio_base64: parsed.base64,
      format: audioFormatFromMime(blob.type || parsed.mime),
      language,
      operation: 'transcribe',
      provider: 'azure',
    }),
  })

  return (payload.text ?? payload.transcript ?? '').trim()
}

export type DictationResult = {
  /** Cleaned-up dictation text (falls back to the raw transcript server-side). */
  text: string
  /** The raw speech-to-text transcript before cleanup. */
  rawText: string
  /** Whether the LLM cleanup pass ran (false = raw transcript fallback). */
  cleaned: boolean
  detectedLanguage?: string
}

type DictationWire = {
  text?: string
  raw_text?: string
  cleaned?: boolean
  detected_language?: string
}

/**
 * Verevon Flow dictation: mic audio in, polished text out. One round trip that
 * chains STT and an LLM cleanup pass (fillers stripped, punctuation fixed,
 * self-corrections applied) in the Model Plane. Prefer this over
 * {@link transcribeAudioBlob} for composer voice input — it also handles the
 * browser's webm/opus recordings, which the plain azure transcribe path does not.
 */
export async function dictateAudioBlob(
  blob: Blob,
  language: string,
  context?: string,
  signal?: AbortSignal,
): Promise<DictationResult> {
  const dataUrl = await blobToDataUrl(blob)
  const parsed = parseDataUrl(dataUrl)
  if (!parsed) throw new Error('Unable to encode voice recording.')

  const payload = await requestJson<DictationWire>('/api/v1/ai/dictate', {
    method: 'POST',
    body: JSON.stringify({
      audio_base64: parsed.base64,
      format: audioFormatFromMime(blob.type || parsed.mime),
      language,
      ...(context ? { context } : {}),
    }),
    signal,
  })

  return {
    text: (payload.text ?? '').trim(),
    rawText: (payload.raw_text ?? '').trim(),
    cleaned: payload.cleaned === true,
    detectedLanguage: payload.detected_language,
  }
}

function audioFormatFromMime(mime: string): string {
  const normalized = mime.toLowerCase()
  if (normalized.includes('webm')) return 'webm'
  if (normalized.includes('ogg')) return 'ogg'
  if (normalized.includes('wav')) return 'wav'
  if (normalized.includes('mpeg') || normalized.includes('mp3')) return 'mp3'
  return 'webm'
}
