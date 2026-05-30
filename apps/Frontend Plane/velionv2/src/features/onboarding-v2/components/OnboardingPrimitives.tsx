"use client";

import Link from "next/link";
import type { Route } from "next";
import { useEffect, useRef, useState } from "react";
import { ArrowLeft, Check, ChevronDown, Fingerprint, Globe, Lock, ShieldCheck } from "lucide-react";
import {
  formatOnboardingText,
  onboardingCopy,
  topOnboardingSteps,
  type OnboardingStep,
} from "@/features/onboarding-v2/lib/onboarding-copy";
import { cn } from "@/lib/utils";

export function OnboardingTopActions({
  step,
  onBack,
  onStepSelect,
  fullScreen = false,
  className,
}: {
  step: OnboardingStep;
  onBack: () => void;
  onStepSelect: (step: OnboardingStep) => void;
  fullScreen?: boolean;
  className?: string;
}) {
  const stepIndex = topOnboardingSteps.indexOf(step);
  const stepNumber = stepIndex >= 0 ? stepIndex + 1 : topOnboardingSteps.length;
  const [languageOpen, setLanguageOpen] = useState(false);
  const [locale, setLocale] = useState<"EN" | "NO">("EN");
  const stepLabel = onboardingCopy.shared.stepLabels[step];

  return (
    <div
      className={
        className ??
        (fullScreen
          ? "mb-8 grid grid-cols-[1fr_auto_1fr] items-center gap-4"
          : "grid w-full grid-cols-[1fr_auto_1fr] items-center gap-4 px-1")
      }
    >
      <div className="justify-self-start">
        <ActionTooltip label={stepIndex > 0 ? `Back to ${stepLabel}` : onboardingCopy.shared.backHome} align="start">
          {stepIndex > 0 ? (
            <button
              type="button"
              onClick={onBack}
              className={cn(
                "inline-flex w-fit items-center gap-2 rounded-[10px] border border-[#E7E5E4] bg-white/80 font-inter font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#191716]/30",
                fullScreen ? "h-10 px-4 text-[13px]" : "h-8 px-2.5 text-[12px]",
              )}
            >
              <ArrowLeft className="size-4" strokeWidth={2} />
              {onboardingCopy.shared.back}
            </button>
          ) : (
            <Link
              href={"/login" as Route}
              className={cn(
                "inline-flex w-fit items-center gap-2 rounded-[10px] border border-[#E7E5E4] bg-white/80 font-inter font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#191716]/30",
                fullScreen ? "h-10 px-4 text-[13px]" : "h-8 px-2.5 text-[12px]",
              )}
            >
              <ArrowLeft className="size-4" strokeWidth={2} />
              {onboardingCopy.shared.back}
            </Link>
          )}
        </ActionTooltip>
      </div>

      <nav className="flex items-center justify-center gap-2" aria-label="Onboarding steps">
        {topOnboardingSteps.map((item, index) => {
          const active = item === step;
          return (
            <ActionTooltip key={item} label={`${index + 1}. ${onboardingCopy.shared.stepLabels[item]}`}>
              <button
                type="button"
                onClick={() => onStepSelect(item)}
                aria-label={formatOnboardingText(onboardingCopy.shared.goToStep, {
                  step: onboardingCopy.shared.stepLabels[item],
                })}
                aria-current={active ? "step" : undefined}
                className={cn(
                  "h-2 rounded-full transition-all hover:bg-[#777169] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#191716]/35",
                  active ? "w-6 bg-[#191716]" : "w-2 bg-[#C9C9C9]",
                )}
              />
            </ActionTooltip>
          );
        })}
      </nav>

      <div className="flex items-center justify-self-end">
        <ActionTooltip label={onboardingCopy.shared.switchLanguage} align="end">
          <span className="inline-flex">
            <OnboardingLanguageSwitcher
              locale={locale}
              open={languageOpen}
              onOpenChange={setLanguageOpen}
              onChoose={(nextLocale) => {
                setLocale(nextLocale);
                setLanguageOpen(false);
              }}
            />
          </span>
        </ActionTooltip>
        <ActionTooltip label={`You are on ${stepLabel}`} align="end">
          <span
            className={cn(
              "ml-2 hidden rounded-full bg-[#F0EFED] font-inter font-semibold uppercase tracking-[0.12em] text-[#777169] sm:inline-flex",
              fullScreen ? "px-3 py-1.5 text-[12px]" : "px-2.5 py-1 text-[10px]",
            )}
            title={`${formatOnboardingText(onboardingCopy.shared.stepOf, {
              current: stepNumber,
              total: topOnboardingSteps.length,
            })}: ${stepLabel}`}
          >
          {formatOnboardingText(onboardingCopy.shared.stepOf, {
            current: stepNumber,
            total: topOnboardingSteps.length,
          })}
          </span>
        </ActionTooltip>
      </div>
    </div>
  );
}

