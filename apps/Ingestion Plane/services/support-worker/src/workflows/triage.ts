import { proxyActivities } from '@temporalio/workflow';
import type * as activities from '../activities';

export interface TicketPayload {
  ticketId: number;
  title: string;
  body: string;
  ownerId?: number;
  groupId?: number;
}

export interface ClassifyResult {
  category: 'billing' | 'technical' | 'general' | 'spam';
  confidence: number;
  team: 'Support::Billing' | 'Support::Triage' | 'Support::Escalations';
  summary: string;
}

/**
 * Maps an AI-recommended team name to the Zammad group name.
 * Adjust these to match your actual Zammad group names.
 */
function resolveGroupName(team: ClassifyResult['team']): string {
  const mapping: Record<ClassifyResult['team'], string> = {
    'Support::Billing': 'Billing',
    'Support::Triage': 'Triage',
    'Support::Escalations': 'Escalations',
  };
  return mapping[team];
}

const { classifyActivity, patchZammadActivity, notifyAgentActivity } =
  proxyActivities<typeof activities>({
    startToCloseTimeout: '30s',
    retry: {
      maximumAttempts: 3,
    },
  });

export async function triageTicket(ticketData: TicketPayload): Promise<void> {
  // Step 1: classify via Model Plane
  const { category, confidence, team, summary } =
    await classifyActivity(ticketData);

  // Step 2: persist AI metadata back to Zammad
  await patchZammadActivity(ticketData.ticketId, {
    ai_category: category,
    ai_confidence: confidence,
    ai_recommended_team: team,
  });

  // Step 3: auto-assign if confidence is high enough
  if (confidence > 70) {
    await patchZammadActivity(ticketData.ticketId, {
      group: resolveGroupName(team),
    });
  }

  // Step 4: notify agent
  await notifyAgentActivity({
    type: 'ticket.triaged',
    ticketId: ticketData.ticketId,
    recipientId: ticketData.ownerId ?? 'broadcast',
    payload: {
      ticketId: ticketData.ticketId,
      message: `Ticket triaged — category: ${category}, confidence: ${confidence}%, team: ${team}. Summary: ${summary}`,
    },
  });
}
