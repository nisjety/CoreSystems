"use client";

import { ChevronDown, type LucideIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import { VerevonInput, VerevonTextarea } from "@/components/ui/verevon-ui";
import { cn } from "@/lib/utils";
import type { SupportIntegrationStatus } from "@/features/agents-v2/lib/use-chatbot-support-status";

export function PlaygroundAccordion({
  children,
  defaultOpen = false,
  Icon,
  title,
}: {
  children: ReactNode;
  defaultOpen?: boolean;
  Icon: LucideIcon;
  title: string;
}) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <details
      className="group rounded-[10px] border border-transparent"
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary className="flex cursor-pointer list-none items-center gap-3 rounded-[10px] px-0 py-2 text-[16px] font-semibold text-[#141519] transition-colors hover:bg-[#FAFAFB] dark:text-white dark:hover:bg-[#202229]">
        <span className="grid size-9 shrink-0 place-items-center rounded-[9px] bg-[#F4F4F5] text-[#1D1D1F] dark:bg-[#202229] dark:text-white">
          <Icon className="size-4" strokeWidth={1.8} />
        </span>
        <span className="min-w-0 flex-1 truncate">{title}</span>
        <ChevronDown className="size-5 text-[#5F6673] transition-transform group-open:rotate-180 dark:text-[#AEB4C0]" />
      </summary>
      <div className="pb-4 pt-2">{children}</div>
    </details>
  );
}

export function SupportIntegrationBanner({ status }: { status: SupportIntegrationStatus }) {
  const connected = status.status === "connected";
  const checking = status.status === "loading";

  return (
    <div
      className={cn(
        "mt-4 rounded-[10px] border p-3 text-[12px] leading-5",
        connected
          ? "border-[#CFE8D7] bg-[#F1FBF4] text-[#216A39] dark:border-[#254832] dark:bg-[#101A13] dark:text-[#BCE8C8]"
          : "border-[#E3DFD7] bg-[#FAF7F1] text-[#6D6257] dark:border-[#34302B] dark:bg-[#161310] dark:text-[#D8C9BA]",
      )}
    >
      <div className="flex items-center gap-2 font-semibold">
        <span className={cn("size-2 rounded-full", checking ? "bg-[#D59F45]" : connected ? "bg-[#10B35A]" : "bg-[#EE7A50]")} />
        {checking ? "Checking live support actions" : connected ? "Live support actions connected" : "Support actions not connected"}
      </div>
      <p className="mt-1">{status.message}</p>
      <div className="mt-2 flex flex-wrap gap-1.5">
        <SupportIntegrationPill label="Agents" value={status.agents} />
        <SupportIntegrationPill label="Groups" value={status.groups} />
        <SupportIntegrationPill label="Macros" value={status.macros} />
      </div>
    </div>
  );
}

function SupportIntegrationPill({ label, value }: { label: string; value: number }) {
  return (
    <span className="rounded-full border border-black/5 bg-white/70 px-2 py-0.5 text-[10px] font-semibold uppercase text-current dark:border-white/10 dark:bg-white/5">
      {label}: {value}
    </span>
  );
}

export function SettingInput({
  compact = false,
  label,
  value,
}: {
  compact?: boolean;
  label: string;
  value: string;
}) {
  return (
    <label className={cn("mt-4 block", compact ? "max-w-[124px]" : "")}>
      <span className="block text-[14px] font-medium text-[#202126] dark:text-white">{label}</span>
      <VerevonInput
        defaultValue={value}
        variant="compact"
        className="mt-2"
      />
    </label>
  );
}

export function SettingTextarea({ label, value }: { label: string; value: string }) {
  return (
    <label className="mt-4 block">
      <span className="block text-[14px] font-medium text-[#202126] dark:text-white">{label}</span>
      <VerevonTextarea
        defaultValue={value}
        variant="compact"
        className="mt-2"
      />
    </label>
  );
}

export function SectionHeader({
  action,
  description,
  title,
}: {
  action?: ReactNode;
  description?: string;
  title: string;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-5">
      <div className="min-w-0">
        <h1 className="text-[26px] font-semibold leading-tight tracking-normal">{title}</h1>
        {description ? <p className="mt-2 text-[14px] leading-6 text-[#555B65] dark:text-[#AEB4C0]">{description}</p> : null}
      </div>
      {action ? <div className="min-w-0">{action}</div> : null}
    </div>
  );
}

export function EmptyStateCard({
  Icon,
  description,
  title,
}: {
  Icon: LucideIcon;
  description: string;
  title: string;
}) {
  return (
    <div className="mt-5 rounded-[12px] border border-dashed border-[#DADDE4] bg-[#FCFCFD] p-8 text-center dark:border-[#303238] dark:bg-[#111216]">
      <span className="mx-auto grid size-11 place-items-center rounded-[10px] bg-white text-[#8A909B] shadow-sm dark:bg-[#17181C] dark:text-[#C6CCD6]">
        <Icon className="size-5" />
      </span>
      <h2 className="mt-4 text-[18px] font-semibold text-[#202126] dark:text-white">{title}</h2>
      <p className="mx-auto mt-2 max-w-[520px] text-[14px] leading-6 text-[#6F747D] dark:text-[#AEB4C0]">{description}</p>
    </div>
  );
}

export function EmptyStateInline({
  Icon,
  description,
  title,
}: {
  Icon: LucideIcon;
  description: string;
  title: string;
}) {
  return (
    <div className="mt-5 rounded-[10px] border border-dashed border-[#DADDE4] bg-[#FCFCFD] px-4 py-5 text-center dark:border-[#303238] dark:bg-[#111216]">
      <Icon className="mx-auto size-5 text-[#8A909B]" />
      <p className="mt-3 text-[14px] font-semibold text-[#30343B] dark:text-white">{title}</p>
      <p className="mx-auto mt-1 max-w-[420px] text-[13px] leading-5 text-[#6F747D] dark:text-[#AEB4C0]">{description}</p>
    </div>
  );
}

export function MetricCard({ Icon, label, value }: { Icon: LucideIcon; label: string; value: string }) {
  return (
    <div className="verevon-panel p-5">
      <div className="flex items-center gap-2 text-[14px] font-semibold text-[#555B65] dark:text-[#D7DCE4]">
        <Icon className="size-5" />
        {label}
      </div>
      <div className="mt-5 text-[30px] font-medium leading-none">{value}</div>
    </div>
  );
}

export function FieldPill({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-full bg-[#F3F1ED] px-2.5 py-1.5 text-[10px] font-semibold uppercase text-[#6E737D] dark:bg-[#202228] dark:text-[#C0C6D0]">
      <span>{label}</span>
      <span className="rounded-full bg-white px-2 py-0.5 text-[#383D47] dark:bg-[#111216] dark:text-white">{value}</span>
    </div>
  );
}
