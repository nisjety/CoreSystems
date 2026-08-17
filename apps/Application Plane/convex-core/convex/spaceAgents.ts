import { v } from "convex/values";
import { action, internalMutation, internalQuery, mutation, query } from "./_generated/server";
import { internal } from "./_generated/api";
import { assertServiceKey, requireGatewayMember } from "./authz";

/**
 * Space agent bindings — the Application half of
 * `docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` §3.2.
 *
 * # What this module is, and what it deliberately is not
 *
 * A binding says "this agent definition is presented as a participant of this
 * Space". It does NOT say the agent may act there. Control owns that, as a
 * `space_memberships` row with `subject_type='service'`, and the gateway
 * intersects the two: Control decides membership, this projection supplies the
 * identity to render.
 *
 * That split is the whole point of the plan. A definition existing in the
 * `agents` table must never imply room membership — the plan calls that out
 * explicitly as the false promise to avoid ("a global chatbot blueprint may
 * appear to be a member of a Space without a server binding"). So nothing here
 * grants anything; a row is presentation and policy only.
 *
 * # No client-authored writes
 *
 * The write below is an `internalMutation` on purpose. Binding an agent to a
 * room is a governed decision that the plan puts behind Control authority and
 * an owner-plane contract (Phase UI-3), so it is reachable from server code
 * only. The browser cannot reach it, and the read path never trusts it alone.
 */

const MAX_DELIVERY_TARGETS = 20;

/** Delivery channels an operator can publish for a bound agent. */
const deliveryTargetValidator = v.object({
  channel: v.union(v.literal("teams"), v.literal("messenger"), v.literal("embed")),
  label: v.string(),
  status: v.union(v.literal("active"), v.literal("pending"), v.literal("failed")),
});

/**
 * The Space's agent bindings, for the gateway to intersect with Control's
 * authoritative roster.
 *
 * Returns bindings for the Space regardless of their own `status`, because
 * "paused" and "failed" are states the room should be able to see truthfully —
 * the plan asks for pending/paused/failed to be shown rather than hidden. Only
 * `revoked` is withheld: a revoked binding is not a participant, and rendering
 * it would reopen the exact "looks like a member but isn't" failure the whole
 * split exists to prevent.
 */
export const spaceAgentBindingsForGateway = query({
  args: {
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);

    const bindings = await ctx.db
      .query("spaceAgentBindings")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .collect();

    const visible = bindings.filter(
      (binding: any) =>
        binding.status !== "revoked" && binding.externalOrgId === args.externalOrgId,
    );

    const projected = [];
    for (const binding of visible) {
      // Read the definition for identity. A binding whose definition is gone
      // is dropped rather than rendered under a placeholder name: an agent card
      // with no real definition behind it is precisely the ghost participant
      // this design refuses to draw.
      const definition = await ctx.db.get(binding.agentId);
      if (!definition || (definition as any).orgId === undefined) continue;

      projected.push({
        bindingRef: binding.bindingRef,
        agentRef: binding.agentId,
        subjectId: binding.subjectId,
        name: binding.displayName ?? (definition as any).name,
        title: binding.title,
        description: (definition as any).description,
        avatarColor: (definition as any).avatarColor,
        status: binding.status,
        // Definition-level lifecycle, kept separate from the binding's own
        // status: an active binding onto a draft definition is a real and
        // confusing state, and flattening the two would hide it.
        definitionStatus: (definition as any).status,
        deliveryTargets: binding.deliveryTargets ?? [],
        // Binding policy, absent on legacy bindings. The gateway enforces
        // these at invocation; here they are projection only.
        triggerModes: binding.triggerModes,
        allowedTools: binding.allowedTools,
        approvalMode: binding.approvalMode,
        projectionVersion: binding.projectionVersion,
        updatedAt: binding.updatedAt,
      });
    }

    return projected;
  },
});

/**
 * The mentioned agent's own voice, for the gateway to inject into a turn it
 * has already authorized (`docs/space-defenition.md`, "Invocation rule").
 *
 * Deliberately separate from `spaceAgentBindingsForGateway`: that projection
 * is presentation and is safe to hand to the browser for every row it
 * returns. `systemPrompt` never should be — an agent's instructions are the
 * owner's authoring, not room-public information — so this returns it only
 * for the one already-authorized agent the gateway names, and the gateway
 * never forwards it to the browser; it goes straight into the model turn.
 */
