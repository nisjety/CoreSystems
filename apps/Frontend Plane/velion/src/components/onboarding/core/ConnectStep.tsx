'use client'

import { useEffect, useRef, useState } from 'react'
import { useRouter } from 'next/navigation'
import { CheckSquare, ChevronRight, FileText, Loader, Mail, MessageSquare, Square, Zap } from 'lucide-react'

import { authService } from '@/components/auth/services/auth-service'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'

type SourceKey = 'sharePoint' | 'oneDrive' | 'teams' | 'outlook'

interface Source {
  key: SourceKey
  label: string
  description: string
  icon: React.ReactNode
}

interface ConnectSessionResponse {
  session_id: string
  mode: 'connect_session' | 'reused' | 'already_connected'
  authorization_url?: string | null
  connect_link?: string | null
  requested_sources: string[]
}

interface ConnectStatusResponse {
  status: 'pending' | 'connected' | 'failed' | 'expired' | 'error'
  error_message?: string | null
}

const MICROSOFT_SOURCES: Source[] = [
  {
    key: 'sharePoint',
    label: 'SharePoint',
    description: 'Dokumenter, wikier og teamnettsteder',
    icon: <FileText size={16} strokeWidth={1.5} />,
  },
  {
    key: 'oneDrive',
    label: 'OneDrive',
    description: 'Filer og mapper fra alle brukere',
    icon: <Zap size={16} strokeWidth={1.5} />,
  },
  {
    key: 'teams',
    label: 'Microsoft Teams',
    description: 'Kanaler, meldinger og delte filer',
    icon: <MessageSquare size={16} strokeWidth={1.5} />,
  },
  {
    key: 'outlook',
    label: 'Outlook-dokumenter',
    description: 'Vedlegg og delte ressurser',
    icon: <Mail size={16} strokeWidth={1.5} />,
  },
]

const POLL_INTERVAL_MS = 1500

function mapSelectedSources(selected: Record<SourceKey, boolean>): string[] {
  return (Object.keys(selected) as SourceKey[])
    .filter((key) => selected[key])
    .map((key) => {
      if (key === 'sharePoint') return 'sharepoint'
      if (key === 'oneDrive') return 'onedrive'
      return key
    })
}

function MicrosoftProviderHeader() {
  return (
    <div className="flex items-center gap-2">
      <div className="flex h-7 w-7 shrink-0 items-center justify-center bg-[#0078D4]">
        <svg viewBox="0 0 16 16" className="h-4 w-4 fill-white">
          <rect x="1" y="1" width="6" height="6" rx="0.5" />
          <rect x="9" y="1" width="6" height="6" rx="0.5" opacity="0.7" />
          <rect x="1" y="9" width="6" height="6" rx="0.5" opacity="0.7" />
          <rect x="9" y="9" width="6" height="6" rx="0.5" opacity="0.5" />
        </svg>
      </div>
      <span className="font-inter text-sm font-medium text-[#2B2B2B]">Microsoft 365</span>
    </div>
  )
}

function SourceList({
  selected,
  onToggle,
}: {
  selected: Record<SourceKey, boolean>
  onToggle: (key: SourceKey) => void
}) {
  return (
    <div className="space-y-2">
      {MICROSOFT_SOURCES.map((source) => {
        const isChecked = selected[source.key]
        return (
          <button
            key={source.key}
            data-testid={`onboarding-connect-source-${source.key}`}
            type="button"
            onClick={() => onToggle(source.key)}
            className={`flex w-full items-start gap-3 border px-4 py-3.5 text-left transition-colors ${
              isChecked
                ? 'border-[#2B2B2B] bg-[#EAE6DF]'
                : 'border-[#D8D2C6] bg-[#F4F1EB] hover:border-[#4A4A48]'
            }`}
          >
            <div className="mt-0.5 shrink-0 text-[#4A4A48]">
              {isChecked ? (
                <CheckSquare size={16} className="text-[#111111]" />
              ) : (
                <Square size={16} className="text-[#D8D2C6]" />
              )}
            </div>

            <div
              className={`mt-0.5 shrink-0 transition-colors ${
                isChecked ? 'text-[#111111]' : 'text-[#A09890]'
              }`}
            >
              {source.icon}
            </div>

            <div>
              <p
                className={`font-inter text-sm font-medium transition-colors ${
                  isChecked ? 'text-[#111111]' : 'text-[#4A4A48]'
                }`}
              >
                {source.label}
              </p>
              <p className="mt-0.5 font-inter text-xs text-[#A09890]">{source.description}</p>
            </div>
          </button>
        )
      })}
    </div>
  )
}

