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
  { id: 'model-picker', render: (ctx) => <ModelPickerEntry ctx={ctx} /> },
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
