/**
 * Organization Events Plugin
 *
 * Publishes organization lifecycle events to NATS when organizations are created,
 * members are added, or members are removed.
 */

import type { BetterAuthPlugin } from 'better-auth';
import { AuthEventPublisher } from '../internal/auth-event.publisher';

let eventPublisher: AuthEventPublisher | null = null;

export function setOrganizationEventPublisher(publisher: AuthEventPublisher) {
  console.log(
    '🔧 Setting organization event publisher for plugin:',
    !!publisher,
  );
  eventPublisher = publisher;
}

export function organizationEventsPlugin(): BetterAuthPlugin {
  return {
    id: 'organization-events',
    init() {
      console.log('🎉 Organization Events Plugin initialized');

      return {
        options: {
          databaseHooks: {
            organization: {
              create: {
                after: async (organization) => {
                  console.log(
                    '🎊 Organization created hook triggered:',
                    organization.name,
                  );

                  if (!eventPublisher) {
                    console.warn(
                      '⚠️ Organization event publisher not available',
                    );
                    return;
                  }

                  try {
                    // Find the creator from the members
                    const creatorMember = organization.members?.[0];

                    await eventPublisher.publishOrganizationCreated({
                      organizationId: organization.id,
                      name: organization.name,
                      slug: organization.slug,
                      creatorId: creatorMember?.userId || 'unknown',
                      creatorEmail: '', // We don't have email in this context
                      metadata: organization.metadata || {},
                    });

                    console.log(
                      '📢 Published organization.created event:',
                      organization.id,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish organization created event:',
                      error,
                    );
                  }
                },
              },
            },
            member: {
              create: {
                after: async (member) => {
                  console.log(
                    '👤 Member added hook triggered for org:',
                    member.organizationId,
                  );

                  if (!eventPublisher) {
                    console.warn(
                      '⚠️ Organization event publisher not available',
                    );
                    return;
                  }

                  try {
                    await eventPublisher.publishOrganizationMemberAdded({
                      organizationId: member.organizationId,
                      organizationName: '', // We don't have this in this context
                      userId: member.userId,
                      userEmail: '', // We don't have this in this context
                      role: member.role,
                      invitedBy: undefined,
                    });

                    console.log(
                      '📢 Published member_added event for user:',
                      member.userId,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish member added event:',
                      error,
                    );
                  }
                },
              },
              delete: {
                after: async (member) => {
                  console.log(
                    '👋 Member removed hook triggered for org:',
                    member.organizationId,
                  );

                  if (!eventPublisher) {
                    console.warn(
                      '⚠️ Organization event publisher not available',
                    );
                    return;
                  }

                  try {
                    await eventPublisher.publishOrganizationMemberRemoved({
                      organizationId: member.organizationId,
                      organizationName: '', // We don't have this in this context
                      userId: member.userId,
                      userEmail: '', // We don't have this in this context
                      removedBy: undefined,
                    });

                    console.log(
                      '📢 Published member_removed event for user:',
                      member.userId,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish member removed event:',
                      error,
                    );
                  }
                },
              },
            },
          },
        },
      } as any;
    },
  };
}
