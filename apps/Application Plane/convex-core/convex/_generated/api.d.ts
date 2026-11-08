/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type * as agentRuns from "../agentRuns.js";
import type * as agents from "../agents.js";
import type * as ai from "../ai.js";
import type * as authorityProjection from "../authorityProjection.js";
import type * as authz from "../authz.js";
import type * as controlSessions from "../controlSessions.js";
import type * as conversationProjection from "../conversationProjection.js";
import type * as conversations from "../conversations.js";
import type * as crons from "../crons.js";
import type * as http from "../http.js";
import type * as imports from "../imports.js";
import type * as ingest from "../ingest.js";
import type * as internalAuth from "../internalAuth.js";
import type * as knowledgeQnA from "../knowledgeQnA.js";
import type * as membershipProjection from "../membershipProjection.js";
import type * as membershipReconciliation from "../membershipReconciliation.js";
import type * as messages from "../messages.js";
import type * as nats from "../nats.js";
import type * as organizations from "../organizations.js";
import type * as plannerDocuments from "../plannerDocuments.js";
import type * as projects from "../projects.js";
import type * as reconciliation from "../reconciliation.js";
import type * as reconciliationAuth from "../reconciliationAuth.js";
import type * as reconciliationNonces from "../reconciliationNonces.js";
import type * as schema_agentDefinitionVersions from "../schema/agentDefinitionVersions.js";
import type * as schema_agents from "../schema/agents.js";
import type * as schema_chat from "../schema/chat.js";
import type * as schema_conversationProjection from "../schema/conversationProjection.js";
import type * as schema_identity from "../schema/identity.js";
import type * as schema_ingest from "../schema/ingest.js";
import type * as schema_planner from "../schema/planner.js";
import type * as schema_spaceAttention from "../schema/spaceAttention.js";
import type * as schema_spaceAudience from "../schema/spaceAudience.js";
import type * as schema_spaceDeletion from "../schema/spaceDeletion.js";
import type * as schema_spacePins from "../schema/spacePins.js";
import type * as schema_spaceReactions from "../schema/spaceReactions.js";
import type * as schema_spaceRoomNotices from "../schema/spaceRoomNotices.js";
import type * as schema_spaces from "../schema/spaces.js";
import type * as searches from "../searches.js";
import type * as spaceAgents from "../spaceAgents.js";
import type * as spaceAttention from "../spaceAttention.js";
import type * as spaceAttentionCourier from "../spaceAttentionCourier.js";
import type * as spaceAttentionEnvelope from "../spaceAttentionEnvelope.js";
import type * as spaceAttentionIngest from "../spaceAttentionIngest.js";
import type * as spaceAttentionRules from "../spaceAttentionRules.js";
import type * as spaceAudience from "../spaceAudience.js";
import type * as spaceAudiencePlan from "../spaceAudiencePlan.js";
import type * as spaceAudienceReconcile from "../spaceAudienceReconcile.js";
import type * as spaceAudienceRegistration from "../spaceAudienceRegistration.js";
import type * as spaceDeletionAuthorization from "../spaceDeletionAuthorization.js";
import type * as spaceDeletionData from "../spaceDeletionData.js";
import type * as spaceDeletionIngestion from "../spaceDeletionIngestion.js";
import type * as spaceDeletionModel from "../spaceDeletionModel.js";
import type * as spaceDeletionReceiptReconciler from "../spaceDeletionReceiptReconciler.js";
import type * as spaceDeletionReceipts from "../spaceDeletionReceipts.js";
import type * as spaceDmHides from "../spaceDmHides.js";
import type * as spaceLifecycle from "../spaceLifecycle.js";
import type * as spaceMemberStatus from "../spaceMemberStatus.js";
import type * as spaceMemberStatusRules from "../spaceMemberStatusRules.js";
import type * as spaceMembers from "../spaceMembers.js";
import type * as spaceMembershipSync from "../spaceMembershipSync.js";
import type * as spaceParticipants from "../spaceParticipants.js";
import type * as spacePins from "../spacePins.js";
import type * as spacePresence from "../spacePresence.js";
import type * as spaceReactions from "../spaceReactions.js";
import type * as spaceReadMarkers from "../spaceReadMarkers.js";
import type * as spaceRegistration from "../spaceRegistration.js";
import type * as spaceReminders from "../spaceReminders.js";
import type * as spaceRemindersRules from "../spaceRemindersRules.js";
import type * as spaceRoomNotices from "../spaceRoomNotices.js";
import type * as spaceRoomRules from "../spaceRoomRules.js";
import type * as spaceSidebar from "../spaceSidebar.js";
import type * as spaceSidebarRules from "../spaceSidebarRules.js";
import type * as spaces from "../spaces.js";
import type * as users from "../users.js";

