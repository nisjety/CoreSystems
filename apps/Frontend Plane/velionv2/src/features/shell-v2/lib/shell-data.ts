export type VerevonRoute =
  | "/dashboard"
  | "/search"
  | "/chat"
  | "/inbox"
  | "/ingestions"
  | "/knowledge"
  | "/agents"
  | "/account"
  | "/settings"
  | "/settings/workspace"
  | "/settings/members"
  | "/settings/billing"
  | "/settings/sso"
  | "/settings/org-security"
  | "/settings/integrations";

export type WorkspaceIdentity = {
  accentColor?: string | null;
  domain?: string | null;
  initial: string;
  logoUrl?: string | null;
  name: string;
  plan: string;
  role?: string | null;
  userEmail?: string | null;
  userName?: string | null;
  userAvatar?: string | null;
};

export const fallbackWorkspaceIdentity: WorkspaceIdentity = {
  name: "Workspace",
  plan: "Free",
  initial: "V",
};

export function formatPlanLabel(plan?: string | null): string {
  const normalized = plan?.trim().toLowerCase();
  if (!normalized) return "Free";

  const labels: Record<string, string> = {
    advanced: "Advanced",
    custom: "Custom",
    enterprise: "Enterprise",
    essential: "Essential",
    expert: "Expert",
    free: "Free",
    hobby: "Hobby",
    pro: "Pro",
    standard: "Standard",
    trial: "Trial",
  };

  return labels[normalized] ?? normalized.replace(/(^|[-_\s])(\w)/g, (_match, prefix: string, char: string) => `${prefix === "_" ? " " : prefix}${char.toUpperCase()}`);
}
