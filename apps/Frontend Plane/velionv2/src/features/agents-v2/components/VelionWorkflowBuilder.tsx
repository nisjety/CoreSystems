"use client";

import { useEffect } from "react";
import { useAgentSelection, useWorkflowBuilderTool } from "@/features/agents-v2/lib/use-agent-selection";
import {
  inspectorByTool,
  toolToCanvasNode,
} from "@/features/agents-v2/lib/velion-workflow-builder-data";
import {
  WorkflowCanvas,
  WorkflowGenerationStatus,
  WorkflowPromptComposer,
  WorkflowTopBar,
} from "@/features/agents-v2/components/VelionWorkflowCanvas";
import { WorkflowInspector } from "@/features/agents-v2/components/VelionWorkflowInspector";
import { WorkflowToolsPanel } from "@/features/agents-v2/components/VelionWorkflowToolsPanel";

export function VelionWorkflowBuilder() {
  const [agentSelection, setAgentSelection] = useAgentSelection();
  const [selectedTool, setSelectedTool] = useWorkflowBuilderTool();
  const selectedNodeId = toolToCanvasNode[selectedTool];
  const inspector = inspectorByTool[selectedNodeId];

  useEffect(() => {
    if (agentSelection !== "workflow") {
      setAgentSelection("workflow");
    }
  }, [agentSelection, setAgentSelection]);

  return (
    <div className="flex h-full min-h-0 bg-[#F5F1EC] p-2 text-[#2B2D31] dark:bg-[#101114] dark:text-[#F7F8F8]">
      <div className="relative min-h-0 flex-1 overflow-hidden rounded-[16px] border border-[#D8D2C8] bg-[#F8F9FA] shadow-[0_18px_42px_rgba(43,45,52,0.07)] dark:border-[#2A2C31] dark:bg-[#15161A]">
        <div
          aria-hidden="true"
          className="absolute inset-0 bg-[radial-gradient(circle_at_1px_1px,rgba(41,44,50,0.12)_1px,transparent_0)] [background-size:22px_22px] dark:bg-[radial-gradient(circle_at_1px_1px,rgba(255,255,255,0.11)_1px,transparent_0)]"
        />
        <div
          aria-hidden="true"
          className="absolute inset-0 bg-[linear-gradient(180deg,rgba(255,255,255,0.62),rgba(242,243,244,0.88))] dark:bg-[linear-gradient(180deg,rgba(16,17,20,0.48),rgba(16,17,20,0.92))]"
        />

        <div className="absolute bottom-0 left-0 top-0 z-40 w-[320px] shadow-[14px_0_40px_rgba(42,44,50,0.08)] lg:hidden">
          <WorkflowToolsPanel />
        </div>

        <WorkflowTopBar />

        <section
          aria-label="Workflow canvas"
          className="absolute inset-y-0 left-0 right-0 overflow-hidden pt-20 lg:right-[338px]"
        >
          <WorkflowCanvas
            selectedNodeId={selectedNodeId}
            onNodeSelect={setSelectedTool}
          />
          <WorkflowGenerationStatus />
          <WorkflowPromptComposer />
        </section>

        <WorkflowInspector inspector={inspector} />
      </div>
    </div>
  );
}
