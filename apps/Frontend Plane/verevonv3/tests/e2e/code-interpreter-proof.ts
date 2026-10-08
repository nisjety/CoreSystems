import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { appendFileSync, mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import type { Page } from '@playwright/test'
import { splitTimedFrames, type StreamChunk } from './product-phase-timing'

/**
 * H5: what the live proof of `code_interpreter` on the ChatGPT Terra route asks, how
 * it reads the stream back, and how it judges a turn. Everything here is free of
 * Playwright at run time (only a type import) so vitest can test it
 * (tests/code-interpreter-proof.test.ts) and the spec can stay short.
 *
 * The proof, in one sentence: on Terra, five fresh threads ask for an .xlsx with
 * formulas and a native chart, a .docx with a table, a .pdf with æøå, a .pptx with
 * a matplotlib picture on a slide and a stand-alone .png chart; each file must
 * arrive as a download card (the PNG as an inline preview), be real when read back
 * with the library for its type (tests/e2e/fixtures/code-interpreter/h5_oracle.py,
 * run in the stack's own image), and be produced inside the sandbox's 30 s limit by
 * a turn whose tool decision actually called code_interpreter.
 */

export type ProofKind = 'xlsx' | 'docx' | 'pdf' | 'pptx' | 'png'

export type ProofCase = {
  kind: ProofKind
  extension: `.${ProofKind}`
  /** The name the prompt asks for; the oracle goes by extension, not by this. */
  fileName: string
  prompt: string
}

// Plain Norwegian, as a user would write it: no library names and no mention of
// the tool, because whether the model chooses code_interpreter for these is part
// of what is being proved. Invented content only. The strings the oracle looks for
// (header words, the PDF sentence) are spelled out here and in h5_oracle.py.
export const PROOF_CASES: readonly ProofCase[] = [
  {
    kind: 'xlsx', extension: '.xlsx', fileName: 'salg-q3.xlsx',
    prompt: 'Lag et Excel-regneark med salgstall for juli, august og september (oppdiktede tall): kolonnene Måned, Omsetning og Kostnad, en kolonne Margin som en ekte formel per rad (omsetning minus kostnad), en sumrad med SUM-formler, og et innebygd søylediagram av omsetning per måned. Gi filen navnet salg-q3.xlsx.',
  },
  {
    kind: 'docx', extension: '.docx', fileName: 'statusnotat.docx',
    prompt: 'Lag et Word-dokument med tittelen Statusnotat, to korte avsnitt om et oppdiktet prosjekt, og en tabell med kolonnene Aktivitet, Ansvarlig og Frist og tre rader med oppdiktede data. Gi filen navnet statusnotat.docx.',
  },
  {
    kind: 'pdf', extension: '.pdf', fileName: 'sammendrag.pdf',
    prompt: 'Lag en PDF med overskriften Sammendrag og dette avsnittet ordrett: «Rød grøt med fløte er søt og sunn, og blåbær fra Ålesund smaker best.» Æ, ø og å må vises riktig. Gi filen navnet sammendrag.pdf.',
  },
  {
    kind: 'pptx', extension: '.pptx', fileName: 'kvartalsrapport.pptx',
    prompt: 'Lag en PowerPoint-presentasjon med to lysbilder. Lysbilde 1 er en tittelside med tittelen Kvartalsrapport. Lysbilde 2 har tittelen Omsetning per måned og et søylediagram laget med matplotlib og satt inn som bilde (PNG) på lysbildet, med oppdiktede tall. Gi filen navnet kvartalsrapport.pptx.',
  },
  {
    kind: 'png', extension: '.png', fileName: 'besokstall.png',
    prompt: 'Lag et linjediagram med matplotlib som viser oppdiktede besøkstall per uke for åtte uker, med akseetiketter og tittel på norsk, og lagre det som besokstall.png.',
  },
]

/**
 * The model the proof runs on. A copy of PRODUCT_MODEL (product-subscription-route.ts), which cannot be
 * imported here: it pulls @playwright/test into vitest, which then takes two minutes to start. The spec
 * fails at load if the two ever differ.
 */
export const TERRA_MODEL = 'gpt-5.6-terra'

/** The sandbox's wall-clock limit as the tool tells the model ("a hard ~30s timeout"). */
export const SANDBOX_LIMIT_MS = 30_000

// Error codes that mean the subscription, not the product, stopped the turn.
export const CAP_CODES = ['subscription_usage_limit', 'rate_limited'] as const

// ---------------------------------------------------------------------------
// Reading the stream back.
// ---------------------------------------------------------------------------

export type ToolRecord = {
  id: string
  name: string
  /** ms from the capture's start to the tool_call frame. */
  calledAtMs: number
  /** ms from the tool_call frame to its tool_result frame; null when no result came. */
  resultAfterMs: number | null
  status: string | null
  error: boolean
  /** code_interpreter only: the program's exit code, when the result carried one. */
  exitCode: number | null
  /** code_interpreter only: the files the gateway says it delivered, by name. */
  filesDelivered: string[]
  /** code_interpreter only: the program's stderr held a Python traceback. */
  traceback: boolean
}

export type AttachmentRecord = {
  id: string
  name: string
  mime: string
  declaredBytes: number
  /** Bytes the data URI decodes to; 0 when the URL is not a base64 data URI. */
  decodedBytes: number
  sha256: string | null
  dataUri: boolean
}

export type TurnSummary = {
  requestId: string | null
  threadId: string | null
  frameCounts: Record<string, number>
  tools: ToolRecord[]
  attachments: AttachmentRecord[]
  errorCodes: string[]
  doneModel: string | null
  done: boolean
  answerChars: number
}

const isRecord = (value: unknown): value is Record<string, unknown> => Boolean(value) && typeof value === 'object' && !Array.isArray(value)
const str = (value: unknown): string | null => (typeof value === 'string' ? value : null)

function parseObject(text: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(text)
    return isRecord(value) ? value : null
  } catch {
    return null
  }
}

