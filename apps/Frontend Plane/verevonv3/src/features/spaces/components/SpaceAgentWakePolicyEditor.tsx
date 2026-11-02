import { createEffect, createSignal, For, Show } from 'solid-js'

import { hasErrorCode } from '@/shared/api/http'
import {
  setSpaceAgentWakePolicy,
  type SpaceAgent,
  type SpaceAgentRespondTo,
  type SpaceRosterMember,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  WAKE_POLICY_MODES,
  allowlistCandidates,
  draftFromAgent,
  draftProblem,
  draftProblemMessage,
  draftsEqual,
  respondToModeHint,
  respondToModeLabel,
  type WakePolicyDraft,
} from '../lib/wake-policy'
import './space-tool-policy.css'
import './space-wake-policy.css'

/**
 * "Who can wake it" (WS08-S8): the per-binding `respond_to` editor in the Agent tab, shown to the
 * room's owner and managers behind `features.agent_wake_policy`. It shares the shell of the tool
 * policy editor (toggle, panel, save/discard) and stays presentation: the gateway checks the
 * caller's live role and compares `expectedUpdatedAt`, and a policy only narrows or widens the
 * mention path; Control still decides who may post at all.
 */
export function SpaceAgentWakePolicyEditor(props: {
  readonly spaceRef: string
  readonly agent: SpaceAgent
  readonly roster: () => readonly SpaceRosterMember[]
  /** Called after a save, and after a conflict, so the agent list is read again. */
  readonly onSaved?: () => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal('')
  const [notice, setNotice] = createSignal('')
  const [draft, setDraft] = createSignal<WakePolicyDraft>(draftFromAgent(props.agent))

  const stored = () => draftFromAgent(props.agent)
  const bindingRef = () => props.agent.binding_ref?.trim() || undefined
  const expectedUpdatedAt = () => props.agent.updated_at
  const problem = () => draftProblem(draft())
  const unchanged = () => draftsEqual(draft(), stored())
  const candidates = () => allowlistCandidates(props.roster())
  const candidateIds = () => new Set(candidates().map((member) => member.subject_id))
  // Ids on the stored list who are no longer postable members still count toward the cap and are
  // shown, so a save never silently drops one the manager did not touch.
  const orphans = () => draft().subjects.filter((id) => !candidateIds().has(id))

  // A fresh read of the binding (after a save or a conflict) replaces the draft.
  createEffect(
    () => `${props.agent.updated_at ?? ''}|${props.agent.respond_to ?? ''}|${(props.agent.respond_to_subjects ?? []).join(',')}`,
    () => {
      setDraft(draftFromAgent(props.agent))
    },
  )

  const setMode = (mode: SpaceAgentRespondTo) =>
    setDraft((current) => ({ mode, subjects: mode === 'allowlist' ? current.subjects : [] }))
  const toggleSubject = (id: string) =>
    setDraft((current) => ({
      mode: current.mode,
      subjects: current.subjects.includes(id)
        ? current.subjects.filter((existing) => existing !== id)
        : [...current.subjects, id],
    }))

  async function save() {
    const ref = bindingRef()
    const expected = expectedUpdatedAt()
    if (saving() || problem() !== null || !ref || expected === undefined) return
    setSaving(true)
    setError('')
    setNotice('')
    const next = draft()
    try {
      await setSpaceAgentWakePolicy(props.spaceRef, ref, {
        respondTo: next.mode,
        respondToSubjects: next.mode === 'allowlist' ? next.subjects : undefined,
        expectedUpdatedAt: expected,
      })
      setNotice(i18n.tr('Lagret.', 'Saved.'))
      props.onSaved?.()
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Innstillingen kunne ikke lagres. Ingenting er endret.',
          en: 'The setting could not be saved. Nothing was changed.',
        }),
      )
      // Someone else changed it: read the binding again rather than overwrite it blind.
      if (hasErrorCode(err, 'policy_conflict')) props.onSaved?.()
    } finally {
      setSaving(false)
    }
  }

  return (
    <div class="verevon-tool-policy verevon-wake-policy">
      <button
        type="button"
        class="verevon-tool-policy__toggle"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={() => {
          setOpen(!open())
        }}
      >
        {open() ? i18n.tr('Skjul', 'Hide') : i18n.tr('Hvem kan tilkalle den', 'Who can wake it')}
      </button>

      <Show when={open()}>
        <div class="verevon-tool-policy__panel">
          <p class="verevon-tool-policy__note">
            {i18n.tr(
              'Gjelder når noen nevner agenten med @. Det gir ingen ekstra rettigheter: Control avgjør fortsatt hvem som kan skrive i rommet.',
              'Applies when someone mentions the agent with @. It grants no extra rights: Control still decides who may post in the room.',
            )}
          </p>

          <fieldset class="verevon-wake-policy__modes" disabled={saving()}>
            <legend class="verevon-tool-policy__heading">{i18n.tr('Hvem kan tilkalle den', 'Who can wake it')}</legend>
            <For each={WAKE_POLICY_MODES}>
              {(mode) => (
                <label class="verevon-wake-policy__mode">
                  <input
                    type="radio"
                    name={`wake-policy-${bindingRef() ?? props.agent.subject_id}`}
                    checked={draft().mode === mode}
                    onChange={() => setMode(mode)}
                  />
                  <span>
                    <strong>{respondToModeLabel(mode, i18n.tr)}</strong>
                    <span class="verevon-tool-policy__row-note">{respondToModeHint(mode, i18n.tr)}</span>
                  </span>
                </label>
              )}
            </For>
          </fieldset>

          <Show when={draft().mode === 'allowlist'}>
            <fieldset class="verevon-wake-policy__people" disabled={saving()}>
              <legend class="verevon-tool-policy__heading">{i18n.tr('Personer', 'People')}</legend>
              <Show
                when={candidates().length > 0 || orphans().length > 0}
                fallback={
                  <p class="verevon-tool-policy__note">
                    {i18n.tr('Ingen i rommet kan velges ennå.', 'Nobody in the room can be chosen yet.')}
                  </p>
                }
              >
                <ul class="verevon-wake-policy__list">
                  <For each={candidates()}>
                    {(member) => (
                      <li>
                        <label>
                          <input
                            type="checkbox"
                            checked={draft().subjects.includes(member.subject_id)}
                            onChange={() => toggleSubject(member.subject_id)}
                          />{' '}
                          {member.display_name.trim() || i18n.tr('Medlem uten navn', 'Member with no name')}
                        </label>
                      </li>
                    )}
                  </For>
                  <For each={orphans()}>
                    {(id) => (
                      <li>
                        <label>
                          <input type="checkbox" checked onChange={() => toggleSubject(id)} />{' '}
                          {i18n.tr('Tidligere medlem', 'Former member')}
                        </label>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </fieldset>
          </Show>

          <Show when={problem()}>
            {(issue) => (
              <p class="verevon-tool-policy__error" role="alert">
                {draftProblemMessage(issue(), i18n.tr)}
              </p>
            )}
          </Show>

          <div class="verevon-tool-policy__actions">
            <button
              type="button"
              class="verevon-tool-policy__button"
              disabled={saving() || problem() !== null || unchanged() || !bindingRef() || expectedUpdatedAt() === undefined}
              onClick={() => void save()}
            >
              {saving() ? i18n.tr('Lagrer …', 'Saving…') : i18n.tr('Lagre', 'Save')}
            </button>
            <button
              type="button"
              class="verevon-tool-policy__button"
              disabled={saving() || unchanged()}
              onClick={() => setDraft(stored())}
            >
              {i18n.tr('Angre endringer', 'Discard changes')}
            </button>
          </div>

          <Show when={error()}>
            {(message) => (
              <p class="verevon-tool-policy__error" role="alert">
                {message()}
              </p>
            )}
          </Show>
          <Show when={notice()}>
            {(message) => (
              <p class="verevon-tool-policy__status" role="status">
                {message()}
              </p>
            )}
          </Show>
        </div>
      </Show>
    </div>
  )
}
