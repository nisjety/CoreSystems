'use client'

/**
 * Phase 1 onboarding · two-pane shell.
 *
 * Mirrors the existing AuthPage chrome (cream card, 1.15fr / 0.85fr
 * grid, rounded-24 border) so the post-sign-in transition feels like
 * the same surface — only the contents of the two panes swap. The
 * left pane holds step inputs / copy; the right pane holds the
 * step-specific visual (video, snippet folder, graph, logo wall,
 * plan grid).
 */

import React from 'react'

import { PageFooterLinks } from '../AuthPageParts'
import { useOnboardingCopy } from './i18n'
import type { OnboardingMachine } from './state/useOnboardingMachine'
import type { BrandingSignals } from './state/types'

import { PostSignInStep } from './steps/PostSignInStep'
import { OrganizationStep } from './steps/OrganizationStep'
import { WebsiteStep } from './steps/WebsiteStep'
import { ConnectStep } from './steps/ConnectStep'
import { SocialProofStep } from './steps/SocialProofStep'
import { PaywallStep } from './steps/PaywallStep'
import { AssemblyStep } from './steps/AssemblyStep'
import { OnboardingTopActions } from './steps/_shared'

interface OnboardingFrameProps {
  machine: OnboardingMachine
}

interface OnboardingChromeState {
  isHydrated: boolean
  isPageVisible: boolean
  viewportHeight: number | null
}

type OnboardingChromeAction =
  | { type: 'SET_HYDRATED' }
  | { type: 'SET_PAGE_VISIBLE' }
  | { type: 'SET_VIEWPORT_HEIGHT'; payload: number }

function chromeReducer(
  state: OnboardingChromeState,
  action: OnboardingChromeAction,
): OnboardingChromeState {
  switch (action.type) {
    case 'SET_HYDRATED':
      return { ...state, isHydrated: true }
    case 'SET_PAGE_VISIBLE':
      return { ...state, isPageVisible: true }
    case 'SET_VIEWPORT_HEIGHT':
      return { ...state, viewportHeight: action.payload }
    default:
      return state
  }
}

export function OnboardingFrame({ machine }: OnboardingFrameProps) {
  const { copy } = useOnboardingCopy()
  const cardRef = React.useRef<HTMLDivElement | null>(null)
  const [chrome, dispatch] = React.useReducer(chromeReducer, {
    isHydrated: false,
    isPageVisible: false,
    viewportHeight: null,
  })

  React.useEffect(() => {
    if (typeof window === 'undefined') return

    const updateViewportHeight = () => {
      dispatch({ type: 'SET_VIEWPORT_HEIGHT', payload: window.innerHeight })
    }

    updateViewportHeight()
    dispatch({ type: 'SET_HYDRATED' })

    window.addEventListener('resize', updateViewportHeight)
    return () => {
      window.removeEventListener('resize', updateViewportHeight)
    }
  }, [])

  React.useEffect(() => {
    if (!chrome.isHydrated || chrome.viewportHeight === null) return

    const fadeFrame = window.requestAnimationFrame(() => {
      dispatch({ type: 'SET_PAGE_VISIBLE' })
    })

    return () => window.cancelAnimationFrame(fadeFrame)
  }, [chrome.isHydrated, chrome.viewportHeight])

  const baseCardHeight = 1140
  const cardScale = chrome.viewportHeight
    ? Math.min(1, Math.max(0.52, (chrome.viewportHeight - 18) / baseCardHeight))
    : 1

  if (machine.state.step === 'paywall') {
    return (
      <div
        className={`relative isolate z-40 min-h-[100dvh] w-[100dvw] overflow-hidden transition-opacity duration-800 ease-out ${
          chrome.isPageVisible ? 'opacity-100' : 'opacity-0'
        }`}
      >
        <PaywallStep machine={machine} fullScreen />
      </div>
    )
  }

  return (
    <div
      className={`relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 transition-opacity duration-800 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10 ${
        chrome.isPageVisible ? 'opacity-100' : 'opacity-0'
      }`}
      style={
        {
          '--primary': '#111111',
          '--primary-foreground': '#ffffff',
          '--ring': '#111111',
        } as React.CSSProperties
      }
    >
      <div
        className="relative z-[120] flex w-full max-w-[70.5rem] flex-col items-center gap-3 xl:max-w-[72rem]"
        style={{
          transform: `scale(${cardScale})`,
          transformOrigin: 'center center',
        }}
      >
        <OnboardingTopActions
          machine={machine}
          className="grid w-full grid-cols-[1fr_auto_1fr] items-center gap-4 px-1"
        />
        <BrandStrip
          branding={machine.state.website?.branding}
          fallbackLabel={copy.brand.detected}
        />

        <div
          ref={cardRef}
          className="relative grid w-full overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr]"
        >
          {renderStep(machine)}
        </div>
      </div>

      <PageFooterLinks
        imprint={copy.footer.imprint}
        privacy={copy.footer.privacy}
        copyright={copy.footer.copyright}
        cookieSettings={copy.footer.cookieSettings}
        onCookieSettings={() => undefined}
      />

      <style>{`
        .scanner-dot {
          animation: scannerMove 5.5s ease-in-out infinite alternate;
        }

        @keyframes scannerMove {
          0% {
            top: 0;
          }
          100% {
            top: calc(100% - 20px);
          }
        }
      `}</style>
    </div>
  )
}