function ConnectionActions({
  loading,
  anySelected,
  statusMessage,
  error,
  onConnect,
  onSkip,
  onBack,
}: {
  loading: boolean
  anySelected: boolean
  statusMessage: string | null
  error: string | null
  onConnect: () => void
  onSkip: () => void
  onBack: () => void
}) {
  return (
    <>
      {statusMessage && (
        <div data-testid="onboarding-connect-status" className="flex items-center gap-2 rounded-md border border-[#D8D2C6] bg-[#F4F1EB] px-3.5 py-2.5">
          <Loader size={14} className="animate-spin text-[#111111]" />
          <p className="font-inter text-xs text-[#4A4A48]">{statusMessage}</p>
        </div>
      )}

      {error && <p data-testid="onboarding-connect-error" className="font-inter text-xs text-[#FF2E63]">{error}</p>}

      <div className="flex items-center justify-between">
        <button
          type="button"
          data-testid="onboarding-connect-back"
          onClick={onBack}
          disabled={loading}
          className="border border-[#D8D2C6] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF] disabled:opacity-50"
        >
          Tilbake
        </button>
        <div className="flex items-center gap-3 pt-1">
          <button
            type="button"
            data-testid="onboarding-connect-connect"
            onClick={onConnect}
            disabled={loading || !anySelected}
            className="flex items-center gap-2 bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
          >
            {loading ? (
              <>
                Kobler til
                <Loader size={14} className="animate-spin" />
              </>
            ) : (
              <>
                Gi tilgang
                <ChevronRight size={14} />
              </>
            )}
          </button>
          <button
            type="button"
            data-testid="onboarding-connect-skip"
            onClick={onSkip}
            disabled={loading}
            className="font-inter text-sm text-[#A09890] transition-colors hover:text-[#4A4A48] disabled:opacity-50"
          >
            Hopp over
          </button>
        </div>
      </div>

      <p className="font-inter text-[11px] leading-relaxed text-[#C8C1B3]">
        Better Auth brukes kun til innlogging. Tilgang til Microsoft-data gis separat via Aqencia
        Integrations og lagres utenfor auth-sesjonen.
      </p>
    </>
  )
}

