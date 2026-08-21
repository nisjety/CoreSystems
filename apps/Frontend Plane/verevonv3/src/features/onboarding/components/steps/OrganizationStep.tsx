import { Fingerprint, Info } from '@/shared/icons'
import { For, Show } from 'solid-js'
import type { BrregEnhet } from '@/features/onboarding/lib/api'
import { OnboardingField } from '@/features/onboarding/components/shared/OnboardingField'
import { OnboardingLinkButton } from '@/features/onboarding/components/shared/OnboardingLinkButton'
import { type OnboardingState, onboardingSizeOptions, type OrgSize } from '@/features/onboarding/lib/model'
import { sizeLabel } from '@/features/onboarding/lib/view'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import { VerevonChoiceChip } from '@/shared/ui/verevon/VerevonChoiceChip'
import { VerevonSelectableRow } from '@/shared/ui/verevon/VerevonSelectableRow'
import { VerevonSwitch } from '@/shared/ui/verevon/VerevonSwitch'

const ZDR_TOOLTIP =
  'Zero Data Retention (ZDR): et valgfritt tillegg for Pro/Enterprise-planer — av som standard. Slå på for at samtaleinnhold ikke lagres og ikke forlater tjenesten (maksimalt personvern). Når organisasjonen slår det på, gjelder valget for hvordan forespørslene behandles, og kan endres senere i organisasjonsinnstillingene.'

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
  onToggleZdr: (value: boolean) => void
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
              <VerevonSelectableRow
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
              <VerevonChoiceChip
                selected={props.organization.size === size}
                onClick={() => props.onSelectSize(size)}
              >
                {sizeLabel(size)}
              </VerevonChoiceChip>
            )}
          </For>
        </div>
      </fieldset>

      <div class="onboarding-zdr-row">
        <div class="onboarding-zdr-row__text">
          <div class="onboarding-zdr-row__label">
            <span>Zero Data Retention</span>
            <Badge tone="accent">Premium</Badge>
            <span
              class="onboarding-info"
              tabindex="0"
              role="img"
              aria-label={ZDR_TOOLTIP}
              title={ZDR_TOOLTIP}
            >
              <Info size={14} aria-hidden="true" />
            </span>
          </div>
          <p>Samtaleinnhold lagres ikke når dette er på. Krever en betalt plan.</p>
        </div>
        <VerevonSwitch
          label="Zero Data Retention"
          checked={props.organization.zeroDataRetention}
          onChange={(value) => props.onToggleZdr(value)}
        />
      </div>

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
