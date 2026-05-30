"use client";

import { useEffect, useReducer, useState } from "react";
import Image from "next/image";
import type { Route } from "next";
import { useRouter, useSearchParams } from "next/navigation";
import { Building2, Check, FileText, GitBranch, Globe2, MessageSquare, Sparkles } from "lucide-react";
import {
  formatOnboardingText,
  onboardingCopy,
  onboardingSteps,
  type OnboardingStep,
} from "@/features/onboarding-v2/lib/onboarding-copy";
import {
  LeftPane,
  OnboardingTopActions,
  PrimaryButton,
  RightPane,
  SkipLink,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "@/features/onboarding-v2/components/OnboardingPrimitives";
import { BrregSearch } from "@/features/onboarding-v2/components/BrregSearch";
import {
  createOrganization,
  setOrganizationPlan,
  startCheckout,
  updateProfile,
  completeOnboarding,
  isPaidPlan,
  OnboardingServiceError,
  type OnboardingPlanId,
} from "@/features/onboarding-v2/lib/onboarding-service";
import { sizeFromEmployeeCount, type BrregEnhet } from "@/lib/services/brreg-service";
import { cn } from "@/lib/utils";

const sizes = ["1", "2-10", "11-50", "51-250", "250+"];
const connectors = [
  { id: "slack", label: "Slack", hint: "Channels + threads", category: "chat", icon: MessageSquare },
  { id: "microsoft365", label: "Microsoft 365", hint: "Teams, Outlook, SharePoint, OneDrive", category: "chat", icon: Building2 },
  { id: "notion", label: "Notion", hint: "Pages + databases", category: "docs", icon: FileText },
  { id: "gdrive", label: "Google Drive", hint: "Docs + Sheets", category: "docs", icon: Globe2 },
  { id: "github", label: "GitHub", hint: "README + issues", category: "tools", icon: GitBranch },
] as const;
const connectorCategories = ["chat", "docs", "tools"] as const;
const connectorsByCategory = {
  chat: connectors.filter((item) => item.category === "chat"),
  docs: connectors.filter((item) => item.category === "docs"),
  tools: connectors.filter((item) => item.category === "tools"),
} as const;

type OnboardingPageState = {
  brief: string;
  brregData: BrregEnhet | null;
  connected: string[];
  orgId: string | null;
  orgNumber: string | null;
  organization: string;
  plan: string;
  size: string;
  step: OnboardingStep;
  viewportHeight: number | null;
  website: string;
};

type OnboardingPageAction =
  | { type: "back" }
  | { type: "go-to"; step: OnboardingStep }
  | { type: "next" }
  | { type: "set-brief"; brief: string }
  | { type: "set-brreg-selection"; enhet: BrregEnhet }
  | { type: "set-org-created"; orgId: string }
  | { type: "set-organization"; organization: string }
  | { type: "set-plan"; plan: string }
  | { type: "set-size"; size: string }
  | { type: "set-viewport-height"; viewportHeight: number }
  | { type: "set-website"; website: string }
  | { type: "toggle-connector"; id: string };

function createInitialOnboardingPageState(): OnboardingPageState {
  return {
    step: "post-signin",
    brregData: null,
    orgId: null,
    orgNumber: null,
    organization: "",
    size: "2-10",
    website: "",
    brief: "",
    connected: ["microsoft365"],
    plan: "standard",
    viewportHeight: typeof window === "undefined" ? null : window.innerHeight,
  };
}

function onboardingPageReducer(
  state: OnboardingPageState,
  action: OnboardingPageAction,
): OnboardingPageState {
  switch (action.type) {
    case "back": {
      const stepIndex = onboardingSteps.indexOf(state.step);
      return {
        ...state,
        step: onboardingSteps[Math.max(stepIndex - 1, 0)],
      };
    }
    case "go-to":
      return {
        ...state,
        step: action.step,
      };
    case "next": {
      const stepIndex = onboardingSteps.indexOf(state.step);
      return {
        ...state,
        step: onboardingSteps[Math.min(stepIndex + 1, onboardingSteps.length - 1)],
      };
    }
    case "set-brief":
      return {
        ...state,
        brief: action.brief,
      };
    case "set-organization":
      return {
        ...state,
        organization: action.organization,
      };
    case "set-plan":
      return {
        ...state,
        plan: action.plan,
      };
    case "set-size":
      return {
        ...state,
        size: action.size,
      };
    case "set-viewport-height":
      return {
        ...state,
        viewportHeight: action.viewportHeight,
      };
    case "set-website":
      return {
        ...state,
        website: action.website,
      };
    case "set-brreg-selection":
      return {
        ...state,
        organization: action.enhet.navn,
        orgNumber: action.enhet.organisasjonsnummer,
        brregData: action.enhet,
        size: sizeFromEmployeeCount(action.enhet.antallAnsatte) || state.size,
      };
    case "set-org-created":
      return {
        ...state,
        orgId: action.orgId,
      };
    case "toggle-connector": {
      const connected = state.connected.includes(action.id)
        ? state.connected.filter((item) => item !== action.id)
        : [...state.connected, action.id];
      return {
        ...state,
        connected,
      };
    }
  }
}

export function VelionOnboardingPage() {
  const [state, dispatch] = useReducer(
    onboardingPageReducer,
    undefined,
    createInitialOnboardingPageState,
  );
  const {
    brief,
    brregData,
    connected,
    orgId,
    orgNumber,
    organization,
    plan,
    size,
    step,
    viewportHeight,
    website,
  } = state;

  const searchParams = useSearchParams();
  const goTo = (nextStep: OnboardingStep) => dispatch({ type: "go-to", step: nextStep });
  const next = () => dispatch({ type: "next" });
  const back = () => dispatch({ type: "back" });

  // Jump to assembly step when Stripe checkout completes.
  useEffect(() => {
    if (searchParams.get("checkout") === "success") {
      goTo("assembly");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const updateViewportHeight = () => {
      dispatch({ type: "set-viewport-height", viewportHeight: window.innerHeight });
    };

    window.addEventListener("resize", updateViewportHeight);
    return () => {
      window.removeEventListener("resize", updateViewportHeight);
    };
  }, []);

  const sourceCount = connected.length + (website ? 1 : 0);
  const graphCounts = {
    sources: sourceCount,
    nodes: 18 + sourceCount * 7,
    edges: 26 + sourceCount * 11,
  };
  const cardScale = viewportHeight
    ? Math.min(1, Math.max(0.52, (viewportHeight - 18) / 1140))
    : 1;

  if (step === "paywall") {
    return (
      <div
        className="relative isolate z-40 min-h-[100dvh] w-[100dvw] overflow-hidden transition-opacity duration-800 ease-out"
      >
        <div className="auth-grain relative isolate min-h-[100dvh] overflow-y-auto bg-[#F7F7F6] px-4 py-8">
          <div className="mx-auto max-w-6xl">
          <OnboardingTopActions step={step} onBack={back} onStepSelect={goTo} fullScreen />
          <PaywallStep
            selected={plan}
            orgId={orgId}
            onSelect={(nextPlan) => dispatch({ type: "set-plan", plan: nextPlan })}
            onContinue={() => goTo("assembly")}
          />
          </div>
        </div>
        <VelionBadge />
      </div>
    );
  }

  return (
    <div
      className="auth-grain relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 transition-opacity duration-800 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10"
      style={
        {
          "--primary": "#111111",
          "--primary-foreground": "#ffffff",
          "--ring": "#111111",
        } as React.CSSProperties
      }
    >
      <div
        className="relative z-[120] flex w-full max-w-[70.5rem] flex-col items-center gap-3 xl:max-w-[72rem]"
        style={{
          transform: `scale(${cardScale})`,
          transformOrigin: "center center",
        }}
      >
        <OnboardingTopActions step={step} onBack={back} onStepSelect={goTo} />
        <BrandStrip organization={organization} website={website} />
        <div className="relative grid w-full overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr]">
          {step === "post-signin" ? <PostSignInStep onContinue={() => goTo("organization")} /> : null}
          {step === "organization" ? (
            <OrganizationStep
              name={organization}
              size={size}
              orgId={orgId}
              orgNumber={orgNumber}
              brregData={brregData}
              onNameChange={(value) => dispatch({ type: "set-organization", organization: value })}
              onSizeChange={(value) => dispatch({ type: "set-size", size: value })}
              onBrregSelect={(enhet) => dispatch({ type: "set-brreg-selection", enhet })}
              onOrgCreated={(id) => dispatch({ type: "set-org-created", orgId: id })}
              onContinue={next}
            />
          ) : null}
          {step === "website" ? (
            <WebsiteStep
              website={website}
              brief={brief}
              onWebsiteChange={(value) => dispatch({ type: "set-website", website: value })}
              onBriefChange={(value) => dispatch({ type: "set-brief", brief: value })}
              onContinue={next}
            />
          ) : null}

          {step === "connect" ? (
            <ConnectStep
              connected={connected}
              graphCounts={graphCounts}
              onToggle={(id) => dispatch({ type: "toggle-connector", id })}
              onContinue={next}
            />
          ) : null}
          {step === "social-proof" ? <SocialProofStep onContinue={() => goTo("paywall")} /> : null}
          {step === "assembly" ? <AssemblyStep plan={plan} orgId={orgId} /> : null}
        </div>
      </div>

      <div className="pointer-events-none absolute bottom-3 left-0 right-0 hidden text-center md:block">
        <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-6 font-inter text-xs tracking-[0.02em] text-[#6A655F]">
          {Object.values(onboardingCopy.footer).map((item) => (
            <button key={item} type="button" className="transition-colors hover:text-[#1C1C1C]">
              {item}
            </button>
          ))}
        </div>
      </div>
      <VelionBadge />
    </div>
  );
}

function VelionBadge() {
  return (
    <button
      type="button"
      aria-label="Velion"
      className="absolute bottom-4 left-5 hidden size-11 items-center justify-center rounded-full border border-white/20 bg-[#202020] text-[20px] text-white shadow-[0_10px_26px_rgba(0,0,0,0.22)] md:flex"
    >
      N
    </button>
  );
}

function BrandStrip({
  organization,
  website,
}: {
  organization: string;
  website: string;
}) {
  if (!organization && !website) {
    return <div aria-hidden className="min-h-0" />;
  }

  return (
    <div className="flex items-center gap-2.5 rounded-full border border-[#D6D2CB] bg-white/95 px-3 py-1.5 shadow-[0_8px_18px_rgba(31,27,23,0.10)] backdrop-blur">
      <span className="inline-block size-3 rounded-full border border-black/10 bg-[#FF2E63]" />
      <span className="text-[11px] uppercase tracking-[0.14em] text-[#1F1B17]">
        {organization || website || "Detected brand"}
      </span>
      <span aria-hidden className="flex items-center gap-1">
        {["#111111", "#FF2E63", "#10B981", "#5E6AD2"].map((hex) => (
          <span key={hex} className="inline-block size-2 rounded-full border border-black/10" style={{ backgroundColor: hex }} />
        ))}
      </span>
    </div>
  );
}

function PostSignInStep({ onContinue }: { onContinue: () => void }) {
  const copy = onboardingCopy.postSignIn;
  return (
    <>
      <LeftPane>
        <StepEyebrow>{copy.eyebrow}</StepEyebrow>
        <StepTitle>{copy.title}</StepTitle>
        <StepDescription>{copy.description}</StepDescription>
        <div className="flex items-center gap-3 text-[12px] text-[#6B6660]">
          <span className="block size-4 animate-spin rounded-full border-2 border-[#D6D2CB] border-t-[#1F1B17]" />
          {copy.spinner}
        </div>
        <PrimaryButton onClick={onContinue}>Continue setup</PrimaryButton>
      </LeftPane>
      <RightPane>
        <Image
          src="/imagens/onboarding/product-reveal-poster.png"
          alt=""
          fill
          sizes="42vw"
          priority
          className="object-cover"
        />
        <div className="pointer-events-none absolute inset-x-6 bottom-6 rounded-xl bg-white/85 px-5 py-3 backdrop-blur-md">
          <p className="text-[11px] uppercase tracking-[0.16em] text-[#A09890]">{copy.overlayTitle}</p>
          <p className="mt-1 text-[12px] font-medium text-[#1F1B17]">{copy.overlayStats}</p>
        </div>
      </RightPane>
    </>
  );
}

function OrganizationStep({
  name,
  size,
  orgId,
  orgNumber,
  brregData,
  onNameChange,
  onSizeChange,
  onBrregSelect,
  onOrgCreated,
  onContinue,
}: {
  name: string;
  size: string;
  orgId: string | null;
  orgNumber: string | null;
  brregData: BrregEnhet | null;
  onNameChange: (value: string) => void;
  onSizeChange: (value: string) => void;
  onBrregSelect: (enhet: BrregEnhet) => void;
  onOrgCreated: (orgId: string) => void;
  onContinue: () => void;
}) {
  const copy = onboardingCopy.organization;
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (!name.trim()) return;
    setError(null);

    // Idempotent: if org already created, just advance.
    if (orgId) {
      onContinue();
      return;
    }

    setLoading(true);
    try {
      const result = await createOrganization({
        name: name.trim(),
        plan: "free",
        orgNumber: orgNumber ?? undefined,
        brregData: brregData ?? undefined,
      });
      onOrgCreated(result.id);
      onContinue();
    } catch (err) {
      setError(
        err instanceof OnboardingServiceError ? err.message : "Noe gikk galt. Prøv igjen.",
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <>
      <LeftPane>
        <StepEyebrow>{copy.eyebrow}</StepEyebrow>
        <StepTitle>{copy.title}</StepTitle>
        <StepDescription>{copy.description}</StepDescription>
        <form
          action={() => {
            void handleSubmit();
          }}
          className="flex flex-col gap-5"
        >
          <label className="block">
            <span className="block text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">{copy.label}</span>
            <input
              required
              type="text"
              aria-label={copy.label}
              value={name}
              onChange={(event) => onNameChange(event.target.value)}
              placeholder="Aquatiq AS"
              className="mt-2 w-full rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none"
            />
          </label>
          <BrregSearch
            initialQuery={name}
            onSelect={onBrregSelect}
            onManualEntry={() => {
              /* user has typed name manually — nothing extra needed */
            }}
          />
          <fieldset>
            <legend className="block text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">{copy.sizeLegend}</legend>
            <div className="mt-2 flex flex-wrap gap-2">
              {sizes.map((option) => (
                <button
                  key={option}
                  type="button"
                  onClick={() => onSizeChange(option)}
                  className={cn(
                    "rounded-full border px-3.5 py-1.5 text-[12px] transition-colors",
                    size === option
                      ? "border-[#1F1B17] bg-[#1F1B17] text-white"
                      : "border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890]",
                  )}
                >
                  {option}
                </button>
              ))}
            </div>
          </fieldset>
          {error && (
            <p className="text-[12px] font-medium text-[#9A3412]">{error}</p>
          )}
          <PrimaryButton type="submit" disabled={!name.trim() || loading}>
            {loading ? "Oppretter…" : copy.continue}
          </PrimaryButton>
        </form>
      </LeftPane>
      <RightPane>
        <div className="grid h-full place-items-center bg-[#F4EFE5] p-10">
          <div className="relative size-[360px] rounded-full border border-[#E5DFD3] bg-white/60">
            {Array.from({ length: 18 }).map((_, index) => (
              <span
                key={index}
                className="absolute grid size-9 place-items-center rounded-full border border-[#E5DFD3] bg-white text-[10px] text-[#6B6660] shadow-sm"
                style={{
                  left: `${50 + Math.cos((index / 18) * Math.PI * 2) * (30 + (index % 3) * 8)}%`,
                  top: `${50 + Math.sin((index / 18) * Math.PI * 2) * (30 + (index % 3) * 8)}%`,
                }}
              >
                {index % 3 === 0 ? "ID" : index % 3 === 1 ? "CRM" : "KB"}
              </span>
            ))}
          </div>
        </div>
        <div className="pointer-events-none absolute inset-x-6 bottom-6 rounded-xl bg-white/85 px-5 py-3 backdrop-blur-md">
          <p className="text-[11px] uppercase tracking-[0.16em] text-[#A09890]">{copy.personalizing}</p>
          <p className="mt-1 text-[12px] font-medium text-[#1F1B17]">{name ? `Fetching public info about ${name} …` : copy.enterName}</p>
        </div>
      </RightPane>
    </>
  );
}

function WebsiteStep({
  website,
  brief,
  onWebsiteChange,
  onBriefChange,
  onContinue,
}: {
  website: string;
  brief: string;
  onWebsiteChange: (value: string) => void;
  onBriefChange: (value: string) => void;
  onContinue: () => void;
}) {
  const copy = onboardingCopy.website;

  const handleContinue = () => {
    // Best-effort — never blocks progression.
    void updateProfile({ website: website.trim() || undefined, brief: brief.trim() || undefined });
    onContinue();
  };

  return (
    <>
      <LeftPane>
        <StepEyebrow>{copy.eyebrow}</StepEyebrow>
        <StepTitle>{copy.title}</StepTitle>
        <StepDescription>{copy.description}</StepDescription>
        <form
          action={() => {
            handleContinue();
          }}
          className="flex flex-col gap-5"
        >
          <label className="block">
            <span className="block text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">{copy.urlLabel}</span>
            <div className="mt-2 flex items-stretch overflow-hidden rounded-md border border-[#D6D2CB] bg-white focus-within:border-[#1F1B17]">
              <span className="flex items-center bg-[#F7F4ED] px-3 text-[12px] text-[#6B6660]">https://</span>
              <input
                required
                type="text"
                aria-label={copy.urlLabel}
                inputMode="url"
                value={website.replace(/^https?:\/\//, "")}
                onChange={(event) => onWebsiteChange(event.target.value.replace(/^https?:\/\//, "").replace(/\s+/g, ""))}
                placeholder="aquatiq.com"
                className="flex-1 bg-white px-3 py-2.5 text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:outline-none"
              />
            </div>
          </label>
          <label className="block">
            <span className="block text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">{copy.briefLabel}</span>
            <textarea
              aria-label={copy.briefLabel}
              rows={2}
              value={brief}
              onChange={(event) => onBriefChange(event.target.value)}
              placeholder={copy.briefPlaceholder}
              className="mt-2 w-full resize-none rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 text-[13px] leading-5 text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none"
            />
          </label>
          <div className="flex items-center gap-4">
            <PrimaryButton type="submit" disabled={!website.trim()}>{copy.continue}</PrimaryButton>
            <SkipLink onClick={handleContinue}>{copy.skip}</SkipLink>
          </div>
        </form>
      </LeftPane>
      <RightPane>
        <SnippetDropFolder />
      </RightPane>
    </>
  );
}

function SnippetDropFolder() {
  const cards = ["Pricing", "FAQ", "Returns", "Shipping", "Catalog", "Terms"];
  return (
    <div className="relative flex size-full items-end justify-center px-10 pb-10">
      <div className="pointer-events-none absolute inset-x-0 top-0 h-[58%] overflow-hidden">
        {cards.map((card, index) => (
          <div
            key={card}
            className={cn(
              "velion-snippet-card absolute top-0 w-[10.5rem] rounded-md border border-[#E5DFD3] bg-white p-3 shadow-[0_8px_18px_rgba(31,27,23,0.10)]",
              `velion-snippet-card-${index}`,
            )}
          >
            <p className="text-[10px] uppercase tracking-[0.16em] text-[#A09890]">Page</p>
            <p className="mt-1 text-[12px] font-medium text-[#1F1B17]">{card}</p>
          </div>
        ))}
      </div>
      <div className="w-full max-w-[320px] rounded-[28px] border border-[#D6D2CB] bg-white p-5 shadow-[0_18px_38px_rgba(31,27,23,0.10)]">
        <div className="flex items-center gap-3">
          <span className="grid size-10 place-items-center rounded-xl bg-[#F7F4ED] text-[#1F1B17]">
            <Globe2 className="size-5" />
          </span>
          <div>
            <p className="text-[13px] font-semibold text-[#1F1B17]">{onboardingCopy.website.folderTitle}</p>
            <p className="text-[11px] text-[#6B6660]">6 snippets gathered</p>
          </div>
        </div>
        <div className="mt-4 h-2 overflow-hidden rounded-full bg-[#E5DFD3]">
          <div className="h-full w-[76%] rounded-full bg-[#1F1B17]" />
        </div>
      </div>
    </div>
  );
}

function ConnectStep({
  connected,
  graphCounts,
  onToggle,
  onContinue,
}: {
  connected: string[];
  graphCounts: { sources: number; nodes: number; edges: number };
  onToggle: (id: string) => void;
  onContinue: () => void;
}) {
  const copy = onboardingCopy.connect;
  return (
    <>
      <LeftPane>
        <StepEyebrow>{copy.eyebrow}</StepEyebrow>
        <StepTitle>{copy.title}</StepTitle>
        <StepDescription>{copy.description}</StepDescription>
        <div className="flex flex-col gap-5">
          {connectorCategories.map((category) => (
            <div key={category}>
              <p className="text-[10px] uppercase tracking-[0.16em] text-[#A09890]">{copy.categories[category]}</p>
              <ul className="mt-2 flex flex-col gap-1.5">
                {connectorsByCategory[category].map((item) => {
                  const active = connected.includes(item.id);
                  const Icon = item.icon;
                  return (
                    <li key={item.id}>
                      <button
                        type="button"
                        onClick={() => onToggle(item.id)}
                        className={cn(
                          "flex w-full items-center justify-between rounded-md border px-3.5 py-2.5 text-left transition-colors",
                          active
                            ? "border-[#1F1B17] bg-[#1F1B17] text-white"
                            : "border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890]",
                        )}
                      >
                        <span className="flex items-center gap-3">
                          <Icon className="size-4" />
                          <span className="flex flex-col">
                            <span className="text-[12.5px]">{item.label}</span>
                            <span className={cn("mt-0.5 text-[11px]", active ? "text-white/70" : "text-[#6B6660]")}>{item.hint}</span>
                          </span>
                        </span>
                        <span className={cn("text-[10px] uppercase tracking-[0.16em]", active ? "text-white/80" : "text-[#A09890]")}>
                          {active ? "Connected" : "Add"}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}
        </div>
        <div className="flex items-center justify-between pt-2">
          <div className="flex items-center gap-4">
            <PrimaryButton onClick={onContinue}>{copy.continue}</PrimaryButton>
            <SkipLink onClick={onContinue}>{copy.skip}</SkipLink>
          </div>
          <p className="font-mono text-[10px] uppercase tracking-[0.14em] text-[#A09890]">
            {formatOnboardingText(copy.counts, graphCounts)}
          </p>
        </div>
      </LeftPane>
      <RightPane showIcons={false}>
        <GraphReveal connected={connected} />
      </RightPane>
    </>
  );
}

function GraphReveal({ connected }: { connected: string[] }) {
  const nodes = ["Velion", "Website", ...connected, "Orders", "Policies", "Tickets", "Agent"];
  return (
    <div className="relative size-full overflow-hidden bg-[#F4EFE5]">
      <svg className="absolute inset-0 size-full" viewBox="0 0 520 640" role="img" aria-label="Knowledge graph preview">
        {nodes.slice(1).map((node, index) => {
          const x = 260 + Math.cos((index / (nodes.length - 1)) * Math.PI * 2) * (120 + (index % 2) * 42);
          const y = 320 + Math.sin((index / (nodes.length - 1)) * Math.PI * 2) * (136 + (index % 3) * 28);
          return <line key={node} x1="260" y1="320" x2={x} y2={y} stroke="#D6D2CB" strokeWidth="1.2" />;
        })}
      </svg>
      {nodes.map((node, index) => {
        const center = index === 0;
        const x = center ? 50 : 50 + Math.cos(((index - 1) / Math.max(1, nodes.length - 1)) * Math.PI * 2) * (24 + (index % 2) * 8);
        const y = center ? 50 : 50 + Math.sin(((index - 1) / Math.max(1, nodes.length - 1)) * Math.PI * 2) * (25 + (index % 3) * 5);
        return (
          <span
            key={node}
            className={cn(
              "absolute -translate-x-1/2 -translate-y-1/2 rounded-full border shadow-sm",
              center
                ? "bg-[#1F1B17] px-4 py-2 text-xs text-white"
                : "border-[#E5DFD3] bg-white px-3 py-1.5 text-[11px] text-[#1F1B17]",
            )}
            style={{ left: `${x}%`, top: `${y}%` }}
          >
            {node}
          </span>
        );
      })}
    </div>
  );
}

function SocialProofStep({ onContinue }: { onContinue: () => void }) {
  const copy = onboardingCopy.socialProof;
  return (
    <>
      <LeftPane>
        <StepEyebrow>{copy.eyebrow}</StepEyebrow>
        <StepTitle>{copy.title}</StepTitle>
        <StepDescription>{copy.description}</StepDescription>
        <ul className="text-[13px] text-[#1F1B17]">
          <li className="border-b border-[#E5DFD3] py-3"><strong>97%</strong> {copy.statOne}</li>
          <li className="border-b border-[#E5DFD3] py-3"><strong>42%</strong> {copy.statTwo}</li>
          <li className="py-3"><strong>SOC 2</strong> {copy.statThree}</li>
        </ul>
        <PrimaryButton onClick={onContinue}>{copy.cta}</PrimaryButton>
      </LeftPane>
      <RightPane>
        <div className="grid size-full grid-cols-3 grid-rows-2">
          {["Apple", "Microsoft", "Slack", "Notion", "Zendesk", "Sanity"].map((label) => (
            <div key={label} className="flex items-center justify-center border border-[#E5DFD3] bg-[#F4EFE5] text-[13px] uppercase tracking-[0.18em] text-[#6B6660]">
              {label}
            </div>
          ))}
        </div>
      </RightPane>
    </>
  );
}

function PaywallStep({
  selected,
  orgId,
  onSelect,
  onContinue,
}: {
  selected: string;
  orgId: string | null;
  onSelect: (value: string) => void;
  onContinue: () => void;
}) {
  const copy = onboardingCopy.paywall;
  const [billingCycle, setBillingCycle] = useState<"monthly" | "yearly">("monthly");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recommended = selected || "standard";

  const choosePlan = (planId: string) => {
    onSelect(planId);
  };

  const handleContinue = async (planOverride?: OnboardingPlanId) => {
    if (!orgId) {
      setError("Organisasjon er ikke opprettet ennå. Gå tilbake og prøv igjen.");
      return;
    }
    setError(null);
    setLoading(true);
    const plan = planOverride ?? (selected as OnboardingPlanId);
    try {
      if (isPaidPlan(plan)) {
        const { url } = await startCheckout(orgId, plan, {
          successUrl: `${window.location.origin}/onboarding?checkout=success`,
          cancelUrl: `${window.location.origin}/onboarding?checkout=cancel`,
        });
        if (url) {
          window.location.href = url;
          return; // hard redirect — do not call onContinue
        }
        // No URL returned — fall back to free-plan path.
        await setOrganizationPlan(orgId, plan, { selected_plan_id: plan });
      } else {
        await setOrganizationPlan(orgId, plan, { selected_plan_id: plan });
      }
      onContinue();
    } catch (err) {
      setError(
        err instanceof OnboardingServiceError ? err.message : "Noe gikk galt. Prøv igjen.",
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="relative isolate min-h-[calc(100dvh-140px)] w-full overflow-visible text-[#0C0A09]">
      <div className="mb-8 flex flex-col gap-5 md:flex-row md:items-end md:justify-between">
        <div>
          <StepEyebrow>{copy.eyebrow}</StepEyebrow>
          <h1 className="mt-3 font-inter text-[clamp(40px,5vw,68px)] font-[450] leading-[1.02] tracking-normal text-[#191716]">
            {copy.fullTitle}
          </h1>
          <p className="mt-3 font-inter text-[15px] font-medium text-[#777169]">{copy.fullSubtitle}</p>
        </div>
        <BillingToggle
          value={billingCycle}
          onChange={setBillingCycle}
          monthlyLabel={copy.monthly}
          yearlyLabel={copy.yearly}
          badgeLabel={copy.trialBadge}
        />
      </div>

      <div className="grid grid-cols-1 items-stretch gap-4 sm:grid-cols-2 lg:grid-cols-5">
        {copy.plans.map((item) => {
          const active = selected === item.id;
          const featured = recommended === item.id;
          const badge = "badge" in item ? item.badge : undefined;
          const yearlyPrice = "yearlyPrice" in item ? item.yearlyPrice : undefined;
          return (
            <button
              type="button"
              key={item.id}
              onClick={() => choosePlan(item.id)}
              className="group relative h-full min-h-[392px] cursor-pointer rounded-[24px] p-[2px] text-left"
            >
              {active ? (
                <div className="velion-paywall-active-ring absolute inset-0 rounded-[24px] shadow-[0_10px_24px_rgba(12,10,9,0.10)]" />
              ) : null}
              <div
                className={cn(
                  "relative z-10 flex h-full flex-col rounded-[22px] border bg-white p-5 text-left transition duration-200",
                  active ? "border-transparent shadow-[0_12px_28px_rgba(12,10,9,0.08)]" : "border-[#E7E5E4] hover:border-[#D6D3D1]",
                )}
              >
                <div className="flex min-h-[42px] items-start justify-between gap-2">
                  <h2 className="font-inter text-[20px] font-semibold leading-tight text-[#191716]">{item.name}</h2>
                  <div className="flex flex-wrap justify-end gap-1.5">
                    {featured ? (
                      <span className="inline-flex items-center rounded-full bg-[#F0EFED] px-2.5 py-1 font-inter text-[11px] font-semibold text-[#191716]">
                        Recommended
                      </span>
                    ) : null}
                    {badge ? (
                      <span className="rounded-full bg-[#DDFBEA] px-3 py-1 font-inter text-[12px] font-semibold text-[#1F5135]">
                        {badge}
                      </span>
                    ) : null}
                  </div>
                </div>

                <div className="mt-5 flex min-h-[40px] items-end gap-2">
                  <span className="font-inter text-[32px] font-bold leading-none tracking-normal text-[#191716]">
                    {billingCycle === "yearly" && yearlyPrice ? yearlyPrice : item.price}
                  </span>
                  <span className="pb-1 font-inter text-[16px] font-medium text-[#191716]">{item.cadence}</span>
                </div>

                <p className="mt-5 min-h-[60px] font-inter text-[14px] font-medium leading-5 text-[#777169]">
                  {item.description}
                </p>

                <ul className="mt-5 min-h-[132px] space-y-3">
                  {item.features.map((feature) => (
                    <li key={feature} className="flex items-start gap-3 font-inter text-[14px] font-semibold leading-5 text-[#292524]">
                      <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-[#191716]" aria-hidden="true">
                        <Check className="size-3 text-white" strokeWidth={3} />
                      </span>
                      {feature}
                    </li>
                  ))}
                </ul>

                <span
                  className="mt-auto inline-flex h-10 items-center justify-center rounded-[10px] border border-[#E7E5E4] bg-white px-4 font-inter text-[14px] font-semibold text-[#191716] transition-colors group-hover:border-[#D6D3D1]"
                >
                  {active ? copy.selected : copy.choosePlan}
                </span>
              </div>
            </button>
          );
        })}
      </div>

      <div className="mt-6 rounded-[18px] bg-white/70 p-5 text-center shadow-[inset_0_0_0_1px_rgba(231,229,228,0.8)]">
        <p className="font-inter text-[15px] font-semibold text-[#191716]">
          {formatOnboardingText(copy.recommendedShort, { plan: copy.plans.find((item) => item.id === recommended)?.name ?? "Advanced" })}
        </p>
        <p className="mx-auto mt-2 max-w-[760px] font-inter text-[14px] leading-6 text-[#777169]">
          {copy.summary}
        </p>
      </div>

      {error && (
        <p className="mt-4 text-center text-[13px] font-medium text-[#9A3412]">{error}</p>
      )}
      <div className="mt-7 flex items-center justify-center gap-2">
        <button
          type="button"
          disabled={loading}
          onClick={() => {
            onSelect("trial");
            void handleContinue("trial");
          }}
          className="h-10 rounded-[10px] border border-[#E7E5E4] bg-white px-5 font-inter text-[14px] font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] disabled:cursor-not-allowed disabled:opacity-50"
        >
          {copy.skipToSetup}
        </button>
        <button
          type="button"
          disabled={loading}
          onClick={() => void handleContinue()}
          className="h-10 rounded-[10px] border border-[#E7E5E4] bg-white px-5 font-inter text-[14px] font-semibold text-[#191716] transition-colors hover:border-[#D6D3D1] disabled:cursor-not-allowed disabled:opacity-50"
        >
          {loading ? "Even på bekreftelse…" : copy.continueToSetup}
        </button>
      </div>
    </div>
  );
}

function BillingToggle({
  value,
  onChange,
  monthlyLabel,
  yearlyLabel,
  badgeLabel,
}: {
  value: "monthly" | "yearly";
  onChange: (value: "monthly" | "yearly") => void;
  monthlyLabel: string;
  yearlyLabel: string;
  badgeLabel: string;
}) {
  const yearly = value === "yearly";

  return (
    <div className="flex flex-wrap items-center justify-end gap-2 font-inter text-[14px] font-medium text-[#191716]">
      <button type="button" onClick={() => onChange("monthly")} className={yearly ? "text-[#777169]" : "text-[#191716]"}>
        {monthlyLabel}
      </button>
      <button
        type="button"
        role="switch"
        aria-checked={yearly}
        aria-label="Toggle yearly billing"
        onClick={() => onChange(yearly ? "monthly" : "yearly")}
        className="relative h-6 w-11 shrink-0 rounded-full bg-[#E5E5E7] transition-colors data-[checked=true]:bg-[#D8F8E7]"
        data-checked={yearly}
      >
        <span className={cn("absolute left-1 top-1 size-4 rounded-full bg-white shadow-sm transition-transform", yearly ? "translate-x-5" : "translate-x-0")} />
      </button>
      <button type="button" onClick={() => onChange("yearly")} className={yearly ? "text-[#191716]" : "text-[#777169]"}>
        {yearlyLabel}
      </button>
      <span className="ml-1 rounded-full bg-[#DDFBEA] px-3 py-1 font-inter text-[12px] font-semibold text-[#1F5135]">
        {badgeLabel}
      </span>
    </div>
  );
}

function AssemblyStep({ plan, orgId }: { plan: string; orgId: string | null }) {
  const copy = onboardingCopy.assembly;
  const router = useRouter();
  const [status, setStatus] = useState<"error" | "idle" | "saving">("idle");

  const openDashboard = async () => {
    if (!orgId) {
      setStatus("error");
      return;
    }
    setStatus("saving");
    try {
      await completeOnboarding({ plan, orgId });
      router.push("/dashboard" as Route);
      router.refresh();
    } catch {
      setStatus("error");
    }
  };

  return (
    <>
      <LeftPane>
        <StepEyebrow>{copy.eyebrow}</StepEyebrow>
        <StepTitle>{copy.title}</StepTitle>
        <StepDescription>{copy.description}</StepDescription>
        <ul className="flex flex-col gap-2">
          {copy.ticks.map((label) => (
            <li key={label} className="flex items-center gap-3 text-[13px] text-[#1F1B17]">
              <span className="flex size-5 items-center justify-center rounded-full border border-[#1F1B17] bg-[#1F1B17] text-white">✓</span>
              {label}
            </li>
            ))}
          </ul>
        <button
          type="button"
          onClick={openDashboard}
          disabled={status === "saving"}
          className="inline-flex w-fit items-center justify-center rounded-md bg-[#111111] px-5 py-3 text-[11px] uppercase tracking-[0.22em] text-white transition-opacity hover:opacity-80 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {status === "saving" ? "Saving setup" : "Open dashboard"}
        </button>
        {status === "error" ? (
          <p className="text-[12px] font-medium text-[#9A3412]">
            {!orgId
              ? "Organisasjonen ble ikke opprettet. Gå tilbake og prøv igjen."
              : "We could not save onboarding completion. Try again before opening the dashboard."}
          </p>
        ) : null}
      </LeftPane>
      <RightPane>
        <div className="size-full bg-[#FCFCFD] p-8">
          <div className="h-full rounded-[24px] border border-[#E6E6E8] bg-white p-5 shadow-[0_18px_38px_rgba(20,21,24,0.08)]">
            <div className="flex items-center gap-3 border-b border-[#ECECF1] pb-4">
              <Sparkles className="size-5 text-[#5E6AD2]" />
              <span className="text-sm font-semibold text-[#26282f]">Velion dashboard</span>
            </div>
            <div className="mt-5 grid grid-cols-2 gap-3">
              {["Inbox", "Agent", "Knowledge", "SLA"].map((label) => (
                <div key={label} className="rounded-2xl border border-[#E6E6E8] bg-[#F7F7F8] p-4">
                  <p className="text-xs text-[#7d828a]">{label}</p>
                  <div className="mt-4 h-2 rounded-full bg-[#E2E3E9]">
                    <div className="h-full w-2/3 rounded-full bg-[#111111]" />
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </RightPane>
    </>
  );
}