export const agentPersonaForGateway = query({
  args: {
    externalOrgId: v.string(),
    agentId: v.id("agents"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);

    const organizations = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", args.externalOrgId))
      .collect();
    const organization = organizations.find((candidate: any) => candidate.syncStatus !== "deleted");
    if (!organization) return null;

    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== organization._id) return null;

    return { name: agent.name, systemPrompt: agent.systemPrompt ?? null };
  },
});

/**
 * Create or update one binding, idempotently on `(spaceRef, subjectId)`.
 *
 * Keyed on the Control subject rather than on the agent definition, because the
 * subject is what Control's roster reports — matching on it is what lets the
 * gateway line a binding up with an authoritative membership row instead of
 * guessing by name.
 */
export const upsertSpaceAgentBinding = internalMutation({
  args: {
    spaceRef: v.string(),
    externalOrgId: v.string(),
    agentId: v.id("agents"),
    subjectId: v.string(),
    displayName: v.optional(v.string()),
    title: v.optional(v.string()),
    status: v.union(
      v.literal("pending"),
      v.literal("active"),
      v.literal("paused"),
      v.literal("revoked"),
      v.literal("failed"),
    ),
    deliveryTargets: v.optional(v.array(deliveryTargetValidator)),
    triggerModes: v.optional(v.array(v.union(v.literal("mention"), v.literal("group")))),
    allowedTools: v.optional(v.array(v.string())),
    approvalMode: v.optional(
      v.union(v.literal("auto"), v.literal("require_confirmation"), v.literal("blocked")),
    ),
  },
  handler: async (ctx, args) => {
    const subjectId = args.subjectId.trim();
    if (!subjectId) throw new Error("A Control subject id is required");
    if ((args.deliveryTargets?.length ?? 0) > MAX_DELIVERY_TARGETS) {
      throw new Error("Too many delivery targets");
    }

    const definition = await ctx.db.get(args.agentId);
    if (!definition) throw new Error("Agent definition not found");

    const existing = await ctx.db
      .query("spaceAgentBindings")
      .withIndex("by_space_and_subject", (q: any) =>
        q.eq("spaceRef", args.spaceRef).eq("subjectId", subjectId),
      )
      .unique();

    const now = Date.now();
    if (existing) {
      await ctx.db.patch(existing._id, {
        agentId: args.agentId,
        displayName: args.displayName,
        title: args.title,
        status: args.status,
        deliveryTargets: args.deliveryTargets,
        ...(args.triggerModes !== undefined ? { triggerModes: args.triggerModes } : {}),
        ...(args.allowedTools !== undefined ? { allowedTools: args.allowedTools } : {}),
        ...(args.approvalMode !== undefined ? { approvalMode: args.approvalMode } : {}),
        projectionVersion: existing.projectionVersion + 1,
        updatedAt: now,
      });
      return { bindingRef: existing.bindingRef, created: false };
    }

    const bindingRef = `sab_${args.spaceRef}_${subjectId}`;
    await ctx.db.insert("spaceAgentBindings", {
      bindingRef,
      spaceRef: args.spaceRef,
      externalOrgId: args.externalOrgId,
      agentId: args.agentId,
      subjectId,
      displayName: args.displayName,
      title: args.title,
      status: args.status,
      deliveryTargets: args.deliveryTargets,
      triggerModes: args.triggerModes,
      allowedTools: args.allowedTools,
      approvalMode: args.approvalMode,
      projectionVersion: 1,
      createdAt: now,
      updatedAt: now,
    });
    return { bindingRef, created: true };
  },
});

const MAX_AGENT_NAME_LENGTH = 60;
const MAX_AGENT_INSTRUCTIONS_LENGTH = 4000;