/** The bytes inside a `data:<mime>;base64,<payload>` URI, or null for anything else. */
export function dataUriBytes(uri: string): Buffer | null {
  const match = /^data:[^,]*;base64,(.*)$/s.exec(uri)
  return match ? Buffer.from(match[1]!.replace(/\s+/g, ''), 'base64') : null
}

/**
 * code_interpreter's tool_result `output`. With files it is the gateway's own
 * summary (one JSON line, then a sentence for the model); with none it is the
 * execution result itself, which execution-core renders behind a provenance header
 * line (`[source: org-internal]`, see model-gateway's extract_json_object). Either
 * way the JSON is the first line that opens an object.
 */
function parseCodeResult(output: string): { exitCode: number | null; filesDelivered: string[]; traceback: boolean } {
  const parsed = parseObject(output.split('\n').find(line => line.trimStart().startsWith('{'))?.trim() ?? '')
  if (!parsed) return { exitCode: null, filesDelivered: [], traceback: /Traceback \(most recent call last\)/.test(output) }
  const delivered = Array.isArray(parsed.files_delivered)
    ? parsed.files_delivered.filter((entry): entry is string => typeof entry === 'string').map(entry => entry.split(' (')[0]!.trim())
    : Array.isArray(parsed.files)
      ? parsed.files.flatMap(file => (isRecord(file) && typeof file.name === 'string' ? [file.name] : []))
      : []
  return {
    exitCode: typeof parsed.exit_code === 'number' ? parsed.exit_code : null,
    filesDelivered: delivered,
    traceback: typeof parsed.stderr === 'string' && /Traceback \(most recent call last\)/.test(parsed.stderr),
  }
}

