"use client";

/**
 * Step 2 — organization. BRREG-verified lookup (search-first, manual fallback)
 * + company-size selection. Creates the org in org-core on submit (idempotent
 * if the machine already holds an id) and advances to the website step.
 */

import { useState } from "react";

import { cn } from "@/lib/utils";
import type { BrregEnhet } from "@/lib/services/brreg-service";
import { BrregSearch } from "../BrregSearch";
import { formatOnboardingText, useOnboardingCopy } from "../../lib/onboarding-i18n";
import {
  sizeFromEmployees,
  type OnboardingMachine,
  type OrganizationSize,
} from "../../lib/onboarding-machine";
import {
  createOrganization,
  OnboardingServiceError,
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

export function OrganizationStep({ machine }: { machine: OnboardingMachine }) {
  const { copy, formatNumber } = useOnboardingCopy();
  const initial = machine.state.organization;
  const [name, setName] = useState(initial?.name ?? "");
  const [orgNumber, setOrgNumber] = useState<string | undefined>(initial?.brregOrgNumber);
  const [employeeCount, setEmployeeCount] = useState<number | undefined>(initial?.employeeCount);
  const [brregData, setBrregData] = useState<BrregEnhet | undefined>();
  const [size, setSize] = useState<OrganizationSize | undefined>(initial?.size);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [manualEntry, setManualEntry] = useState(false);

  const handleBrregSelect = (enhet: BrregEnhet) => {
    setName(enhet.navn);
    setOrgNumber(enhet.organisasjonsnummer);
    setEmployeeCount(enhet.antallAnsatte);
    setBrregData(enhet);
    const inferred = sizeFromEmployees(enhet.antallAnsatte);
    if (inferred) setSize(inferred);
  };

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed || submitting) return;

    // Idempotent: if the org was already created this session, just advance.
    if (initial?.id) {
      machine.setOrganization({ ...initial, name: trimmed, size, brregOrgNumber: orgNumber, employeeCount });
      machine.goTo("website");
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      const created = await createOrganization({
        name: trimmed,
        plan: "trial",
        orgNumber: orgNumber ?? undefined,
        brregData: brregData ?? undefined,
      });
      machine.setOrganization({
        id: created.id,
        name: created.name || trimmed,
        slug: created.slug,
        size,
        brregOrgNumber: orgNumber,
        employeeCount,
      });
      machine.goTo("website");
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
          {manualEntry ? (
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
          ) : (
            <BrregSearch
              initialQuery={name}
              onSelect={handleBrregSelect}
              onManualEntry={() => {
                setManualEntry(true);
                setBrregData(undefined);
                setOrgNumber(undefined);
                setEmployeeCount(undefined);
              }}
            />
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
        <div className="grid size-full place-items-center bg-[#F4EFE5] p-10">
          <div className="relative size-[360px] rounded-full border border-[#E5DFD3] bg-white/60">
            {Array.from({ length: 18 }).map((_, index) => (
              <span
                key={index}
                className="absolute grid size-9 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border border-[#E5DFD3] bg-white text-[10px] text-[#6B6660] shadow-sm"
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
          <p className="font-inter text-[11px] uppercase tracking-[0.16em] text-[#A09890]">
            {copy.organization.personalizing}
          </p>
          <p className="mt-1 font-inter text-[12px] font-medium text-[#1F1B17]">
            {name
              ? employeeCount != null
                ? formatOnboardingText(copy.organization.foundEmployees, {
                    name,
                    count: formatNumber(employeeCount),
                  })
                : formatOnboardingText(copy.organization.fetchingPublicInfo, { name })
              : copy.organization.enterName}
          </p>
        </div>
      </RightPane>
    </>
  );
}