/**
 * Compact brand pill rendered above the wizard card. Appears the
 * moment the live crawl preview's `branding` event delivers signals
 * for the user's seed page and stays put through subsequent steps so
 * the user keeps seeing their site reflected in the chrome. Renders
 * nothing when no branding has been captured yet — keeps the layout
 * spacing stable via `min-h-0`.
 */
function BrandStrip({
  branding,
  fallbackLabel,
}: {
  branding: BrandingSignals | undefined
  fallbackLabel: string
}) {
  const present = Boolean(
    branding &&
      (branding.siteName ||
        branding.favicon ||
        branding.themeColor ||
        branding.logoCandidate ||
        (branding.palette && branding.palette.length > 0)),
  )

  if (!present || !branding) {
    return <div aria-hidden className="min-h-0" />
  }

  const hostFallback = (() => {
    try {
      return branding.url ? new URL(branding.url).host : undefined
    } catch {
      return undefined
    }
  })()
  const label = branding.siteName || hostFallback || fallbackLabel
  const accent = branding.themeColor && safeColor(branding.themeColor)
  const swatches = (branding.palette ?? [])
    .filter(safeColor)
    .slice(0, 4)

  return (
    <div
      className="flex items-center gap-2.5 rounded-full border border-[#D6D2CB] bg-white/95 px-3 py-1.5 shadow-[0_8px_18px_rgba(31,27,23,0.10)] backdrop-blur"
      style={accent ? { borderColor: accent } : undefined}
      role="status"
      aria-label={`Detected brand: ${label}`}
    >
      {branding.favicon && (
        <img
          src={branding.favicon}
          alt=""
          width={16}
          height={16}
          referrerPolicy="no-referrer"
          onError={(e) => {
            ;(e.currentTarget as HTMLImageElement).style.display = 'none'
          }}
          className="h-4 w-4 rounded-sm object-contain"
        />
      )}
      <span
        className="font-inter text-[11px] uppercase tracking-[0.14em] text-[#1F1B17]"
        style={{ fontFamily: brandFontStack(branding.fontFamily) }}
      >
        {label}
      </span>
      {accent && (
        <span
          aria-hidden
          className="ml-0.5 inline-block h-3 w-3 rounded-full border border-black/10"
          style={{ backgroundColor: accent }}
        />
      )}
      {swatches.length > 0 && (
        <span aria-hidden className="flex items-center gap-1">
          {swatches.map((hex) => (
            <span
              key={hex}
              className="inline-block h-2 w-2 rounded-full border border-black/10"
              style={{ backgroundColor: hex }}
            />
          ))}
        </span>
      )}
    </div>
  )
}

/**
 * Defensive CSS color validation. We only allow `#rgb`, `#rrggbb`,
 * `#rrggbbaa` and a handful of safe named tokens — Quarry returns
 * arbitrary strings extracted from inline CSS and we render them
 * directly into a `style={{backgroundColor}}` attribute.
 */
function safeColor(input: string | undefined): string | undefined {
  if (!input) return undefined
  const trimmed = input.trim().toLowerCase()
  if (/^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/.test(trimmed)) {
    return trimmed
  }
  return undefined
}

function brandFontStack(family: string | undefined): string | undefined {
  if (!family) return undefined
  // The detected family is sanitized at extract time but we still
  // restrict to alphanumerics + spaces / dashes here as defense-in-
  // depth so a malicious payload can't inject CSS via font-family.
  if (!/^[A-Za-z0-9 _-]{1,48}$/.test(family)) return undefined
  return `"${family}", var(--font-inter), system-ui, sans-serif`
}

function renderStep(machine: OnboardingMachine) {
  switch (machine.state.step) {
    case 'post-signin':
      return <PostSignInStep machine={machine} />
    case 'organization':
      return <OrganizationStep machine={machine} />
    case 'website':
      return <WebsiteStep machine={machine} />
    case 'connect':
      return <ConnectStep machine={machine} />
    case 'social-proof':
      return <SocialProofStep machine={machine} />
    case 'paywall':
      return <PaywallStep machine={machine} />
    case 'assembly':
      return <AssemblyStep machine={machine} />
    default:
      return <PostSignInStep machine={machine} />
  }
}
