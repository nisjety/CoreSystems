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
import type * as searches from "../searches.js";
import type * as spaceAgents from "../spaceAgents.js";
import type * as spaceAudience from "../spaceAudience.js";
import type * as spaceAudienceRegistration from "../spaceAudienceRegistration.js";
import type * as spaceDeletionAuthorization from "../spaceDeletionAuthorization.js";
import type * as spaceDeletionData from "../spaceDeletionData.js";
import type * as spaceDeletionIngestion from "../spaceDeletionIngestion.js";
import type * as spaceDeletionModel from "../spaceDeletionModel.js";
import type * as spaceDeletionReceiptReconciler from "../spaceDeletionReceiptReconciler.js";
import type * as spaceDeletionReceipts from "../spaceDeletionReceipts.js";
import type * as spaceLifecycle from "../spaceLifecycle.js";
import type * as spaceMembershipSync from "../spaceMembershipSync.js";
import type * as spaceRegistration from "../spaceRegistration.js";
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
  searches: typeof searches;
  spaceAgents: typeof spaceAgents;
  spaceAudience: typeof spaceAudience;
  spaceAudienceRegistration: typeof spaceAudienceRegistration;
  spaceDeletionAuthorization: typeof spaceDeletionAuthorization;
  spaceDeletionData: typeof spaceDeletionData;
  spaceDeletionIngestion: typeof spaceDeletionIngestion;
  spaceDeletionModel: typeof spaceDeletionModel;
  spaceDeletionReceiptReconciler: typeof spaceDeletionReceiptReconciler;
  spaceDeletionReceipts: typeof spaceDeletionReceipts;
  spaceLifecycle: typeof spaceLifecycle;
  spaceMembershipSync: typeof spaceMembershipSync;
  spaceRegistration: typeof spaceRegistration;
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
