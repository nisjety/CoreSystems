import { Show } from 'solid-js'
import type { BrandingSignals } from '@/features/onboarding/lib/api'
import { brandHost, hasBrandSignals } from '@/features/onboarding/lib/view'

export function OnboardingBrandStrip(props: {
  branding?: BrandingSignals
  websiteUrl?: string
}) {
  return (
    <Show when={hasBrandSignals(props.branding)} fallback={<div aria-hidden="true" class="onboarding-brand-strip onboarding-brand-strip--empty" />}>
      <div class="onboarding-brand-strip">
        <Show when={props.branding?.favicon}>
          <img src={props.branding?.favicon} alt="" class="onboarding-brand-strip__favicon" />
        </Show>
        <span>{props.branding?.siteName || brandHost(props.websiteUrl) || 'Detected brand'}</span>
        <Show when={props.branding?.themeColor}>
          <span class="onboarding-brand-strip__swatch" style={{ background: props.branding?.themeColor }} />
        </Show>
      </div>
    </Show>
  )
}
