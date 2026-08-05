"use client";

/**
 * Step 3 — organization. BRREG-verified lookup (website-inferred first,
 * search/manual fallback)
 * + company-size selection. Creates the org in org-core on submit (idempotent
 * if the machine already holds an id) and advances to connector setup.
 */

import { useEffect, useMemo, useState } from "react";

import { cn } from "@/lib/utils";
import {
  brregService,
  formatBrregAddress,
  type BrregEnhet,
} from "@/lib/services/brreg-service";
import { BrregSearch } from "../BrregSearch";
import { formatOnboardingText, useOnboardingCopy } from "../../lib/onboarding-i18n";
import { saveOnboardingBrandTheme } from "../../lib/onboarding-api";
import {
  allOnboardingWebsites,
  DEFAULT_ONBOARDING_ACCENT,
  displayOrganizationName,
  resolveBrandThemeColor,
  safeHexColor,
  updateCrawlEvidence,
} from "../../lib/onboarding-evidence";
import {
  sizeFromEmployees,
  type BrandingSignals,
  type OnboardingMachine,
  type OnboardingBrandTheme,
  type OrganizationSize,
  type WebsitePayload,
} from "../../lib/onboarding-machine";
import {
  createOrganization,
  organizationExists,
  OnboardingServiceError,
  startWebsiteIngest,
  updateProfile,
} from "../../lib/onboarding-service";
import {
  LeftPane,
  PrimaryButton,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "../onboarding-shared";

const SIZES: { value: OrganizationSize; label: string }[] = [
  { value: "solo", label: "1" },
  { value: "small", label: "2–10" },
  { value: "medium", label: "11–50" },
  { value: "large", label: "51–250" },
  { value: "enterprise", label: "250+" },
];

type SuggestionStatus = "idle" | "loading" | "ready" | "empty" | "error";

export function OrganizationStep({ machine }: { machine: OnboardingMachine }) {
  const { locale, copy, formatNumber } = useOnboardingCopy();
  const initial = machine.state.organization;
  const inferredQuery = useMemo(
    () => inferOrganizationQuery(machine.state.website),
    [machine.state.website],
  );
  const [name, setName] = useState(initial?.name ?? inferredQuery);
  const [orgNumber, setOrgNumber] = useState<string | undefined>(initial?.brregOrgNumber);
  const [employeeCount, setEmployeeCount] = useState<number | undefined>(initial?.employeeCount);
  const [brregData, setBrregData] = useState<BrregEnhet | undefined>();
  const [size, setSize] = useState<OrganizationSize | undefined>(initial?.size);
  const [suggestions, setSuggestions] = useState<BrregEnhet[]>([]);
  const [suggestionStatus, setSuggestionStatus] = useState<SuggestionStatus>(
    inferredQuery ? "loading" : "idle",
  );
  const [suggestionDismissed, setSuggestionDismissed] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [manualEntry, setManualEntry] = useState(false);
  const [themeWarning, setThemeWarning] = useState<string | null>(null);

  useEffect(() => {
    if (!inferredQuery || initial?.id || orgNumber || suggestionDismissed || manualEntry) {
      return undefined;
    }

    const controller = new AbortController();
    const loadingTimer = window.setTimeout(() => setSuggestionStatus("loading"), 0);

    brregService
      .searchByName(inferredQuery, 5, controller.signal)
      .then((results) => {
        const ranked = rankBrregSuggestions(results, inferredQuery);
        setSuggestions(ranked);
        setSuggestionStatus(ranked.length > 0 ? "ready" : "empty");
      })
      .catch((err: unknown) => {
        if (
          controller.signal.aborted ||
          (err instanceof Error && err.name === "AbortError")
        ) {
          return;
        }
        setSuggestions([]);
        setSuggestionStatus("error");
      });

    return () => {
      window.clearTimeout(loadingTimer);
      controller.abort();
    };
  }, [inferredQuery, initial?.id, manualEntry, orgNumber, suggestionDismissed]);

  const handleBrregSelect = (enhet: BrregEnhet) => {
    setName(enhet.navn);
    setOrgNumber(enhet.organisasjonsnummer);
    setEmployeeCount(enhet.antallAnsatte);
    setBrregData(enhet);
    setSuggestionDismissed(true);
    const inferred = sizeFromEmployees(enhet.antallAnsatte);
    if (inferred) setSize(inferred);
  };

  const clearVerifiedSelection = () => {
    setOrgNumber(undefined);
    setEmployeeCount(undefined);
    setBrregData(undefined);
    setSuggestionDismissed(true);
    setManualEntry(false);
  };

  const persistWebsiteForOrg = (orgId: string, orgName: string) => {
    const websites = allOnboardingWebsites(machine.state);
    const website = websites[0];
    void updateProfile({
      name: orgName,
      website: website?.url,
      brief: website?.agentBrief || undefined,
    });
    for (const site of websites) {
      machine.addWebsite({
        ...site,
        crawlEvidence: updateCrawlEvidence(site.crawlEvidence, { seedStatus: "pending" }),
      });
      void startWebsiteIngest({
        orgId,
        url: site.url,
        brief: site.agentBrief || undefined,
      }).then((accepted) => {
        machine.addWebsite({
          ...site,
          crawlEvidence: updateCrawlEvidence(site.crawlEvidence, {
            seedStatus: accepted ? "ready" : "failed",
          }),
        });
      });
    }
  };

  const chooseTheme = async (mode: OnboardingBrandTheme["mode"]) => {
    const primaryColor =
      mode === "brand"
        ? resolveBrandThemeColor(machine.state.website?.branding)
        : DEFAULT_ONBOARDING_ACCENT;
    const selectedAt = new Date().toISOString();
    machine.setBrandTheme({ mode, primaryColor, selectedAt, saveStatus: "saving" });
    setThemeWarning(null);
    const result = await saveOnboardingBrandTheme({ mode, primaryColor }).catch(() => ({ persisted: false }));
    machine.setBrandTheme({
      mode,
      primaryColor,
      selectedAt,
      saveStatus: result.persisted ? "saved" : "failed",
    });
    if (!result.persisted) {
      setThemeWarning(
        locale === "nb"
          ? "Temaet vises her, men kunne ikke lagres akkurat nå. Prøv igjen eller bytt tilbake."
          : "The theme is previewed here, but could not be saved yet. Try again or switch back.",
      );
    }
  };

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed || submitting) return;

    setSubmitting(true);
    setError(null);
    try {
      if (initial?.id) {
        const exists = await organizationExists(initial.id);
        if (exists) {
          machine.setOrganization({ ...initial, name: trimmed, size, brregOrgNumber: orgNumber, employeeCount });
          persistWebsiteForOrg(initial.id, trimmed);
          machine.goTo("connect");
          return;
        }
      }

      const created = await createOrganization({
        name: trimmed,
        plan: "trial",
        orgNumber: orgNumber ?? undefined,
        brregData: brregData ?? undefined,
        branding: machine.state.website?.branding,
      });
      const nextOrg = {
        id: created.id,
        name: created.name || trimmed,
        slug: created.slug,
        size,
        brregOrgNumber: orgNumber,
        employeeCount,
      };
      machine.setOrganization({
        ...nextOrg,
      });
      persistWebsiteForOrg(created.id, nextOrg.name);
      machine.goTo("connect");
    } catch (err) {
      setError(
        err instanceof OnboardingServiceError ? err.message : copy.organization.createError,
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.organization.eyebrow}</StepEyebrow>
        <StepTitle>{copy.organization.title}</StepTitle>
        <StepDescription>{copy.organization.description}</StepDescription>

        <form onSubmit={submit} className="flex flex-col gap-5">
          {!manualEntry && orgNumber ? (
            <VerifiedOrganizationCard
              name={name}
              orgNumber={orgNumber}
              employeeCount={employeeCount}
              brregData={brregData}
              labels={copy.brreg}
              formatNumber={formatNumber}
              onChange={clearVerifiedSelection}
            />
          ) : manualEntry ? (
            <label className="block">
              <span className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
                {copy.organization.label}
              </span>
              <input
                autoFocus
                required
                type="text"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setOrgNumber(undefined);
                  setEmployeeCount(undefined);
                  setBrregData(undefined);
                }}
                placeholder="Aquatiq AS"
                className="mt-2 w-full rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 font-inter text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none"
              />
              <button
                type="button"
                onClick={() => setManualEntry(false)}
                className="mt-2 font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890] hover:text-[#1F1B17]"
              >
                {copy.organization.brregBack}
              </button>
            </label>
          ) : suggestionStatus === "ready" && suggestions[0] ? (
            <SuggestedOrganizationCard
              suggestion={suggestions[0]}
              alternatives={suggestions.slice(1, 3)}
              copy={copy.organization}
              labels={copy.brreg}
              formatNumber={formatNumber}
              onAccept={handleBrregSelect}
              onChange={() => {
                setSuggestionDismissed(true);
                setSuggestions([]);
              }}
            />
          ) : (
            <div className="flex flex-col gap-3">
              {suggestionStatus === "loading" && inferredQuery ? (
                <p className="font-inter text-[12px] leading-5 text-[#6B6660]">
                  {copy.organization.detecting}
                </p>
              ) : suggestionStatus === "empty" && inferredQuery ? (
                <p className="font-inter text-[12px] leading-5 text-[#6B6660]">
                  {copy.organization.noSuggestion}
                </p>
              ) : null}
              <BrregSearch
                key={inferredQuery || "manual"}
                initialQuery={name}
                onSelect={handleBrregSelect}
                onManualEntry={() => {
                  setManualEntry(true);
                  setBrregData(undefined);
                  setOrgNumber(undefined);
                  setEmployeeCount(undefined);
                }}
              />
            </div>
          )}

          <fieldset>
            <legend className="block font-inter text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
              {copy.organization.sizeLegend}
            </legend>
            <div className="mt-2 flex flex-wrap gap-2">
              {SIZES.map((option) => {
                const active = size === option.value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => setSize(option.value)}
                    style={
                      active
                        ? {
                            backgroundColor: "var(--onboarding-accent, #1F1B17)",
                            borderColor: "var(--onboarding-accent, #1F1B17)",
                          }
                        : undefined
                    }
                    className={cn(
                      "rounded-full border px-3.5 py-1.5 font-inter text-[12px] transition-colors",
                      active
                        ? "border-[#1F1B17] bg-[#1F1B17] text-white"
                        : "border-[#D6D2CB] bg-white text-[#1F1B17] hover:border-[#A09890]",
                    )}
                  >
                    {option.label}
                  </button>
                );
              })}
            </div>
            {employeeCount != null && (
              <p className="mt-2 font-inter text-[11px] text-[#6B6660]">
                {formatOnboardingText(copy.organization.employeeHint, {
                  count: formatNumber(employeeCount),
                })}
              </p>
            )}
          </fieldset>

          {error && <p className="font-inter text-[12px] leading-5 text-[#B42318]">{error}</p>}

          <PrimaryButton type="submit" disabled={!name.trim() || submitting}>
            {submitting ? copy.organization.creating : copy.organization.continue}
          </PrimaryButton>
        </form>
      </LeftPane>

      <RightPane>
        <OrganizationEvidencePanel
          name={name}
          website={machine.state.website}
          brregData={brregData}
          employeeCount={employeeCount}
          brandTheme={machine.state.brandTheme}
          themeWarning={themeWarning}
          locale={locale}
          formatNumber={formatNumber}
          onTheme={chooseTheme}
        />
      </RightPane>
    </>
  );
}

