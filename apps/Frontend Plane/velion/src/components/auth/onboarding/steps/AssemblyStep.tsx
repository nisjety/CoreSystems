'use client'

/**
 * Step 7 — "assembling your dashboard" finale.
 *
 * Left pane: text scroll describing what we're "putting together"
 * (sidebar, knowledge graph, agents, settings). Each item ticks
 * through with a small delay so the moment feels like the system is
 * actually wiring things up.
 *
 * Right pane: 6-second dashboard-assembly video (Slot 7 in the
 * prompts MD). When the asset is missing we render a static mock so
 * the timing is preserved.
 *
 * Auto-redirects to /dashboard once the final tick lands.
 */

import React, { useEffect, useState } from 'react'
import { useRouter } from 'next/navigation'

import type { OnboardingMachine } from '../state/useOnboardingMachine'

import {
  LeftPane,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

const TICKS = [
  'Setter opp arbeidsplassen',
  'Importerer kunnskap fra nettsiden',
  'Kobler til integrasjoner',
  'Trener første agent',
  'Klargjør dashboard',
]

export function AssemblyStep({ machine }: { machine: OnboardingMachine }) {
  const router = useRouter()
  const [completed, setCompleted] = useState(0)

  useEffect(() => {
    const timers: number[] = []
    TICKS.forEach((_, i) => {
      timers.push(
        window.setTimeout(() => setCompleted(i + 1), 700 * (i + 1)),
      )
    })
    // After the last tick + a beat, hop into the dashboard. Reset the
    // wizard so a future visit to /login goes through the regular auth
    // flow (no resume).
    timers.push(
      window.setTimeout(() => {
        machine.reset()
        // eslint-disable-next-line react-doctor/nextjs-no-client-side-redirect
        router.push('/dashboard')
      }, 700 * (TICKS.length + 1) + 400),
    )
    return () => {
      timers.forEach((t) => window.clearTimeout(t))
    }
  }, [machine, router])

  return (
    <>
      <LeftPane>
        <StepEyebrow>Ferdig</StepEyebrow>
        <StepTitle>Setter sammen Velion til deg.</StepTitle>
        <StepDescription>
          Vi flytter inn alt vi har samlet — kunnskap, integrasjoner og
          agenten din — og åpner dashboardet om noen sekunder.
        </StepDescription>
        <ul className="flex flex-col gap-2">
          {TICKS.map((label, i) => {
            const done = i < completed
            const active = i === completed
            return (
              <li
                key={label}
                className="flex items-center gap-3 font-inter text-[13px]"
              >
                <span
                  className={`flex h-5 w-5 items-center justify-center rounded-full border ${
                    done
                      ? 'border-[#1F1B17] bg-[#1F1B17] text-white'
                      : active
                        ? 'border-[#1F1B17] bg-white text-[#1F1B17]'
                        : 'border-[#D6D2CB] bg-white text-transparent'
                  }`}
                >
                  {done ? '✓' : active ? '·' : ''}
                </span>
                <span
                  className={
                    done ? 'text-[#1F1B17]' : 'text-[#A09890]'
                  }
                >
                  {label}
                </span>
              </li>
            )
          })}
        </ul>
      </LeftPane>

      <RightPane>
        <video
          src="/videos/onboarding/dashboard-assembly.webm"
          poster="/imagens/onboarding/dashboard-assembly-mock.png"
          autoPlay
          muted
          playsInline
          loop
          className="h-full w-full object-cover"
        />
      </RightPane>
    </>
  )
}
