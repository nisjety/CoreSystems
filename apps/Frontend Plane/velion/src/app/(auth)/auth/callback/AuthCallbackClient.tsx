'use client'

import { Suspense, useEffect, useState } from 'react'
import { useSearchParams } from 'next/navigation'

import { AuthShellPlaceholder } from '@/components/auth/onboarding/AuthShellPlaceholder'
import { authORPCClient } from '@/components/auth/lib/orpc/client'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'
import type { ServerOnboardingState } from '@/components/onboarding/lib/onboarding-server'
import { emit as emitTelemetry } from '@/lib/telemetry/client'

interface AuthCallbackClientProps {
  /**
   * Pre-resolved auth + onboarding state from the server component.
   * When present, callback completion can route without extra client
   * session-context polling.
   */
  initialState?: ServerOnboardingState
}

async function waitForProfile() {
  let lastError: unknown

  for (let attempt = 0; attempt < 6; attempt += 1) {
    try {
      return await authORPCClient.getProfile()
    } catch (error) {
      lastError = error
      await new Promise((resolve) =>
        window.setTimeout(resolve, 400 * (attempt + 1)),
      )
    }
  }

  throw lastError instanceof Error
    ? lastError
    : new Error('No authenticated user')
}

function normalizeRedirect(value: string | null): string {
  if (!value || value === '/') return '/dashboard'
  try {
    const target = new URL(value, window.location.origin)
    if (target.origin !== window.location.origin) return '/dashboard'
    return `${target.pathname}${target.search}${target.hash}`
  } catch {
    return value.startsWith('/') && !value.startsWith('//')
      ? value
      : '/dashboard'
  }
}

function replaceLocation(destination: string): void {
  window.location.replace(destination)
}

function AuthCallbackClientContent({ initialState }: AuthCallbackClientProps) {
  const searchParams = useSearchParams()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    const routeAfterCallback = async () => {
      const redirectTo = normalizeRedirect(
        searchParams.get('redirectTo') || searchParams.get('redirect'),
      )
      const error = searchParams.get('error')
      const errorDescription = searchParams.get('error_description')

      if (error) {
        const message =
          errorDescription || 'Vi kunne ikke fullføre innloggingen.'
        setErrorMessage(message)
        emitTelemetry('onboarding.zero_input.failed', {
          props: {
            reason: 'oauth_error',
            error,
            error_description: errorDescription ?? null,
          },
        })
        window.setTimeout(() => {
          if (!cancelled) replaceLocation('/login')
        }, 2200)
        return
      }

      try {
        if (
          initialState?.hasSession &&
          initialState.needsOnboarding !== null
        ) {
          let destination = redirectTo

          if (initialState.needsOnboarding) {
            await onboardingService.startOnboarding()
            destination = '/login'
            emitTelemetry('onboarding.zero_input.failed', {
              props: { reason: 'needs_onboarding', resolved_by: 'server' },
            })
          } else {
            emitTelemetry('onboarding.zero_input.resolved', {
              props: { resolved_by: 'server' },
            })
          }

          emitTelemetry('auth.login.completed', {
            props: { destination, fast_path: true },
          })

          if (!cancelled) replaceLocation(destination)
          return
        }

        const userProfile = await waitForProfile()
        if (!userProfile) throw new Error('No user session found')

        emitTelemetry('auth.login.completed', {
          props: { destination: '/login', fast_path: false },
        })

        if (!cancelled) replaceLocation('/login')
      } catch (error) {
        console.error('Callback error:', error)
        setErrorMessage('Vi kunne ikke fullføre innloggingen.')
        window.setTimeout(() => {
          if (!cancelled) replaceLocation('/login')
        }, 2200)
      }
    }

    void routeAfterCallback()

    return () => {
      cancelled = true
    }
  }, [searchParams, initialState])

  return (
    <AuthShellPlaceholder
      tone={errorMessage ? 'error' : 'loading'}
      title={errorMessage ? 'Innlogging feilet' : 'Logger inn'}
      description={
        errorMessage ??
        'Vi fullfører autentiseringen i samme innloggingsflate.'
      }
    />
  )
}

export default function AuthCallbackClient({
  initialState,
}: AuthCallbackClientProps) {
  return (
    <Suspense fallback={<AuthShellPlaceholder title="Logger inn" />}>
      <AuthCallbackClientContent initialState={initialState} />
    </Suspense>
  )
}
