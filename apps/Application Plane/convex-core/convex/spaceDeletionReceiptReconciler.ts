/** Deadline reconciler for the Space deletion receipt aggregate. It makes an
 * unanswered owner explicitly unknown; it never creates a success receipt. */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

export const reconcileOne = internalAction({
  args: { requestId: v.string() },
  handler: async (ctx, args) => ctx.runMutation(internal.spaces.markExpiredDeletionOwnerReceiptsUnknown, {
    requestId: args.requestId,
    now: Date.now(),
  }),
});
