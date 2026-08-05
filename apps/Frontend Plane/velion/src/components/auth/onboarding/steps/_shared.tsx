'use client'

/**
 * Phase 1 onboarding · shared bits used by every step.
 *
 * These primitives intentionally mirror AuthPage. The step files own
 * product copy and interaction; this file owns the shared shell,
 * typography, scanner strip, language row, progress rhythm and CTA
 * shapes so the auth → onboarding handoff stays visually continuous.
 */

import Link from 'next/link'
import React, { useEffect, useRef, useState } from 'react'
import { ArrowLeft, Fingerprint, Lock, ShieldCheck } from 'lucide-react'

import { LanguageSwitcher } from '../../ui/LanguageSwitcher'
import {
  formatOnboardingText,
  onboardingStepLabel,
  TOP_ONBOARDING_STEP_ITEMS,
  useOnboardingCopy,
} from '../i18n'
import type { OnboardingMachine } from '../state/useOnboardingMachine'
import { ONBOARDING_STEPS, type OnboardingStep } from '../state/types'

function toCamelCaseText(value: string) {
  return value
    .toLowerCase()
    .replace(/(^|\s)\S/g, (character) => character.toUpperCase())
}

export function LeftPane({
  children,
  machine,
}: {
  children: React.ReactNode
  machine: OnboardingMachine
}) {
  const { copy } = useOnboardingCopy()
  const contentRef = useRef<HTMLDivElement | null>(null)
  const [contentHeight, setContentHeight] = useState<number | null>(null)

  useEffect(() => {
    const node = contentRef.current
    if (!node) return

    const measureContent = () => {
      setContentHeight(node.scrollHeight)
    }

    measureContent()
    const frame = window.requestAnimationFrame(measureContent)
    const resizeObserver =
      typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(measureContent)
        : null
    resizeObserver?.observe(node)

    return () => {
      window.cancelAnimationFrame(frame)
      resizeObserver?.disconnect()
    }
  }, [children, machine.state.step])

  return (
    <div className="flex items-center justify-center rounded-l-[24px] bg-white px-5 py-6 sm:px-7 sm:py-7 md:px-8 md:py-8 lg:px-10 lg:py-9 xl:px-16 xl:py-10">
      <div className="w-full max-w-[21rem] sm:max-w-[22rem] lg:max-w-[22.75rem] xl:max-w-[25rem]">
        <div
          className="relative overflow-hidden transition-[height] duration-[520ms] ease-in-out"
          style={{
            height: contentHeight ? `${contentHeight}px` : undefined,
          }}
        >
          <div ref={contentRef} className="flex transform-gpu flex-col gap-5 xl:gap-6">
            {children}
          </div>
        </div>

        <div className="mt-2 hidden text-left sm:block xl:mt-3">
          <p className="font-inter text-xs tracking-[0.02em] text-[#6A655F]">
            {copy.shared.supportPrefix}{' '}
            <a
              href="mailto:support@verevon.com"
              className="text-[#3E3A35] transition-colors hover:text-[#1C1C1C]"
            >
              support@verevon.com
            </a>
          </p>
        </div>
      </div>
    </div>
  )
}