/** Fold the timed stream reads of one turn into what the proof needs to know. */
export function summariseTurn(chunks: readonly StreamChunk[], originAt: number): TurnSummary {
  const frames = splitTimedFrames(chunks)
  const summary: TurnSummary = {
    requestId: null, threadId: null, frameCounts: {}, tools: [], attachments: [], errorCodes: [],
    doneModel: null, done: false, answerChars: 0,
  }
  const open = new Map<string, ToolRecord>()
  for (const frame of frames) {
    const event = frame.event ?? 'message'
    summary.frameCounts[event] = (summary.frameCounts[event] ?? 0) + 1
    const data = parseObject(frame.data)
    if (!data) continue
    if (event === 'connected') {
      summary.requestId = str(data.request_id) ?? summary.requestId
      summary.threadId = str(data.thread_id) ?? summary.threadId
    } else if (event === 'chunk') {
      summary.answerChars += (str(data.delta) ?? str(data.content) ?? '').length
    } else if (event === 'tool_call') {
      const record: ToolRecord = {
        id: str(data.id) ?? `call-${summary.tools.length + 1}`,
        name: str(data.name) ?? 'unknown',
        calledAtMs: frame.at - originAt,
        resultAfterMs: null, status: null, error: false, exitCode: null, filesDelivered: [], traceback: false,
      }
      summary.tools.push(record)
      open.set(record.id, record)
    } else if (event === 'tool_result') {
      const record = open.get(str(data.id) ?? '')
      if (!record) continue
      record.resultAfterMs = Math.round(frame.at - originAt - record.calledAtMs)
      record.status = str(data.status)
      record.error = Boolean(data.error) || data.status === 'error'
      if (record.name === 'code_interpreter') {
        const result = parseCodeResult(str(data.output) ?? '')
        record.exitCode = result.exitCode
        record.filesDelivered = result.filesDelivered
        record.traceback = result.traceback
      }
    } else if (event === 'attachment') {
      const url = str(data.url) ?? ''
      const bytes = dataUriBytes(url)
      summary.attachments.push({
        id: str(data.id) ?? '',
        name: str(data.name) ?? '',
        mime: str(data.mime) ?? str(data.type) ?? '',
        declaredBytes: typeof data.size === 'number' ? data.size : 0,
        decodedBytes: bytes?.length ?? 0,
        sha256: bytes ? createHash('sha256').update(bytes).digest('hex') : null,
        dataUri: url.startsWith('data:'),
      })
    } else if (event === 'error') {
      const nested = isRecord(data.error) ? data.error : null
      summary.errorCodes.push(str(data.code) ?? str(nested?.code) ?? 'error')
    } else if (event === 'done') {
      summary.done = true
      summary.doneModel = str(data.model_used) ?? str(data.modelUsed)
    }
  }
  return summary
}

// ---------------------------------------------------------------------------
// The page: capture the stream with timestamps, and read what the message shows.
// ---------------------------------------------------------------------------

type CaptureWindow = typeof window & {
  h5Capture?: { startedAt: number; status: number; endAt: number | null; chunks: StreamChunk[]; done: Promise<void> | null }
  restoreH5Fetch?: () => void
}

/**
 * Record the chat stream's reads with this page's `performance.now()`. Arm it just
 * before the submit click; the first `/api/v1/chat/stream` response after that is
 * the one captured. The UTF-8 decode is the browser's own, as the app's reader
 * does it, so Norwegian text is not corrupted on the way into the evidence.
 */
export async function armStreamCapture(page: Page): Promise<void> {
  await page.evaluate(() => {
    const target = window as CaptureWindow
    target.restoreH5Fetch?.()
    const capture = { startedAt: performance.now(), status: 0, endAt: null as number | null, chunks: [] as StreamChunk[], done: null as Promise<void> | null }
    target.h5Capture = capture
    const original = window.fetch.bind(window)
    window.fetch = async (...args: Parameters<typeof fetch>) => {
      const input = args[0]
      const url = input instanceof Request ? input.url : String(input)
      const response = await original(...args)
      if (!capture.done && new URL(url, location.href).pathname === '/api/v1/chat/stream') {
        capture.status = response.status
        const body = response.clone().body
        capture.done = (async () => {
          const decoder = new TextDecoder('utf-8')
          const reader = body?.getReader()
          while (reader) {
            const { done, value } = await reader.read()
            if (done) break
            const text = decoder.decode(value, { stream: true })
            if (text) capture.chunks.push({ at: performance.now(), text })
          }
          const tail = decoder.decode()
          if (tail) capture.chunks.push({ at: performance.now(), text: tail })
          capture.endAt = performance.now()
        })()
      }
      return response
    }
    target.restoreH5Fetch = () => { window.fetch = original }
  })
}

