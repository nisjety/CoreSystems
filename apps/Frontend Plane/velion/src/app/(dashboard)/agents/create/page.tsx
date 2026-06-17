import { AgentCreateForm } from '@/components/agents/AgentCreateForm';

/**
 * Wave 9 (ui-ux-velion-gap.md §19) follow-up: real create-agent flow.
 *
 * The earlier placeholder ("This route is reserved for the creation flow
 * while the builder is being integrated") meant operators could see
 * existing agents but could not actually create new ones — the New Agent
 * button at /agents linked to this page and dead-ended.
 *
 * The form below submits to POST /api/agents (Zod-validated by
 * `createAgentSchema` in `src/app/api/agents/route.ts`) and redirects
 * the user into the new agent's workspace on success. Once created the
 * agent has full access to the playground, knowledge, tools, embed,
 * fine-tune, and analytics surfaces.
 */
export default function AgentsCreatePage() {
  return <AgentCreateForm />;
}