export function OnboardingTopActions({
  machine,
  fullScreen = false,
  className,
}: {
  machine: OnboardingMachine
  fullScreen?: boolean
  className?: string
}) {
  const { locale, copy } = useOnboardingCopy()
  const stepIndex = ONBOARDING_STEPS.indexOf(machine.state.step)
  const canGoBack = stepIndex > 0
  const stepItemIndex = TOP_ONBOARDING_STEP_ITEMS.findIndex(
    (step) => step === machine.state.step,
  )
  const stepNumber =
    stepItemIndex >= 0 ? stepItemIndex + 1 : TOP_ONBOARDING_STEP_ITEMS.length
  const currentLabel = onboardingStepLabel(machine.state.step, locale)
  const previousStep =
    stepIndex > 0 ? ONBOARDING_STEPS[stepIndex - 1] : undefined
  const backTooltip = previousStep
    ? formatOnboardingText(copy.shared.backTo, {
        step: onboardingStepLabel(previousStep, locale),
      })
    : copy.shared.backHome
  const stepOfText = formatOnboardingText(copy.shared.stepOf, {
    current: stepNumber,
    total: TOP_ONBOARDING_STEP_ITEMS.length,
  })

  return (
    <div
      className={
        className ??
        (fullScreen
          ? 'mb-8 grid grid-cols-[1fr_auto_1fr] items-center gap-4'
          : 'mb-4 grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-3 xl:mb-6')
      }
    >
      <div className="justify-self-start">
        <ActionTooltip label={backTooltip} align="start">
          {canGoBack ? (
            <button
              type="button"
              onClick={machine.back}
              aria-label={backTooltip}
              title={backTooltip}
              className={`inline-flex w-fit items-center gap-2 rounded-[10px] border border-[#E7E5E4] bg-white/80 font-inter font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#191716]/30 ${
                fullScreen
                  ? 'h-10 px-4 text-[13px]'
                  : 'h-8 px-2.5 text-[12px]'
              }`}
            >
              <ArrowLeft className="h-4 w-4" strokeWidth={2} />
              {copy.shared.back}
            </button>
          ) : (
            <Link
              href="/"
              aria-label={backTooltip}
              title={backTooltip}
              className={`inline-flex w-fit items-center gap-2 rounded-[10px] border border-[#E7E5E4] bg-white/80 font-inter font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#191716]/30 ${
                fullScreen
                  ? 'h-10 px-4 text-[13px]'
                  : 'h-8 px-2.5 text-[12px]'
              }`}
            >
              <ArrowLeft className="h-4 w-4" strokeWidth={2} />
              {copy.shared.back}
            </Link>
          )}
        </ActionTooltip>
      </div>

      <OnboardingStepDots
        currentStep={machine.state.step}
        onStepSelect={(step) => machine.goTo(step)}
      />

      <div className="flex items-center justify-self-end">
        <ActionTooltip label={copy.shared.switchLanguage} align="end">
          <span className="inline-flex">
            <LanguageSwitcher
              variant="dropdown"
              size="sm"
              className="opacity-90 hover:opacity-100"
            />
          </span>
        </ActionTooltip>
        <ActionTooltip
          label={formatOnboardingText(copy.shared.currentStep, {
            step: currentLabel,
          })}
          align="end"
        >
          <span
            className={`ml-2 hidden rounded-full bg-[#F0EFED] font-inter font-semibold uppercase tracking-[0.12em] text-[#777169] sm:inline-flex ${
              fullScreen
                ? 'px-3 py-1.5 text-[12px]'
                : 'px-2.5 py-1 text-[10px]'
            }`}
            title={`${stepOfText}: ${currentLabel}`}
          >
            {stepOfText}
          </span>
        </ActionTooltip>
      </div>
    </div>
  )
}

function OnboardingStepDots({
  currentStep,
  onStepSelect,
}: {
  currentStep: OnboardingStep
  onStepSelect: (step: OnboardingStep) => void
}) {
  const { locale, copy } = useOnboardingCopy()
  return (
    <nav
      className="flex items-center justify-center gap-2"
      aria-label={locale === 'nb' ? 'Onboarding-steg' : 'Onboarding steps'}
    >
      {TOP_ONBOARDING_STEP_ITEMS.map((step, index) => {
        const active = step === currentStep
        const label = onboardingStepLabel(step, locale)
        const tooltip = `${index + 1}. ${label}`
        return (
          <ActionTooltip key={step} label={tooltip}>
            <button
              type="button"
              onClick={() => onStepSelect(step)}
              aria-label={formatOnboardingText(copy.shared.goToStep, {
                step: label,
              })}
              aria-current={active ? 'step' : undefined}
              title={tooltip}
              className={`h-2 rounded-full transition-all hover:bg-[#777169] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#191716]/35 ${
                active ? 'w-6 bg-[#191716]' : 'w-2 bg-[#C9C9C9]'
              }`}
            />
          </ActionTooltip>
        )
      })}
    </nav>
  )
}

function ActionTooltip({
  label,
  align = 'center',
  children,
}: {
  label: string
  align?: 'start' | 'center' | 'end'
  children: React.ReactNode
}) {
  const alignment =
    align === 'start'
      ? 'left-0'
      : align === 'end'
        ? 'right-0'
        : 'left-1/2 -translate-x-1/2'

  return (
    <span className="group relative inline-flex">
      {children}
      <span
        role="tooltip"
        aria-hidden="true"
        className={`invisible pointer-events-none absolute top-[calc(100%+8px)] z-50 whitespace-nowrap rounded-md border border-[#292524]/10 bg-[#191716] px-2 py-1 font-inter text-[10px] font-semibold text-white opacity-0 shadow-[0_10px_24px_rgba(12,10,9,0.14)] transition-opacity duration-150 group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100 ${alignment}`}
      >
        {label}
      </span>
    </span>
  )
}

