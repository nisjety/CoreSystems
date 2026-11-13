import { For, Show, createMemo } from 'solid-js'
import type { ChatToolCall } from '@/features/chat/components/chat-types'
import { envelope, toolPresentation } from '@/shared/chat-nodes/tool-presentation'
import { pickComputerViewer } from '@/shared/computer/viewer-registry'
import { Folder, Globe, Terminal } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { FileViewer } from './FileViewer'
import { TerminalViewer, type LiveProcessTarget } from './TerminalViewer'
import './computer-viewer.css'

/**
 * The tabs of the computer panel (EP-1 W4-P4, F-t3-035). Browser is the run's browser region, which the caller draws
 * beneath the strip while Browser is selected. Terminal and Files each draw one tool call.
 */
export type ComputerTab = 'browser' | 'terminal' | 'files'

/**
 * The latest call each non-browser tab draws. `process` is set for a process call, the only kind a live tail reads.
 */
export type ComputerPicks = {
  readonly terminal?: { readonly call: ChatToolCall; readonly process: boolean }
  readonly file?: ChatToolCall
}

/** Model Gateway's id shape for a run and for a process (1 to 64 of `[A-Za-z0-9_-]`). */
const RUN_ID_SHAPE = /^[A-Za-z0-9_-]{1,64}$/
/** The URL-safe Space reference the BFF route accepts. */
const SPACE_REF_SHAPE = /^[A-Za-z0-9_.~-]{1,200}$/

/**
 * The latest terminal or process call and the latest workspace file call, each by the intent `toolPresentation`
 * recomputes for it. A call whose intent has no viewer (a browser step, a search) draws no tab.
 */
export function computerPicksOf(calls: readonly ChatToolCall[]): ComputerPicks {
  let terminal: ComputerPicks['terminal'] = undefined
  let file: ChatToolCall | undefined = undefined
  for (const call of calls) {
    const { intent } = toolPresentation(call)
    const kind = pickComputerViewer(intent)?.kind
    if (kind === 'terminal') terminal = { call, process: intent === 'process' }
    else if (kind === 'file') file = call
  }
  return { terminal, file }
}

/** The process a terminal call names: the `process_id` it was given, or the id a `process_start` returned. */
function processIdOf(call: ChatToolCall): string | undefined {
  const args = call.args && typeof call.args === 'object' ? (call.args as Record<string, unknown>) : undefined
  const given = args?.process_id
  if (typeof given === 'string' && given.trim()) return given.trim()
  const started = envelope(call.output)?.process_id
  return typeof started === 'string' && started.trim() ? started.trim() : undefined
}

/**
 * The live tail a process call reads: its run, its process, and the Space the run belongs to. Anything missing or
 * malformed reads nothing, so the Terminal tab shows the transcript alone.
 */
function liveTargetOf(
  pick: { call: ChatToolCall; process: boolean },
  runId: string,
  spaceRef: string | undefined,
): LiveProcessTarget | undefined {
  const space = spaceRef?.trim() ?? ''
  if (!pick.process || !RUN_ID_SHAPE.test(runId) || !SPACE_REF_SHAPE.test(space)) return undefined
  const pid = processIdOf(pick.call)
  return pid && RUN_ID_SHAPE.test(pid) ? { runId, pid, spaceRef: space } : undefined
}

/**
 * The computer panel's tab strip (EP-1 W4-P4). Browser is always there; Terminal and Files appear only for the kinds
 * the run used. The selected non-browser viewer is mounted only while it is selected, so a tab that is not shown
 * reads nothing and polls nothing. The caller owns the selection and draws the browser region under this strip.
 */
export function ComputerPanelTabs(props: {
  picks: ComputerPicks
  selected: ComputerTab
  onSelect: (tab: ComputerTab) => void
  /** The run the calls belong to. */
  runId: string
  /** The Space the run's processes live in. Without it the terminal stays transcript-only. */
  spaceRef?: string
}) {
  const i18n = useI18n()
  const tabs = createMemo(() => {
    const list: ComputerTab[] = ['browser']
    if (props.picks.terminal) list.push('terminal')
    if (props.picks.file) list.push('files')
    return list
  })
  const label = (tab: ComputerTab): string => {
    if (tab === 'browser') return i18n.tr('Nettleser', 'Browser')
    if (tab === 'terminal') return i18n.tr('Terminal', 'Terminal')
    return i18n.tr('Filer', 'Files')
  }
  const icon = (tab: ComputerTab) => {
    if (tab === 'browser') return <Globe size={14} />
    if (tab === 'terminal') return <Terminal size={14} />
    return <Folder size={14} />
  }
  const onTabKeyDown = (event: KeyboardEvent, tab: ComputerTab) => {
    const list = tabs()
    const index = list.indexOf(tab)
    let next: number
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = (index + 1) % list.length
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') next = (index - 1 + list.length) % list.length
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = list.length - 1
    else return
    const target = list[next]
    if (target === undefined) return
    event.preventDefault()
    props.onSelect(target)
    document.getElementById(`verevon-computer-tab-${target}`)?.focus()
  }
  const terminalPick = () => (props.selected === 'terminal' ? props.picks.terminal : undefined)
  const filePick = () => (props.selected === 'files' ? props.picks.file : undefined)

  return (
    <div class="verevon-computer-panel">
      <div
        class="verevon-chat-tabs verevon-computer-tabs"
        role="tablist"
        aria-label={i18n.tr('Visninger i datamaskinpanelet', 'Computer panel views')}
      >
        <For each={tabs()}>
          {(tab) => (
            <button
              type="button"
              role="tab"
              id={`verevon-computer-tab-${tab}`}
              aria-selected={props.selected === tab ? 'true' : 'false'}
              aria-controls={props.selected === tab && tab !== 'browser' ? `verevon-computer-tabpanel-${tab}` : undefined}
              tabindex={props.selected === tab ? 0 : -1}
              class={{ 'verevon-chat-tab': true, 'verevon-chat-tab--active': props.selected === tab }}
              onClick={() => props.onSelect(tab)}
              onKeyDown={(event) => onTabKeyDown(event, tab)}
            >
              {icon(tab)}
              <span>{label(tab)}</span>
            </button>
          )}
        </For>
      </div>
      <Show when={terminalPick()}>
        {(pick) => (
          <section
            class="verevon-computer-panel__view"
            role="tabpanel"
            id="verevon-computer-tabpanel-terminal"
            aria-labelledby="verevon-computer-tab-terminal"
          >
            <TerminalViewer call={pick().call} live={liveTargetOf(pick(), props.runId, props.spaceRef)} />
          </section>
        )}
      </Show>
      <Show when={filePick()}>
        {(call) => (
          <section
            class="verevon-computer-panel__view"
            role="tabpanel"
            id="verevon-computer-tabpanel-files"
            aria-labelledby="verevon-computer-tab-files"
          >
            <FileViewer call={call()} />
          </section>
        )}
      </Show>
    </div>
  )
}
