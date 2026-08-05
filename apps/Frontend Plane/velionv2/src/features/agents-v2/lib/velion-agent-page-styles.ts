import type { AgentBlueprint } from "@/features/agents-v2/lib/verevon-agent-page-types";

const rolePanelClassById: Record<"service" | "sales" | "ecommerce", string> = {
  service: "border-[#D3CEC6] bg-[#FFFDF8] dark:border-[#3B332D] dark:bg-[#191715]",
  sales: "border-[#DADDF8] bg-white dark:border-[#2A2E50] dark:bg-[#121318]",
  ecommerce: "border-[#DDEBDD] bg-[#FBFBF5] dark:border-[#1E2C31] dark:bg-[#111612]",
};

const roleInsetClassById: Record<"service" | "sales" | "ecommerce", string> = {
  service: "border-[#EBE7E1] bg-[#F8F3EC] dark:border-[#332E28] dark:bg-[#141210]",
  sales: "border-[#E4E6FB] bg-[#F8F9FF] dark:border-[#252A4C] dark:bg-[#0F1016]",
  ecommerce: "border-[#DCEFE1] bg-[#F4FBF2] dark:border-[#203629] dark:bg-[#101A13]",
};

const roleEyebrowClassById: Record<"service" | "sales" | "ecommerce", string> = {
  service: "text-[#7B5B47] dark:text-[#D8B7A0]",
  sales: "text-[#5E6AD2] dark:text-[#9EA7FF]",
  ecommerce: "text-[#2E6C45] dark:text-[#C1FBD4]",
};

export const controlFocusClass = "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#111111] dark:focus-visible:outline-white";

export function rolePanelClass(role: AgentBlueprint) {
  if (role.id === "service" || role.id === "sales" || role.id === "ecommerce") {
    return rolePanelClassById[role.id];
  }

  return "border-[#E6E7EB] bg-white dark:border-[#292B31] dark:bg-[#15161A]";
}

export function roleInsetClass(role: AgentBlueprint) {
  if (role.id === "service" || role.id === "sales" || role.id === "ecommerce") {
    return roleInsetClassById[role.id];
  }

  return "border-[#ECECF0] bg-[#FAFAFB] dark:border-[#2B2D33] dark:bg-[#111216]";
}

export function roleEyebrowClass(role: AgentBlueprint) {
  if (role.id === "service" || role.id === "sales" || role.id === "ecommerce") {
    return roleEyebrowClassById[role.id];
  }

  return "text-[#8C929E]";
}
