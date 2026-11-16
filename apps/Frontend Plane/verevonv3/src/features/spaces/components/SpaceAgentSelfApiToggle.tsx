import { createSignal, For, Show } from 'solid-js'

import { spaceSelfApiOfferedRows } from '@/shared/actions/agent-self-api'
import { useI18n } from '@/shared/i18n'
import { selfApiErrorMessage, setSpaceAgentSelfApi } from '../lib/space-self-api'
import './space-tool-policy.css'
import './space-wake-policy.css'

/**
 * WS18-S6: the owner's and managers' switch for one bound agent's own-thread tools (rename, pin). It writes through the
 * action registry (`spaces.set_agent_self_api`), so the gateway checks the caller's live role; it grants nothing on its
 * own, since Control decides each action at run time. The page shows it only behind `features.agent_self_api`, to an
 * owner or manager, for an active binding.
 */
export function SpaceAgentSelfApiToggle(props: {
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
  const checked = () => confirmed() ?? props.enabled
  let box: HTMLInputElement | undefined

  async function change(next: boolean) {
    if (saving() || !props.orgId || !props.actorId) return
    setSaving(true)
    setError('')
    setNotice('')
    try {
      const reply = await setSpaceAgentSelfApi(
        props.spaceRef,
        { type: 'human', userId: props.actorId, orgId: props.orgId },
        props.bindingRef,
        next,
      )
      setConfirmed(reply.selfApiEnabled)
      setNotice(i18n.tr('Lagret.', 'Saved.'))
      props.onChanged?.()
    } catch (err) {
      // The box showed the attempted value; put it back to what the gateway last confirmed.
      if (box) box.checked = checked()
      setError(selfApiErrorMessage(err, i18n.tr))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div class="verevon-tool-policy verevon-self-api">
      <label class="verevon-wake-policy__mode verevon-self-api__switch">
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
            'La agenten organisere tråder: gi nytt navn og fest',
            'Let this agent organise threads: rename and pin',
          )}
        </span>
      </label>
      <p class="verevon-tool-policy__note">
        {i18n.tr(
          'Av som standard. Den gir agenten ingen rettigheter alene: hver handling krever fortsatt Controls godkjenning for den kjøringen, og innstillingen har ingen virkning før organisasjonen har tillatt agenthandlinger i rommet.',
          "Off by default. It grants the agent no rights on its own: each action still needs Control's approval for that run, and it has no effect until your organization allows agent actions in this room.",
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
        <ul class="verevon-self-api__rows" aria-label={i18n.tr('Handlinger agenten kan utføre', 'Actions the agent can take')}>
          <For each={spaceSelfApiOfferedRows()}>
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
