import { Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'

export function DashboardPlanBadge(props: { planLabel: string; planTrial?: boolean }) {
  const i18n = useI18n()
  // Upgrade is only offered on Trial. On any paid plan we drop the CTA and tint
  // the plan text with the accent (the same color the Upgrade button used).
  // billing-core elevates a trialing org to the `pro` tier ("Expert"), so
  // the label alone reads as paid; `planTrial` is the billing truth.
  const isTrial = () => props.planTrial === true || props.planLabel.trim().toLowerCase() === 'trial'
  return (
    <span
      class={['dashboard-home-plan-badge', { 'dashboard-home-plan-badge--paid': !isTrial() }]}
    >
      {localPlanLabel(props.planLabel, i18n)} {i18n.tr('plan', 'Plan')}
      <Show when={props.planTrial}>
        <span>·</span>
        <span>{i18n.tr('prøveperiode', 'free trial')}</span>
      </Show>
      <Show when={isTrial()}>
        <span>·</span>
        <button type="button">{i18n.tr('Oppgrader', 'Upgrade')}</button>
      </Show>
    </span>
  )
}

function localPlanLabel(planLabel: string, i18n: ReturnType<typeof useI18n>) {
  const normalized = planLabel.trim().toLowerCase()
  if (normalized === 'advanced') return i18n.tr('Avansert', 'Advanced')
  if (normalized === 'custom') return i18n.tr('Tilpasset', 'Custom')
  if (normalized === 'enterprise') return i18n.tr('Enterprise', 'Enterprise')
  if (normalized === 'essential') return i18n.tr('Essential', 'Essential')
  if (normalized === 'expert') return i18n.tr('Ekspert', 'Expert')
  if (normalized === 'free') return i18n.tr('Gratis', 'Free')
  if (normalized === 'hobby') return i18n.tr('Hobby', 'Hobby')
  if (normalized === 'standard') return i18n.tr('Standard', 'Standard')
  if (normalized === 'trial') return i18n.tr('Prøve', 'Trial')
  return planLabel
}
