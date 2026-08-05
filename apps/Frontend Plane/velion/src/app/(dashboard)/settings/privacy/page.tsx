import { SettingsPlaceholderPanel } from '@/components/settings/SettingsPlaceholderPanel'
import { SettingsSectionFrame } from '@/components/settings/SettingsSectionFrame'

export default function SettingsPrivacyPage() {
  return (
    <SettingsSectionFrame
      eyebrow="Access & control"
      title="Privacy"
      description="Set guardrails for data visibility, retention, and how connected systems are exposed inside Verevon."
    >
      <SettingsPlaceholderPanel
        title="Privacy controls are reserved"
        copy="This section will hold workspace privacy rules and data handling preferences. The route exists now so the original settings sidebar stays consistent while the controls are wired."
      />
    </SettingsSectionFrame>
  )
}
