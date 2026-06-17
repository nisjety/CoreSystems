import { KeyRound, ShieldAlert } from 'lucide-solid'
import {
  SectionHeader,
  SettingsButton,
} from '@/features/settings/components/settings-ui'

export function PasskeySecuritySection() {
  return (
    <section id="security" class="velion-settings-section">
      <SectionHeader
        title="Security"
        description="Manage stronger sign-in methods for your account."
      />

      <div class="velion-settings-list-card">
        <div class="velion-settings-list-row">
          <div>
            <p>Passkeys</p>
            <span>Passkey registration is not available for this workspace yet.</span>
          </div>
          <SettingsButton settingsSize="sm" disabled>
            <KeyRound class="size-4" />
            Unavailable
          </SettingsButton>
        </div>
        <div class="velion-settings-list-row">
          <div>
            <p>Current state</p>
            <span>No passkeys are registered through Velion.</span>
          </div>
          <ShieldAlert class="size-5" aria-hidden="true" />
        </div>
      </div>
    </section>
  )
}
