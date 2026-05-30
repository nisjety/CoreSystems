import { SettingsPlaceholderPanel } from '@/components/settings/SettingsPlaceholderPanel'
import { SettingsSectionFrame } from '@/components/settings/SettingsSectionFrame'

export default function SettingsMembersPage() {
  return (
    <SettingsSectionFrame
      title="Members"
      description="Manage workspace membership, access boundaries, and team roles from one place."
    >
      <SettingsPlaceholderPanel
        title="Team membership surface"
        copy="This workspace members view is being folded into the unified Velion settings layer. Use the team workspace to manage active members until this panel is fully connected."
        ctaHref="/team"
        ctaLabel="Open team workspace"
      />
    </SettingsSectionFrame>
  )
}
