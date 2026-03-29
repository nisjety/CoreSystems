'use client'

import { useEffect, useSyncExternalStore } from 'react'
import { createPortal } from 'react-dom'
import { useRouter } from 'next/navigation'

interface DashboardTransitionProps {
  /** Where to navigate after the curtain completes. Defaults to '/'. */
  to?: string
}

/**
 * Full-screen white curtain wipe — mirrors the Barba.js / GSAP pattern.
 * Rendered via React Portal into document.body so it escapes the CSS
 * transform stacking context created by the scaled card in OnboardingPage.
 *
 *  Phase 1 (0 → 1.1 s)   — curtain covers screen   (Expo ease-in-out)
 *  Phase 2 (1.1 → 1.5 s)  — text visible, hold
 *  Phase 3 (1.5 → 2.5 s)  — curtain exits right     (Expo ease-in-out)
 *  After 2.6 s             — router.push
 */
export function DashboardTransition({ to = '/' }: DashboardTransitionProps) {
  const router = useRouter()

  // useSyncExternalStore is the React-recommended SSR-safe way to detect
  // client mount without calling setState inside a useEffect.
  const isClient = useSyncExternalStore(
    () => () => {},
    () => true,
    () => false,
  )

  useEffect(() => {
    // Navigate the moment the curtain starts sweeping out (1.5 s) so the
    // dashboard loads underneath while the curtain is still covering the screen.
    // The curtain finishes at 2.5 s — by then the new page is already mounted.
    const timer = setTimeout(() => router.push(to), 1500)
    return () => clearTimeout(timer)
  }, [router, to])

  if (!isClient) return null

  return createPortal(
    <>
      {/* Rendered into document.body — bypasses the scaled card's transform stacking context */}
      <div
        aria-hidden="true"
        style={{
          position: 'fixed',
          inset: 0,
          zIndex: 9999,
          pointerEvents: 'all',
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            height: '100%',
            width: 0,
            backgroundColor: '#ffffff',
            animation:
              'curtainIn 1.1s cubic-bezier(0.77,0,0.18,1) forwards,' +
              'curtainOut 1s cubic-bezier(0.77,0,0.18,1) 1.5s forwards',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            overflow: 'hidden',
          }}
        >
          <p
            style={{
              whiteSpace: 'nowrap',
              fontFamily: 'var(--font-cormorant-garamond), Georgia, serif',
              fontSize: 'clamp(28px, 4vw, 56px)',
              fontWeight: 400,
              letterSpacing: '-0.01em',
              color: '#111111',
              opacity: 0,
              animation: 'textFade 0.5s ease forwards 0.95s',
              userSelect: 'none',
            }}
          >
            Setter opp ditt dashbord
          </p>
        </div>
      </div>

      <style jsx global>{`
        @keyframes curtainIn {
          0%   { width: 0%;   left: 0; }
          100% { width: 100%; left: 0; }
        }
        @keyframes curtainOut {
          0%   { width: 100%; left: 0%;   }
          100% { width: 100%; left: 100%; }
        }
        @keyframes textFade {
          0%   { opacity: 0; transform: translateY(8px); }
          100% { opacity: 1; transform: translateY(0);   }
        }
      `}</style>
    </>,
    document.body,
  )
}
