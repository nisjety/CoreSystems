'use client'

import { useCallback, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { authService } from '@/components/auth/services/auth-service'
import type { IntegrationProviderSummary } from '@/lib/integrations/types'

type CreateConnectionResponse = {
  connection?: {
    id: string
    provider: string
    integration_key: string
    selected_sources: string[]
  } | null
  sync_jobs?: Array<{
    id: string
    source_type: string
    sync_name: string
    status: string
  }>
  authorization_url?: string
  connect_link?: string
  session_id?: string
  mode?: string
  provider?: string
  expires_at?: string
  error?: string
}

export function useIntegrationConnect(orgId?: string) {
  const queryClient = useQueryClient()

  const [pendingProvider, setPendingProvider] = useState<string | null>(null)
  const [statusMessage, setStatusMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const refreshIntegrationState = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ['knowledge', 'integrations'] })
    await queryClient.refetchQueries({ queryKey: ['knowledge', 'integrations'] })
  }, [queryClient])

  const connectProvider = useCallback(async (provider: IntegrationProviderSummary) => {
    if (!orgId) {
      setError('No active organization found for integrations.')
      return
    }

    setPendingProvider(provider.key)
    setError(null)
    setStatusMessage(`Connecting ${provider.label} to Verevon...`)

    try {
      const user = await authService.getCurrentUser()

      if (!user?.id || !user.email) {
        throw new Error('User session is unavailable. Reload the page and try again.')
      }

      const response = await fetch('/api/connections/create', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({
          org_id: orgId,
          user_id: user.id,
          user_email: user.email,
          provider: provider.key,
          sources: provider.defaultSources.length > 0
            ? provider.defaultSources
            : provider.supportedSources,
        }),
      })

      const payload = (await response.json()) as CreateConnectionResponse

      if (!response.ok) {
        throw new Error(payload.error || `Could not connect ${provider.label}.`)
      }

      const authorizationUrl = payload.authorization_url || payload.connect_link
      if (authorizationUrl) {
        const popup = window.open(
          authorizationUrl,
          'verevon-integration-oauth',
          'popup=yes,width=600,height=760,noopener,noreferrer',
        )

        if (!popup || popup.closed || typeof popup.closed === 'undefined') {
          setPendingProvider(null)
          setStatusMessage(null)
          setError('Popup blocked. Allow popups for this site and try again.')
          return
        }

        setStatusMessage(`${provider.label} authorization opened. Complete the sign-in flow in the popup.`)

        let checks = 0
        const popupTimer = window.setInterval(() => {
          checks += 1

          if (!popup.closed && checks < 120) {
            return
          }

          window.clearInterval(popupTimer)
          setPendingProvider(null)
          setStatusMessage(`${provider.label} authorization closed. Refreshing integrations...`)
          void refreshIntegrationState().then(() => {
            setStatusMessage(`${provider.label} authorization checked. Connected services appear below when Nango confirms the webhook.`)
          })
        }, 1_000)
        return
      }

      setPendingProvider(null)
      const syncCount = Array.isArray(payload.sync_jobs) ? payload.sync_jobs.length : 0
      setStatusMessage(
        syncCount > 0
          ? `${provider.label} connected. Verevon queued ${syncCount} sync job${syncCount === 1 ? '' : 's'} for this workspace.`
          : `${provider.label} connected.`,
      )
      await refreshIntegrationState()
    } catch (connectError) {
      setPendingProvider(null)
      setStatusMessage(null)
      setError(
        connectError instanceof Error
          ? connectError.message
          : `Could not connect ${provider.label}.`,
      )
    }
  }, [orgId, refreshIntegrationState])

  return {
    connectProvider,
    error,
    pendingProvider,
    setError,
    statusMessage,
  }
}