export function LeftPane({ children }: { children: React.ReactNode }) {
  const contentRef = useRef<HTMLDivElement | null>(null);
  const [contentHeight, setContentHeight] = useState<number | undefined>(undefined);

  useEffect(() => {
    const node = contentRef.current;
    if (!node) {
      return;
    }

    const measureContent = () => {
      const nextHeight = node.scrollHeight;
      setContentHeight((currentHeight) => currentHeight === nextHeight ? currentHeight : nextHeight);
    };
    const frame = window.requestAnimationFrame(measureContent);
    const observer =
      typeof ResizeObserver !== "undefined" ? new ResizeObserver(measureContent) : null;
    observer?.observe(node);
    const mutationObserver =
      typeof MutationObserver !== "undefined" ? new MutationObserver(measureContent) : null;
    mutationObserver?.observe(node, { childList: true, subtree: true, characterData: true });

    return () => {
      window.cancelAnimationFrame(frame);
      observer?.disconnect();
      mutationObserver?.disconnect();
    };
  }, []);

  return (
    <div className="flex items-center justify-center rounded-[24px] bg-white px-5 py-6 sm:p-7 md:rounded-l-[24px] md:rounded-r-none md:p-8 lg:px-10 lg:py-9 xl:px-16 xl:py-10">
      <div className="w-full max-w-[21rem] sm:max-w-[22rem] lg:max-w-[22.75rem] xl:max-w-[25rem]">
        <div
          className="relative overflow-hidden transition-[height] duration-[520ms] ease-in-out"
          style={{ height: contentHeight ? `${contentHeight}px` : undefined }}
        >
          <div ref={contentRef} className="flex transform-gpu flex-col gap-5 xl:gap-6">
            {children}
          </div>
        </div>
        <div className="mt-2 hidden text-left sm:block xl:mt-3">
          <p className="font-inter text-xs tracking-[0.02em] text-[#6A655F]">
            {onboardingCopy.shared.supportPrefix}{" "}
            <a href="mailto:support@velion.com" className="text-[#3E3A35] transition-colors hover:text-[#1C1C1C]">
              support@velion.com
            </a>
          </p>
        </div>
      </div>
    </div>
  );
}

export function RightPane({
  children,
  showIcons = true,
}: {
  children: React.ReactNode;
  showIcons?: boolean;
}) {
  return (
    <div className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] md:block lg:min-h-[600px] xl:min-h-[640px]">
      <div className="absolute inset-0 bg-[#F4EFE5]" />
      <div className="relative z-10 h-full min-h-[560px] lg:min-h-[600px] xl:min-h-[640px]">{children}</div>
      <div className="pointer-events-none absolute bottom-8 left-5 top-8 z-20">
        <div className="absolute inset-y-0 left-0 w-px bg-[#FF2E63]/90" />
        <div className="absolute inset-y-0 -left-[3px] w-[8px] bg-[#FF3B5C]/35 blur-[7px]" />
        <div className="scanner-dot absolute -left-[4px] top-0 h-[20px] w-[9px] rounded-lg bg-gradient-to-b from-[#FF3B5C]/15 via-[#FF3B5C]/40 to-[#FF3B5C]/15 shadow-[0_0_8px_rgba(255,59,92,0.5),0_0_16px_rgba(255,59,92,0.3),0_0_32px_rgba(255,59,92,0.15)]" />
      </div>
      {showIcons ? (
        <div className="absolute left-4 top-1/2 z-30 -translate-y-1/2 xl:left-5">
          <div className="flex flex-col items-center gap-10 text-white/90 lg:gap-11 xl:gap-12">
            <ShieldCheck strokeWidth={1.4} className="size-4 text-[#10B981] xl:size-5" />
            <Lock strokeWidth={1.4} className="size-4 xl:size-5" />
            <Fingerprint strokeWidth={1.4} className="size-4 text-[#FF2E63]/80 xl:size-5" />
          </div>
        </div>
      ) : null}
    </div>
  );
}

