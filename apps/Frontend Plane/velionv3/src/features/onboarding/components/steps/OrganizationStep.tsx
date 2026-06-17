import { Fingerprint } from 'lucide-solid'
import { For, Show } from 'solid-js'
import type { BrregEnhet } from '@/features/onboarding/lib/api'
import { OnboardingField } from '@/features/onboarding/components/shared/OnboardingField'
import { OnboardingLinkButton } from '@/features/onboarding/components/shared/OnboardingLinkButton'
import { type OnboardingState, onboardingSizeOptions, type OrgSize } from '@/features/onboarding/lib/model'
import { sizeLabel } from '@/features/onboarding/lib/view'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import { VelionChoiceChip } from '@/shared/ui/velion/VelionChoiceChip'
import { VelionSelectableRow } from '@/shared/ui/velion/VelionSelectableRow'

type OrganizationStepContentProps = {
  organization: OnboardingState['organization']
  searchResults: BrregEnhet[]
  searching: boolean
  submitting: boolean
  websiteSkipped: boolean
  onClearResults: () => void
  onContinue: () => void | Promise<void>
  onNameInput: (value: string) => void
  onSearch: () => void | Promise<void>
  onSelectResult: (item: BrregEnhet) => void
  onSelectSize: (size: OrgSize) => void
  onSkipStep: () => void
}

export function OrganizationStepContent(props: OrganizationStepContentProps) {
  return (
    <section class="onboarding-copy onboarding-copy--organization">
      <p class="onboarding-eyebrow">Organisasjon</p>
      <h1>Bekreft Organisasjon</h1>
      <p>Vi prøver å kjenne igjen firmaet fra nettsiden. Bekreft forslaget, søk i Enhetsregisteret eller skriv inn navnet manuelt.</p>

      <OnboardingField label="Verifiser i Enhetsregisteret" optionalLabel="valgfritt" class="onboarding-field--org">
        <input
          value={props.organization.name}
          onInput={(event) => props.onNameInput(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            void props.onSearch()
          }}
          placeholder="Søk på organisasjonsnavn..."
        />
      </OnboardingField>

      <OnboardingLinkButton emphasis="large" onClick={props.onClearResults}>
        Hopp over verifisering
      </OnboardingLinkButton>

      <Show when={props.searching}>
        <p class="onboarding-search-status">Søker i Enhetsregisteret ...</p>
      </Show>

      <Show when={props.searchResults.length > 0}>
        <div class="onboarding-results">
          <For each={props.searchResults}>
            {(item) => (
              <VelionSelectableRow
                onClick={() => props.onSelectResult(item)}
                title={item.navn}
                description={item.organisasjonsnummer}
                meta={
                  <Show when={item.antallAnsatte != null}>
                    <Badge tone="accent">{item.antallAnsatte} ansatte</Badge>
                  </Show>
                }
              />
            )}
          </For>
        </div>
      </Show>

      <Show when={props.organization.orgNumber}>
        <div class="onboarding-verified">
          <Badge tone="accent">Verifisert</Badge>
          <span>{props.organization.orgNumber}</span>
        </div>
      </Show>

      <fieldset class="onboarding-size-fieldset">
        <legend>Hvor mange er dere?</legend>
        <div class="onboarding-size-grid">
          <For each={onboardingSizeOptions}>
            {(size) => (
              <VelionChoiceChip
                selected={props.organization.size === size}
                onClick={() => props.onSelectSize(size)}
              >
                {sizeLabel(size)}
              </VelionChoiceChip>
            )}
          </For>
        </div>
      </fieldset>

      <div class="onboarding-actions">
        <Button
          variant="primary"
          size="sm"
          fullWidth={props.websiteSkipped}
          onClick={() => void props.onContinue()}
          disabled={props.submitting || !props.organization.name.trim()}
        >
          {props.submitting ? 'Oppretter organisasjon' : 'Fortsett'}
        </Button>
        <Show when={!props.websiteSkipped}>
          <OnboardingLinkButton onClick={props.onSkipStep}>Hopp over</OnboardingLinkButton>
        </Show>
      </div>
    </section>
  )
}

export function OrganizationStepVisual(props: { organization: OnboardingState['organization'] }) {
  return (
    <div class="onboarding-organization-visual">
      <Show when={props.organization.name || props.organization.orgNumber}>
        <div class="onboarding-organization-card">
          <Fingerprint size={18} />
          <strong>{props.organization.name}</strong>
          <span>{props.organization.orgNumber}</span>
        </div>
      </Show>
    </div>
  )
}