/** Wait for the captured stream to end and hand back its reads. */
export async function readStreamCapture(page: Page): Promise<{ status: number; startedAt: number; endAt: number | null; chunks: StreamChunk[] }> {
  return page.evaluate(async () => {
    const target = window as CaptureWindow
    const capture = target.h5Capture
    try {
      if (!capture?.done) throw new Error('The browser did not capture a chat stream')
      await capture.done
      return { status: capture.status, startedAt: capture.startedAt, endAt: capture.endAt, chunks: capture.chunks }
    } finally { target.restoreH5Fetch?.() }
  })
}

export type DeliveryDom = {
  cards: Array<{ name: string; href: string; label: string }>
  previews: Array<{ src: string; loaded: boolean; width: number; download: string }>
}

/** What the last assistant message offers the user for the files it delivered. */
export async function readDelivery(page: Page): Promise<DeliveryDom> {
  return page.evaluate(() => {
    const message = Array.from(document.querySelectorAll('.verevon-chat-message--assistant')).at(-1)
    const cards = Array.from(message?.querySelectorAll<HTMLAnchorElement>('.verevon-chat-generated-file a[download]') ?? []).map(anchor => ({
      name: anchor.getAttribute('download') ?? '',
      href: anchor.getAttribute('href') ?? '',
      label: anchor.querySelector('em')?.textContent ?? '',
    }))
    const previews = Array.from(message?.querySelectorAll<HTMLElement>('.verevon-chat-image-preview') ?? []).map(figure => {
      const image = figure.querySelector<HTMLImageElement>('img')
      return {
        src: image?.getAttribute('src') ?? '',
        loaded: Boolean(image?.complete && image.naturalWidth > 0),
        width: image?.naturalWidth ?? 0,
        download: figure.querySelector('a[download]')?.getAttribute('download') ?? '',
      }
    })
    return { cards, previews }
  })
}

// ---------------------------------------------------------------------------
// The oracle: Python, with the libraries the sandbox ships, in the stack's own image.
// ---------------------------------------------------------------------------

// Relative to the working directory, like the other specs' fixtures: Playwright and vitest both run from the project root.
export const ORACLE_DIR = resolve('tests/e2e/fixtures/code-interpreter')
export const DEFAULT_ORACLE_IMAGE = 'model-plane-execution-core'

export type OracleVerdict = {
  file: string
  kind: string
  ok: boolean
  problems: string[]
  facts: Record<string, unknown>
  bytes?: number
}

export type OracleCommand = 'check' | 'reference' | 'self-test'

/**
 * How to run the oracle. By default in a throwaway container from the stack's own
 * execution-core image (python3 plus openpyxl, python-docx, pypdf, python-pptx and
 * Pillow at the versions the sandbox runs), offline, with the oracle directory and
 * the files to check mounted read-only. H5_ORACLE_PYTHON points at a local
 * interpreter that has those libraries instead; H5_ORACLE_IMAGE names another image.
 */
export function oracleInvocation(
  command: OracleCommand,
  filesDir: string | null,
  env: Record<string, string | undefined> = process.env,
): { command: string; args: string[] } {
  const python = env.H5_ORACLE_PYTHON?.trim()
  if (python) {
    return { command: python, args: [resolve(ORACLE_DIR, 'h5_oracle.py'), command, ...(filesDir ? [resolve(filesDir)] : [])] }
  }
  const image = env.H5_ORACLE_IMAGE?.trim() || DEFAULT_ORACLE_IMAGE
  return {
    command: 'docker',
    args: [
      'run', '--rm', '--network', 'none',
      '-v', `${ORACLE_DIR}:/h5/oracle:ro`,
      ...(filesDir ? ['-v', `${resolve(filesDir)}:/h5/files:ro`] : []),
      '--entrypoint', 'python3', image,
      '/h5/oracle/h5_oracle.py', command, ...(filesDir ? ['/h5/files'] : []),
    ],
  }
}

/** Run the oracle and parse its one JSON document; a failure to run it is an error, never a pass. */
export function runOracle<T = { results: OracleVerdict[] }>(command: OracleCommand, filesDir: string | null = null): T {
  const { command: program, args } = oracleInvocation(command, filesDir)
  const run = spawnSync(program, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: 180_000 })
  if (run.error) throw new Error(`The oracle could not be started (${run.error.message}). Run it where docker and the stack image exist, or set H5_ORACLE_PYTHON to a Python with openpyxl, python-docx, pypdf, python-pptx and Pillow.`)
  // self-test exits 1 when it caught itself out, and still prints its report.
  const parsed = parseObject(run.stdout.trim().split('\n').at(-1) ?? '')
  if (!parsed) throw new Error(`The oracle printed no JSON (exit ${run.status}): ${run.stderr.trim().slice(-400)}`)
  return parsed as T
}

