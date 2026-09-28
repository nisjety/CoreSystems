// Proves the governed-speech lint guard fires.
//
// Runs the REAL project ESLint flat config (auto-discovered eslint.config.js,
// resolved from the vitest cwd, as the A8 fabrication guard does) against the
// browser speech APIs the SPA must never use: SpeechRecognition,
// webkitSpeechRecognition, speechSynthesis and SpeechSynthesisUtterance. Their
// recognisers and online voices send audio and reply text to the browser
// vendor. The rule applies to product source only; tests may stub the APIs to
// prove they are never called.
import { ESLint } from 'eslint'
import { beforeAll, describe, expect, it } from 'vitest'

let eslint: ESLint

beforeAll(async () => {
  eslint = new ESLint()
  // Warm the flat config and TS parser once (see fabrication-guard.test.ts).
  await eslint.lintText('export const warmup = 1\n', { filePath: 'src/__speech_guard_warmup__.ts' })
}, 180_000)

const SPEECH_RULES = new Set(['no-restricted-globals', 'no-restricted-properties', 'no-restricted-syntax'])

async function speechGuardMessages(code: string, relativeFilePath: string): Promise<string[]> {
  const [result] = await eslint.lintText(code, { filePath: relativeFilePath })
  return (result?.messages ?? []).filter((m) => m.ruleId && SPEECH_RULES.has(m.ruleId)).map((m) => m.message)
}

/** The recogniser lookup this guard exists to keep out, as the old Voice mode wrote it. */
const REMOVED_RECOGNISER_LOOKUP = `type RecognitionCtor = new () => unknown
export function getSpeechRecognitionCtor(): RecognitionCtor | undefined {
  const win = window as unknown as {
    SpeechRecognition?: RecognitionCtor
    webkitSpeechRecognition?: RecognitionCtor
  }
  return win.SpeechRecognition ?? win.webkitSpeechRecognition
}
`

describe('governed speech lint guard', () => {
  it.each([
    ['a bare recogniser', 'export const recogniser = new SpeechRecognition()\n'],
    ['the prefixed recogniser', 'export const recogniser = new webkitSpeechRecognition()\n'],
    ['the recogniser off window', 'export const Recogniser = window.SpeechRecognition\n'],
    ['bare synthesis', "export const say = () => speechSynthesis.speak(new SpeechSynthesisUtterance('hei'))\n"],
    ['synthesis off window', 'export const stop = () => window.speechSynthesis.cancel()\n'],
    ['synthesis off globalThis', 'export const synth = globalThis.speechSynthesis\n'],
    // TypeScript's usual way to a vendor-prefixed API: cast or alias the global first.
    ['the removed recogniser lookup (a cast, then an alias)', REMOVED_RECOGNISER_LOOKUP],
    ['a property read through a cast', 'export const R = (window as unknown as { webkitSpeechRecognition: unknown }).webkitSpeechRecognition\n'],
    ['a property read through an any cast', 'export const synth = (globalThis as any).speechSynthesis\n'],
    ['a non-null window', 'export const synth = window!.speechSynthesis\n'],
    ['another window object', 'export const synth = document.defaultView?.speechSynthesis\n'],
    // The name as a string: a feature test, a reflective read or a computed key.
    ['an `in` feature test', "export const canSpeak = 'speechSynthesis' in window\n"],
    ['a reflective read', "export const synth = Reflect.get(window, 'speechSynthesis')\n"],
    ['a computed template key', 'export const synth = (window as never)[`speechSynthesis`]\n'],
    // A type shim or annotation is the first step towards using one.
    ['an ad-hoc type shim', 'export type SpeechWindow = { SpeechRecognition?: unknown }\n'],
    ['a type annotation', 'export let utterance: SpeechSynthesisUtterance | undefined\n'],
  ])('fires on %s', async (_label, code) => {
    const messages = await speechGuardMessages(code, 'src/features/voice/__speech_guard_fixture__.ts')
    expect(messages.length).toBeGreaterThan(0)
    expect(messages.every((message) => /governed speech path/.test(message))).toBe(true)
  }, 30_000)

  it('has no exception left for the old "Les høyt" module', async () => {
    const messages = await speechGuardMessages(
      "export const say = () => window.speechSynthesis.speak(new SpeechSynthesisUtterance('hei'))\n",
      'src/features/chat/components/chat-media-markdown.tsx',
    )
    expect(messages.length).toBeGreaterThanOrEqual(2)
  }, 30_000)

  it('leaves tests free to stub the APIs, and governed playback alone', async () => {
    expect(
      await speechGuardMessages(
        "vi.stubGlobal('speechSynthesis', { speak: () => undefined })\nexport const synth = window.speechSynthesis\n",
        'src/features/voice/__speech_guard_fixture__.test.ts',
      ),
    ).toEqual([])
    expect(
      await speechGuardMessages(
        "import { speechPlayer } from '@/features/voice/speech-playback'\nexport const read = () => speechPlayer().speak({ id: 'a', text: 'hei', language: 'nb' })\n",
        'src/features/voice/__speech_guard_fixture_valid__.ts',
      ),
    ).toEqual([])
  }, 30_000)
})
