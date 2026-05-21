'use client'

/**
 * Step 6 — paywall (Intercom full-screen × Chatbase grid + trial card).
 *
 * Left pane: copy explaining what we recommend, with the
 * LLM-generated reason ("based on your sources + size we suggest …").
 *
 * Right pane: 4 plan cards + 1 trial card in the SAME row. The card
 * the LLM picked gets a thin coral border; the trial card is a
 * different shape (no price, "14 days free") so it reads as the
 * "no-commitment" lane.
 *
 * If nothing was crawled / no connectors were picked we skip the LLM
 * call and pre-select the trial card outright.
 */

import React, { useEffect, useState } from 'react'

import type { OnboardingMachine } from '../state/useOnboardingMachine'
import type { PlanRecommendation } from '../state/types'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

interface Plan {
  id: PlanRecommendation['planId']
  name: string
  price: string
  description: string
  features: string[]
}

const PLANS: Plan[] = [
  {
    id: 'hobby',
    name: 'Hobby',
    price: '$32 /m',
    description: 'For sideprosjekter',
    features: ['1.5k credits/mo', 'Integrasjoner', 'AI-modeller'],
  },
  {
    id: 'standard',
    name: 'Standard',
    price: '$120 /m',
    description: 'For voksende team',
    features: ['10k credits/mo', '2× lagring', 'Auto-retrening'],
  },
  {
    id: 'pro',
    name: 'Pro',
    price: '$400 /m',
    description: 'For skala-team',
    features: ['40k credits/mo', 'Avansert analyse', 'AI-forslag'],
  },
  {
    id: 'enterprise',
    name: 'Enterprise',
    price: 'Snakk med oss',
    description: 'For tilpassede behov',
    features: ['SLA', 'Egen CSM', 'SSO + audit'],
  },
  {
    id: 'trial',
    name: '14 dager gratis',
    price: 'Gratis',
    description: 'Test før du forplikter',
    features: ['Alt i Standard', 'Ingen kort', 'Fortsett til Standard etterpå'],
  },
]

export function PaywallStep({ machine }: { machine: OnboardingMachine }) {
  const [recommendation, setRecommendation] = useState<PlanRecommendation | null>(
    machine.state.recommendation ?? null,
  )
  const [loading, setLoading] = useState(false)
  const [selected, setSelected] = useState<Plan['id']>(
    machine.state.recommendation?.planId ?? 'trial',
  )

  // Ask Model Plane for a recommendation. If the user crawled nothing
  // AND connected nothing, skip the round-trip and recommend trial.
  useEffect(() => {
    if (recommendation) return
    const hasSignal =
      Boolean(machine.state.website?.url) ||
      machine.state.connectors.length > 0
    if (!hasSignal) {
      const trial: PlanRecommendation = {
        planId: 'trial',
        reason:
          'Du har ikke koblet til kilder ennå — start med 14 dagers prøveperiode.',
        generatedAt: new Date().toISOString(),
      }
      setRecommendation(trial)
      machine.setRecommendation(trial)
      setSelected('trial')
      return
    }

    let cancelled = false
    setLoading(true)
    fetch('/api/onboarding/recommend-plan', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        organization: machine.state.organization,
        website: machine.state.website,
        connectors: machine.state.connectors,
      }),
    })
      .then((res) => res.json())
      .then((body: { recommendation?: PlanRecommendation }) => {
        if (cancelled) return
        if (body.recommendation) {
          setRecommendation(body.recommendation)
          machine.setRecommendation(body.recommendation)
          setSelected(body.recommendation.planId)
        }
      })
      .catch(() => {
        // Fail open — fall back to the trial card without disturbing the user.
        const fallback: PlanRecommendation = {
          planId: 'trial',
          reason: 'Anbefalingen er midlertidig utilgjengelig — prøv gratis.',
          generatedAt: new Date().toISOString(),
        }
        if (!cancelled) {
          setRecommendation(fallback)
          machine.setRecommendation(fallback)
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
    // We only want this effect to run once per visit to the step.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <>
      <LeftPane>
        <StepEyebrow>Steg 6 av 6</StepEyebrow>
        <StepTitle>Velg plan.</StepTitle>
        <StepDescription>
          {loading
            ? 'Analyserer kildene dine for å foreslå riktig plan …'
            : recommendation?.reason ??
              'Du kan endre plan når som helst fra innstillinger.'}
        </StepDescription>
        <PrimaryButton onClick={() => machine.goTo('assembly')}>
          Velg {nameForId(selected)}
        </PrimaryButton>
      </LeftPane>

      <RightPane>
        <div className="grid h-full w-full grid-cols-1 gap-3 overflow-y-auto p-6">
          {PLANS.map((plan) => {
            const isRecommended = recommendation?.planId === plan.id
            const isSelected = selected === plan.id
            return (
              <button
                key={plan.id}
                type="button"
                onClick={() => setSelected(plan.id)}
                className={`relative rounded-xl border bg-white px-4 py-3 text-left transition-shadow ${
                  isSelected
                    ? 'border-[#1F1B17] shadow-md'
                    : 'border-[#E5DFD3] hover:border-[#A09890]'
                }`}
                style={
                  isRecommended
                    ? {
                        backgroundImage:
                          'linear-gradient(white, white), linear-gradient(120deg, #FF2E63, #6366F1, #34D399)',
                        backgroundOrigin: 'border-box',
                        backgroundClip: 'padding-box, border-box',
                        borderColor: 'transparent',
                      }
                    : undefined
                }
              >
                {isRecommended && (
                  <span className="absolute right-3 top-3 rounded-full bg-[#FF2E63] px-2 py-0.5 font-inter text-[9px] uppercase tracking-[0.16em] text-white">
                    Anbefalt
                  </span>
                )}
                <p className="font-inter text-[13px] font-semibold text-[#1F1B17]">
                  {plan.name}
                </p>
                <p className="mt-0.5 font-inter text-[12px] text-[#6B6660]">
                  {plan.description}
                </p>
                <p className="mt-1.5 font-inter text-[14px] font-semibold tabular-nums text-[#1F1B17]">
                  {plan.price}
                </p>
                <ul className="mt-2 space-y-0.5">
                  {plan.features.map((f) => (
                    <li
                      key={f}
                      className="font-inter text-[11px] text-[#6B6660]"
                    >
                      — {f}
                    </li>
                  ))}
                </ul>
              </button>
            )
          })}
        </div>
      </RightPane>
    </>
  )
}

function nameForId(id: Plan['id']): string {
  return PLANS.find((p) => p.id === id)?.name ?? 'planen'
}
