import { Suspense } from 'react'
import { IntegrationsSettingsPage } from '@/components/integrations/IntegrationsSettingsPage'

export default function SettingsIntegrationsPage() {
  return (
    <Suspense fallback={null}>
      <IntegrationsSettingsPage />
    </Suspense>
  )
}