function SuggestedOrganizationCard({
  suggestion,
  alternatives,
  copy,
  labels,
  formatNumber,
  onAccept,
  onChange,
}: {
  suggestion: BrregEnhet;
  alternatives: BrregEnhet[];
  copy: {
    inferredFromWebsite: string;
    acceptSuggestion: string;
    changeSuggestion: string;
  };
  labels: { orgNumber: string; employees: string };
  formatNumber: (value: number) => string;
  onAccept: (enhet: BrregEnhet) => void;
  onChange: () => void;
}) {
  return (
    <div className="rounded-md border border-[#D6D2CB] bg-white px-4 py-3 shadow-[0_8px_18px_rgba(31,27,23,0.06)]">
      <p className="font-inter text-[10px] uppercase tracking-[0.16em] text-[#A09890]">
        {copy.inferredFromWebsite}
      </p>
      <OrganizationSummary enhet={suggestion} labels={labels} formatNumber={formatNumber} />
      {alternatives.length > 0 && (
        <div className="mt-3 border-t border-[#F1ECDF] pt-2">
          {alternatives.map((item) => (
            <button
              key={item.organisasjonsnummer}
              type="button"
              onClick={() => onAccept(item)}
              className="block w-full truncate py-1 text-left font-inter text-[11px] text-[#6B6660] transition-colors hover:text-[#1F1B17]"
            >
              {item.navn}
            </button>
          ))}
        </div>
      )}
      <div className="mt-4 flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={() => onAccept(suggestion)}
          className="rounded-md bg-[#1F1B17] px-3.5 py-2 font-inter text-[11px] font-medium uppercase tracking-[0.16em] text-white transition-colors hover:bg-black"
        >
          {copy.acceptSuggestion}
        </button>
        <button
          type="button"
          onClick={onChange}
          className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890] transition-colors hover:text-[#1F1B17]"
        >
          {copy.changeSuggestion}
        </button>
      </div>
    </div>
  );
}

