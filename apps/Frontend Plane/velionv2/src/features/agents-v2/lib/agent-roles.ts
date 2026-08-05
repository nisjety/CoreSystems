export type AgentRoleId = "service" | "sales" | "ecommerce" | "chatbot" | "workflow";
export type CoreAgentRoleId = Extract<AgentRoleId, "service" | "sales" | "ecommerce">;
export type AgentSelectionId = "all" | AgentRoleId;
export type AgentStageId = "train" | "test" | "deploy" | "analyze";
export type AgentFeatureId =
  | "service-resolution"
  | "service-knowledge"
  | "service-actions"
  | "service-channels"
  | "service-quality"
  | "service-insights"
  | "sales-lead-capture"
  | "sales-qualification"
  | "sales-objections"
  | "sales-booking"
  | "sales-crm"
  | "sales-insights"
  | "commerce-shopping"
  | "commerce-support"
  | "commerce-product-finder"
  | "commerce-cart"
  | "commerce-brand"
  | "commerce-store"
  | "commerce-insights";
export type ChatbotBuilderSectionId =
  | "playground"
  | "chat-logs"
  | "data-sources"
  | "integrations"
  | "actions"
  | "analytics"
  | "leads"
  | "insights"
  | "install"
  | "settings";
export type ChatbotAddOnId =
  | "subscription-action"
  | "instructions"
  | "channels"
  | "user-info"
  | "guidance"
  | "billing-analytics"
  | "automation-rate"
  | "performance";
export type WorkflowToolTabId = "all" | "controls" | "ai-apps" | "custom";
export type WorkflowBuilderToolId =
  | "schedule-daily-post"
  | "generate-image"
  | "generate-caption"
  | "post-facebook"
  | "post-instagram"
  | "post-linkedin"
  | "merge"
  | "update-status"
  | "chatgpt"
  | "gemini"
  | "grok"
  | "perplexity"
  | "google-drive"
  | "google-slides"
  | "google-docs"
  | "slack"
  | "notion";

export const defaultAgentSelectionId: AgentSelectionId = "all";
const defaultAgentFeatureByRole: Record<CoreAgentRoleId, AgentFeatureId> = {
  service: "service-resolution",
  sales: "sales-lead-capture",
  ecommerce: "commerce-shopping",
};
export const defaultChatbotBuilderSectionId: ChatbotBuilderSectionId = "playground";
export const defaultChatbotAddOnId: ChatbotAddOnId = "subscription-action";
export const defaultWorkflowBuilderToolId: WorkflowBuilderToolId = "generate-caption";
export const agentSelectionEventName = "verevon:agent-selection-change";

export const agentRoleOptions: ReadonlyArray<{
  id: AgentSelectionId;
  label: string;
}> = [
  { id: "all", label: "All roles" },
  { id: "service", label: "Service" },
  { id: "sales", label: "Sales" },
  { id: "ecommerce", label: "Ecommerce" },
  { id: "chatbot", label: "Chatbot" },
  { id: "workflow", label: "Workflow builder" },
];

export const agentFeatureOptionsByRole: Record<
  CoreAgentRoleId,
  ReadonlyArray<{
    id: AgentFeatureId;
    label: string;
    description: string;
  }>
