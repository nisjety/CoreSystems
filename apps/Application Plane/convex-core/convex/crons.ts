/**
 * Scheduled jobs.
 *
 * Every job is safe to run repeatedly and is gated at handler time by its own
 * env flag, so a deployment that has not enabled it pays one no-op action per
 * interval.
 */
import { cronJobs } from "convex/server";
import { internal } from "./_generated/api";

const crons = cronJobs();

// WS04-S1: re-declare every organization room's roster from org-core. Heals a
// lost org-core membership event and a lost debounced run.
crons.interval(
  "space org-room roster reconcile",
  { minutes: 10 },
  internal.spaceMembershipSync.syncAllOrganizationRooms,
  {},
);

// WS04-S5: keep every shared Space's recipient audience equal to Control's
// roster. Heals a missed trigger and retries `waiting` and `projection_lag`.
crons.interval(
  "space audience reconcile",
  { minutes: 10 },
  internal.spaceAudienceReconcile.reconcileAllRoomAudiences,
  {},
);

// WS16-S11: delete sidebar-preference rows whose owner left the organization
// (or whose organization was deleted). Housekeeping only: such rows are already
// inert because `requireGatewayMember` refuses their owner.
crons.interval(
  "space sidebar orphan sweep",
  { hours: 6 },
  internal.spaceSidebar.sweepOrphans,
  {},
);

// WS11-S6: expire attention items and drop old dedupe rows. Deletes only, so it
// is safe whether or not SPACE_ATTENTION_INGEST is on.
crons.interval(
  "space attention sweep",
  { hours: 1 },
  internal.spaceAttentionIngest.sweep,
  {},
);

export default crons;
