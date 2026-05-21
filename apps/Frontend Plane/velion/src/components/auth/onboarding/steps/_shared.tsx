'use client'

/**
 * Phase 1 onboarding · shared bits used by every step.
 *
 * `LeftPane` / `RightPane` keep the per-step components free of
 * grid-layout boilerplate; they also lock the inter-step typography
 * (Cormorant Garamond on H1, Inter on body) so design drift stays
 * impossible without touching this one file.
 */

import React from 'react'

export function LeftPane({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col justify-center gap-7 px-8 py-12 sm:px-12 md:py-16">
      {children}
    </div>
  )
}

export function RightPane({ children }: { children: React.ReactNode }) {
  return (
    <div className="relative hidden overflow-hidden rounded-r-[24px] bg-[#F4EFE5] md:flex md:items-center md:justify-center">
      {children}
    </div>
  )
}

export function StepEyebrow({ children }: { children: React.ReactNode }) {
  return (
    <span className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">
      {children}
    </span>
  )
}

export function StepTitle({ children }: { children: React.ReactNode }) {
  return (
    <h1
      className="text-[30px] leading-[1.1] text-[#1F1B17] md:text-[34px]"
      style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
    >
      {children}
    </h1>
  )
}

export function StepDescription({ children }: { children: React.ReactNode }) {
  return <p className="font-inter text-[13px] leading-6 text-[#6B6660]">{children}</p>
}

export function PrimaryButton({
  children,
  disabled,
  onClick,
  type = 'button',
}: {
  children: React.ReactNode
  disabled?: boolean
  onClick?: () => void
  type?: 'button' | 'submit'
}) {
  return (
    <button
      type={type}
      disabled={disabled}
      onClick={onClick}
      className="inline-flex items-center justify-center rounded-md bg-[#111111] px-5 py-3 font-inter text-[11px] uppercase tracking-[0.22em] text-white transition-opacity hover:opacity-80 disabled:cursor-not-allowed disabled:opacity-40"
    >
      {children}
    </button>
  )
}

export function SkipLink({
  onClick,
  children = 'Hopp over',
}: {
  onClick: () => void
  children?: React.ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="self-start font-inter text-[11px] uppercase tracking-[0.18em] text-[#A09890] transition-colors hover:text-[#111111]"
    >
      {children}
    </button>
  )
}

/** Inline spinner used while a step is loading the next one. */
export function StepSpinner({ label }: { label?: string }) {
  return (
    <div className="flex items-center gap-3">
      <span
        aria-hidden="true"
        className="block h-4 w-4 animate-spin rounded-full border-2 border-[#D6D2CB] border-t-[#1F1B17]"
      />
      {label && (
        <span className="font-inter text-[12px] text-[#6B6660]">{label}</span>
      )}
    </div>
  )
}
