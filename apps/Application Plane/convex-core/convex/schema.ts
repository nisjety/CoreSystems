/**
 * Convex Schema Definition
 * 
 * This defines the shape of data in the Convex database.
 * TypeScript types are automatically generated from this schema.
 *
 * P-CX1: the table definitions live per feature under `./schema/<feature>.ts`
 * and are composed here. This file only lists the groups.
 */

import { defineSchema } from "convex/server";
import { identityTables } from "./schema/identity";
import { chatTables } from "./schema/chat";
import { plannerTables } from "./schema/planner";
import { agentTables } from "./schema/agents";
import { ingestTables } from "./schema/ingest";
import { spaceTables } from "./schema/spaces";
import { spaceDeletionTables } from "./schema/spaceDeletion";
import { spaceAudienceTables } from "./schema/spaceAudience";
import { spacePinTables } from "./schema/spacePins";
import { spaceAttentionTables } from "./schema/spaceAttention";
import { spaceReactionTables } from "./schema/spaceReactions";
import { conversationProjectionTables } from "./schema/conversationProjection";

export default defineSchema({
  ...identityTables,
  ...chatTables,
  ...plannerTables,
  ...agentTables,
  ...ingestTables,
  ...spaceTables,
  ...spaceDeletionTables,
  ...spaceAudienceTables,
  ...spacePinTables,
  ...spaceAttentionTables,
  ...spaceReactionTables,
  ...conversationProjectionTables,
});
