"use client";

import type { ReactNode } from "react";
import {
  CheckCircle2,
  ChevronRight,
  PanelRight,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { VelionButton } from "@/components/ui/velion-ui";
import type {
  AgentBlueprint,
  RoleFeature,
  RoleMetric,
  RoleOperatingModel,
  StageCard,
  StageSystem,
} from "@/features/agents-v2/lib/velion-agent-page-types";
import {
  controlFocusClass,
  roleEyebrowClass,
  roleInsetClass,
  rolePanelClass,
} from "@/features/agents-v2/lib/velion-agent-page-styles";

export function AgentMetricStrip({ metrics, role }: { metrics: RoleMetric[]; role: AgentBlueprint }) {
  return (
    <div className="grid gap-3 sm:grid-cols-3" aria-label="Agent readiness metrics">
      {metrics.map((metric) => (
        <div key={metric.label} className={cn("velion-agent-panel", rolePanelClass(role))}>
          <p className={cn("velion-agent-eyebrow", roleEyebrowClass(role))}>{metric.label}</p>
          <p className="mt-2 text-[28px] font-semibold leading-none tracking-[-0.02em] text-[#202126] dark:text-white">{metric.value}</p>
          <p className="velion-agent-body mt-2">{metric.detail}</p>
        </div>
      ))}
    </div>
  );
}

export function CounterpartPanel({
  children,
  className,
  description,
  eyebrow,
  icon: Icon,
  role,
  title,
}: {
  children: ReactNode;
  className?: string;
  description: string;
  eyebrow: string;
  icon: LucideIcon;
  role: AgentBlueprint;
  title: string;
}) {
  return (
    <div className={cn("velion-agent-panel", rolePanelClass(role), className)}>
      <div className="mb-3 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className={cn("velion-agent-eyebrow", roleEyebrowClass(role))}>{eyebrow}</p>
          <h2 className="velion-agent-title mt-1">{title}</h2>
          <p className="velion-agent-body mt-1 max-w-[640px]">{description}</p>
        </div>
        <span className={cn("grid size-8 shrink-0 place-items-center rounded-[8px] text-white", role.accentClass)}>
          <Icon className="size-4" />
        </span>
      </div>
      {children}
    </div>
  );
}

export function StatusRow({ label, role, value }: { label: string; role: AgentBlueprint; value: string }) {
  return (
    <div className={cn("velion-agent-inset-row mt-2 justify-between first:mt-0", roleInsetClass(role))}>
      <div className="flex min-w-0 items-center gap-2 text-[11px] font-semibold text-[#4B515C] dark:text-[#DCE2EC]">
        <span className={cn("size-1.5 shrink-0 rounded-full", role.accentClass)} />
        <span>{label}</span>
      </div>
      <span className={cn("velion-agent-chip shrink-0 py-0.5", role.ringClass, role.iconClass)}>{value}</span>
    </div>
  );
}

export function AgentFeatureBoard({
  operatingModel,
  role,
}: {
  operatingModel: RoleOperatingModel;
  role: AgentBlueprint;
}) {
  const sections = [
    { title: "Knowledge", description: "What the agent can trust.", items: operatingModel.knowledge },
    { title: "Actions", description: "What the agent can safely do.", items: operatingModel.actions },
    { title: "Channels", description: "Where the agent can operate.", items: operatingModel.channels },
    { title: "Guardrails", description: "How the agent avoids risky behavior.", items: operatingModel.guardrails },
  ];

  return (
    <section className="grid gap-3 lg:grid-cols-2" aria-label={`${role.shortTitle} configured capabilities`}>
      {sections.map((section) => (
        <div key={section.title} className={cn("velion-agent-panel", rolePanelClass(role))}>
          <div className="mb-3 flex items-end justify-between gap-3">
            <div>
              <h2 className="text-[14px] font-semibold text-[#202126] dark:text-white">{section.title}</h2>
              <p className="mt-1 text-[11px] text-[#7A808B] dark:text-[#AEB4C0]">{section.description}</p>
            </div>
            <span className={cn("size-2 rounded-full", role.accentClass)} />
          </div>
          <div className="space-y-2">
            {section.items.map((item) => (
              <FeatureRow key={item.title} feature={item} role={role} />
            ))}
          </div>
        </div>
      ))}
    </section>
  );
}

export function FeatureRow({ feature, role }: { feature: RoleFeature; role: AgentBlueprint }) {
  const Icon = feature.icon;

  return (
    <div className={cn("velion-agent-inset flex gap-3", roleInsetClass(role))}>
      <span className="grid size-8 shrink-0 place-items-center rounded-[7px] bg-white text-[#3F444D] shadow-sm dark:bg-[#1B1D22] dark:text-[#DCE2EC]">
        <Icon className="size-4" strokeWidth={2} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-start justify-between gap-2">
          <h3 className="text-[12px] font-semibold text-[#202126] dark:text-white">{feature.title}</h3>
          <span className={cn("velion-agent-chip shrink-0 py-0.5", role.ringClass, role.iconClass)}>
            {feature.status}
          </span>
        </div>
        <p className="mt-1 text-[11px] leading-4 text-[#69707B] dark:text-[#B8BFCA]">{feature.description}</p>
      </div>
    </div>
  );
}

export function RoleConversationPreview({
  operatingModel,
  role,
}: {
  operatingModel: RoleOperatingModel;
  role: AgentBlueprint;
}) {
  return (
    <div className={cn("velion-agent-inset mt-3", roleInsetClass(role))}>
      <div className="ml-auto max-w-[84%] rounded-[14px] bg-[#111111] px-3 py-2 text-[12px] leading-5 text-white dark:bg-white dark:text-[#111111]">
        {operatingModel.conversation.customer}
      </div>
      <div className="mt-2 max-w-[90%] rounded-[14px] bg-white px-3 py-2 text-[12px] leading-5 text-[#2F343C] shadow-sm dark:bg-[#1B1D22] dark:text-[#E7EBF1]">
        {operatingModel.conversation.agent}
      </div>
      <div className="mt-3 rounded-[7px] border border-dashed border-[#DDE0E5] px-3 py-2 text-[11px] leading-4 text-[#656C78] dark:border-[#343842] dark:text-[#B7BEC9]">
        {operatingModel.conversation.note}
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        {operatingModel.conversation.quickReplies.map((reply) => (
          <button
            key={reply}
            type="button"
            className={cn(
              "rounded-full border border-[#E2E3E8] bg-white px-2.5 py-1 text-[10px] font-semibold text-[#3C414A] transition hover:bg-[#F2F3F5] dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#DCE2EC]",
              role.ringClass,
              controlFocusClass,
            )}
          >
            {reply}
          </button>
        ))}
      </div>
    </div>
  );
}

export function StageReadinessPanel({ role, system }: { role: AgentBlueprint; system: StageSystem }) {
  return (
    <div className={cn("velion-agent-panel velion-agent-panel-strong", rolePanelClass(role))}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className={cn("velion-agent-eyebrow", roleEyebrowClass(role))}>Operational checklist</p>
          <h2 className="velion-agent-title mt-1">{role.shortTitle} readiness</h2>
        </div>
        <VelionButton variant="primary" size="xs" radius="pill" className={cn("px-3 font-semibold", controlFocusClass)}>
          <Zap className="size-3.5" />
          {system.primaryAction}
        </VelionButton>
      </div>
      <div className="mt-4 grid gap-2">
        {system.checklist.map((item) => (
          <div key={item} className={cn("velion-agent-inset-row", roleInsetClass(role))}>
            <CheckCircle2 className={cn("size-4 shrink-0", role.iconClass)} strokeWidth={2.1} />
            <span className="text-[12px] font-medium text-[#333740] dark:text-[#E6EAF0]">{item}</span>
          </div>
        ))}
      </div>
      <VelionButton variant="secondary" size="xs" radius="pill" className={cn("mt-3 w-full px-3 font-semibold", controlFocusClass)}>
        <PanelRight className="size-3.5" />
        {system.secondaryAction}
      </VelionButton>
    </div>
  );
}

export function StageSystemCard({ card, role }: { card: StageCard; role: AgentBlueprint }) {
  const Icon = card.icon;

  return (
    <div className={cn("velion-agent-panel velion-agent-panel-strong", rolePanelClass(role))}>
      <span className={cn("grid size-9 place-items-center rounded-[8px] text-white", role.accentClass)}>
        <Icon className="size-4" strokeWidth={2.1} />
      </span>
      <h2 className="velion-agent-title mt-3">{card.title}</h2>
      <p className="velion-agent-body mt-1 min-h-10">{card.description}</p>
      <div className="mt-3 space-y-1.5 border-t border-dashed border-[#E3E4E8] pt-3 dark:border-[#2D3037]">
        {card.items.map((item) => (
          <div key={item} className="flex items-start gap-2 text-[11px] leading-4 text-[#3F444D] dark:text-[#D7DCE4]">
            <span className={cn("mt-1 size-1.5 shrink-0 rounded-full", role.accentClass)} />
            <span>{item}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function RoleCard({
  active,
  onSelect,
  role,
}: {
  active: boolean;
  onSelect: () => void;
  role: AgentBlueprint;
}) {
  const Icon = role.Icon;
  const Visual = role.Visual;

  return (
    <button
      type="button"
      aria-label={role.title}
      aria-pressed={active}
      onClick={onSelect}
      className={cn(
        "group flex min-h-[344px] flex-col rounded-[8px] border bg-white p-2 text-left shadow-[0_16px_38px_rgba(20,21,24,0.055)] transition duration-200 hover:-translate-y-0.5 hover:shadow-[0_20px_52px_rgba(20,21,24,0.08)] dark:bg-[#15161A]",
        active
          ? cn("border-transparent ring-2", role.ringClass)
          : "border-[#E7E7EA] hover:border-[#D9DADF] dark:border-[#292B31] dark:hover:border-[#373A42]",
        controlFocusClass,
      )}
    >
      <Visual />

      <div className="flex flex-1 flex-col px-2 pb-2 pt-3">
        <div className="flex items-start gap-2">
          <span className={cn("grid size-6 shrink-0 place-items-center rounded-[6px] text-white", role.accentClass)}>
            <Icon className="size-3.5" strokeWidth={2.1} />
          </span>
          <div className="min-w-0">
            <h2 className="text-[16px] font-semibold leading-5 text-[#202126] dark:text-white">{role.title}</h2>
            <p className="mt-1 text-[12px] leading-5 text-[#6F747D] dark:text-[#AEB4C0]">{role.eyebrow}.</p>
          </div>
        </div>

        <p className="mt-3 text-[12px] leading-5 text-[#4E535C] dark:text-[#C0C6D0]">{role.description}</p>

        <div className="mt-3 space-y-1.5 border-t border-dashed border-[#E3E4E8] pt-3 dark:border-[#2D3037]">
          {role.capabilities.map((capability) => (
            <div key={capability} className="flex items-start gap-2 text-[11px] leading-4 text-[#3F444D] dark:text-[#D7DCE4]">
              <span className={cn("mt-1 size-1.5 shrink-0 rounded-full", role.accentClass)} />
              <span>{capability}</span>
            </div>
          ))}
        </div>

        <span className="velion-button velion-button-primary velion-button-xs velion-button-pill mt-auto w-max px-3 font-semibold group-hover:bg-[#000000]">
          {role.cta}
          <ChevronRight className="size-3.5" strokeWidth={2.2} />
        </span>
      </div>
    </button>
  );
}