function VerifiedOrganizationCard({
  name,
  orgNumber,
  employeeCount,
  brregData,
  labels,
  formatNumber,
  onChange,
}: {
  name: string;
  orgNumber: string;
  employeeCount: number | undefined;
  brregData: BrregEnhet | undefined;
  labels: { orgNumber: string; employees: string; change: string };
  formatNumber: (value: number) => string;
  onChange: () => void;
}) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-md border border-[#D6D2CB] bg-[#F7F4ED] px-4 py-3">
      <div className="min-w-0">
        {brregData ? (
          <OrganizationSummary enhet={brregData} labels={labels} formatNumber={formatNumber} />
        ) : (
          <>
            <p className="truncate font-inter text-[14px] font-medium text-[#1F1B17]">{name}</p>
            <p className="mt-1 font-inter text-[11px] text-[#6B6660]">
              {labels.orgNumber} {orgNumber}
              {employeeCount != null ? ` · ${formatNumber(employeeCount)} ${labels.employees}` : ""}
            </p>
          </>
        )}
      </div>
      <button
        type="button"
        onClick={onChange}
        className="shrink-0 font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890] transition-colors hover:text-[#1F1B17]"
      >
        {labels.change}
      </button>
    </div>
  );
}

function OrganizationSummary({
  enhet,
  labels,
  formatNumber,
}: {
  enhet: BrregEnhet;
  labels: { orgNumber: string; employees: string };
  formatNumber: (value: number) => string;
}) {
  const address = formatBrregAddress(enhet.forretningsadresse);
  return (
    <div className="min-w-0">
      <p className="truncate font-inter text-[14px] font-medium text-[#1F1B17]">{enhet.navn}</p>
      <p className="mt-1 font-inter text-[11px] text-[#6B6660]">
        {labels.orgNumber} {enhet.organisasjonsnummer}
        {enhet.organisasjonsform?.beskrivelse ? ` · ${enhet.organisasjonsform.beskrivelse}` : ""}
        {enhet.antallAnsatte != null ? ` · ${formatNumber(enhet.antallAnsatte)} ${labels.employees}` : ""}
      </p>
      {address && <p className="mt-1 truncate font-inter text-[11px] text-[#A09890]">{address}</p>}
    </div>
  );
}

