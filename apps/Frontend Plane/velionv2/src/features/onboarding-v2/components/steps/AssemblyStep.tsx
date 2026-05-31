"use client";

/**
 * Step 7 — "assembling your dashboard" finale. Ticks through the setup items,
 * marks onboarding complete on user-core (breaks the dashboard ↔ /login guard
 * loop), resets the wizard's localStorage, and redirects to /dashboard.
 *
 * Fires-and-forgets the completion call: if it fails the user still lands on
 * the dashboard and the next OnboardingGuard run reattempts.
 */

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import type { Route } from "next";
import { Sparkles } from "lucide-react";

import { useOnboardingCopy } from "../../lib/onboarding-i18n";
import type { OnboardingMachine } from "../../lib/onboarding-machine";
import { completeOnboarding } from "../../lib/onboarding-service";
import {
  LeftPane,
  RightPane,
  StepDescription,
  StepEyebrow,
  StepTitle,
} from "../onboarding-shared";

export function AssemblyStep({ machine }: { machine: OnboardingMachine }) {
  const { copy } = useOnboardingCopy();
  const router = useRouter();
  const [completed, setCompleted] = useState(0);
  const ticks = copy.assembly.ticks;
  const orgId = machine.state.organization?.id;
  const plan = machine.state.organization?.plan;

  useEffect(() => {
    const timers: number[] = [];
    ticks.forEach((_, index) => {
      timers.push(window.setTimeout(() => setCompleted(index + 1), 700 * (index + 1)));
    });
    timers.push(
      window.setTimeout(
        () => {
          void (async () => {
            await completeOnboarding({ plan, orgId, source: "wizard-v2" }).catch(() => undefined);
            machine.reset();
            router.push("/dashboard" as Route);
            router.refresh();
          })();
        },
        700 * (ticks.length + 1) + 400,
      ),
    );
    return () => timers.forEach((timer) => window.clearTimeout(timer));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [machine, router]);

  return (
    <>
      <LeftPane machine={machine}>
        <StepEyebrow>{copy.assembly.eyebrow}</StepEyebrow>
        <StepTitle>{copy.assembly.title}</StepTitle>
        <StepDescription>{copy.assembly.description}</StepDescription>
        <ul className="flex flex-col gap-2">
          {ticks.map((label, index) => {
            const done = index < completed;
            const active = index === completed;
            return (
              <li key={label} className="flex items-center gap-3 font-inter text-[13px]">
                <span
                  className={
                    done
                      ? "flex size-5 items-center justify-center rounded-full border border-[#1F1B17] bg-[#1F1B17] text-white"
                      : active
                        ? "flex size-5 items-center justify-center rounded-full border border-[#1F1B17] bg-white text-[#1F1B17]"
                        : "flex size-5 items-center justify-center rounded-full border border-[#D6D2CB] bg-white text-transparent"
                  }
                >
                  {done ? "✓" : active ? "·" : ""}
                </span>
                <span className={done ? "text-[#1F1B17]" : "text-[#A09890]"}>{label}</span>
              </li>
            );
          })}
        </ul>
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
