'use client'

import { useState, useEffect } from 'react'
import { useRouter, useSearchParams } from 'next/navigation'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'
import { CheckSquare, Square, ChevronRight, ArrowRight, Zap, FileText, MessageSquare, Mail, Loader, Zap as ZapIcon } from 'lucide-react'

interface Source {
  key: 'sharePoint' | 'oneDrive' | 'teams' | 'outlook'
  label: string
  description: string
  icon: React.ReactNode
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

export function ConnectStep() {
  const router = useRouter()
  const searchParams = useSearchParams()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [processingCallback, setProcessingCallback] = useState(false)
  
  // New state for auth-core session detection
  const [hasMicrosoftToken, setHasMicrosoftToken] = useState<boolean | null>(null)
  const [authenticatedEmail, setAuthenticatedEmail] = useState<string | null>(null)
  const [checkingSession, setCheckingSession] = useState(true)
  
  const [selected, setSelected] = useState<Record<string, boolean>>({
    sharePoint: true,
    oneDrive: true,
    teams: false,
    outlook: false,
  })

  // Check if user already has Microsoft token from auth-core session (on mount)
  useEffect(() => {
    const checkAuthCoreSession = async () => {
      try {
        setCheckingSession(true)
        
        const response = await fetch('/api/oauth/session-check', {
          method: 'GET',
          headers: {
            'Content-Type': 'application/json',
          },
          credentials: 'include', // Include cookies from auth-core
        })

        if (response.ok) {
          const data = await response.json()
          if (data.authenticated && data.has_microsoft) {
            setHasMicrosoftToken(true)
            setAuthenticatedEmail(data.user_email || null)
          } else {
            setHasMicrosoftToken(false)
          }
        } else {
          setHasMicrosoftToken(false)
        }
      } catch (err) {
        console.error('Failed to check auth-core session:', err)
        setHasMicrosoftToken(false)
      } finally {
        setCheckingSession(false)
      }
    }

    checkAuthCoreSession()
  }, [])

  // Handle OAuth callback (for users without auth-core Microsoft token)
  useEffect(() => {
    const code = searchParams.get('code')
    const state = searchParams.get('state')

    if (code && state && !processingCallback) {
      handleOAuthCallback(code, state)
    }
  }, [searchParams])

  const handleOAuthCallback = async (code: string, state: string) => {
    setProcessingCallback(true)
    setError(null)

    try {
      // Get org_id and user_id from session/state storage
      const state_data = sessionStorage.getItem('oauth_state')
      if (!state_data) {
        throw new Error('OAuth state not found. Please start the connection process again.')
      }

      const { orgId, userId, selectedSources } = JSON.parse(state_data)

      // Call backend to exchange code for token
      const response = await fetch('/api/oauth/callback', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ code, org_id: orgId, user_id: userId }),
        credentials: 'include',
      })

      if (!response.ok) {
        const data = await response.json()
        throw new Error(data.detail || 'OAuth callback failed')
      }