import type {
  ApiFromModules,
  FilterApi,
  FunctionReference,
} from "convex/server";

declare const fullApi: ApiFromModules<{
  agentRuns: typeof agentRuns;
  agents: typeof agents;
  ai: typeof ai;
  authorityProjection: typeof authorityProjection;
  authz: typeof authz;
  controlSessions: typeof controlSessions;
  conversationProjection: typeof conversationProjection;
  conversations: typeof conversations;
  crons: typeof crons;
  http: typeof http;
  imports: typeof imports;
  ingest: typeof ingest;
  internalAuth: typeof internalAuth;
  knowledgeQnA: typeof knowledgeQnA;
  membershipProjection: typeof membershipProjection;
  membershipReconciliation: typeof membershipReconciliation;
  messages: typeof messages;
  nats: typeof nats;
  organizations: typeof organizations;
  plannerDocuments: typeof plannerDocuments;
  projects: typeof projects;
  reconciliation: typeof reconciliation;
  reconciliationAuth: typeof reconciliationAuth;
  reconciliationNonces: typeof reconciliationNonces;
  "schema/agentDefinitionVersions": typeof schema_agentDefinitionVersions;
  "schema/agents": typeof schema_agents;
  "schema/chat": typeof schema_chat;
  "schema/conversationProjection": typeof schema_conversationProjection;
  "schema/identity": typeof schema_identity;
  "schema/ingest": typeof schema_ingest;
  "schema/planner": typeof schema_planner;
  "schema/spaceAttention": typeof schema_spaceAttention;
  "schema/spaceAudience": typeof schema_spaceAudience;
  "schema/spaceDeletion": typeof schema_spaceDeletion;
  "schema/spacePins": typeof schema_spacePins;
  "schema/spaceReactions": typeof schema_spaceReactions;
  "schema/spaceRoomNotices": typeof schema_spaceRoomNotices;
  "schema/spaces": typeof schema_spaces;
  searches: typeof searches;
  spaceAgents: typeof spaceAgents;
  spaceAttention: typeof spaceAttention;
  spaceAttentionCourier: typeof spaceAttentionCourier;
  spaceAttentionEnvelope: typeof spaceAttentionEnvelope;
  spaceAttentionIngest: typeof spaceAttentionIngest;
  spaceAttentionRules: typeof spaceAttentionRules;
  spaceAudience: typeof spaceAudience;
  spaceAudiencePlan: typeof spaceAudiencePlan;
  spaceAudienceReconcile: typeof spaceAudienceReconcile;
  spaceAudienceRegistration: typeof spaceAudienceRegistration;
  spaceDeletionAuthorization: typeof spaceDeletionAuthorization;
  spaceDeletionData: typeof spaceDeletionData;
  spaceDeletionIngestion: typeof spaceDeletionIngestion;
  spaceDeletionModel: typeof spaceDeletionModel;
  spaceDeletionReceiptReconciler: typeof spaceDeletionReceiptReconciler;
  spaceDeletionReceipts: typeof spaceDeletionReceipts;
  spaceDmHides: typeof spaceDmHides;
  spaceLifecycle: typeof spaceLifecycle;
  spaceMemberStatus: typeof spaceMemberStatus;
  spaceMemberStatusRules: typeof spaceMemberStatusRules;
  spaceMembers: typeof spaceMembers;
  spaceMembershipSync: typeof spaceMembershipSync;
  spaceParticipants: typeof spaceParticipants;
  spacePins: typeof spacePins;
  spacePresence: typeof spacePresence;
  spaceReactions: typeof spaceReactions;
  spaceReadMarkers: typeof spaceReadMarkers;
  spaceRegistration: typeof spaceRegistration;
  spaceReminders: typeof spaceReminders;
  spaceRemindersRules: typeof spaceRemindersRules;
  spaceRoomNotices: typeof spaceRoomNotices;
  spaceRoomRules: typeof spaceRoomRules;
  spaceSidebar: typeof spaceSidebar;
  spaceSidebarRules: typeof spaceSidebarRules;
  spaces: typeof spaces;
  users: typeof users;
}>;

/**
 * A utility for referencing Convex functions in your app's public API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = api.myModule.myFunction;
 * ```
 */
export declare const api: FilterApi<
  typeof fullApi,
  FunctionReference<any, "public">
>;

/**
 * A utility for referencing Convex functions in your app's internal API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = internal.myModule.myFunction;
 * ```
 */
export declare const internal: FilterApi<
  typeof fullApi,
  FunctionReference<any, "internal">
>;

export declare const components: {};
