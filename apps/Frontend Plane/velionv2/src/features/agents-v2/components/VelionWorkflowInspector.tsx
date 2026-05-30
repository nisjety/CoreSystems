"use client";

import {
  ChevronDown,
  GripVertical,
  Info,
  MoreHorizontal,
  Pencil,
  Plus,
  X,
} from "lucide-react";
import { VelionButton, VelionIconButton } from "@/components/ui/velion-ui";
import type { InspectorField, ToolInspector } from "@/features/agents-v2/lib/velion-workflow-builder-data";
import { BrandMark } from "@/features/agents-v2/components/VelionWorkflowBrandMark";

export function WorkflowInspector({ inspector }: { inspector: ToolInspector }) {
  const Icon = inspector.Icon;

  return (
    <aside className="absolute bottom-4 right-4 top-4 z-30 hidden w-[318px] flex-col overflow-hidden rounded-[18px] border border-white/76 bg-white/82 shadow-[0_24px_70px_rgba(43,45,52,0.16)] backdrop-blur-xl dark:border-white/10 dark:bg-[#17181C]/90 lg:flex">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-[#E8E9EC] px-4 dark:border-[#2A2C31]">
        <span className="grid size-6 place-items-center rounded-full bg-[#F2F3F5] text-[#343842] dark:bg-[#202228] dark:text-white">
          {inspector.brand ? (
            <BrandMark brand={inspector.brand} size="small" />
          ) : Icon ? (
            <Icon className="size-3.5" strokeWidth={2} />
          ) : null}
        </span>
        <h2 className="min-w-0 flex-1 truncate text-[14px] font-semibold text-[#282B31] dark:text-white">
          {inspector.title}
        </h2>
        <VelionIconButton type="button" size="xs" radius="pill" aria-label="More tool options">
          <MoreHorizontal className="size-3.5" strokeWidth={2} />
        </VelionIconButton>
        <VelionIconButton type="button" size="xs" radius="pill" aria-label="Close tool settings">
          <X className="size-3.5" strokeWidth={2} />
        </VelionIconButton>
      </div>

      <div className="grid grid-cols-[1fr_auto_1fr_auto_1fr] items-center border-b border-[#E8E9EC] px-4 py-3 text-[11px] font-semibold dark:border-[#2A2C31]">
        <span className="text-[#606672]">Setup</span>
        <ChevronDown className="-rotate-90 text-[#A2A7B0]" size={15} />
        <span className="border-b-2 border-[#343842] pb-3 text-center text-[#343842] dark:border-white dark:text-white">Configure</span>
        <ChevronDown className="-rotate-90 text-[#A2A7B0]" size={15} />
        <span className="text-right text-[#B2B6BE]">Test</span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        <div className="space-y-4">
          {inspector.fields.map((field) => (
            <InspectorSelect key={field.label} field={field} />
          ))}

          <div>
            <InspectorLabel label="Prompt" />
            <div className="mt-2 rounded-[8px] border border-[#DDE0E5] bg-white/70 p-3 text-[11px] leading-5 text-[#4C515B] dark:border-[#30333B] dark:bg-[#111216]/70 dark:text-[#D8DDE6]">
              {inspector.prompt.split("\n").map((line, index) => (
                <p key={`${line}-${index}`} className={line.startsWith("-") ? "pl-3" : undefined}>
                  {line}
                </p>
              ))}
            </div>
          </div>

          <VelionButton variant="secondary" size="xs" radius="pill" className="px-3 font-semibold">
            <Pencil className="size-3.5" strokeWidth={2} />
            Improve prompt
          </VelionButton>

          <div>
            <InspectorLabel label="Output" />
            <div className="mt-2 rounded-[8px] border border-[#DDE0E5] bg-white/70 p-2 dark:border-[#30333B] dark:bg-[#111216]/70">
              {inspector.outputs.map((output) => (
                <div key={output} className="flex h-8 items-center gap-2 rounded-[7px] px-1.5 text-[11px] font-medium text-[#5B616C] dark:text-[#D6DBE4]">
                  <GripVertical className="size-4 text-[#B1B6BE]" strokeWidth={1.8} />
                  <span className="min-w-0 flex-1 truncate">{output}</span>
                  <Pencil className="size-3.5 text-[#A5AAB3]" strokeWidth={2} />
                </div>
              ))}
            </div>
            <VelionButton variant="secondary" size="xs" radius="pill" className="mt-2 px-3 font-semibold">
              <Plus className="size-3.5" strokeWidth={2} />
              Add output
            </VelionButton>
          </div>
        </div>
      </div>

      <div className="shrink-0 px-4 pb-4 pt-2">
        <VelionButton variant="secondary" size="sm" radius="pill" className="w-full px-3 font-semibold">
          {inspector.nextLabel}
        </VelionButton>
      </div>
    </aside>
  );
}

function InspectorLabel({ label }: { label: string }) {
  return (
    <div className="flex items-center gap-1.5 text-[11px] font-medium text-[#5B616C] dark:text-[#C8CDD6]">
      {label}
      <Info className="size-3.5 text-[#A6ABB4]" strokeWidth={2} />
    </div>
  );
}

function InspectorSelect({ field }: { field: InspectorField }) {
  return (
    <label className="block">
      <InspectorLabel label={field.label} />
      <span className="mt-2 flex h-9 items-center rounded-[8px] border border-[#DDE0E5] bg-white/70 px-3 text-[11px] font-medium text-[#555B65] dark:border-[#30333B] dark:bg-[#111216]/70 dark:text-[#D8DDE6]">
        <span className="min-w-0 flex-1 truncate">{field.value}</span>
        <ChevronDown className="size-4 text-[#9EA4AD]" strokeWidth={2} />
      </span>
    </label>
  );
}
