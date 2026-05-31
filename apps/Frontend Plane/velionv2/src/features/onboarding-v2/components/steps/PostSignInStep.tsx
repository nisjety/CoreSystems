"use client";

/**
 * Step 1 — post-sign-in. Brief greeting + spinner on the left; a product
 * reveal video (with poster fallback) on the right. Auto-advances to the
 * organization step after the video ends OR after 3s, whichever comes first.
 */

import { useEffect, useRef } from "react";

import { useOnboardingCopy } from "../../lib/onboarding-i18n";
import type { OnboardingMachine } from "../../lib/onboarding-machine";
import {
  LeftPane,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepSpinner,
  StepTitle,
} from "../onboarding-shared";

const AUTO_ADVANCE_MS = 3_000;

export function PostSignInStep({ machine }: { machine: OnboardingMachine }) {
  const { copy } = useOnboardingCopy();
  const advancedRef = useRef(false);

  const advance = () => {
    if (advancedRef.current) return;
    advancedRef.current = true;
    machine.markIntroPlayed();
    machine.goTo("organization");
  };

  useEffect(() => {
    const timer = window.setTimeout(advance, AUTO_ADVANCE_MS);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [machine]);

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.postSignIn.eyebrow}</StepEyebrow>
        <StepTitle>{copy.postSignIn.title}</StepTitle>
        <StepDescription>{copy.postSignIn.description}</StepDescription>
        <StepSpinner label={copy.postSignIn.spinner} />
      </LeftPane>

      <RightPane>
        <video
          src="/videos/onboarding/product-reveal.webm"
          poster="/imagens/onboarding/product-reveal-poster.png"
          autoPlay
          muted
          playsInline
          onEnded={advance}
          className="size-full object-cover"
        />
        <div className="pointer-events-none absolute inset-x-6 bottom-6 rounded-xl border border-white/30 bg-white/80 px-5 py-4 backdrop-blur-md">
          <p className="font-inter text-[12px] font-semibold tracking-tight text-[#1F1B17]">
            {copy.postSignIn.overlayTitle}
          </p>
          <p className="mt-1 font-inter text-[11px] text-[#6B6660]">{copy.postSignIn.overlayStats}</p>
        </div>
      </RightPane>
    </>
  );
}