      // Token saved! Now create connections
      const createResponse = await fetch('/api/connections/create', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          org_id: orgId,
          user_id: userId,
          sources: selectedSources,
        }),
        credentials: 'include',
      })

      if (!createResponse.ok) {
        const data = await createResponse.json()
        console.warn('Connection creation had warnings:', data)
        // Don't fail on warning; proceed to next step
      }

      // Clean up and proceed
      sessionStorage.removeItem('oauth_state')
      await onboardingService.setupConnections({
        sharePoint: selectedSources.includes('sharepoint'),
        oneDrive: selectedSources.includes('onedrive'),
        teams: selectedSources.includes('teams'),
        outlook: selectedSources.includes('outlook'),
        skipped: false,
      })
      router.replace('/onboarding/team')
    } catch (err: any) {
      setError(err.message || 'OAuth callback failed. Please try again.')
      setProcessingCallback(false)

      // Clear invalid state
      sessionStorage.removeItem('oauth_state')
    }
  }

  const toggleSource = (key: string) => {
    setSelected((prev) => ({ ...prev, [key]: !prev[key] }))
  }

  const anySelected = Object.values(selected).some(Boolean)

  // Handle "Gi tilgang" (OAuth flow) - for users without auth-core token
  const handleConnect = async () => {
    setError(null)
    setLoading(true)

    try {
      // Get current org_id and user_id from onboarding state
      const state = onboardingService.getOnboardingState()
      const orgId = state.orgId || 'unknown_org'
      const userId = state.userId || `user_${Date.now()}`

      // Map selected sources to snake_case for backend
      const selectedSources = (Object.keys(selected) as Array<keyof Source>)
        .filter((key) => selected[key])
        .map((key) => {
          if (key === 'sharePoint') return 'sharepoint'
          if (key === 'oneDrive') return 'onedrive'
          if (key === 'teams') return 'teams'
          if (key === 'outlook') return 'outlook'
          return key
        })

      // Store state for callback
      sessionStorage.setItem(
        'oauth_state',
        JSON.stringify({
          orgId,
          userId,
          selectedSources,
        })
      )

      // Initiate OAuth flow via backend
      const response = await fetch('/api/oauth/initiate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ org_id: orgId, user_id: userId }),
        credentials: 'include',
      })

      if (!response.ok) {
        throw new Error('Failed to initiate OAuth flow')
      }

      const data = await response.json()
      if (!data.url) {
        throw new Error('No OAuth URL returned')
      }

      // Redirect to Microsoft OAuth consent screen
      window.location.href = data.url
    } catch (err: any) {
      setError(err.message || 'Noe gikk galt. Prøv igjen.')
      setLoading(false)
    }
  }

  // Handle "Aktiver" (quick connect for users with auth-core token)
  const handleQuickConnect = async () => {
    setError(null)
    setLoading(true)

    try {
      const state = onboardingService.getOnboardingState()
      const orgId = state.orgId || 'unknown_org'
      const userId = state.userId || `user_${Date.now()}`

      const selectedSources = (Object.keys(selected) as Array<keyof Source>)
        .filter((key) => selected[key])
        .map((key) => {
          if (key === 'sharePoint') return 'sharepoint'
          if (key === 'oneDrive') return 'onedrive'
          if (key === 'teams') return 'teams'
          if (key === 'outlook') return 'outlook'
          return key
        })

      // Call API to create connections using auth-core token
      const response = await fetch('/api/connections/from-auth-core', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          org_id: orgId,
          user_id: userId,
          sources: selectedSources,
        }),
        credentials: 'include',
      })

      if (!response.ok) {
        const data = await response.json()
        throw new Error(data.error || 'Failed to create connections')
      }

      // Success! Proceed to next step
      await onboardingService.setupConnections({
        sharePoint: selectedSources.includes('sharepoint'),
        oneDrive: selectedSources.includes('onedrive'),
        teams: selectedSources.includes('teams'),
        outlook: selectedSources.includes('outlook'),
        skipped: false,
      })
      router.replace('/onboarding/team')
    } catch (err: any) {
      setError(err.message || 'Failed to activate connections. Try again.')
      setLoading(false)
    }
  }

  // Show loading during callback processing
  if (processingCallback) {
    return (
      <div className="space-y-5 text-center">
        <Loader className="mx-auto animate-spin text-[#111111]" />
        <p className="font-inter text-sm text-[#4A4A48]">
          Behandler Microsoft-godkjenning...
        </p>
      </div>
    )
  }

  // Show loading while checking session
  if (checkingSession) {
    return (
      <div className="space-y-5 text-center">
        <Loader className="mx-auto animate-spin text-[#111111]" />
        <p className="font-inter text-sm text-[#4A4A48]">
          Sjekker sesjon...
        </p>
      </div>
    )
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
      {/* Microsoft badge */}
      <div className="flex items-center gap-2">
        <div className="flex h-7 w-7 shrink-0 items-center justify-center bg-[#0078D4]">
          {/* M365 icon — simplified grid */}
          <svg viewBox="0 0 16 16" className="h-4 w-4 fill-white">
            <rect x="1" y="1" width="6" height="6" rx="0.5" />
            <rect x="9" y="1" width="6" height="6" rx="0.5" opacity="0.7" />
            <rect x="1" y="9" width="6" height="6" rx="0.5" opacity="0.7" />
            <rect x="9" y="9" width="6" height="6" rx="0.5" opacity="0.5" />
          </svg>
        </div>
        <span className="font-inter text-sm font-medium text-[#2B2B2B]">Microsoft 365</span>
      </div>

      {/* Quick connect badge (if user has auth-core Microsoft token) */}
      {hasMicrosoftToken && (
        <div className="flex items-center gap-2 rounded-md border border-[#4CAF50] bg-[#F0F8F4] px-3.5 py-2.5">
          <ZapIcon size={14} className="text-[#4CAF50]" />
          <p className="font-inter text-xs text-[#2B5E3B]">
            {authenticatedEmail ? (
              <>Du er koblet til som <strong>{authenticatedEmail}</strong></>
            ) : (
              <>Du er allerede autentisert med Microsoft</>
            )}
          </p>
        </div>
      )}

      {/* Source list */}
      <div className="space-y-2">
        {MICROSOFT_SOURCES.map((source) => {
          const isChecked = !!selected[source.key]
          return (
            <button
              key={source.key}
              type="button"
              onClick={() => toggleSource(source.key)}
              className={`flex w-full items-start gap-3 border px-4 py-3.5 text-left transition-colors ${
                isChecked
                  ? 'border-[#2B2B2B] bg-[#EAE6DF]'
                  : 'border-[#D8D2C6] bg-[#F4F1EB] hover:border-[#4A4A48]'
              }`}
            >
              {/* Checkbox */}
              <div className="mt-0.5 shrink-0 text-[#4A4A48]">
                {isChecked ? (
                  <CheckSquare size={16} className="text-[#111111]" />
                ) : (
                  <Square size={16} className="text-[#D8D2C6]" />
                )}
              </div>

              {/* Source icon */}
              <div
                className={`mt-0.5 shrink-0 transition-colors ${
                  isChecked ? 'text-[#111111]' : 'text-[#A09890]'
                }`}
              >
                {source.icon}
              </div>

              {/* Labels */}
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

      {error && (
        <p className="font-inter text-xs text-[#FF2E63]">{error}</p>
      )}

      {/* Actions */}
      <div className="flex items-center gap-3 pt-1">
        <button
          type="button"
          onClick={hasMicrosoftToken ? handleQuickConnect : handleConnect}
          disabled={loading || !anySelected}
          className="flex items-center gap-2 bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
        >
          {hasMicrosoftToken ? (
            <>
              Aktiver
              <ChevronRight size={14} />
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
          onClick={handleSkip}
          disabled={loading}
          className="font-inter text-sm text-[#A09890] transition-colors hover:text-[#4A4A48] disabled:opacity-50"
        >
          Hopp over
        </button>
      </div>

      {/* Privacy note */}
      <p className="font-inter text-[11px] leading-relaxed text-[#C8C1B3]">
        Vi leser kun innhold for å bygge kunnskapsbasen din. Ingenting lagres utenfor din organisasjon.
      </p>
    </div>
  )
}