// ---------------------------------------------------------------------------
// Judging a turn.
// ---------------------------------------------------------------------------

export type TurnObservation = {
  summary: TurnSummary
  dom: DeliveryDom
  /** What the oracle said about the files this turn delivered (all of them). */
  verdicts: OracleVerdict[]
  /** The final answer text; only ever tested, never stored. */
  answer: string
}

const mentionsFile = (names: readonly string[], extension: string) => names.some(name => name.toLowerCase().endsWith(extension))

/** A markdown link or a sandbox path to a file: none of them resolve, the cards are the way. */
export function answerHasDeadFileLink(answer: string): boolean {
  return /sandbox:\//i.test(answer) || /\]\(\s*(?:sandbox:|[^)\s]*\.(?:xlsx|docx|pdf|pptx|png)\b)/i.test(answer)
}

/**
 * Every way this turn fell short of the proof for `proofCase`, in words; an empty
 * list is a pass. All of them at once, so one run says everything that is wrong.
 */
export function judgeCase(proofCase: ProofCase, observation: TurnObservation): string[] {
  const { summary, dom, verdicts, answer } = observation
  const problems: string[] = []
  const cap = summary.errorCodes.find(code => (CAP_CODES as readonly string[]).includes(code))
  if (cap) {
    problems.push(`the subscription stopped the turn (${cap}). Record it and wait for the reset; do not switch model or retry on another route`)
  } else if (summary.errorCodes.length > 0) {
    problems.push(`the stream carried an error frame (${summary.errorCodes.join(', ')})`)
  }
  if (!summary.done && summary.errorCodes.length === 0) problems.push('the stream ended without a done frame')
  if (summary.done && summary.doneModel !== TERRA_MODEL) problems.push(`done reported model ${summary.doneModel ?? 'none'}, not ${TERRA_MODEL}`)

  const calls = summary.tools.filter(tool => tool.name === 'code_interpreter')
  if (calls.length === 0) {
    const called = [...new Set(summary.tools.map(tool => tool.name))]
    problems.push(`the tool decision never called code_interpreter (tools called: ${called.join(', ') || 'none'})`)
  }
  const produced = calls.filter(call => call.exitCode === 0 && !call.error && mentionsFile(call.filesDelivered, proofCase.extension))
  if (calls.length > 0 && produced.length === 0) {
    problems.push(`no code_interpreter call exited 0 and delivered a ${proofCase.extension} file (exit codes: ${calls.map(call => String(call.exitCode)).join(', ')})`)
  }
  for (const call of calls) {
    if (call.resultAfterMs !== null && call.resultAfterMs >= SANDBOX_LIMIT_MS) problems.push(`a code_interpreter call took ${call.resultAfterMs} ms, past the ${SANDBOX_LIMIT_MS} ms sandbox limit`)
  }

  const delivered = summary.attachments.filter(attachment => attachment.name.toLowerCase().endsWith(proofCase.extension))
  if (delivered.length === 0) problems.push(`no attachment frame delivered a ${proofCase.extension} file`)
  for (const attachment of delivered) {
    if (!attachment.dataUri || attachment.decodedBytes === 0) problems.push(`${attachment.name} did not arrive as a non-empty data URI`)
    else if (attachment.declaredBytes !== attachment.decodedBytes) problems.push(`${attachment.name} declared ${attachment.declaredBytes} bytes but carries ${attachment.decodedBytes}`)
  }

  if (proofCase.kind === 'png') {
    if (dom.previews.length === 0) problems.push('the PNG did not appear as an inline preview')
    else if (!dom.previews.some(preview => preview.loaded)) problems.push('the PNG preview is on the page but the image did not load')
    if (mentionsFile(dom.cards.map(card => card.name), proofCase.extension)) problems.push('the PNG was also shown as a file card')
  } else {
    const cards = dom.cards.filter(card => card.name.toLowerCase().endsWith(proofCase.extension))
    if (cards.length === 0) problems.push(`no download card for a ${proofCase.extension} file (cards: ${dom.cards.map(card => card.name).join(', ') || 'none'})`)
    if (cards.some(card => !card.href.startsWith('data:'))) problems.push('a download card does not carry the data URI')
  }

  const verdict = verdicts.filter(entry => entry.file.toLowerCase().endsWith(proofCase.extension))
  if (verdict.length === 0) problems.push(`the oracle was given no ${proofCase.extension} file`)
  else if (!verdict.some(entry => entry.ok)) problems.push(...verdict.flatMap(entry => entry.problems.map(problem => `${entry.file}: ${problem}`)))

  if (answerHasDeadFileLink(answer)) problems.push('the answer links to the file; a link or sandbox: path resolves to nothing')
  return problems
}

