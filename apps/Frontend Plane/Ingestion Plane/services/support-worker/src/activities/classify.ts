import { config } from '../config';
import type { TicketPayload, ClassifyResult } from '../workflows/triage';

const SYSTEM_PROMPT = `You are a support ticket classifier. Classify the ticket and return ONLY JSON:
{"category":"billing|technical|general|spam","confidence":0-100,"team":"Support::Billing|Support::Triage|Support::Escalations","summary":"one line"}`;

interface AiCoreResponse {
  answer: string;
}

/**
 * Calls the Model Plane v2 reasoning endpoint to classify a support ticket.
 * Returns a typed classification result.
 */
export async function classifyActivity(
  ticketData: TicketPayload,
): Promise<ClassifyResult> {
  const query = `${ticketData.title}\n\n${ticketData.body}`.trim();

  const response = await fetch(
    `${config.AI_CORE_URL}/api/v1/reason`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ system_prompt: SYSTEM_PROMPT, query }),
    },
  );

  if (!response.ok) {
    throw new Error(
      `AI Core request failed: ${response.status} ${response.statusText}`,
    );
  }

  const data = (await response.json()) as AiCoreResponse;

  let parsed: unknown;
  try {
    parsed = JSON.parse(data.answer);
  } catch {
    throw new Error(
      `AI Core returned non-JSON answer: ${data.answer.slice(0, 200)}`,
    );
  }

  // Basic shape validation before casting
  if (
    typeof parsed !== 'object' ||
    parsed === null ||
    !('category' in parsed) ||
    !('confidence' in parsed) ||
    !('team' in parsed) ||
    !('summary' in parsed)
  ) {
    throw new Error(
      `AI Core answer missing required fields: ${JSON.stringify(parsed)}`,
    );
  }

  return parsed as ClassifyResult;
}
