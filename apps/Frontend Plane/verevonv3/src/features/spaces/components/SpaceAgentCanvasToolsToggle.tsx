import { createEffect, createSignal, For, Show } from 'solid-js'

import { spaceCanvasToolRows } from '@/shared/actions/agent-canvas-tools'
import { useI18n } from '@/shared/i18n'
import { canvasToolsErrorMessage, setSpaceAgentCanvasTools } from '../lib/space-canvas-tools'
import './space-tool-policy.css'
import './space-wake-policy.css'

/**
 * W3-DEC-CANVAS (D2): the owner's and managers' switch for one bound agent's room canvas tools (read the canvas, save a new
 * version of it). Its own switch, separate from the self-API switch (rename, pin, post cards). It writes through the action
 * registry (`spaces.set_agent_canvas_tools`), so the gateway checks the caller's live role; it grants nothing on its own,
 * since Control decides each canvas action at run time. The page shows it only behind `features.agent_canvas_tools`, to an
 * owner or manager, for an active binding.
 */
export function SpaceAgentCanvasToolsToggle(props: {
  readonly spaceRef: string
  readonly bindingRef: string
  /** The switch as the agent list last stated it. */
  readonly enabled: boolean
  readonly orgId: string
  readonly actorId: string
  /** Called after a change lands, so the agent list is read again. */
  readonly onChanged?: () => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal('')
  const [notice, setNotice] = createSignal('')
  // The value the gateway confirmed for this card; the agent list's value applies until one lands.
  const [confirmed, setConfirmed] = createSignal<boolean | undefined>(undefined)
  // The agent list is read again after a change. A value it states replaces the gateway's last answer, so a change another
  // owner made shows here rather than the one this card confirmed earlier.
  createEffect(
    () => props.enabled,
    () => {
      setConfirmed(undefined)
    },
  )
  const checked = () => confirmed() ?? props.enabled
  let box: HTMLInputElement | undefined

  async function change(next: boolean) {
    if (saving() || !props.orgId || !props.actorId) return
    setSaving(true)
    setError('')
    setNotice('')
    try {
      const reply = await setSpaceAgentCanvasTools(
        props.spaceRef,
        { type: 'human', userId: props.actorId, orgId: props.orgId },
        props.bindingRef,
        next,
      )
      setConfirmed(reply.canvasToolsEnabled)
      setNotice(i18n.tr('Lagret.', 'Saved.'))
      props.onChanged?.()
    } catch (err) {
      // The box showed the attempted value; put it back to what the gateway last confirmed.
      if (box) box.checked = checked()
      setError(canvasToolsErrorMessage(err, i18n.tr))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div class="verevon-tool-policy verevon-canvas-tools">
      <label class="verevon-wake-policy__mode verevon-canvas-tools__switch">
        <input
          ref={(element) => {
            box = element
          }}
          type="checkbox"
          checked={checked()}
          disabled={saving()}
          onChange={(event) => {
            void change(event.currentTarget.checked)
          }}
        />
        <span>
          {i18n.tr(
            'La agenten bruke rommets lerret: lese det og lagre endringer',
            'Let this agent use the room canvas: read it and save changes',
          )}
        </span>
      </label>
      <p class="verevon-tool-policy__note">
        {i18n.tr(
          'Av som standard. Den gir agenten ingen rettigheter alene: Control avgjør fortsatt hver lesing og lagring for den kjøringen, og innstillingen har ingen virkning før organisasjonen har tillatt agenthandlinger i rommet.',
          'Off by default. It grants the agent no rights on its own: Control still decides each read and save for that run, and it has no effect until your organization allows agent actions in this room.',
        )}
      </p>
      <button
        type="button"
        class="verevon-tool-policy__toggle"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={() => {
          setOpen(!open())
        }}
      >
        {open() ? i18n.tr('Skjul', 'Hide') : i18n.tr('Hva kan agenten gjøre?', 'What can the agent do?')}
      </button>

      <Show when={open()}>
        <ul class="verevon-canvas-tools__rows" aria-label={i18n.tr('Lerretshandlinger agenten kan utføre', 'Canvas actions the agent can take')}>
          <For each={spaceCanvasToolRows()}>
            {(row) => (
              <li>
                <strong>{i18n.tr(row.label.no, row.label.en)}</strong>
                <span class="verevon-tool-policy__row-note">
                  {' '}
                  {i18n.tr(row.consequence.no, row.consequence.en)}{' '}
                  {i18n.tr(`Inntil ${row.maxPerRun} ganger per kjøring.`, `Up to ${row.maxPerRun} times per run.`)}
                </span>
              </li>
            )}
          </For>
        </ul>
      </Show>

      <Show when={saving()}>
        <p class="verevon-tool-policy__note" role="status">
          {i18n.tr('Lagrer …', 'Saving …')}
        </p>
      </Show>
      <Show when={error()}>
        {(message) => (
          <p class="verevon-tool-policy__error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={!error() && notice()}>
        <p class="verevon-tool-policy__note" role="status">
          {notice()}
        </p>
      </Show>
    </div>
  )
}