function OrganizationEvidencePanel({
  name,
  website,
  brregData,
  employeeCount,
  brandTheme,
  themeWarning,
  locale,
  formatNumber,
  onTheme,
}: {
  name: string;
  website: WebsitePayload | undefined;
  brregData: BrregEnhet | undefined;
  employeeCount: number | undefined;
  brandTheme: OnboardingBrandTheme | undefined;
  themeWarning: string | null;
  locale: "nb" | "en";
  formatNumber: (value: number) => string;
  onTheme: (mode: OnboardingBrandTheme["mode"]) => void | Promise<void>;
}) {
  const branding = website?.branding;
  const copy = organizationPanelCopy(locale);
  const brandColor = resolveBrandThemeColor(branding);
  const palette = validPalette(branding);
  const logoUrl = brandLogoUrl(branding);
  const displayName = displayOrganizationName(name || branding?.siteName || websiteHost(website?.url ?? ""));
  const activeMode = brandTheme?.mode ?? "verevon";
  const activeAccent = activeMode === "brand" ? brandColor : DEFAULT_ONBOARDING_ACCENT;
  return (
    <div
      className="relative size-full overflow-hidden p-8 text-[#1F1B17]"
      style={{
        background:
          `linear-gradient(145deg, color-mix(in srgb, ${activeAccent} 11%, #F4EFE5), #F4EFE5 48%, color-mix(in srgb, ${activeAccent} 6%, #FFFFFF))`,
      }}
    >
      <div
        aria-hidden
        className="absolute inset-0 opacity-[0.16]"
        style={{
          backgroundImage:
            "radial-gradient(circle at 20% 20%, var(--onboarding-accent, #111111) 0 1px, transparent 1px)",
          backgroundSize: "28px 28px",
        }}
      />
      <div className="relative z-10 flex h-full flex-col justify-between gap-5">
        <section className="rounded-2xl border border-[#E5DFD3] bg-white/90 p-5 shadow-[0_18px_42px_rgba(31,27,23,0.10)] backdrop-blur">
          <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#A09890]">{copy.readOnly}</p>
          <div className="mt-4 flex items-center gap-4">
            <span className="grid size-16 shrink-0 place-items-center overflow-hidden rounded-2xl border border-[#E5DFD3] bg-white">
              {logoUrl ? (
                // eslint-disable-next-line @next/next/no-img-element
                <img src={logoUrl} alt="" className="max-h-12 max-w-12 object-contain" referrerPolicy="no-referrer" />
              ) : (
                <span className="font-inter text-lg font-semibold">{displayName.slice(0, 1) || "V"}</span>
              )}
            </span>
            <div className="min-w-0">
              <h3 className="truncate font-inter text-[19px] font-semibold leading-6">{displayName || copy.unknownOrg}</h3>
              <p className="mt-1 truncate font-inter text-[12px] text-[#6B6660]">{websiteHost(website?.url ?? "") || copy.noDomain}</p>
            </div>
          </div>

          <div className="mt-5 grid grid-cols-2 gap-2">
            <PanelMetric label={copy.employees} value={employeeCount != null ? formatNumber(employeeCount) : copy.unknown} />
            <PanelMetric label={copy.orgSource} value={brregData ? "BRREG" : copy.inferred} />
          </div>
          {brregData && (
            <p className="mt-3 line-clamp-2 font-inter text-[11px] leading-4 text-[#6B6660]">
              {formatBrregAddress(brregData.forretningsadresse) || brregData.organisasjonsform?.beskrivelse || copy.publicRegistry}
            </p>
          )}
        </section>

        <section className="rounded-2xl border border-[#E5DFD3] bg-white/90 p-5 shadow-[0_18px_42px_rgba(31,27,23,0.08)] backdrop-blur">
          <p className="font-inter text-[10px] font-semibold uppercase tracking-[0.16em] text-[#A09890]">{copy.themeTitle}</p>
          <div className="mt-3 grid grid-cols-2 gap-2">
            <ThemeChoice
              active={activeMode === "verevon"}
              label="Verevon"
              color={DEFAULT_ONBOARDING_ACCENT}
              onClick={() => void onTheme("verevon")}
            />
            <ThemeChoice
              active={activeMode === "brand"}
              label={displayName || copy.brand}
              color={brandColor}
              onClick={() => void onTheme("brand")}
            />
          </div>
          {palette.length > 0 && (
            <div className="mt-4 flex items-center gap-2">
              {palette.map((color) => (
                <span key={color} className="size-5 rounded-full border border-black/10" style={{ backgroundColor: color }} />
              ))}
            </div>
          )}
          {themeWarning && <p className="mt-3 font-inter text-[11px] leading-4 text-[#B07C2E]">{themeWarning}</p>}
          <p className="mt-3 font-inter text-[11px] leading-4 text-[#6B6660]">{copy.themeHint}</p>
        </section>
      </div>
    </div>
  );
}