// ---------------------------------------------------------------------------
// Evidence: identifier-shaped strings, numbers and booleans only.
// ---------------------------------------------------------------------------

const SAFE_TOKEN = /^[A-Za-z0-9_.:+\-/]{0,100}$/

export function scrub(value: unknown): unknown {
  if (typeof value === 'string') return SAFE_TOKEN.test(value) ? value : '[omitted]'
  if (Array.isArray(value)) return value.map(scrub)
  if (isRecord(value)) return Object.fromEntries(Object.entries(value).filter(([key]) => SAFE_TOKEN.test(key)).map(([key, entry]) => [key, scrub(entry)]))
  return value
}

export const DEFAULT_PROOF_DIR = 'test-results/code-interpreter-proof'

export function proofDir(env: Record<string, string | undefined> = process.env): string {
  return resolve(env.H5_PROOF_DIR?.trim() || DEFAULT_PROOF_DIR)
}

export function appendEvidence(path: string, record: unknown): string {
  const line = JSON.stringify(scrub(record))
  mkdirSync(dirname(path), { recursive: true })
  appendFileSync(path, `${line}\n`)
  return line
}

// ---------------------------------------------------------------------------
// A mocked turn, for the dry run that proves the proof's own plumbing.
// ---------------------------------------------------------------------------

export type MockFile = { name: string; mime: string; bytes: number; content_b64: string }

const sseFrame = (event: string, data: object, id: number) => `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`

/**
 * The frames the gateway sends for one code_interpreter call that produced `files`:
 * the shapes are read from model-gateway (sse_events.rs ChatEvent::payload and
 * tool_loop.rs code_interpreter_events), so the real frontend renders them as it
 * would a live turn. No model is involved.
 */
export function mockCodeInterpreterTurn(files: readonly MockFile[], answer: string): string {
  let id = 0
  const next = () => ++id
  const delivered = files.map(file => `${file.name} (${file.mime}, ${file.bytes} bytes)`)
  const summaryLine = JSON.stringify({ stdout: files.map(file => `saved ${file.name}`).join('\n'), stderr: '', exit_code: 0, files_delivered: delivered })
  let body = sseFrame('connected', { request_id: 'h5-dry-run', thread_id: 'H5-DRY-RUN-THREAD', ok: true }, next())
  body += sseFrame('tool_call', { id: 'call-1', name: 'code_interpreter', args: { language: 'python', code: '# reference programs, see h5_oracle.py' } }, next())
  body += sseFrame('tool_result', { id: 'call-1', status: 'ok', output: `${summaryLine}\nThese files were delivered to the user as downloadable artifacts.`, error: null }, next())
  files.forEach((file, index) => {
    const url = `data:${file.mime};base64,${file.content_b64}`
    const artifactId = `call-1-file-${index + 1}`
    const kind = file.mime.startsWith('image/') ? 'image' : file.name.endsWith('.xlsx') ? 'spreadsheet' : 'file'
    body += sseFrame('artifact', { id: artifactId, kind, title: file.name, content: url, version: 1 }, next())
    body += sseFrame('attachment', { id: artifactId, name: file.name, type: file.mime, url, size: file.bytes }, next())
  })
  body += sseFrame('chunk', { request_id: 'h5-dry-run', delta: answer }, next())
  body += sseFrame('done', { request_id: 'h5-dry-run', model_used: TERRA_MODEL }, next())
  return body
}
