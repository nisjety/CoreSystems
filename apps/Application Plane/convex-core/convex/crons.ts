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

export default crons;
