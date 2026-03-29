'use client'

import { useEffect, useState } from 'react'
import { useRouter, useSearchParams } from 'next/navigation'
import { Loader, AlertCircle } from 'lucide-react'

export default function OAuthCallbackPage() {
  const router = useRouter()
  const searchParams = useSearchParams()
  const [error, setError] = useState<string | null>(null)
  const [processing, setProcessing] = useState(true)

  useEffect(() => {
    const processCallback = async () => {
      try {
        const code = searchParams.get('code')
        const errorParam = searchParams.get('error')
        const errorDescription = searchParams.get('error_description')

        // Check for errors from Microsoft OAuth
        if (errorParam) {
          setError(`Microsoft OAuth error: ${errorDescription || errorParam}`)
          setProcessing(false)
          setTimeout(() => router.push('/onboarding/connect'), 3000)
          return
        }

        if (!code) {
          setError('No authorization code received from Microsoft')
          setProcessing(false)
          setTimeout(() => router.push('/onboarding/connect'), 3000)
          return
        }

        // Callback page will emit event to parent window (ConnectStep)
        // ConnectStep will handle exchanging the code for token
        if (window.opener) {
          window.opener.postMessage(
            {
              type: 'oauth_callback',
              code,
              search: window.location.search,
            },
            window.location.origin
          )
          window.close()
        } else {
          // If not opened by parent window, store in sessionStorage and navigate
          sessionStorage.setItem('oauth_callback', JSON.stringify({ code }))
          // Don't navigate immediately; let parent handle via search params
        }
      } catch (err: any) {
        setError(err.message || 'Failed to process OAuth callback')
        setProcessing(false)
      }
    }

    processCallback()
  }, [searchParams, router])

  if (error) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-[#F7F5F0] p-4">
        <div className="max-w-md space-y-4 text-center">
          <AlertCircle className="mx-auto text-[#FF2E63]" size={48} />
          <h1 className="font-inter text-lg font-medium text-[#111111]">
            Godkjenningsfeil
          </h1>
          <p className="font-inter text-sm text-[#4A4A48]">{error}</p>
          <p className="font-inter text-xs text-[#A09890]">
            Omdirigerer tilbake til tilkoblingssiden...
          </p>
        </div>
      </div>
    )
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-[#F7F5F0]">
      <div className="text-center">
        <Loader className="mx-auto mb-4 animate-spin text-[#111111]" size={48} />
        <p className="font-inter text-sm text-[#4A4A48]">
          {processing ? 'Behandler godkjenning...' : 'Ferdig!'}
        </p>
      </div>
    </div>
  )
}
