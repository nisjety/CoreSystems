'use client'

/**
 * Step 2 — organization name.
 *
 * Left pane: a single input ("What's the name of your company?") plus
 * an optional company-size select. Submit advances to `website`.
 *
 * Right pane: 3s personalization animation (Slot 2). The static globe
 * mock stands in until the webm lands; the org-name appears in the
 * caption as soon as the user starts typing so the user feels the
 * personalization happen live.
 */

import React, { useState } from 'react'

import type { OnboardingMachine } from '../state/useOnboardingMachine'
import type { OrganizationPayload } from '../state/types'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

const SIZES: { value: OrganizationPayload['size']; label: string }[] = [
  { value: 'solo', label: '1' },
  { value: 'small', label: '2–10' },
  { value: 'medium', label: '11–50' },
  { value: 'large', label: '51–250' },
  { value: 'enterprise', label: '250+' },
]

export function OrganizationStep({ machine }: { machine: OnboardingMachine }) {
  const initial = machine.state.organization
  const [name, setName] = useState(initial?.name ?? '')
  const [size, setSize] = useState<OrganizationPayload['size']>(initial?.size)

  const submit = (event: React.FormEvent) => {
    event.preventDefault()
    if (!name.trim()) return
    machine.setOrganization({ name: name.trim(), size })
    machine.goTo('website')
  }

  return (
    <>
      <LeftPane>
        <StepEyebrow>Steg 2 av 6</StepEyebrow>
        <StepTitle>Hva heter organisasjonen din?</StepTitle>
        <StepDescription>
          Vi bruker navnet til å sette opp arbeidsplassen. Du kan endre
          alt senere.
        </StepDescription>

        <form onSubmit={submit} className="flex flex-col gap-5">
          <label className="block">
            <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              Organisasjon
            </span>
            <input
              autoFocus
              required
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Aquatiq AS"
              className="mt-2 w-full rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 font-inter text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none"
            />
          </label>

          <fieldset>
            <legend className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              Hvor mange er dere?
            </legend>
            <div className="mt-2 flex flex-wrap gap-2">
              {SIZES.map((option) => {
                const active = size === option.value
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => setSize(option.value)}
                    className={`rounded-full border px-3.5 py-1.5 font-inter text-[12px] transition-colors ${
                      active
                        ? 'border-[#1F1B17] bg-[#1F1B17] text-white'
                        : 'border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890]'
                    }`}
                  >
                    {option.label}
                  </button>
                )
              })}
            </div>
          </fieldset>

          <PrimaryButton type="submit" disabled={!name.trim()}>
            Fortsett
          </PrimaryButton>
        </form>
      </LeftPane>

      <RightPane>
        {/* Slot 2 placeholder. Replace with the webm when ready. */}
        <img
          src="/imagens/onboarding/org-personalization-mock.png"
          alt=""
          className="h-full w-full object-cover"
        />
        <div className="pointer-events-none absolute inset-x-6 bottom-6 rounded-xl bg-white/85 px-5 py-3 backdrop-blur-md">
          <p className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890]">
            Personaliserer Velion
          </p>
          <p className="mt-1 font-inter text-[12px] font-medium text-[#1F1B17]">
            {name
              ? `Henter offentlig info om ${name} …`
              : 'Skriv inn navnet ditt for å starte'}
          </p>
        </div>
      </RightPane>
    </>
  )
}
