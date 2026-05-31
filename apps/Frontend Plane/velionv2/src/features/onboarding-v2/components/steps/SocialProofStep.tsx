"use client";

/**
 * Step 5 — social proof. A line of copy + trust stats and a "See plans" CTA on
 * the left; a greyscale logo wall on the right.
 */

import { useOnboardingCopy } from "../../lib/onboarding-i18n";
import type { OnboardingMachine } from "../../lib/onboarding-machine";
import {
  LeftPane,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "../onboarding-shared";

const LOGOS = ["Apple", "Microsoft", "Slack", "Notion", "Zammad", "Sanity"];

export function SocialProofStep({ machine }: { machine: OnboardingMachine }) {
  const { copy } = useOnboardingCopy();

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.socialProof.eyebrow}</StepEyebrow>
        <StepTitle>{copy.socialProof.title}</StepTitle>
        <StepDescription>{copy.socialProof.description}</StepDescription>

        <ul className="font-inter text-[13px] text-[#1F1B17]">
          <li className="border-b border-[#E5DFD3] py-3">
            <strong>97 %</strong> {copy.socialProof.statOne}
          </li>
          <li className="border-b border-[#E5DFD3] py-3">
            <strong>42 %</strong> {copy.socialProof.statTwo}
          </li>
          <li className="py-3">
            <strong>SOC 2</strong> {copy.socialProof.statThree}
          </li>
        </ul>

        <PrimaryButton onClick={() => machine.goTo("paywall")}>{copy.socialProof.cta}</PrimaryButton>
      </LeftPane>

      <RightPane>
        <div className="grid size-full grid-cols-3 grid-rows-2 gap-0">
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
  );
}
