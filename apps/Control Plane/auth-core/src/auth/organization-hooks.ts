/**
 * Organization Event Hooks
 *
 * Provides access to AuthEventPublisher for publishing organization events
 * from orpc-router procedures
 */

import { AuthEventPublisher } from '../internal/auth-event.publisher';

let eventPublisher: AuthEventPublisher | null = null;

export function setOrganizationEventPublisher(publisher: AuthEventPublisher) {
  console.log('🔧 Setting organization event publisher:', !!publisher);
  eventPublisher = publisher;
}

export function getOrganizationEventPublisher(): AuthEventPublisher | null {
  return eventPublisher;
}

/**
 * Publish organization created event
 */
export async function publishOrganizationCreated(data: {
  organizationId: string;
  name: string;
  slug: string;
  creatorId: string;
  creatorEmail: string;
  metadata?: Record<string, any>;
}): Promise<void> {
  if (!eventPublisher) {
    console.warn('⚠️ Organization event publisher not available');
    return;
  }

  await eventPublisher.publishOrganizationCreated(data);
}

/**
 * Publish organization member added event
 */
export async function publishOrganizationMemberAdded(data: {
  organizationId: string;
  organizationName: string;
  userId: string;
  userEmail: string;
  role: string;
  invitedBy?: string;
}): Promise<void> {
  if (!eventPublisher) {
    console.warn('⚠️ Organization event publisher not available');
    return;
  }

  await eventPublisher.publishOrganizationMemberAdded(data);
}

/**
 * Publish organization member removed event
 */
export async function publishOrganizationMemberRemoved(data: {
  organizationId: string;
  organizationName: string;
  userId: string;
  userEmail: string;
  removedBy?: string;
}): Promise<void> {
  if (!eventPublisher) {
    console.warn('⚠️ Organization event publisher not available');
    return;
  }

  await eventPublisher.publishOrganizationMemberRemoved(data);
}
