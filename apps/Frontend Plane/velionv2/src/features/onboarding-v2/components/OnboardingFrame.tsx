"use client";

/**
 * Onboarding · two-pane shell (ported from verevon v1 OnboardingFrame).
 *
 * Mirrors the auth-page chrome (cream card, 1.15fr / 0.85fr grid, rounded-24
 * border) so the post-sign-in transition feels like the same surface. The
 * paywall step renders full-screen; every other step renders inside the card.
 */

import { useEffect, useReducer } from "react";

import { useOnboardingCopy } from "../lib/onboarding-i18n";
import type { BrandingSignals, OnboardingMachine } from "../lib/onboarding-machine";
import { DEFAULT_ONBOARDING_ACCENT, resolveBrandThemeColor } from "../lib/onboarding-evidence";
import { OnboardingTopActions } from "./onboarding-shared";
import { PostSignInStep } from "./steps/PostSignInStep";
import { OrganizationStep } from "./steps/OrganizationStep";
import { WebsiteStep } from "./steps/WebsiteStep";
import { ConnectStep } from "./steps/ConnectStep";
import { SocialProofStep } from "./steps/SocialProofStep";
import { PaywallStep } from "./steps/PaywallStep";
import { AssemblyStep } from "./steps/AssemblyStep";

interface ChromeState {
  isHydrated: boolean;
  isPageVisible: boolean;
  viewportHeight: number | null;
}

type ChromeAction =
  | { type: "hydrated" }
  | { type: "visible" }
  | { type: "viewport"; value: number };

function chromeReducer(state: ChromeState, action: ChromeAction): ChromeState {
  switch (action.type) {
    case "hydrated":
      return { ...state, isHydrated: true };
    case "visible":
      return { ...state, isPageVisible: true };
    case "viewport":
      return { ...state, viewportHeight: action.value };
  }
}

