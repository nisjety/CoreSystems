import { For } from 'solid-js'
import { Button } from '@/shared/ui/Button'

export function SocialProofStepContent(props: { onContinue: () => void }) {
  return (
    <section class="onboarding-copy onboarding-copy--trust">
      <p class="onboarding-eyebrow">Trygghet</p>
      <h1>Bygg Med Verevon</h1>
      <p>Vi gir samme infrastruktur som større supportteam - uten tungt oppsett. Datakildene du nettopp koblet til er allerede klare.</p>

      <ul class="onboarding-stat-list">
        <li>
          <strong>97 %</strong> av førsteforespørsler besvares innen 60 s.
        </li>
        <li>
          <strong>42 %</strong> raskere førstesvar etter første uke.
        </li>
        <li>
          <strong>SOC 2</strong> Type II · GDPR · ZDR-modus tilgjengelig.
        </li>
      </ul>

      <Button variant="primary" size="sm" fullWidth onClick={props.onContinue}>
        Se planene
      </Button>
    </section>
  )
}

export function SocialProofStepVisual() {
  return (
    <div class="onboarding-logo-grid">
      <For each={['Apple', 'Microsoft', 'Slack', 'Notion', 'Zammad', 'Sanity']}>
        {(item) => <div>{item}</div>}
      </For>
    </div>
  )
}
