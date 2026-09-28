import { stripMarkdownPreview } from '@/shared/lib/markdown-plain-text'
import { requestJson } from './http'

/**
 * Governed text-to-speech: reply text goes to Verevon's own speech route
 * (BFF `/api/v1/ai/speech` → model-gateway → inference-core → Azure Speech),
 * never to the browser's online voices, which send it to the browser vendor.
 */

/**
 * The only text-to-speech provider Verevon asks for. `azure-speech` pins the
 * EU-class Azure Speech region; an empty or `openai` hint could fall through
 * to a provider the EU gate does not cover, and Azure OpenAI would reject the
 * Azure voice names below anyway.
 */
export const TTS_PROVIDER = 'azure-speech'

/**
 * Characters per synthesis call. Inference-core refuses more than 5,000, and
 * at 96 kbps a long clip nears tonic's 4 MiB message limit, so each call stays
 * well under both. The player fetches the next chunk while one plays.
 */
export const SPEECH_CHUNK_MAX_CHARS = 800

/** The voice for a Norwegian UI, and for any language without its own. */
export const DEFAULT_TTS_VOICE = 'nb-NO-PernilleNeural'

const TTS_VOICES_BY_LOCALE: Record<string, string> = {
  'nb-no': 'nb-NO-PernilleNeural',
  'en-gb': 'en-GB-SoniaNeural',
  'en-us': 'en-US-JennyNeural',
  'en-au': 'en-AU-NatashaNeural',
  'de-de': 'de-DE-KatjaNeural',
  'fr-fr': 'fr-FR-DeniseNeural',
  'es-es': 'es-ES-ElviraNeural',
  'sv-se': 'sv-SE-SofieNeural',
  'da-dk': 'da-DK-ChristelNeural',
  'pt-br': 'pt-BR-FranciscaNeural',
}

const TTS_VOICES_BY_LANGUAGE: Record<string, string> = {
  nb: 'nb-NO-PernilleNeural',
  no: 'nb-NO-PernilleNeural',
  // Azure Speech has no Nynorsk voice; Bokmål is the closest.
  nn: 'nb-NO-PernilleNeural',
  en: 'en-GB-SoniaNeural',
  de: 'de-DE-KatjaNeural',
  fr: 'fr-FR-DeniseNeural',
  es: 'es-ES-ElviraNeural',
  sv: 'sv-SE-SofieNeural',
  da: 'da-DK-ChristelNeural',
  pt: 'pt-BR-FranciscaNeural',
}

/**
 * The Azure Speech voice for a language tag: an exact locale first, then the
 * primary language (`nb` → Pernille, `en` → Sonia), then Norwegian.
 */
export function ttsVoiceFor(language: string): string {
  const tag = language.trim().toLowerCase().replace(/_/g, '-')
  const primary = tag.split('-')[0] ?? ''
  return TTS_VOICES_BY_LOCALE[tag] ?? TTS_VOICES_BY_LANGUAGE[primary] ?? DEFAULT_TTS_VOICE
}

/** The locale a voice speaks (`nb-NO-PernilleNeural` → `nb-NO`); the SSML language has to match the voice. */
export function ttsLanguageOf(voice: string): string {
  const [language, region] = voice.split('-')
  return language && region ? `${language}-${region}` : voice
}

type SpeechAudioWire = {
  audio_base64?: string
  format?: string
  provider_used?: string
}

export type SynthesizeSpeechInput = {
  text: string
  language: string
  voice: string
  signal?: AbortSignal
}

/**
 * One governed synthesis call: mp3 from Azure Speech, returned as a Blob so it
 * can play from a `blob:` URL (the CSP's `media-src` allows `'self'` and
 * `blob:` only; `data:` audio is blocked). Fails closed when the server
 * reports audio from any provider other than the one asked for.
 */
export async function synthesizeSpeech(input: SynthesizeSpeechInput): Promise<Blob> {
  const payload = await requestJson<SpeechAudioWire>('/api/v1/ai/speech', {
    method: 'POST',
    body: JSON.stringify({
      operation: 'tts',
      text: input.text,
      language: input.language,
      voice: input.voice,
      format: 'mp3',
      provider: TTS_PROVIDER,
    }),
    signal: input.signal,
  })

  const provider = typeof payload.provider_used === 'string' ? payload.provider_used.trim() : ''
  if (provider && provider !== TTS_PROVIDER) throw new Error('Speech came from an unexpected provider.')
  const format = typeof payload.format === 'string' ? payload.format.trim().toLowerCase() : ''
  if (format && format !== 'mp3') throw new Error('Speech came back in an unexpected format.')
  const base64 = typeof payload.audio_base64 === 'string' ? payload.audio_base64 : ''
  if (!base64) throw new Error('Speech synthesis returned no audio.')
  return new Blob([base64ToBytes(base64)], { type: 'audio/mpeg' })
}

