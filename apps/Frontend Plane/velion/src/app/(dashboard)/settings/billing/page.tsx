import { SettingsPlaceholderPanel } from '@/components/settings/SettingsPlaceholderPanel'
import { SettingsSectionFrame } from '@/components/settings/SettingsSectionFrame'

export default function SettingsBillingPage() {
  return (
    <SettingsSectionFrame
      title="Billing"
      description="Track plan state, quotas, and billing controls for the workspace."
    >
      <SettingsPlaceholderPanel
        title="Billing controls are moving here"
        copy="Billing is reserved for the first-party Velion workspace settings flow. This panel is ready for plan controls and usage visibility once the billing surface is connected."
      />
    </SettingsSectionFrame>
  )
}
