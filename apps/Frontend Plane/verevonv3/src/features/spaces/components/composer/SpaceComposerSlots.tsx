import { For, Show } from 'solid-js'

import { WandSparkles, X } from '@/shared/icons'
import { AiModelPicker } from '@/shared/components/AiModelPicker'
import type { Skill } from '@/shared/api/skills-client'
import { useI18n } from '@/shared/i18n'
import type { SpaceComposerSlotContext, SpaceComposerSlotEntry } from '../../lib/space-composer-registry'

/**
 * The composer's two slots (P-SPA2). A slice that adds a control adds one entry
 * to the matching array below; entries render in array order, with no wrapper
 * element, so an unfilled slot leaves the DOM exactly as it was.
 *
 * The toolbar slot sits above the textarea (the model picker today). The chip
 * slot sits under it (chosen skills today). Neither slot is an authority
 * surface: an entry shows or collects input, the owning plane decides.
 */

/**
 * The scope a skill actually has. Two values exist in the registry — `org`
 * and `user` — and both are stated; anything else is left unbadged rather than
 * guessed. There is deliberately no "this room": the registry has no Space
 * scope to promise.
 */
export function SkillScope(props: { readonly scope: Skill['scope'] }) {
  const i18n = useI18n()
  const label = () =>
    props.scope === 'user'
      ? i18n.tr('Personlig', 'Personal')
      : props.scope === 'org'
        ? i18n.tr('Organisasjon', 'Organization')
        : undefined
  return (
    <Show when={label()}>
      {(text) => (
        <span class="verevon-space-room-skill-scope" data-scope={props.scope}>
          {text()}
        </span>
      )}
    </Show>
  )
}

/**
 * Send or Ask (WS01-S6). Shown only while the composer offers more than one mode: with human
 * posts off, or when the message can only be a question to the agent (a picked agent or skill, a
 * reply in someone else's thread), there is nothing to choose and the box looks as it always did.
 */
function ModeSwitchEntry(props: { readonly ctx: SpaceComposerSlotContext }) {
  const i18n = useI18n()
  // In the thread pane the first choice is a reply to that thread, not a post to the room.
  const label = (mode: { readonly id: string; readonly labelNo: string; readonly labelEn: string }) =>
    mode.id === 'post' && props.ctx.inThread?.() === true
      ? i18n.tr('Svar i samtalen', 'Reply in thread')
      : i18n.tr(mode.labelNo, mode.labelEn)
  return (
    <div
      class="verevon-space-room-composer__modes"
      role="radiogroup"
      aria-label={i18n.tr('Hvordan meldingen sendes', 'How the message is sent')}
    >
      <For each={props.ctx.modes()}>
        {(mode) => (
          <button
            type="button"
            role="radio"
            aria-checked={props.ctx.mode() === mode.id ? 'true' : 'false'}
            data-mode={mode.id}
            disabled={props.ctx.disabled()}
            onClick={() => props.ctx.setMode(mode.id)}
          >
            {label(mode)}
          </button>
        )}
      </For>
    </div>
  )
}

function ModelPickerEntry(props: { readonly ctx: SpaceComposerSlotContext }) {
  return (
    <AiModelPicker
      orgId={props.ctx.orgId}
      class="verevon-space-room-composer__model-picker"
      catalogue={props.ctx.model.catalogue}
      onChange={props.ctx.model.onChange}
    />
  )
}

function SkillChipsEntry(props: { readonly ctx: SpaceComposerSlotContext }) {
  const i18n = useI18n()
  return (
    <Show when={props.ctx.skills.picked().length > 0}>
      <ul class="verevon-space-room-skills" aria-label={i18n.tr('Valgte ferdigheter', 'Chosen skills')}>
        <For each={props.ctx.skills.picked()}>
          {(skill) => (
            <li class="verevon-space-room-skill-chip">
              <WandSparkles class="verevon-space-room-skill-chip__icon" aria-hidden="true" />
              <span>{skill.name}</span>
              <SkillScope scope={skill.scope} />
              <button
                type="button"
                class="verevon-space-room-skill-chip__remove"
                onClick={() => props.ctx.skills.remove(skill.id)}
                aria-label={i18n.tr(`Fjern ferdigheten ${skill.name}`, `Remove the skill ${skill.name}`)}
              >
                <X aria-hidden="true" />
              </button>
            </li>
          )}
        </For>
      </ul>
    </Show>
  )
}

export const SPACE_COMPOSER_TOOLBAR: readonly SpaceComposerSlotEntry[] = [
  { id: 'mode-switch', enabled: (ctx) => ctx.modes().length > 1, render: (ctx) => <ModeSwitchEntry ctx={ctx} /> },
  // The model only matters to a turn the agent answers; a human post has none.
  { id: 'model-picker', enabled: (ctx) => ctx.mode() !== 'post', render: (ctx) => <ModelPickerEntry ctx={ctx} /> },
]

export const SPACE_COMPOSER_CHIPS: readonly SpaceComposerSlotEntry[] = [
  { id: 'skills', render: (ctx) => <SkillChipsEntry ctx={ctx} /> },
]

function SpaceComposerSlot(props: {
  readonly entries: readonly SpaceComposerSlotEntry[]
  readonly ctx: SpaceComposerSlotContext
}) {
  return (
    <For each={props.entries}>
      {(entry) => <Show when={entry.enabled?.(props.ctx) !== false}>{entry.render(props.ctx)}</Show>}
    </For>
  )
}

export function SpaceComposerToolbarSlot(props: { readonly ctx: SpaceComposerSlotContext }) {
  return <SpaceComposerSlot entries={SPACE_COMPOSER_TOOLBAR} ctx={props.ctx} />
}

export function SpaceComposerChipSlot(props: { readonly ctx: SpaceComposerSlotContext }) {
  return <SpaceComposerSlot entries={SPACE_COMPOSER_CHIPS} ctx={props.ctx} />
}
