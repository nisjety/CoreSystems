"use client";

import {
  Play,
  Rocket,
  Sparkles,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { VelionButton, VelionIconButton } from "@/components/ui/velion-ui";
import {
  agentFeatureOptionsByRole,
  type AgentRoleId,
  type AgentFeatureId,
} from "@/features/agents-v2/lib/agent-roles";
import { useAgentFeature, useAgentSelection } from "@/features/agents-v2/lib/use-agent-selection";
import { VelionChatbotStudio } from "@/features/agents-v2/components/VelionChatbotStudio";
import { VelionWorkflowBuilder } from "@/features/agents-v2/components/VelionWorkflowBuilder";
import { RoleCounterpartSurface } from "@/features/agents-v2/components/VelionAgentRoleSurfaces";
import { agentBlueprints } from "@/features/agents-v2/lib/velion-agent-blueprints";
import {
  AgentMetricStrip,
  RoleCard,
  RoleConversationPreview,
  StageReadinessPanel,
  StageSystemCard,
} from "@/features/agents-v2/components/VelionAgentsWorkspacePrimitives";
import type {
  AgentBlueprint,
} from "@/features/agents-v2/lib/velion-agent-page-types";
import {
  controlFocusClass,
  roleEyebrowClass,
  rolePanelClass,
} from "@/features/agents-v2/lib/velion-agent-page-styles";

export function VelionAgentsPage() {
  const [agentSelection, setAgentSelection] = useAgentSelection();
  const [agentFeature, setAgentFeature] = useAgentFeature(agentSelection);
  const activeRoleId = agentSelection === "all" ? null : agentSelection;
  const activeRole = agentBlueprints.find((role) => role.id === activeRoleId) ?? null;

  if (activeRole?.id === "workflow") {
    return <VelionWorkflowBuilder />;
  }

  if (activeRole?.id === "chatbot") {
    return <VelionChatbotStudio />;
  }

  return (
    <div className="h-full overflow-y-auto bg-[#FCFCFD] text-[#202126] transition-colors dark:bg-[#101114] dark:text-[#F7F8F8]">
      <div className="mx-auto flex min-h-full w-full max-w-[1540px] flex-col px-4 pb-8 pt-5 sm:px-6 lg:px-8">
        {activeRole ? (
          <SelectedAgentWorkspace
            role={activeRole}
            feature={agentFeature}
            onFeatureSelect={setAgentFeature}
          />
        ) : (
          <AllRolesOverview onRoleSelect={setAgentSelection} />
        )}
      </div>
    </div>
  );
}

function AllRolesOverview({ onRoleSelect }: { onRoleSelect: (role: AgentRoleId) => void }) {
  return (
    <>
      <header className="mx-auto w-full max-w-[760px] text-center">
        <div className="mx-auto grid size-7 place-items-center rounded-[8px] bg-[#111111] text-white shadow-[0_10px_24px_rgba(17,17,17,0.14)] dark:bg-white dark:text-[#111111]">
          <Sparkles className="size-4" strokeWidth={2} />
        </div>
        <h1
          id="agent-role-heading"
          className="mx-auto mt-4 max-w-[680px] text-[34px] font-semibold leading-[0.98] tracking-[-0.02em] text-[#1C1C1E] dark:text-white sm:text-[42px] lg:text-[48px]"
        >
          One agent system for the entire customer journey
        </h1>
        <p className="mx-auto mt-3 max-w-[620px] text-[14px] leading-6 text-[#6D717B] dark:text-[#AEB4C0]">
          Choose a Velion blueprint, then configure the role with knowledge, tests, channels, and insight loops.
        </p>
      </header>

      <section aria-labelledby="agent-role-heading" className="mt-7">
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-5">
          {agentBlueprints.map((role) => (
            <RoleCard
              key={role.id}
              active={false}
              role={role}
              onSelect={() => onRoleSelect(role.id)}
            />
          ))}
        </div>
      </section>
    </>
  );
}

// SelectedAgentWorkspace renders the prebuilt role agent selected from the shared Agents sidebar.
function SelectedAgentWorkspace({
  feature,
  onFeatureSelect,
  role,
}: {
  feature: AgentFeatureId;
  onFeatureSelect: (feature: AgentFeatureId) => void;
  role: AgentBlueprint;
}) {
  const Icon = role.Icon;
  const Visual = role.Visual;
  const operatingModel = role.operatingModel;

  if (!operatingModel) {
    return null;
  }

  if (role.id !== "service" && role.id !== "sales" && role.id !== "ecommerce") {
    return null;
  }

  const featureOptions = agentFeatureOptionsByRole[role.id];
  const activeFeature = featureOptions.some((option) => option.id === feature)
    ? feature
    : featureOptions[0].id;
  const system = role.featureWorkspaces?.[activeFeature] ?? role.system.train;

  return (
    <div className="pb-8">
      <header className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_390px] lg:items-start">
        <div className="min-w-0">
          <div className={cn("inline-flex items-center gap-2 rounded-full px-3 py-1 text-[12px] font-semibold", role.ringClass, role.iconClass)}>
            <Icon className="size-3.5" strokeWidth={2.1} />
            {role.shortTitle}
          </div>
          <h1 className="mt-3 max-w-[780px] text-[30px] font-semibold leading-[1.04] tracking-[-0.02em] text-[#1C1C1E] dark:text-white sm:text-[38px]">
            {system.title}
          </h1>
          <p className="mt-3 max-w-[760px] text-[14px] leading-6 text-[#636873] dark:text-[#AEB4C0]">{system.description}</p>
          <div className="mt-4 flex flex-wrap gap-2">
            {[operatingModel.model, operatingModel.confidence, operatingModel.automationTarget].map((item) => (
              <span
                key={item}
                className="rounded-full border border-[#E3E4E8] bg-white px-3 py-1.5 text-[11px] font-medium text-[#4D535D] shadow-sm dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#C8CED8]"
              >
                {item}
              </span>
            ))}
          </div>
        </div>

        <div className={cn("rounded-[8px] border p-4 shadow-[0_14px_34px_rgba(20,21,24,0.055)]", rolePanelClass(role))}>
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className={cn("text-[11px] font-semibold uppercase", roleEyebrowClass(role))}>Activation</p>
              <p className="mt-1 text-[15px] font-semibold text-[#202126] dark:text-white">{operatingModel.activationStatus}</p>
            </div>
            <span className={cn("grid size-9 place-items-center rounded-[8px] text-white", role.accentClass)}>
              <Icon className="size-4" strokeWidth={2.1} />
            </span>
          </div>
          <p className="mt-3 text-[12px] leading-5 text-[#626873] dark:text-[#AEB4C0]">{operatingModel.activationSummary}</p>
          <div className="mt-4 flex gap-2">
            <VelionButton variant="primary" size="sm" radius="pill" className={cn("flex-1 px-4 text-[12px] font-semibold", controlFocusClass)}>
              <Rocket className="size-3.5" />
              {operatingModel.activationLabel}
            </VelionButton>
            <VelionIconButton
              type="button"
              size="md"
              radius="pill"
              aria-label={`Run ${role.shortTitle} readiness test`}
              className={cn("border border-[#E2E3E8] bg-white dark:border-[#2B2D33] dark:bg-[#17181C]", controlFocusClass)}
            >
              <Play className="size-3.5" />
            </VelionIconButton>
          </div>
          <div className="mt-4 grid gap-1 sm:grid-cols-2" aria-label={`${role.shortTitle} feature areas`}>
            {featureOptions.map((option) => (
              <button
                key={option.id}
                type="button"
                aria-label={`Open ${option.label} workspace`}
                aria-pressed={option.id === activeFeature}
                onClick={() => onFeatureSelect(option.id)}
                className={cn(
                  "min-h-8 rounded-[7px] px-2 py-1 text-[11px] font-semibold leading-4 transition-colors",
                  option.id === activeFeature
                    ? "bg-[#111111] text-white dark:bg-white dark:text-[#111111]"
                    : "bg-[#F2F3F5] text-[#68707D] hover:bg-[#EAECF0] dark:bg-[#202228] dark:text-[#C0C6D0]",
                  controlFocusClass,
                )}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
      </header>

      <section className="mt-5 grid gap-4 xl:grid-cols-[minmax(0,1fr)_400px]">
        <div className="space-y-4">
          <AgentMetricStrip metrics={operatingModel.metrics} role={role} />

          <RoleCounterpartSurface feature={activeFeature} operatingModel={operatingModel} role={role} />

          <div className="grid gap-3 md:grid-cols-3" aria-label={`${role.shortTitle} ${system.eyebrow.toLowerCase()} system`}>
            {system.cards.map((card) => (
              <StageSystemCard key={card.title} card={card} role={role} />
            ))}
          </div>
        </div>

        <aside className="space-y-4">
          <div className={cn("rounded-[8px] border p-3 shadow-[0_18px_48px_rgba(20,21,24,0.055)]", rolePanelClass(role))}>
            <div className="mb-3 flex items-center justify-between gap-3">
              <div>
                <p className={cn("text-[11px] font-semibold uppercase", roleEyebrowClass(role))}>Preview</p>
                <h2 className="mt-1 text-[16px] font-semibold text-[#202126] dark:text-white">{system.previewTitle}</h2>
              </div>
              <span className={cn("grid size-8 place-items-center rounded-[8px] text-white", role.accentClass)}>
                <Sparkles className="size-4" />
              </span>
            </div>
            <Visual />
            <RoleConversationPreview operatingModel={operatingModel} role={role} />
            <p className="mt-3 text-[12px] leading-5 text-[#636873] dark:text-[#AEB4C0]">{system.previewDescription}</p>
          </div>

          <StageReadinessPanel role={role} system={system} />
        </aside>
      </section>
    </div>
  );
}