> = {
  service: [
    { id: "service-resolution", label: "Resolution queue", description: "Autonomous answers, actions, verified QA, and handoff states." },
    { id: "service-knowledge", label: "Knowledge", description: "Trusted sources, Fin-style guidance, attributes, and answer coverage." },
    { id: "service-actions", label: "Service actions", description: "Ada-style Actions, procedures, ticketing, lookup, and routing tools." },
    { id: "service-channels", label: "Channels", description: "Chat, email, voice, social, third-party, and inbox rollout." },
    { id: "service-quality", label: "Quality supervisor", description: "Source fit, policy match, verified resolution, and review queues." },
    { id: "service-insights", label: "Insights", description: "Suggestions, content recommendations, trends, and escalations." },
  ],
  sales: [
    { id: "sales-lead-capture", label: "Lead capture", description: "Piper-style proactive greetings, guided tours, and visitor engagement." },
    { id: "sales-qualification", label: "Qualification", description: "Drift-style visitor intelligence, fit score, discovery, and routing." },
    { id: "sales-objections", label: "Objections", description: "Agentforce-style product answers, pricing, security, and timing." },
    { id: "sales-booking", label: "Meeting booking", description: "Owner calendars, live-chat booking, and meeting links." },
    { id: "sales-crm", label: "CRM handoff", description: "Breeze-style research, buying signals, outreach, and CRM context." },
    { id: "sales-insights", label: "Pipeline insights", description: "Conversion quality, audit trail, automation controls, and objections." },
  ],
  ecommerce: [
    { id: "commerce-shopping", label: "Shopping assistant", description: "Product questions, quick replies, upsells, and recommendations." },
    { id: "commerce-support", label: "Support & orders", description: "Tracking, returns, exchanges, and subscriptions." },
    { id: "commerce-product-finder", label: "Product finder", description: "Guided Search, discovery, and comparison." },
    { id: "commerce-cart", label: "Cart recovery", description: "Checkout concerns and conversion nudges." },
    { id: "commerce-brand", label: "Brand voice", description: "AI Personas, social replies, tone, and autonomous CX." },
    { id: "commerce-store", label: "Store actions", description: "Sidekick-style guidance, content, apps, tasks, and approvals." },
    { id: "commerce-insights", label: "Shopper insights", description: "Intent, friction, and revenue opportunities." },
  ],
};

export const chatbotBuilderSectionOptions: ReadonlyArray<{
  id: ChatbotBuilderSectionId;
  label: string;
}> = [
  { id: "playground", label: "Playground" },
  { id: "chat-logs", label: "Chat logs" },
  { id: "data-sources", label: "Fine-tuning" },
  { id: "integrations", label: "Integrations" },
  { id: "actions", label: "Tools" },
  { id: "analytics", label: "Analytics" },
  { id: "leads", label: "Leads" },
  { id: "insights", label: "Insights" },
  { id: "install", label: "Install" },
  { id: "settings", label: "Settings" },
];

export const chatbotAddOnOptions: ReadonlyArray<{
  id: ChatbotAddOnId;
  label: string;
}> = [
  { id: "subscription-action", label: "Update subscription" },
  { id: "instructions", label: "Reading instructions" },
  { id: "channels", label: "Channel ID" },
  { id: "user-info", label: "User information" },
  { id: "guidance", label: "Applying guidance" },
  { id: "billing-analytics", label: "Billing chart" },
  { id: "automation-rate", label: "Automation rate" },
  { id: "performance", label: "Performance over time" },
];

export const workflowToolTabOptions: ReadonlyArray<{
  id: WorkflowToolTabId;
  label: string;
}> = [
  { id: "all", label: "All" },
  { id: "controls", label: "Controls" },
  { id: "ai-apps", label: "AI & Apps" },
  { id: "custom", label: "Custom" },
];

export const workflowToolOptions: ReadonlyArray<{
  id: WorkflowBuilderToolId;
  label: string;
  tabId: WorkflowToolTabId;
}> = [
  { id: "schedule-daily-post", label: "Schedule", tabId: "controls" },
  { id: "merge", label: "Merge", tabId: "controls" },
  { id: "chatgpt", label: "ChatGPT", tabId: "ai-apps" },
  { id: "gemini", label: "Gemini", tabId: "ai-apps" },
  { id: "grok", label: "Grok", tabId: "ai-apps" },
  { id: "perplexity", label: "Perplexity", tabId: "ai-apps" },
  { id: "google-drive", label: "Google Drive", tabId: "ai-apps" },
  { id: "update-status", label: "Google Sheets", tabId: "ai-apps" },
  { id: "google-slides", label: "Google Slides", tabId: "ai-apps" },
  { id: "google-docs", label: "Google Docs", tabId: "ai-apps" },
  { id: "slack", label: "Slack", tabId: "ai-apps" },
  { id: "notion", label: "Notion", tabId: "ai-apps" },
  { id: "post-linkedin", label: "LinkedIn", tabId: "ai-apps" },
  { id: "post-instagram", label: "Instagram", tabId: "ai-apps" },
  { id: "post-facebook", label: "Facebook", tabId: "ai-apps" },
];

