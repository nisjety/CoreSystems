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

function audioFormatFromMime(mime: string): string {
  const normalized = mime.toLowerCase()
  if (normalized.includes('webm')) return 'webm'
  if (normalized.includes('ogg')) return 'ogg'
  if (normalized.includes('wav')) return 'wav'
  if (normalized.includes('mpeg') || normalized.includes('mp3')) return 'mp3'
  return 'webm'
}