export function ConnectStep() {
  const router = useRouter()
  const popupRef = useRef<Window | null>(null)
  const pollTimerRef = useRef<number | null>(null)
  const popupClosedChecksRef = useRef(0)

  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [statusMessage, setStatusMessage] = useState<string | null>(null)
  const [selected, setSelected] = useState<Record<SourceKey, boolean>>({
    sharePoint: true,
    oneDrive: true,
    teams: false,
    outlook: false,
  })

  useEffect(() => {
    return () => {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current)
      }
      if (popupRef.current && !popupRef.current.closed) {
        popupRef.current.close()
      }
    }
  }, [])

  const toggleSource = (key: SourceKey) => {
    setSelected((prev) => ({ ...prev, [key]: !prev[key] }))
  }

  const anySelected = Object.values(selected).some(Boolean)

  const finalizeConnection = async (requestedSources: string[]) => {
    await onboardingService.setupConnections({
      sharePoint: requestedSources.includes('sharepoint'),
      oneDrive: requestedSources.includes('onedrive'),
      teams: requestedSources.includes('teams'),
      outlook: requestedSources.includes('outlook'),
      skipped: false,
    })
    router.replace('/onboarding/team')
  }

  const cleanupPolling = () => {
    if (pollTimerRef.current !== null) {
      window.clearInterval(pollTimerRef.current)
      pollTimerRef.current = null
    }
  }

  const pollConnectSession = (sessionId: string, requestedSources: string[]) => {
    cleanupPolling()
    popupClosedChecksRef.current = 0
    setStatusMessage('Fullfor koblingen i Microsoft-vinduet. Vi oppdager forbindelsen automatisk.')

    pollTimerRef.current = window.setInterval(async () => {
      try {
        const response = await fetch(`/api/oauth/status/${sessionId}`, {
          method: 'GET',
          credentials: 'include',
        })

        const data = (await response.json()) as ConnectStatusResponse

        if (data.status === 'connected') {
          cleanupPolling()
          if (popupRef.current && !popupRef.current.closed) {
            popupRef.current.close()
          }
          setStatusMessage('Microsoft 365 er koblet til. Starter synkronisering...')
          await finalizeConnection(requestedSources)
          return
        }

        if (data.status === 'failed' || data.status === 'expired' || data.status === 'error') {
          cleanupPolling()
          setLoading(false)
          setStatusMessage(null)
          setError(data.error_message || 'Koblingen ble ikke fullfort. Proev igjen.')
          return
        }

        if (popupRef.current?.closed) {
          popupClosedChecksRef.current += 1
          if (popupClosedChecksRef.current >= 3) {
            cleanupPolling()
            setLoading(false)
            setStatusMessage(null)
            setError('Koblingsvinduet ble lukket for forbindelsen ble opprettet.')
          }
        }
      } catch (pollError) {
        cleanupPolling()
        setLoading(false)
        setStatusMessage(null)
        setError(
          pollError instanceof Error
            ? pollError.message
            : 'Kunne ikke bekrefte integrasjonen. Proev igjen.'
        )
      }
    }, POLL_INTERVAL_MS)
  }

  const handleConnect = async () => {
    setError(null)
    setLoading(true)

    try {
      const onboardingState = onboardingService.getOnboardingState()
      const orgId = onboardingState?.organization?.id
      const user = await authService.getCurrentUser()

      if (!orgId) {
        throw new Error('Organisasjon mangler. Fullfor organisasjonstrinnet foerst.')
      }

      if (!user?.id || !user.email) {
        throw new Error('Brukersesjonen er ikke tilgjengelig. Last siden pa nytt.')
      }

      const requestedSources = mapSelectedSources(selected)
      const response = await fetch('/api/oauth/initiate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({
          org_id: orgId,
          user_id: user.id,
          user_email: user.email,
          provider: 'microsoft',
          sources: requestedSources,
        }),
      })

      const data = (await response.json()) as Partial<ConnectSessionResponse> & {
        error?: string
        detail?: string
      }

      if (!response.ok || !data.session_id || !data.mode) {
        throw new Error(data.detail || data.error || 'Kunne ikke starte Microsoft-koblingen.')
      }

      if (data.mode === 'reused' || data.mode === 'already_connected') {
        setStatusMessage(
          data.mode === 'reused'
            ? 'Microsoft-kontoen fra innloggingen kunne gjenbrukes. Starter synkronisering...'
            : 'Microsoft 365 er allerede koblet til. Oppdaterer kildevalgene...'
        )
        await finalizeConnection(data.requested_sources || requestedSources)
        return
      }

      const authorizationUrl = data.authorization_url ?? data.connect_link

      if (!authorizationUrl) {
        throw new Error('Kunne ikke starte Microsoft-koblingen.')
      }

      const popup = window.open(
        authorizationUrl,
        'aqencia-integrations-connect',
        'popup=yes,width=520,height=760,noopener,noreferrer'
      )

      if (!popup) {
        throw new Error('Nettleseren blokkerte koblingsvinduet. Tillat popup-vinduer og proev igjen.')
      }

      popupRef.current = popup
      pollConnectSession(data.session_id, data.requested_sources || requestedSources)
    } catch (connectError) {
      setLoading(false)
      setStatusMessage(null)
      setError(
        connectError instanceof Error
          ? connectError.message
          : 'Noe gikk galt. Proev igjen.'
      )
    }
  }

  const handleSkip = async () => {
    setLoading(true)
    try {
      await onboardingService.setupConnections({
        sharePoint: false,
        oneDrive: false,
        teams: false,
        outlook: false,
        skipped: true,
      })
      router.push('/onboarding/team')
    } catch {
      router.push('/onboarding/team')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="space-y-5">
      <MicrosoftProviderHeader />

      <SourceList selected={selected} onToggle={toggleSource} />

      <ConnectionActions
        loading={loading}
        anySelected={anySelected}
        statusMessage={statusMessage}
        error={error}
        onConnect={handleConnect}
        onSkip={handleSkip}
        onBack={async () => { await onboardingService.saveCurrentStep('website'); router.back() }}
      />
    </div>
  )
}
