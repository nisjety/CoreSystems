'use client'

/**
 * Step 1 — post-sign-in.
 *
 * Left pane: a brief greeting + loading spinner. The form fields from
 * the auth page have collapsed; this is the moment between "submit
 * sign-in" and "first interactive step".
 *
 * Right pane: 3–5s product-in-action video loop (Slot 1 in
 * `docs/onboarding-asset-prompts.md`). While the asset is missing we
 * render the current static auth right-pane image as a placeholder so
 * the flow remains testable end-to-end.
 *
 * Auto-advance to `organization` after the video plays through once OR
 * after 3 s, whichever comes first. The user never has to click here.
 */

import React, { useEffect, useRef } from 'react'

import { useOnboardingCopy } from '../i18n'
import type { OnboardingMachine } from '../state/useOnboardingMachine'

import {
  LeftPane,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepSpinner,
  StepTitle,
} from './_shared'

const AUTO_ADVANCE_MS = 3_000

export function PostSignInStep({ machine }: { machine: OnboardingMachine }) {
  const { copy } = useOnboardingCopy()
  const videoRef = useRef<HTMLVideoElement>(null)
  const advancedRef = useRef(false)

  useEffect(() => {
    if (advancedRef.current) return
    const timer = window.setTimeout(() => {
      if (advancedRef.current) return
      advancedRef.current = true
      machine.markIntroPlayed()
      machine.goTo('organization')
    }, AUTO_ADVANCE_MS)
    return () => window.clearTimeout(timer)
  }, [machine])

  // If the real video lands later, call markIntroPlayed + advance the
  // step from `onEnded` so a longer asset doesn't get cut off.
  const handleVideoEnded = () => {
    if (advancedRef.current) return
    advancedRef.current = true
    machine.markIntroPlayed()
    machine.goTo('organization')
  }

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.postSignIn.eyebrow}</StepEyebrow>
        <StepTitle>{copy.postSignIn.title}</StepTitle>
        <StepDescription>{copy.postSignIn.description}</StepDescription>
        <StepSpinner label={copy.postSignIn.spinner} />
      </LeftPane>

      <RightPane>
        {/*
          Asset slot 1 — replace the <img> placeholder with the actual
          webm once it's generated. Prompt lives in
          docs/onboarding-asset-prompts.md.
        */}
        <video
          ref={videoRef}
          src="/videos/onboarding/product-reveal.webm"
          poster="/imagens/onboarding/product-reveal-poster.png"
          autoPlay
          muted
          playsInline
          onEnded={handleVideoEnded}
          className="h-full w-full object-cover"
        />
        {/* Below-the-fold pitch + stats card sits on top of the final video frame. */}
        <div className="pointer-events-none absolute bottom-6 left-6 right-6 rounded-xl border border-white/30 bg-white/80 px-5 py-4 backdrop-blur-md">
          <p className="font-inter text-[12px] font-semibold tracking-tight text-[#1F1B17]">
            {copy.postSignIn.overlayTitle}
          </p>
          <p className="mt-1 font-inter text-[11px] text-[#6B6660]">
            {copy.postSignIn.overlayStats}
          </p>
        </div>
      </RightPane>
    </>
  )
}
