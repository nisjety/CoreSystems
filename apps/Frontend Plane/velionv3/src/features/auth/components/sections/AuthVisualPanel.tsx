import { Fingerprint, Lock, ShieldCheck } from 'lucide-solid'
import { Show } from 'solid-js'
import type { Accessor } from 'solid-js'
import type { AuthCopy } from '@/features/auth/lib/model'
import { Button } from '@/shared/ui/Button'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'

type AuthVisualPanelProps = {
  copy: Accessor<AuthCopy>
  showConsent: Accessor<boolean>
  onDismissConsent: () => void
}

export function AuthVisualPanel(props: AuthVisualPanelProps) {
  return (
    <>
      <div class="auth-visual__image" />
      <div class="auth-visual__rail" aria-hidden="true">
        <span class="auth-visual__rail-line" />
        <span class="auth-visual__rail-glow" />
        <span class="auth-visual__scanner-dot" />
      </div>

      <div class="auth-visual__icons" aria-hidden="true">
        <ShieldCheck size={18} />
        <Lock size={18} />
        <Fingerprint size={18} />
      </div>

      <Show when={props.showConsent()}>
        <div class="auth-consent-banner">
          <VelionIconButton type="button" class="auth-consent-banner__icon" aria-label="Cookie settings">
            <span />
          </VelionIconButton>
          <p>{props.copy().cookies}</p>
          <VelionIconButton type="button" class="auth-consent-banner__prefs" aria-label="Open preferences">
            <span />
            <span />
            <span />
          </VelionIconButton>
          <Button
            type="button"
            variant="secondary"
            size="md"
            shape="pill"
            class="auth-consent-banner__secondary"
            onClick={() => props.onDismissConsent()}
          >
            {props.copy().decline}
          </Button>
          <Button
            type="button"
            variant="primary"
            size="md"
            shape="pill"
            class="auth-consent-banner__primary"
            onClick={() => props.onDismissConsent()}
          >
            {props.copy().accept}
          </Button>
        </div>
      </Show>
    </>
  )
}
