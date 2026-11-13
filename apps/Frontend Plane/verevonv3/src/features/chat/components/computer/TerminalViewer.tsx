import { For, Show, createEffect, createMemo, createSignal } from 'solid-js'
import type { ChatToolCall } from '@/features/chat/components/chat-types'
import { readRunProcessOutput, type RunProcessOutput } from '@/shared/api/computer-client'
import { envelope } from '@/shared/chat-nodes/tool-presentation'
import { useI18n } from '@/shared/i18n'

type Streams = { stdout: string; stderr: string; exitCode?: number; truncated: boolean }

/** The streams of a run that reported them in its envelope, or `null` for plain text. */
function streamsOf(output: string | undefined): Streams | null {
  const body = envelope(output)
  if (!body || (typeof body.stdout !== 'string' && typeof body.stderr !== 'string')) return null
  return {
    stdout: typeof body.stdout === 'string' ? body.stdout : '',
    stderr: typeof body.stderr === 'string' ? body.stderr : '',
    exitCode: typeof body.exit_code === 'number' ? body.exit_code : undefined,
    truncated: body.truncated === true,
  }
}

function commandOf(args: unknown): string | undefined {
  if (!args || typeof args !== 'object') return undefined
  const { command } = args as Record<string, unknown>
  return typeof command === 'string' && command.trim() ? command.trim() : undefined
}

/** The text of a process read's chunks, in order, and its exit code when it has one. `null` when there is no chunk list. */
function chunksOf(output: string | undefined): { text: string; exitCode?: number } | null {
  const body = envelope(output)
  if (!body || !Array.isArray(body.chunks)) return null
  let text = ''
  for (const chunk of body.chunks) {
    const piece: unknown = chunk && typeof chunk === 'object' ? (chunk as Record<string, unknown>).text : undefined
    if (typeof piece === 'string') text += piece
  }
  return { text, exitCode: typeof body.exit_code === 'number' ? body.exit_code : undefined }
}

/** A process a live tail reads: its run, its process id, and the Space the run belongs to. */
export type LiveProcessTarget = { runId: string; pid: string; spaceRef: string }

/** What a live tail has read so far, and whether it is still following the process. */
type LiveTail = {
  /** At least one page was read, so the tail holds the process's output (which may still be empty). */
  readonly received: boolean
  readonly text: string
  readonly state: string
  readonly exitCode: number | null
  readonly gapBefore: boolean
  /** The earliest output was dropped to keep the view bounded. */
  readonly trimmed: boolean
  /**
   * `following`: the process is alive and the tail keeps reading. `ended`: it exited and its output is read to the end.
   * `unavailable`: the read was refused, or the state is not one this view follows. `failed`: reads kept failing.
   */
  readonly status: 'following' | 'ended' | 'unavailable' | 'failed'
}

/** How often a live tail reads while its process is alive. */
const LIVE_POLL_MS = 2_000
/** Read failures in a row before the tail gives up and says so. */
const LIVE_MAX_FAILURES = 3
/** The most characters a live tail keeps. The oldest go first. */
const LIVE_TAIL_MAX_CHARS = 262_144

/** A process that can still produce output. */
function isLiveState(state: string): boolean {
  return state === 'STARTING' || state === 'RUNNING'
}

/** A process that has stopped producing output, as Model Gateway names it. */
const ENDED_STATES: ReadonlySet<string> = new Set(['EXITED', 'KILLED', 'EXPIRED', 'LOST'])

/**
 * Reads one process's output from the start of what is retained, page by page, and hands each change to `publish`.
 * It reads every two seconds while the process is alive, and at once while more output is retained. It pauses while
 * the tab is hidden and resumes when the tab is shown again. It stops when the process has exited and its output is
 * read to the end, when a read is refused, or when reads keep failing. The function it returns stops it for good.
 */