function ThemeChoice({
  active,
  label,
  color,
  onClick,
}: {
  active: boolean;
  label: string;
  color: string;
  onClick: () => void;
}) {
  const activeStyle = active
    ? {
        backgroundColor: color,
        borderColor: color,
        color: readableTextColor(color),
      }
    : undefined;

  return (
    <button
      type="button"
      onClick={onClick}
      style={activeStyle}
      className={cn(
        "flex min-w-0 items-center gap-2 rounded-lg border px-3 py-2 text-left transition-colors",
        active ? "border-[#1F1B17] bg-[#1F1B17] text-white" : "border-[#E5DFD3] bg-white text-[#1F1B17] hover:border-[#A09890]",
      )}
    >
      <span className="size-4 shrink-0 rounded-full border border-black/10" style={{ backgroundColor: color }} />
      <span className="truncate font-inter text-[12px] font-semibold">{label}</span>
    </button>
  );
}

function readableTextColor(color: string): "#1F1B17" | "#FFFFFF" {
  const raw = color.replace("#", "");
  const expanded = raw.length === 3 ? raw.split("").map((char) => char + char).join("") : raw.slice(0, 6);
  const r = Number.parseInt(expanded.slice(0, 2), 16);
  const g = Number.parseInt(expanded.slice(2, 4), 16);
  const b = Number.parseInt(expanded.slice(4, 6), 16);
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return luminance > 0.68 ? "#1F1B17" : "#FFFFFF";
}

function PanelMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-[#F1ECDF] px-3 py-2">
      <p className="font-inter text-[9px] uppercase tracking-[0.14em] text-[#A09890]">{label}</p>
      <p className="mt-1 truncate font-inter text-[13px] font-semibold text-[#1F1B17]">{value}</p>
    </div>
  );
}