function base64ToBytes(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64.replace(/\s+/g, ''))
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

/** A block-level line ends a thought; give it a full stop so the voice pauses. */
function withPause(line: string): string {
  const trimmed = line.trimEnd()
  return !trimmed || /[.!?…:;,]$/.test(trimmed) ? trimmed : `${trimmed}.`
}

const HORIZONTAL_RULE = /^\s*([-*_])(?:\s*\1){2,}\s*$/
const TABLE_SEPARATOR = /^\s*\|?\s*:?-{2,}:?\s*(?:\|\s*:?-{2,}:?\s*)*\|?\s*$/
const BLOCK_START = /^\s*(?:#{1,6}\s|[-*+]\s|\d+[.)]\s|>)/

/**
 * A reply as it should be heard: markdown decoration, code blocks, images,
 * link targets, bare URLs and citation markers (`[1]`, `[1, 2]`, `[^1]`,
 * `【…】`) are dropped, a link keeps only its label, and headings, list
 * items, table rows and paragraph ends get a full stop so the voice pauses.
 */
export function speakableText(markdown: string): string {
  const text = markdown
    .replace(/\r\n?/g, '\n')
    .replace(/```[\s\S]*?(?:```|$)/g, '\n')
    .replace(/~~~[\s\S]*?(?:~~~|$)/g, '\n')
    .replace(/\$\$[\s\S]*?\$\$/g, ' ')
    // Reference-style link definitions (`[1]: https://…`) are all target.
    .replace(/^[ \t]*\[[^\]\n]+\]:[ \t]*\S.*$/gm, '')
    .replace(/!\[[^\]]*\]\([^)]*\)/g, '')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/<https?:\/\/[^>]*>/gi, '')
    // A bare URL, but not the sentence punctuation after it.
    .replace(/https?:\/\/[^\s<>"'\])]*[^\s<>"'\]).,;:!?…]/gi, '')
    .replace(/[ \t]*【[^】]*】/g, '')
    .replace(/[ \t]*\[\^?\d+(?:\s*,\s*\d+)*\]/g, '')
    .replace(/<\/?[a-z][a-z0-9-]*(?:\s[^<>]*)?\/?>/gi, ' ')

  const lines = text.split('\n')
  const spoken: string[] = []
  lines.forEach((line, index) => {
    if (HORIZONTAL_RULE.test(line) || TABLE_SEPARATOR.test(line)) return
    const tableRow = /^\s*\|/.test(line)
    const content = tableRow
      ? line
          .split('|')
          .map((cell) => cell.trim())
          .filter(Boolean)
          .join(', ')
      : line
    const paragraphEnd = !(lines[index + 1] ?? '').trim()
    spoken.push(tableRow || paragraphEnd || BLOCK_START.test(line) ? withPause(content) : content)
  })
  // Whatever was dropped can leave a gap before punctuation ("se  ."); close it.
  return stripMarkdownPreview(spoken.join('\n')).replace(/\s+([.,;:!?…])/g, '$1')
}

/**
 * Split text into chunks of at most `maxChars`, at sentence ends where it can,
 * then at clause punctuation, then between words. Joining the chunks with a
 * space gives back the text with its whitespace collapsed.
 */
export function chunkForSpeech(text: string, maxChars: number = SPEECH_CHUNK_MAX_CHARS): string[] {
  const clean = text.replace(/\s+/g, ' ').trim()
  if (!clean) return []
  const limit = Math.max(1, Math.floor(maxChars))
  const pieces = clean
    .split(/(?<=[.!?…]["'»”’)\]]*)\s+/)
    .flatMap((sentence) => (sentence.length <= limit ? [sentence] : splitLongSentence(sentence, limit)))

  const chunks: string[] = []
  let current = ''
  for (const piece of pieces) {
    if (!current) current = piece
    else if (current.length + 1 + piece.length <= limit) current = `${current} ${piece}`
    else {
      chunks.push(current)
      current = piece
    }
  }
  if (current) chunks.push(current)
  return chunks
}

function splitLongSentence(sentence: string, limit: number): string[] {
  const parts: string[] = []
  let rest = sentence
  while (rest.length > limit) {
    const window = rest.slice(0, limit + 1)
    // After clause punctuation keeps the punctuation with its clause.
    let cut = Math.max(window.lastIndexOf(', '), window.lastIndexOf('; '), window.lastIndexOf(': '))
    if (cut > 0) cut += 1
    else cut = window.lastIndexOf(' ')
    if (cut <= 0) cut = limit
    parts.push(rest.slice(0, cut).trim())
    rest = rest.slice(cut).trim()
  }
  if (rest) parts.push(rest)
  return parts
}