function followProcessOutput(target: LiveProcessTarget, publish: (tail: LiveTail) => void): () => void {
  let cursor = 0
  let text = ''
  let state = ''
  let exitCode: number | null = null
  let gapBefore = false
  let trimmed = false
  let received = false
  let failures = 0
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let inFlight: AbortController | undefined

  const clearTimer = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
  }
  const tailNow = (status: LiveTail['status']): LiveTail => ({
    received,
    text,
    state,
    exitCode,
    gapBefore,
    trimmed,
    status,
  })
  const stop = () => {
    stopped = true
    clearTimer()
    inFlight?.abort()
    document.removeEventListener('visibilitychange', onVisibility)
  }
  const later = (ms: number) => {
    clearTimer()
    if (stopped || document.visibilityState === 'hidden') return
    timer = setTimeout(() => {
      timer = undefined
      void read()
    }, ms)
  }
  const read = async () => {
    if (stopped || inFlight || document.visibilityState === 'hidden') return
    clearTimer()
    const request = new AbortController()
    inFlight = request
    let page: RunProcessOutput | null
    try {
      page = await readRunProcessOutput(target.runId, target.pid, target.spaceRef, cursor, request.signal)
    } catch {
      inFlight = undefined
      if (stopped) return
      failures += 1
      if (failures >= LIVE_MAX_FAILURES) {
        publish(tailNow('failed'))
        stop()
      } else {
        later(LIVE_POLL_MS)
      }
      return
    }
    inFlight = undefined
    if (stopped) return
    if (page === null) {
      publish(tailNow('unavailable'))
      stop()
      return
    }
    failures = 0
    received = true
    state = page.state
    exitCode = page.exitCode
    gapBefore = gapBefore || page.gapBefore
    for (const chunk of page.chunks) {
      if (chunk.seq > cursor) text += chunk.text
    }
    if (text.length > LIVE_TAIL_MAX_CHARS) {
      text = text.slice(text.length - LIVE_TAIL_MAX_CHARS)
      trimmed = true
    }
    const advanced = page.nextCursor > cursor
    cursor = page.nextCursor
    const alive = isLiveState(page.state)
    // A state the gateway does not name cannot be followed honestly, so the tail says it is not available.
    if (!alive && !ENDED_STATES.has(page.state)) {
      publish(tailNow('unavailable'))
      stop()
      return
    }
    publish(tailNow(alive ? 'following' : 'ended'))
    if (page.hasMore && advanced) later(0)
    else if (alive) later(LIVE_POLL_MS)
    else stop()
  }
  function onVisibility() {
    if (stopped) return
    if (document.visibilityState === 'hidden') {
      clearTimer()
      return
    }
    if (!inFlight && timer === undefined) void read()
  }

  document.addEventListener('visibilitychange', onVisibility)
  void read()
  return stop
}

/**
 * The terminal viewer of the computer panel (P1, W4-P4): a command or a process, read as the transcript its tool returned.
 * The output is text in a monospace block, never markup. A code run that reports its streams shows stdout and stderr
 * apart, with its exit code, and a process read shows the text of its chunks. With no `live` target it runs nothing: it
 * reads what the transcript already holds. With one, it follows the process's output through the BFF and shows that.
 */