export function StepEyebrow({ children }: { children: React.ReactNode }) {
  return <span className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">{children}</span>;
}

export function StepTitle({ children }: { children: React.ReactNode }) {
  return (
    <h1 className="font-inter text-[clamp(42px,4.4vw,68px)] font-[450] leading-[1.02] tracking-normal text-[#1C1C1C]">
      {children}
    </h1>
  );
}

export function StepDescription({ children }: { children: React.ReactNode }) {
  return <p className="mt-3 font-inter text-[15px] leading-[1.6] text-[#66615B] xl:mt-4">{children}</p>;
}

export function PrimaryButton({
  children,
  disabled,
  onClick,
  type = "button",
}: {
  children: React.ReactNode;
  disabled?: boolean;
  onClick?: () => void;
  type?: "button" | "submit";
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
  );
}

export function SkipLink({
  children,
  onClick,
}: {
  children?: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="self-start font-inter text-[11px] uppercase tracking-[0.18em] text-[#A09890] transition-colors hover:text-[#111111]"
    >
      {children ?? onboardingCopy.shared.skip}
    </button>
  );
}

function ActionTooltip({
  label,
  align = "center",
  children,
}: {
  label: string;
  align?: "start" | "center" | "end";
  children: React.ReactNode;
}) {
  const alignment =
    align === "start" ? "left-0" : align === "end" ? "right-0" : "left-1/2 -translate-x-1/2";

  return (
    <span className="group relative inline-flex">
      {children}
      <span
        role="tooltip"
        aria-hidden="true"
        className={cn(
          "pointer-events-none invisible absolute top-[calc(100%+8px)] z-50 whitespace-nowrap rounded-md border border-[#292524]/10 bg-[#191716] px-2 py-1 font-inter text-[10px] font-semibold text-white opacity-0 shadow-[0_10px_24px_rgba(12,10,9,0.14)] transition-opacity duration-150 group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100",
          alignment,
        )}
      >
        {label}
      </span>
    </span>
  );
}

function OnboardingLanguageSwitcher({
  locale,
  open,
  onOpenChange,
  onChoose,
}: {
  locale: "EN" | "NO";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChoose: (locale: "EN" | "NO") => void;
}) {
  return (
    <div className="relative">
      <button
        type="button"
        aria-label="Select language"
        aria-expanded={open}
        aria-haspopup="true"
        onClick={() => onOpenChange(!open)}
        className="inline-flex items-center gap-2 rounded-lg bg-background/80 p-2 text-sm font-medium text-muted-foreground shadow-sm backdrop-blur-sm transition-colors hover:text-foreground hover:shadow-md focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
      >
        <Globe className="size-4" />
        <ChevronDown className={cn("size-4 transition-transform", open ? "rotate-180" : "rotate-0")} />
      </button>

      {open ? (
        <div className="absolute right-0 top-full z-[300] mt-2 min-w-[140px] rounded-lg border border-border bg-background py-2 shadow-lg">
          {([
            { code: "NO", name: "Norsk", flag: "🇳🇴" },
            { code: "EN", name: "English", flag: "🇺🇸" },
          ] as const).map((language) => {
            const selected = locale === language.code;
            return (
              <button
                key={language.code}
                type="button"
                onClick={() => onChoose(language.code)}
                className={cn(
                  "flex w-full items-center gap-3 px-4 py-3 text-base transition-colors hover:bg-muted",
                  selected ? "text-foreground" : "text-muted-foreground",
                )}
              >
                <span className="text-lg">{language.flag}</span>
                <span className="flex-1 text-left">{language.name}</span>
                {selected ? <Check className="size-4" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