function organizationPanelCopy(locale: "nb" | "en") {
  return locale === "nb"
    ? {
        readOnly: "Identitet og tema",
        unknownOrg: "Ukjent organisasjon",
        noDomain: "Ingen nettside valgt",
        employees: "Ansatte",
        unknown: "Ukjent",
        orgSource: "Kilde",
        inferred: "Foreslått",
        publicRegistry: "Offentlig registersignal funnet.",
        themeTitle: "Velg uttrykk",
        brand: "Merkevare",
        themeHint: "Du kan bytte tilbake her. Flere tema-valg kommer i Settings.",
      }
    : {
        readOnly: "Identity and theme",
        unknownOrg: "Unknown organization",
        noDomain: "No website selected",
        employees: "Employees",
        unknown: "Unknown",
        orgSource: "Source",
        inferred: "Inferred",
        publicRegistry: "Public registry signal found.",
        themeTitle: "Choose appearance",
        brand: "Brand",
        themeHint: "You can switch back here. More theme controls will live in Settings.",
      };
}

function validPalette(branding: BrandingSignals | undefined): string[] {
  const colors = [branding?.themeColor, ...(branding?.palette ?? [])]
    .map((color) => safeHexColor(color))
    .filter((color): color is string => Boolean(color));
  return Array.from(new Set(colors)).slice(0, 5);
}

function brandLogoUrl(branding: BrandingSignals | undefined): string | null {
  const raw = branding?.logoCandidate ?? branding?.favicon ?? branding?.appleTouchIcon;
  if (!raw) return null;
  try {
    const url = new URL(raw);
    return url.protocol === "http:" || url.protocol === "https:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function websiteHost(value: string): string {
  if (!value) return "";
  try {
    return new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(value) ? value : `https://${value}`).host.replace(/^www\./i, "");
  } catch {
    return value.replace(/^https?:\/\//i, "").split(/[/?#]/)[0] || value;
  }
}

function inferOrganizationQuery(website: WebsitePayload | undefined): string {
  const brandName = cleanBrandName(website?.branding?.siteName);
  if (brandName) return brandName;

  const rawUrl = website?.url?.trim();
  if (!rawUrl) return "";
  try {
    const parsed = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(rawUrl) ? rawUrl : `https://${rawUrl}`);
    const labels = parsed.hostname.replace(/^www\./i, "").split(".").filter(Boolean);
    const candidate = labels.length > 1 ? labels[labels.length - 2] : labels[0];
    return cleanBrandName(candidate?.replace(/[-_]+/g, " ") ?? "");
  } catch {
    return cleanBrandName(rawUrl.replace(/^https?:\/\//i, "").split(/[/?#]/)[0] ?? "");
  }
}

function cleanBrandName(value: string | undefined): string {
  if (!value) return "";
  const firstPart = value
    .replace(/\s+/g, " ")
    .split(/\s[|·\-–—]\s/)
    .at(0)
    ?.trim();
  return firstPart && firstPart.length >= 2 ? titleCase(firstPart).slice(0, 80) : "";
}

function titleCase(value: string): string {
  if (/[A-ZÆØÅ]/.test(value.slice(1))) return value;
  return value.replace(/\b[\p{L}\p{N}]/gu, (char) => char.toLocaleUpperCase("nb-NO"));
}

function rankBrregSuggestions(results: BrregEnhet[], query: string): BrregEnhet[] {
  const needle = normalizeSearch(query);
  return [...results]
    .filter((item) => !item.konkurs && !item.underAvvikling)
    .filter((item) => scoreBrreg(item, needle) > 0)
    .sort((a, b) => scoreBrreg(b, needle) - scoreBrreg(a, needle))
    .slice(0, 3);
}

function scoreBrreg(item: BrregEnhet, needle: string): number {
  const name = normalizeSearch(item.navn);
  let score = 0;
  if (name === needle) score += 100;
  if (name.startsWith(needle)) score += 50;
  if (name.includes(needle)) score += 25;
  if (item.hjemmeside && normalizeSearch(item.hjemmeside).includes(needle)) score += 20;
  if (item.antallAnsatte && item.antallAnsatte > 0) score += Math.min(10, Math.log10(item.antallAnsatte + 1) * 3);
  return score;
}

function normalizeSearch(value: string): string {
  return value
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/æ/g, "ae")
    .replace(/ø/g, "o")
    .replace(/å/g, "a")
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}
