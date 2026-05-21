'use client'

/**
 * Step 5 — social proof.
 *
 * Left pane: a single line of copy ("Companies that ship support with
 * Velion") + a "Continue" CTA. The page footer carries the trust
 * messaging since the dashboard arrival animation comes next.
 *
 * Right pane: 3×2 greyscale logo wall (Slot 5). The logos are inline
 * SVG placeholders today; replace with the real sprite when ready.
 */

import React from 'react'

import type { OnboardingMachine } from '../state/useOnboardingMachine'

import {
  LeftPane,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from './_shared'

const LOGOS = [
  'Apple',
  'Microsoft',
  'Slack',
  'Notion',
  'Zammad',
  'Sanity',
]

export function SocialProofStep({ machine }: { machine: OnboardingMachine }) {
  return (
    <>
      <LeftPane>
        <StepEyebrow>Steg 5 av 6</StepEyebrow>
        <StepTitle>Selskap som bygger med Velion.</StepTitle>
        <StepDescription>
          Vi gir samme infrastruktur som store team — uten oppsettet.
          Datakildene du nettopp koblet til er allerede klare.
        </StepDescription>

        <ul className="font-inter text-[13px] text-[#1F1B17]">
          <li className="border-b border-[#E5DFD3] py-3">
            <strong>97 %</strong> av førsteforespørsler besvares innen 60 s.
          </li>
          <li className="border-b border-[#E5DFD3] py-3">
            <strong>42 %</strong> raskere førstesvar etter første uke.
          </li>
          <li className="py-3">
            <strong>SOC 2</strong> Type II · GDPR · ZDR-modus tilgjengelig.
          </li>
        </ul>

        <PrimaryButton onClick={() => machine.goTo('paywall')}>
          Se planene
        </PrimaryButton>
      </LeftPane>

      <RightPane>
        <div className="grid h-full w-full grid-cols-3 grid-rows-2 gap-0">
          {LOGOS.map((label) => (
            <div
              key={label}
              className="flex items-center justify-center border border-[#E5DFD3] bg-[#F4EFE5] font-inter text-[13px] uppercase tracking-[0.18em] text-[#6B6660] transition-colors hover:text-[#1F1B17]"
            >
              {label}
            </div>
          ))}
        </div>
      </RightPane>
    </>
  )
}
