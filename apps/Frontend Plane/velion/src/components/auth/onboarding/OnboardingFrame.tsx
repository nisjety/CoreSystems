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

import type { OnboardingMachine } from './state/useOnboardingMachine'

import { PostSignInStep } from './steps/PostSignInStep'
import { OrganizationStep } from './steps/OrganizationStep'
import { WebsiteStep } from './steps/WebsiteStep'
import { ConnectStep } from './steps/ConnectStep'
import { SocialProofStep } from './steps/SocialProofStep'
import { PaywallStep } from './steps/PaywallStep'
import { AssemblyStep } from './steps/AssemblyStep'

interface OnboardingFrameProps {
  machine: OnboardingMachine
}

export function OnboardingFrame({ machine }: OnboardingFrameProps) {
  return (
    <div className="relative z-[120] grid w-full max-w-[70.5rem] overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr] xl:max-w-[72rem]">
      {renderStep(machine)}
    </div>
  )
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
