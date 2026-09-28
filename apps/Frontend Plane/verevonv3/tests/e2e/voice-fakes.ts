import type { Page } from '@playwright/test'

/**
 * Browser fakes for the Voice mode and "Les høyt" specs. Kept apart from
 * product-acceptance-helpers.ts, which another lane edits.
 *
 * Installed with `addInitScript`, so they are in place before the app runs:
 * - `getUserMedia` hands out a silent stream and counts calls; no real
 *   microphone is opened.
 * - `MediaRecorder` records nothing and yields one small webm take on stop.
 * - Audio playback never loads or decodes anything: a media element's `src` is
 *   only remembered, and each clip "plays" for `clipMs` and then ends, unless
 *   it was paused or replaced first. Every source played is kept.
 * - The Web Speech globals (`SpeechRecognition`, `webkitSpeechRecognition`,
 *   `speechSynthesis`, `SpeechSynthesisUtterance`) are replaced by getters
 *   that count every read and give back nothing. Verevon must never touch
 *   them, not even to feature-detect.
 */
export type VoiceFakeReport = {
  webSpeech: Record<'SpeechRecognition' | 'webkitSpeechRecognition' | 'speechSynthesis' | 'SpeechSynthesisUtterance', number>
  getUserMedia: number
  recorders: Array<{ mimeType?: string; audioBitsPerSecond?: number }>
  played: string[]
}

/** What the fake recorder hands back for every take. */
export const FAKE_TAKE_TEXT = 'fake webm take'

export async function installVoiceFakes(page: Page, options: { clipMs?: number } = {}): Promise<void> {
  await page.addInitScript(
    ({ clipMs, takeText }) => {
      const report = {
        webSpeech: { SpeechRecognition: 0, webkitSpeechRecognition: 0, speechSynthesis: 0, SpeechSynthesisUtterance: 0 },
        getUserMedia: 0,
        recorders: [] as Array<{ mimeType?: string; audioBitsPerSecond?: number }>,
        played: [] as string[],
      }
      Object.defineProperty(window, '__verevonVoiceFakes', { value: report })

      for (const name of Object.keys(report.webSpeech) as Array<keyof typeof report.webSpeech>) {
        Object.defineProperty(window, name, {
          configurable: true,
          get() {
            report.webSpeech[name] += 1
            return undefined
          },
        })
      }

      const silentStream = (): MediaStream => {
        try {
          return new AudioContext().createMediaStreamDestination().stream
        } catch {
          return { getTracks: () => [{ stop() {} }] } as unknown as MediaStream
        }
      }
      if (navigator.mediaDevices) {
        navigator.mediaDevices.getUserMedia = async () => {
          report.getUserMedia += 1
          return silentStream()
        }
      }

      class FakeMediaRecorder {
        static isTypeSupported(type: string) {
          return type === 'audio/webm;codecs=opus' || type === 'audio/webm'
        }

        state: 'inactive' | 'recording' = 'inactive'
        mimeType: string
        ondataavailable: ((event: { data: Blob }) => void) | null = null
        onstop: (() => void) | null = null
        onerror: ((event: unknown) => void) | null = null

        constructor(_stream: MediaStream, recorderOptions: MediaRecorderOptions = {}) {
          this.mimeType = recorderOptions.mimeType ?? 'audio/webm'
          report.recorders.push({ mimeType: recorderOptions.mimeType, audioBitsPerSecond: recorderOptions.audioBitsPerSecond })
        }

        start() {
          this.state = 'recording'
        }

        stop() {
          if (this.state === 'inactive') return
          this.state = 'inactive'
          setTimeout(() => {
            this.ondataavailable?.({ data: new Blob([takeText], { type: this.mimeType }) })
            this.onstop?.()
          }, 0)
        }
      }
      Object.defineProperty(window, 'MediaRecorder', { configurable: true, writable: true, value: FakeMediaRecorder })

      // Sources are only remembered, never loaded. Assigning a real element's
      // `src` starts Chromium's media pipeline, which cannot decode the fake
      // mp3 bytes the speech mock returns and fires `error`
      // (MEDIA_ERR_SRC_NOT_SUPPORTED) a few milliseconds later: the player
      // rightly takes that as a failed reading and fails closed.
      const sources = new WeakMap<HTMLMediaElement, string>()
      const resolveSource = (value: string) => {
        try {
          return value ? new URL(value, document.baseURI).href : ''
        } catch {
          return value
        }
      }
      Object.defineProperty(HTMLMediaElement.prototype, 'src', {
        configurable: true,
        enumerable: true,
        get(this: HTMLMediaElement) {
          return sources.get(this) ?? ''
        },
        set(this: HTMLMediaElement, value: string) {
          sources.set(this, resolveSource(String(value)))
        },
      })
      const removeAttribute = Element.prototype.removeAttribute
      HTMLMediaElement.prototype.removeAttribute = function remove(this: HTMLMediaElement, name: string) {
        if (name.toLowerCase() === 'src') sources.delete(this)
        else removeAttribute.call(this, name)
      }
      HTMLMediaElement.prototype.load = function load() {}

      const paused = new WeakSet<HTMLMediaElement>()
      HTMLMediaElement.prototype.play = function play(this: HTMLMediaElement) {
        const source = this.src
        report.played.push(source)
        paused.delete(this)
        setTimeout(() => {
          if (this.src === source && !paused.has(this)) this.dispatchEvent(new Event('ended'))
        }, clipMs)
        return Promise.resolve()
      }
      HTMLMediaElement.prototype.pause = function pause(this: HTMLMediaElement) {
        paused.add(this)
      }
    },
    { clipMs: options.clipMs ?? 50, takeText: FAKE_TAKE_TEXT },
  )
}

export async function voiceFakeReport(page: Page): Promise<VoiceFakeReport> {
  return page.evaluate(() => (window as unknown as { __verevonVoiceFakes: VoiceFakeReport }).__verevonVoiceFakes)
}

/**
 * Every request the page makes to another origin, as `type origin/path` (no
 * query string). The stylesheet and font files global.css imports from Google
 * Fonts are the one expected exception and are left out.
 */
export function collectOffOriginRequests(page: Page, origin: string): string[] {
  const offOrigin: string[] = []
  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.protocol === 'blob:' || url.protocol === 'data:' || url.origin === origin) return
    const type = request.resourceType()
    const webFont =
      (type === 'stylesheet' || type === 'font')
      && (url.hostname === 'fonts.googleapis.com' || url.hostname === 'fonts.gstatic.com')
    if (!webFont) offOrigin.push(`${type} ${url.origin}${url.pathname}`)
  })
  return offOrigin
}
