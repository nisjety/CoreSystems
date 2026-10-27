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

export default crons;
