import { For, type JSX, Show } from 'solid-js'
import type { TenderStrictness } from '@/shared/api/anbud-client'
import { useI18n } from '@/shared/i18n'
import { type ProfileDraft, type ProfileField, type ProfileProblem, sourceLabel } from '@/features/anbud/lib/profile'
import { anbudWorkspace } from '@/features/anbud/lib/anbud-state'
import { strictnessLabel, strictnessSteps } from '@/features/anbud/lib/strictness'

type Props = {
  draft: () => ProfileDraft
  onDraft: (draft: ProfileDraft) => void
  strictness: () => TenderStrictness
  onStrictness: (step: TenderStrictness) => void
  problems: ProfileProblem[]
  busy: boolean
  onSubmit: (event: SubmitEvent) => void
}

function Field(props: { label: string; hint?: string; source?: string; problem?: () => string | undefined; children: JSX.Element }) {
  return (
    <label class="verevon-anbud__field">
      <span>
        {props.label}
        <Show when={props.source}>{(text) => <small class="verevon-anbud__source">{` · ${text()}`}</small>}</Show>
      </span>
      {props.children}
      <Show when={props.hint}>{(text) => <span class="verevon-anbud__hint">{text()}</span>}</Show>
      <Show when={props.problem?.()}>{(text) => <span class="verevon-anbud__problem">{text()}</span>}</Show>
    </label>
  )
}

/** The company profile the matches and the radar are made for, and the slider for how far they reach. */
export function TenderProfileForm(props: Props) {
  const { tr } = useI18n()

  // Where a field's value came from, when the server kept it: a suggestion, the organisation's record, or the user.
  const sourceOf = (field: string) => {
    const label = sourceLabel(anbudWorkspace.profileSources()[field])
    return label ? tr(label.no, label.en) : undefined
  }
  const update = (field: keyof ProfileDraft, event: Event) => {
    const value = (event.currentTarget as HTMLInputElement).value
    props.onDraft({ ...props.draft(), [field]: value })
  }

  const problemText = (problems: ProfileProblem[], field: ProfileField): string | undefined => {
    const message = problems.find((problem) => problem.field === field)?.message
    return message ? tr(message.no, message.en) : undefined
  }

  return (
    <form class="verevon-panel verevon-anbud__profile" onSubmit={(event) => props.onSubmit(event)}>
      <Field
        label={tr('Organisasjonsnummer (valgfritt)', 'Organisation number (optional)')}
        hint={tr('Ni sifre. Selskapet slås opp i Enhetsregisteret.', 'Nine digits. The company is checked in the register.')}
        source={sourceOf('orgNumber')}
        problem={() => problemText(props.problems, 'orgNumber')}
      >
        <input
          class="verevon-anbud__input"
          inputmode="numeric"
          autocomplete="off"
          value={props.draft().orgNumber}
          onInput={(event) => update('orgNumber', event)}
        />
      </Field>

      <Field
        label={tr('CPV-koder', 'CPV codes')}
        hint={tr('Åtte sifre, skilt med komma.', 'Eight digits, separated by commas.')}
        source={sourceOf('cpv')}
        problem={() => problemText(props.problems, 'cpv')}
      >
        <input
          class="verevon-anbud__input"
          placeholder="72212000, 72000000"
          autocomplete="off"
          value={props.draft().cpv}
          onInput={(event) => update('cpv', event)}
        />
      </Field>

      <Field
        label={tr('Regioner (NUTS)', 'Regions (NUTS)')}
        hint={tr('For eksempel NO08 for Vestlandet.', 'For example NO08 for Western Norway.')}
        source={sourceOf('nuts')}
        problem={() => problemText(props.problems, 'nuts')}
      >
        <input
          class="verevon-anbud__input"
          placeholder="NO08"
          autocomplete="off"
          value={props.draft().nuts}
          onInput={(event) => update('nuts', event)}
        />
      </Field>

      <Field
        label={tr('Nøkkelord', 'Keywords')}
        hint={tr('Skilt med komma. Brukes når CPV-kodene ikke er nok.', 'Separated by commas. Used when the CPV codes are not enough.')}
        problem={() => problemText(props.problems, 'keywords')}
      >
        <input class="verevon-anbud__input" autocomplete="off" value={props.draft().keywords} onInput={(event) => update('keywords', event)} />
      </Field>

      <div class="verevon-anbud__field">
        <Field label={tr('Verdi fra (NOK)', 'Value from (NOK)')} problem={() => problemText(props.problems, 'value')}>
          <input
            class="verevon-anbud__input"
            inputmode="numeric"
            autocomplete="off"
            value={props.draft().valueMin}
            onInput={(event) => update('valueMin', event)}
          />
        </Field>
        <Field label={tr('Verdi til (NOK)', 'Value to (NOK)')}>
          <input
            class="verevon-anbud__input"
            inputmode="numeric"
            autocomplete="off"
            value={props.draft().valueMax}
            onInput={(event) => update('valueMax', event)}
          />
        </Field>
      </div>

      <fieldset class="verevon-anbud__strictness">
        <legend>{tr('Hvor bredt skal søket være?', 'How wide should the search be?')}</legend>
        <div class="verevon-segmented" role="group">
          <For each={strictnessSteps}>
            {(step) => (
              <button
                type="button"
                class="verevon-segmented-button"
                classList={{ 'verevon-segmented-button--selected': props.strictness() === step }}
                aria-pressed={props.strictness() === step}
                onClick={() => props.onStrictness(step)}
              >
                {strictnessLabel(step, tr)}
              </button>
            )}
          </For>
        </div>
        <p class="verevon-anbud__hint">
          {tr(
            'Strikt viser bare det som ligger inne i profilen. Bredere steg viser også nære koder.',
            'Strict shows only what lies inside the profile. Wider steps also show near codes.',
          )}
        </p>
      </fieldset>

      <button class="button button--primary button--md" type="submit" disabled={props.busy}>
        {props.busy ? tr('Søker…', 'Searching…') : tr('Søk', 'Search')}
      </button>
    </form>
  )
}