export function TerminalViewer(props: { call: ChatToolCall; live?: LiveProcessTarget }) {
  const i18n = useI18n()
  const streams = () => streamsOf(props.call.output)
  const chunks = () => chunksOf(props.call.output)
  const command = () => commandOf(props.call.args)
  // The live target by value: a re-rendered call that names the same process keeps its tail and its cursor.
  const liveKey = createMemo(() => (props.live ? `${props.live.runId}|${props.live.pid}|${props.live.spaceRef}` : ''))
  const [tail, setTail] = createSignal<LiveTail | null>(null)
  createEffect(
    () => liveKey(),
    (key) => {
      setTail(null)
      if (!key) return undefined
      const [runId = '', pid = '', spaceRef = ''] = key.split('|')
      return followProcessOutput({ runId, pid, spaceRef }, (next) => setTail(next))
    },
  )
  // The live view follows its own end, unless the reader has scrolled up to read earlier output.
  let liveOutput: HTMLPreElement | undefined
  let atEnd = true
  createEffect(
    () => tail()?.text,
    () => {
      const element = liveOutput
      if (!element || !atEnd) return undefined
      requestAnimationFrame(() => {
        element.scrollTop = element.scrollHeight
      })
      return undefined
    },
  )
  const notes = (): string[] => {
    const current = tail()
    if (!current) return []
    const list: string[] = []
    if (current.status === 'following') {
      list.push(i18n.tr('Kjører', 'Running'))
      if (!current.text) list.push(i18n.tr('Venter på utdata …', 'Waiting for output …'))
    }
    if (current.status === 'ended') {
      const ended = i18n.tr('Avsluttet', 'Ended')
      list.push(current.exitCode === null ? ended : `${ended} · ${i18n.tr('Avslutningskode', 'Exit code')} ${current.exitCode}`)
    }
    if (current.status === 'unavailable') {
      list.push(i18n.tr('Live-utdataen er ikke tilgjengelig for denne prosessen.', 'Live output is not available for this process.'))
    }
    if (current.status === 'failed') {
      list.push(i18n.tr('Live-utdataen kan ikke leses akkurat nå.', 'Live output cannot be read right now.'))
    }
    if (current.trimmed) list.push(i18n.tr('Eldre utdata vises ikke.', 'Earlier output is not shown.'))
    if (current.gapBefore) list.push(i18n.tr('Noe av utdataen er ikke lenger lagret.', 'Some of the output is no longer kept.'))
    return list
  }

  return (
    <div class="verevon-computer-viewer" data-computer-viewer="terminal">
      <Show when={command()}>
        {(line) => <div class="verevon-computer-viewer__meta">$ {line()}</div>}
      </Show>
      <Show
        when={tail()?.received === true}
        fallback={(
          <Show
            when={streams()}
            fallback={(
              <Show
                when={chunks()}
                fallback={(
                  <Show when={props.call.output}>
                    {(text) => <pre class="verevon-computer-viewer__output">{text()}</pre>}
                  </Show>
                )}
              >
                {(value) => (
                  <>
                    <Show when={value().text}>
                      {(text) => <pre class="verevon-computer-viewer__output">{text()}</pre>}
                    </Show>
                    <Show when={!value().text && value().exitCode === undefined}>
                      <div class="verevon-computer-viewer__meta">{i18n.tr('Ingen ny utdata', 'No new output')}</div>
                    </Show>
                    <Show when={value().exitCode !== undefined}>
                      <div class="verevon-computer-viewer__meta">
                        {i18n.tr('Avslutningskode', 'Exit code')} {value().exitCode}
                      </div>
                    </Show>
                  </>
                )}
              </Show>
            )}
          >
            {(value) => (
              <>
                <Show when={value().stdout}>
                  {(text) => <pre class="verevon-computer-viewer__output">{text()}</pre>}
                </Show>
                <Show when={value().stderr}>
                  {(text) => <pre class="verevon-computer-viewer__stderr">{text()}</pre>}
                </Show>
                <Show when={value().exitCode !== undefined}>
                  <div class="verevon-computer-viewer__meta">
                    {i18n.tr('Avslutningskode', 'Exit code')} {value().exitCode}
                  </div>
                </Show>
              </>
            )}
          </Show>
        )}
      >
        <pre
          class="verevon-computer-viewer__output"
          ref={(element) => {
            liveOutput = element
          }}
          onScroll={(event) => {
            const element = event.currentTarget
            atEnd = element.scrollHeight - element.scrollTop - element.clientHeight < 16
          }}
        >
          {tail()?.text}
        </pre>
      </Show>
      <For each={notes()}>
        {(note) => <div class="verevon-computer-viewer__meta">{note}</div>}
      </For>
      <Show when={streams()?.truncated}>
        <div class="verevon-computer-viewer__meta">{i18n.tr('Utdataen ble avkortet', 'The output was truncated')}</div>
      </Show>
    </div>
  )
}
