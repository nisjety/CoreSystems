import { Match, Switch } from 'solid-js'
import type { ChatToolCall } from '@/features/chat/components/chat-types'
import type { ToolIntent } from '@/shared/chat-nodes/tool-presentation'
import { pickComputerViewer } from '@/shared/computer/viewer-registry'
import { FileViewer } from './FileViewer'
import { TerminalViewer } from './TerminalViewer'
import './computer-viewer.css'

/**
 * `VITE_CHAT_COMPUTER_PANEL=true` draws a tool call's terminal output and workspace file in the computer panel's typed
 * viewers. Unset, or any value but `true` (the default), every tool card keeps its raw output. Exact string, as the
 * other chat switches.
 */
export const chatComputerPanelEnabled = () => import.meta.env.VITE_CHAT_COMPUTER_PANEL === 'true'

/** The viewer of one tool call, picked by its intent. Renders nothing for an intent the registry does not claim. */
export function ComputerViewer(props: { call: ChatToolCall; intent: ToolIntent }) {
  const kind = () => pickComputerViewer(props.intent)?.kind

  return (
    <Switch>
      <Match when={kind() === 'terminal'}>
        <TerminalViewer call={props.call} />
      </Match>
      <Match when={kind() === 'file'}>
        <FileViewer call={props.call} />
      </Match>
    </Switch>
  )
}