/**
 * The in-room create flow (`SPACE_AGENT_SCOPE_PLAN` §UI-3b), step 1 of 2.
 *
 * Creates the definition and its binding in ONE transaction, with the binding
 * `pending`: the agent exists and is visible in the room truthfully as
 * not-yet-a-member, but nothing may invoke it — the mention list and the
 * gateway's invocation authorization both require `active`, which only step 2
 * (`confirmSpaceAgentMembershipForGateway`) grants after Control has accepted
 * the service membership. The creating human's identity is the authorizing
 * decision here (QM rule: authority for future agent behavior comes from
 * outside the agent); the gateway verifies their room role before calling.
 */
export const createSpaceAgentForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    name: v.string(),
    instructions: v.optional(v.string()),
    avatarColor: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);

    const name = args.name.trim();
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");
    if (name.length < 2 || name.length > MAX_AGENT_NAME_LENGTH) {
      throw new Error("Agent name must be between 2 and 60 characters");
    }
    const instructions = args.instructions?.trim() || undefined;
    if ((instructions?.length ?? 0) > MAX_AGENT_INSTRUCTIONS_LENGTH) {
      throw new Error("Agent instructions are too long");
    }

    const now = Date.now();
    const agentId = await ctx.db.insert("agents", {
      orgId: organization._id,
      name,
      avatarColor: args.avatarColor?.trim() || undefined,
      useCase: "other",
      status: "active",
      // Room agents answer through the room's own stream; the definition's
      // model field records the routing intent the rest of Agent Studio uses.
      model: "Verevon Balance",
      systemPrompt: instructions,
      createdAt: now,
      updatedAt: now,
    });

    // The Control subject is derived from the definition id: unique by
    // construction, and the roster join keys on it — never on the name.
    const subjectId = `agent-${agentId}`;
    const bindingRef = `sab_${spaceRef}_${subjectId}`;
    await ctx.db.insert("spaceAgentBindings", {
      bindingRef,
      spaceRef,
      externalOrgId: args.externalOrgId,
      agentId,
      subjectId,
      status: "pending",
      // Born-with policy per space-defenition.md "Creating agents": room-born
      // agents answer only when mentioned, run nothing autonomously, and get
      // no tool surface. Anything more is a deliberate Agent-page grant.
      triggerModes: ["mention"],
      allowedTools: [],
      approvalMode: "require_confirmation",
      projectionVersion: 1,
      createdAt: now,
      updatedAt: now,
    });

    return { agentRef: agentId, subjectId, bindingRef };
  },
});

/**
 * Definitions an owner/manager can browse to bind an EXISTING agent to this
 * room (`SPACE_AGENT_SCOPE_PLAN` §UI-3, distinct from §UI-3b's "create new").
 * Each row reports whether it already has a non-revoked binding here, so the
 * UI can say "already added" truthfully instead of hiding it — this project's
 * standing rule against a state that looks like absence when it is really a
 * fact the caller chose not to show.
 */
export const listInstallableSpaceAgentsForGateway = query({
  args: {
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);

    const definitions = await ctx.db
      .query("agents")
      .withIndex("by_org", (q: any) => q.eq("orgId", organization._id))
      .collect();
    const bindings = await ctx.db
      .query("spaceAgentBindings")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .collect();
    const boundAgentIds = new Set(
      bindings
        .filter((binding: any) => binding.status !== "revoked")
        .map((binding: any) => binding.agentId.toString()),
    );

    return definitions.map((definition: any) => ({
      agentRef: definition._id,
      name: definition.name,
      description: definition.description,
      definitionStatus: definition.status,
      alreadyBound: boundAgentIds.has(definition._id.toString()),
    }));
  },
});

/**
 * Bind an EXISTING agent definition to this room (`SPACE_AGENT_SCOPE_PLAN`
 * §UI-3), the same two-step, `pending`-until-confirmed contract as creation —
 * only step 1 differs (an existing definition instead of a new one). The
 * gateway resolves and checks the caller's room role BEFORE calling this, so
 * this mutation trusts the room-authorization decision the same way
 * `createSpaceAgentForGateway` does; it does not re-derive Control's roster.
 */
export const bindExistingSpaceAgentForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    agentId: v.id("agents"),
  },
  handler: async (ctx, args): Promise<{ subjectId: string; bindingRef: string }> => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);

    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");
    const definition = await ctx.db.get(args.agentId);
    if (!definition || (definition as any).orgId !== organization._id) {
      throw new Error("Agent definition not found");
    }

    const subjectId = `agent-${args.agentId}`;
    const result: { bindingRef: string; created: boolean } = await ctx.runMutation(
      internal.spaceAgents.upsertSpaceAgentBinding,
      {
        spaceRef,
        externalOrgId: args.externalOrgId,
        agentId: args.agentId,
        subjectId,
        status: "pending",
        // Same born-with policy as the create flow: this room only,
        // mention-only, no tools, confirmation required. An existing
        // definition's own (possibly broader) Agent Studio configuration is
        // never inherited into a room binding automatically.
        triggerModes: ["mention"],
        allowedTools: [],
        approvalMode: "require_confirmation",
      },
    );
    return { subjectId, bindingRef: result.bindingRef };
  },
});

/** Every subject the room's service roster must contain — the full declarative
 * set for Control convergence, not a delta. Revoked bindings are the one state
 * excluded on purpose: revocation is exactly the decision to leave the roster. */
export const serviceMembersForSpace = internalQuery({
  args: { spaceRef: v.string(), externalOrgId: v.string() },
  handler: async (ctx, args) => {
    const bindings = await ctx.db
      .query("spaceAgentBindings")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .collect();
    return bindings
      .filter(
        (binding: any) =>
          binding.status !== "revoked" && binding.externalOrgId === args.externalOrgId,
      )
      .map((binding: any) => ({ subjectId: binding.subjectId as string }));
  },
});

export const markSpaceBindingsActive = internalMutation({
  args: { spaceRef: v.string(), externalOrgId: v.string() },
  handler: async (ctx, args) => {
    const bindings = await ctx.db
      .query("spaceAgentBindings")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .collect();
    const now = Date.now();
    for (const binding of bindings) {
      if (binding.status !== "pending" || binding.externalOrgId !== args.externalOrgId) continue;
      await ctx.db.patch(binding._id, {
        status: "active",
        projectionVersion: binding.projectionVersion + 1,
        updatedAt: now,
      });
    }
  },
});

/**
 * Step 2 of the in-room create flow: declare the room's full service roster to
 * Control (`managed_subject_types: ["service"]`, the counterpart of the human
 * sync's `["user"]` — each flow converges only the kind it owns). Only after
 * Control accepts does the pending binding become `active`; a rejected or
 * unreachable Control leaves it `pending`, which the room renders truthfully
 * as not-yet-a-member.
 */
export const confirmSpaceAgentMembershipForGateway = action({
  args: {
    serviceKey: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args): Promise<{ status: string; httpStatus?: number }> => {
    assertServiceKey(args.serviceKey);

    const membershipBaseUrl = (process.env.CONTROL_SPACE_MEMBERSHIP_BASE_URL || "").trim();
    const lifecycleToken = (process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "").trim();
    if (!membershipBaseUrl || !lifecycleToken) {
      return { status: "not_configured" };
    }

    const members: { subjectId: string }[] = await ctx.runQuery(
      internal.spaceAgents.serviceMembersForSpace,
      { spaceRef: args.spaceRef, externalOrgId: args.externalOrgId },
    );

    let applied: Response;
    try {
      applied = await fetch(
        `${membershipBaseUrl.replace(/\/$/, "")}/${encodeURIComponent(args.spaceRef)}/memberships`,
        {
          method: "PUT",
          headers: {
            "Content-Type": "application/json",
            "X-Service-Id": "application-space-lifecycle",
            "X-Service-Token": lifecycleToken,
          },
          body: JSON.stringify({
            members: members.map((member) => ({
              subject_type: "service",
              subject_id: member.subjectId,
              role: "editor",
            })),
            managed_subject_types: ["service"],
          }),
        },
      );
    } catch {
      return { status: "control_unavailable" };
    }
    if (!applied.ok) {
      return { status: "rejected", httpStatus: applied.status };
    }

    await ctx.runMutation(internal.spaceAgents.markSpaceBindingsActive, {
      spaceRef: args.spaceRef,
      externalOrgId: args.externalOrgId,
    });
    return { status: "applied" };
  },
});