const agentSelectionIds = new Set<AgentSelectionId>(agentRoleOptions.map((option) => option.id));
const chatbotBuilderSectionIds = new Set<ChatbotBuilderSectionId>(
  chatbotBuilderSectionOptions.map((option) => option.id),
);
const chatbotAddOnIds = new Set<ChatbotAddOnId>(chatbotAddOnOptions.map((option) => option.id));
const workflowBuilderToolIds = new Set<WorkflowBuilderToolId>(
  workflowToolOptions.map((option) => option.id),
);

function isAgentRoleId(value: string | null | undefined): value is AgentRoleId {
  return value === "service" || value === "sales" || value === "ecommerce" || value === "chatbot" || value === "workflow";
}

export function isCoreAgentRoleId(value: string | null | undefined): value is CoreAgentRoleId {
  return value === "service" || value === "sales" || value === "ecommerce";
}

export function isAgentSelectionId(value: string | null | undefined): value is AgentSelectionId {
  return Boolean(value && agentSelectionIds.has(value as AgentSelectionId));
}

export function isAgentFeatureForRole(role: CoreAgentRoleId, value: string | null | undefined): value is AgentFeatureId {
  return Boolean(value && agentFeatureOptionsByRole[role].some((option) => option.id === value));
}

export function getDefaultAgentFeatureForRole(role: CoreAgentRoleId): AgentFeatureId {
  return defaultAgentFeatureByRole[role];
}

export function isChatbotBuilderSectionId(value: string | null | undefined): value is ChatbotBuilderSectionId {
  return Boolean(value && chatbotBuilderSectionIds.has(value as ChatbotBuilderSectionId));
}

export function isChatbotAddOnId(value: string | null | undefined): value is ChatbotAddOnId {
  return Boolean(value && chatbotAddOnIds.has(value as ChatbotAddOnId));
}

export function isWorkflowBuilderToolId(value: string | null | undefined): value is WorkflowBuilderToolId {
  return Boolean(value && workflowBuilderToolIds.has(value as WorkflowBuilderToolId));
}

export function getAgentSelectionFromSearch(search: string): AgentSelectionId {
  const role = new URLSearchParams(search).get("agent");
  return isAgentRoleId(role) ? role : defaultAgentSelectionId;
}

export function getAgentFeatureFromSearch(search: string, role: CoreAgentRoleId): AgentFeatureId {
  const feature = new URLSearchParams(search).get("feature");
  return isAgentFeatureForRole(role, feature) ? feature : getDefaultAgentFeatureForRole(role);
}

export function getChatbotBuilderSectionFromSearch(search: string): ChatbotBuilderSectionId {
  const view = new URLSearchParams(search).get("view");
  return isChatbotBuilderSectionId(view) ? view : defaultChatbotBuilderSectionId;
}

export function getChatbotAddOnFromSearch(search: string): ChatbotAddOnId {
  const addOn = new URLSearchParams(search).get("addon");
  return isChatbotAddOnId(addOn) ? addOn : defaultChatbotAddOnId;
}

export function getWorkflowBuilderToolFromSearch(search: string): WorkflowBuilderToolId {
  const tool = new URLSearchParams(search).get("tool");
  return isWorkflowBuilderToolId(tool) ? tool : defaultWorkflowBuilderToolId;
}
