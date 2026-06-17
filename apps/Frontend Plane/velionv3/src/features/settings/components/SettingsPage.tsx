import { useParams } from '@solidjs/router'
import { VelionWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { isWorkspaceSettingsSection } from '@/features/settings/lib/settings-sections'

export default function SettingsPage() {
  const params = useParams<{ section?: string }>()
  const section = () => {
    const value = params.section ?? ''
    return isWorkspaceSettingsSection(value)
      ? value
      : 'workspace'
  }

  return <VelionWorkspaceSettingsPage section={section()} />
}