export function OnboardingFrame({ machine }: { machine: OnboardingMachine }) {
  const { copy } = useOnboardingCopy();
  const [chrome, dispatch] = useReducer(chromeReducer, {
    isHydrated: false,
    isPageVisible: false,
    viewportHeight: null,
  });

  useEffect(() => {
    const update = () => dispatch({ type: "viewport", value: window.innerHeight });
    update();
    dispatch({ type: "hydrated" });
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);

  useEffect(() => {
    if (!chrome.isHydrated || chrome.viewportHeight === null) return;
    const frame = window.requestAnimationFrame(() => dispatch({ type: "visible" }));
    return () => window.cancelAnimationFrame(frame);
  }, [chrome.isHydrated, chrome.viewportHeight]);

  useEffect(() => {
    const theme = machine.state.brandTheme;
    if (theme?.mode !== "brand") return;
    const primaryColor = resolveBrandThemeColor(machine.state.website?.branding);
    if (theme.primaryColor === primaryColor) return;
    machine.setBrandTheme({ ...theme, primaryColor, saveStatus: "idle" });
  }, [machine, machine.state.brandTheme, machine.state.website?.branding]);

  const cardScale = chrome.viewportHeight
    ? Math.min(1, Math.max(0.52, (chrome.viewportHeight - 18) / 1140))
    : 1;
  const onboardingAccent = safeColor(machine.state.brandTheme?.primaryColor) ?? DEFAULT_ONBOARDING_ACCENT;

  if (machine.state.step === "paywall") {
    return (
      <div
        className={`relative isolate z-40 min-h-[100dvh] w-[100dvw] overflow-hidden transition-opacity duration-700 ease-out ${
          chrome.isPageVisible ? "opacity-100" : "opacity-0"
        }`}
        style={
          {
            "--primary": onboardingAccent,
            "--primary-foreground": "#ffffff",
            "--ring": onboardingAccent,
            "--onboarding-accent": onboardingAccent,
          } as React.CSSProperties
        }
      >
        <PaywallStep machine={machine} fullScreen />
        <ScannerStyle />
      </div>
    );
  }

  return (
    <div
      className={`relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 transition-opacity duration-700 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10 ${
        chrome.isPageVisible ? "opacity-100" : "opacity-0"
      }`}
      style={
        {
          "--primary": onboardingAccent,
          "--primary-foreground": "#ffffff",
          "--ring": onboardingAccent,
          "--onboarding-accent": onboardingAccent,
        } as React.CSSProperties
      }
    >
      <div
        className="relative z-[120] flex w-full max-w-[70.5rem] flex-col items-center gap-3 xl:max-w-[72rem]"
        style={{ transform: `scale(${cardScale})`, transformOrigin: "center center" }}
      >
        <OnboardingTopActions machine={machine} />
        <BrandStrip branding={machine.state.website?.branding} fallbackLabel={copy.brand.detected} />

        <div className="relative grid w-full overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr]">
          {renderStep(machine)}
        </div>
      </div>

      <OnboardingFooter />
      <ScannerStyle />
    </div>
  );
}

function renderStep(machine: OnboardingMachine) {
  switch (machine.state.step) {
    case "post-signin":
      return <PostSignInStep machine={machine} />;
    case "organization":
      return <OrganizationStep machine={machine} />;
    case "website":
      return <WebsiteStep machine={machine} />;
    case "connect":
      return <ConnectStep machine={machine} />;
    case "social-proof":
      return <SocialProofStep machine={machine} />;
    case "assembly":
      return <AssemblyStep machine={machine} />;
    default:
      return <PostSignInStep machine={machine} />;
  }
}

function OnboardingFooter() {
  const { copy } = useOnboardingCopy();
  const items = [copy.footer.imprint, copy.footer.privacy, copy.footer.copyright, copy.footer.cookieSettings];
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-3 hidden text-center md:block">
      <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-6 font-inter text-xs tracking-[0.02em] text-[#6A655F]">
        {items.map((item) => (
          <button key={item} type="button" className="transition-colors hover:text-[#1C1C1C]">
            {item}
          </button>
        ))}
      </div>
    </div>
  );
}

const HEX_COLOR = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/;

function safeColor(input: string | undefined): string | undefined {
  if (!input) return undefined;
  const trimmed = input.trim().toLowerCase();
  return HEX_COLOR.test(trimmed) ? trimmed : undefined;
}

function brandFontStack(family: string | undefined): string | undefined {
  if (!family || !/^[A-Za-z0-9 _-]{1,48}$/.test(family)) return undefined;
  return `"${family}", var(--font-inter), system-ui, sans-serif`;
}

function BrandStrip({
  branding,
  fallbackLabel,
}: {
  branding: BrandingSignals | undefined;
  fallbackLabel: string;
}) {
  const present = Boolean(
    branding &&
      (branding.siteName ||
        branding.favicon ||
        branding.themeColor ||
        branding.logoCandidate ||
        (branding.palette && branding.palette.length > 0)),
  );
  if (!present || !branding) return <div aria-hidden className="min-h-0" />;

  const hostFallback = (() => {
    try {
      return branding.url ? new URL(branding.url).host : undefined;
    } catch {
      return undefined;
    }
  })();
  const label = branding.siteName || hostFallback || fallbackLabel;
  const accent = branding.themeColor && safeColor(branding.themeColor);
  const swatches = (branding.palette ?? []).filter((c): c is string => Boolean(safeColor(c))).slice(0, 4);

  return (
    <div
      className="flex items-center gap-2.5 rounded-full border border-[#D6D2CB] bg-white/95 px-3 py-1.5 shadow-[0_8px_18px_rgba(31,27,23,0.10)] backdrop-blur"
      style={accent ? { borderColor: accent } : undefined}
      role="status"
      aria-label={`${fallbackLabel}: ${label}`}
    >
      {branding.favicon && (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={branding.favicon}
          alt=""
          width={16}
          height={16}
          referrerPolicy="no-referrer"
          onError={(e) => {
            (e.currentTarget as HTMLImageElement).style.display = "none";
          }}
          className="size-4 rounded-sm object-contain"
        />
      )}
      <span className="font-inter text-[11px] uppercase tracking-[0.14em] text-[#1F1B17]" style={{ fontFamily: brandFontStack(branding.fontFamily) }}>
        {label}
      </span>
      {accent && <span aria-hidden className="ml-0.5 inline-block size-3 rounded-full border border-black/10" style={{ backgroundColor: accent }} />}
      {swatches.length > 0 && (
        <span aria-hidden className="flex items-center gap-1">
          {swatches.map((hex) => (
            <span key={hex} className="inline-block size-2 rounded-full border border-black/10" style={{ backgroundColor: hex }} />
          ))}
        </span>
      )}
    </div>
  );
}

function ScannerStyle() {
  return (
    <style>{`
      .scanner-dot { animation: scannerMove 5.5s ease-in-out infinite alternate; }
      @keyframes scannerMove { 0% { top: 0; } 100% { top: calc(100% - 20px); } }
      @media (prefers-reduced-motion: reduce) { .scanner-dot { animation: none; } }
    `}</style>
  );
}