export function RightPane({
  children,
  showIcons = true,
}: {
  children: React.ReactNode
  showIcons?: boolean
}) {
  return (
    <div className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] md:block lg:min-h-[600px] xl:min-h-[640px]">
      <div className="absolute inset-0 bg-[#F4EFE5]" />
      <div className="relative z-10 h-full min-h-[560px] lg:min-h-[600px] xl:min-h-[640px]">
        {children}
      </div>

      <div className="pointer-events-none absolute bottom-8 left-5 top-8 z-20">
        <div className="absolute inset-y-0 left-0 w-px bg-[#FF2E63]/90" />
        <div className="absolute inset-y-0 -left-[3px] w-[8px] bg-[#FF3B5C]/35 blur-[7px]" />
        <div className="scanner-dot absolute -left-[4px] top-0 h-[20px] w-[9px] rounded-lg bg-gradient-to-b from-[#FF3B5C]/15 via-[#FF3B5C]/40 to-[#FF3B5C]/15 shadow-[0_0_8px_rgba(255,59,92,0.5),0_0_16px_rgba(255,59,92,0.3),0_0_32px_rgba(255,59,92,0.15)]" />
      </div>

      {showIcons && (
        <div className="absolute left-4 top-1/2 z-30 -translate-y-1/2 xl:left-5">
          <div className="flex flex-col items-center gap-10 text-white/90 lg:gap-11 xl:gap-12">
            <ShieldCheck
              strokeWidth={1.4}
              className="h-4 w-4 text-[#10B981] lg:h-4 lg:w-4 xl:h-5 xl:w-5"
            />
            <Lock
              strokeWidth={1.4}
              className="h-4 w-4 lg:h-4 lg:w-4 xl:h-5 xl:w-5"
            />
            <Fingerprint
              strokeWidth={1.4}
              className="h-4 w-4 text-[#FF2E63]/80 lg:h-4 lg:w-4 xl:h-5 xl:w-5"
            />
          </div>
        </div>
      )}
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
  const text = typeof children === 'string' ? toCamelCaseText(children) : children

  return (
    <h1
      className="text-[clamp(42px,4.2vw,64px)] font-normal leading-[1.02] tracking-normal text-[#1C1C1C]"
      style={{ fontFamily: 'var(--font-geist-sans), Arial, sans-serif' }}
    >
      {text}
    </h1>
  )
}

export function StepDescription({ children }: { children: React.ReactNode }) {
  return (
    <p className="mt-3 font-inter text-[15px] leading-[1.6] text-[#66615B] xl:mt-4">
      {children}
    </p>
  )
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
  children,
}: {
  onClick: () => void
  children?: React.ReactNode
}) {
  const { copy } = useOnboardingCopy()

  return (
    <button
      type="button"
      onClick={onClick}
      className="self-start font-inter text-[11px] uppercase tracking-[0.18em] text-[#A09890] transition-colors hover:text-[#111111]"
    >
      {children ?? copy.shared.skip}
    </button>
  )
}

/**
 * Chatbase-style progress indicator: one wide pill for the current
 * step, small pills for completed/upcoming. Used at the top of every
 * input-bearing step so the user always knows where they are in the
 * 6-step flow.
 */
export function ProgressDots({
  current,
  total,
}: {
  current: number
  total: number
}) {
  const { copy } = useOnboardingCopy()
  return (
    <div
      className="flex h-3 items-center gap-1.5"
      aria-label={formatOnboardingText(copy.shared.stepOf, { current, total })}
    >
      {Array.from({ length: total }).map((_, i) => {
        const idx = i + 1
        const completed = idx < current
        const isCurrent = idx === current
        return (
          <span
            key={i}
            aria-hidden="true"
            className={`h-1.5 rounded-full transition-all ${
              isCurrent
                ? 'w-6 bg-[#1F1B17]'
                : completed
                  ? 'w-1.5 bg-[#1F1B17]'
                  : 'w-1.5 bg-[#D6D2CB]'
            }`}
          />
        )
      })}
    </div>
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
