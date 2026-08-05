import { Fingerprint, Lock, ShieldCheck } from 'lucide-solid'
import { Show } from 'solid-js'
import type { Accessor } from 'solid-js'
import type { AuthCopy } from '@/features/auth/lib/model'
import { Button } from '@/shared/ui/Button'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'
import { useI18n } from '@/shared/i18n'

type AuthVisualPanelProps = {
  copy: Accessor<AuthCopy>
  showConsent: Accessor<boolean>
  onDismissConsent: () => void
}

export function AuthVisualPanel(props: AuthVisualPanelProps) {
  const i18n = useI18n()
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
          <VerevonIconButton type="button" class="auth-consent-banner__icon" aria-label={i18n.tr('Informasjonskapsler', 'Cookie settings')}>
            <span />
          </VerevonIconButton>
          <p>{props.copy().cookies}</p>
          <VerevonIconButton type="button" class="auth-consent-banner__prefs" aria-label={i18n.tr('Åpne innstillinger', 'Open preferences')}>
            <span />
            <span />
            <span />
          </VerevonIconButton>
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
