import { Sparkles } from "lucide-react";

export type VelionRoute =
  | "/dashboard"
  | "/chat"
  | "/inbox"
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

export const workspaceIdentity = {
  name: "aquatiq-as",
  plan: "Free",
  profile: "I",
  assistant: "Velion",
  badgeIcon: Sparkles,
} as const;
