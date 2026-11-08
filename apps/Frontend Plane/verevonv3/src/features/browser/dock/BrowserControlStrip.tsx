import { Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Bot, Hand, ShieldCheck } from '@/shared/icons'
import type { BrowserControlMode } from '@/shared/api/browser-client'
import { useI18n } from '@/shared/i18n'
import './browser-control-strip.css'

export type BrowserControlStripProps = {
  /** The control mode from the projection. The strip never holds a mode of its own. */
  controlMode: BrowserControlMode
  /** Quarry's control epoch from the same projection; sent back as `expectedEpoch`. */
  controlEpoch?: number | null
  /** The agent is running a step or the AI loop right now. */
  agentWorking?: boolean
  /** The browser was opened for the person's own browsing, not an agent run. */
  personalBrowsing?: boolean
  /** A control or action request is in flight: the button waits. */
  busy?: boolean
  /** The session cannot be controlled right now (not live, or no session id). */
  disabled?: boolean
  /** Zero Data Retention on: the copy says nothing is stored. */
  zdr?: boolean
  /** Take over (`human_takeover`) or Give back (`agent_control`), with the epoch the strip rendered. */
  onChangeMode?: (mode: BrowserControlMode, expectedEpoch: number | null) => void
  onContinueHere?: () => void
  continueDisabled?: boolean
}

export type BrowserControlStripState = 'human' | 'agent_working' | 'agent'

/**
 * Who holds the browser right now (WS9.I). Three states, all read from the projection: the person
 * is in control (the agent is paused because they took over), the agent is working, or the agent
 * has control and is idle. A stale epoch is handled by the caller, which refetches the projection
 * and passes the new one back in; the strip does not keep its own copy.
 */
export function BrowserControlStrip(props: BrowserControlStripProps): JSX.Element {
  const i18n = useI18n()
  const tr = i18n.tr
  const humanInControl = () => props.controlMode === 'human_takeover'
  const state = (): BrowserControlStripState =>
    humanInControl() ? 'human' : props.agentWorking ? 'agent_working' : 'agent'
  const stateText = () => {
    switch (state()) {
      case 'human':
        if (props.personalBrowsing && !props.agentWorking) return tr('Du styrer nettleseren.', 'You are in control of the browser.')
        return tr(
          'Du styrer nettleseren. Agenten er satt på pause fordi du tok over.',
          'You are in control. The agent is paused because you took over.',
        )
      case 'agent_working':
        return tr('Agenten jobber i nettleseren.', 'The agent is working in the browser.')
      default:
        if (props.personalBrowsing && !props.agentWorking) return tr('Nettleseren er klar for agentstyring.', 'The browser is ready for agent control.')
        return tr('Agenten har kontrollen over nettleseren.', 'The agent has control of the browser.')
    }
  }
  const actionDisabled = () => props.disabled === true || props.busy === true
  const actionLabel = () =>
    props.onContinueHere ? tr('La Verevon fortsette i denne fanen', 'Let Verevon continue in this tab') : humanInControl()
      ? props.personalBrowsing && !props.agentWorking ? tr('Gi agenten kontroll', 'Give the agent control') : tr('Gi tilbake', 'Give back')
      : tr('Ta over', 'Take over')
  const onAction = () => {
    if (actionDisabled()) return
    if (props.onContinueHere) {
      if (!props.continueDisabled) props.onContinueHere()
      return
    }
    const next: BrowserControlMode = humanInControl() ? 'agent_control' : 'human_takeover'
    props.onChangeMode?.(next, typeof props.controlEpoch === 'number' ? props.controlEpoch : null)
  }

  return (
    <div
      class="browser-control-strip"
      data-control-mode={props.controlMode}
      data-state={state()}
      data-busy={props.busy === true ? 'true' : 'false'}
    >
      <div class="browser-control-strip__row">
        <p class="browser-control-strip__state" role="status" aria-live="polite">
          <span class="browser-control-strip__dot" data-state={state()} aria-hidden="true" />
          <span>{stateText()}</span>
        </p>
        <Show when={props.onContinueHere || props.onChangeMode}>
          <button
            type="button"
            class="browser-control-strip__action"
            disabled={actionDisabled() || Boolean(props.onContinueHere && props.continueDisabled)}
            aria-busy={props.busy === true ? 'true' : 'false'}
            onClick={onAction}
          >
            <Show when={humanInControl()} fallback={<Hand class="size-3" aria-hidden="true" />}>
              <Bot class="size-3" aria-hidden="true" />
            </Show>
            <span>{actionLabel()}</span>
          </button>
        </Show>
      </div>
      <Show when={props.zdr === true}>
        <p class="browser-control-strip__zdr">
          <ShieldCheck class="size-3" aria-hidden="true" />
          <span>
            {tr(
              'Zero Data Retention: ingenting fra denne økten lagres.',
              'Zero Data Retention: nothing from this session is stored.',
            )}
          </span>
        </p>
      </Show>
    </div>
  )
}
