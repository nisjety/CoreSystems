import { config } from '../config';

/**
 * PATCHes arbitrary fields on a Zammad ticket via the REST API.
 *
 * @param ticketId Zammad ticket ID.
 * @param fields   Partial ticket fields to update (e.g. { group, tags, ... }).
 */
export async function patchZammadActivity(
  ticketId: number,
  fields: Record<string, unknown>,
): Promise<void> {
  if (!config.ZAMMAD_API_TOKEN) {
    throw new Error('ZAMMAD_API_TOKEN is required to patch Zammad tickets');
  }

  const url = `${config.ZAMMAD_API_URL}/api/v1/tickets/${ticketId}`;

  const response = await fetch(url, {
    method: 'PATCH',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Token token=${config.ZAMMAD_API_TOKEN}`,
    },
    body: JSON.stringify(fields),
  });

  if (!response.ok) {
    const body = await response.text().catch(() => '');
    throw new Error(
      `Zammad PATCH tickets/${ticketId} failed: ${response.status} ${response.statusText} — ${body}`,
    );
  }
}
