/**
 * The Application half of the agent directory resolver (A-REC-03, F-t1-049): the organization's published agents,
 * for execution-core's `space.directory.resolve` tool.
 *
 * `POST /api/internal/space-agent-directory` lists the active, organization-visible agent definitions of one
 * organization whose name starts with the query, each marked `in_room` when the caller's room has an active binding
 * of it. It is presence, never authority: Control's room roster decides who an agent may act for, and a definition
 * existing here never implies room membership (see spaceAgents.ts).
 *
 * What it never returns: another room's reference, name or binding, a binding's room-local display name, a
 * personal (`visibility: "owner"`) definition, a draft or inactive definition, a system prompt, tools or secrets.
 *
 * The caller is execution-core with its own bearer key, `CONVEX_AGENT_DIRECTORY_KEY` (no other key opens this
 * route, and the route fails closed with 503 while the key is unset). The organization and the room come from the
 * run execution-core holds, never from the model. Nothing is written: a lookup persists nothing, so Zero Data
 * Retention is applied by the caller, which drops names and descriptions for a zero-retention run.
 */

import { v } from "convex/values";
import { httpAction, internalQuery } from "./_generated/server";
import { internal } from "./_generated/api";

/** Rows one lookup returns at most. */
export const AGENT_DIRECTORY_LIMIT = 50;
const MAX_QUERY_CHARS = 100;
const MAX_REF_CHARS = 200;
const MAX_BODY_BYTES = 8 << 10;
const KEY_ENV = "CONVEX_AGENT_DIRECTORY_KEY";

/** One directory entry, in the shape execution-core reads. */
export type DirectoryAgent = {
  agent_ref: string;
  display_name: string;
  description: string;
  in_room: boolean;
};

type AgentRow = {
  _id: string;
  name: string;
  description?: string;
  status: string;
  visibility?: string;
};

type BindingRow = {
  agentId: string;
  spaceRef: string;
  externalOrgId: string;
  status: string;
};

/**
 * The pure projection. `agents` are the organization's definitions, `bindings` the caller room's bindings. Only an
 * active, organization-visible definition whose name starts with the query (case-insensitive) is listed; an exact
 * name sorts first. `in_room` is an active binding of the definition in `spaceRef` of the same organization.
 */
export function projectAgentDirectory(
  agents: readonly AgentRow[],
  bindings: readonly BindingRow[],
  externalOrgId: string,
  spaceRef: string,
  query: string,
): DirectoryAgent[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const inRoom = new Set(
    bindings
      .filter(
        (binding) =>
          binding.status === "active" && binding.spaceRef === spaceRef && binding.externalOrgId === externalOrgId,
      )
      .map((binding) => String(binding.agentId)),
  );
  return agents
    .filter((agent) => agent.status === "active" && agent.visibility !== "owner")
    .filter((agent) => agent.name.trim().toLowerCase().startsWith(needle))
    .map((agent) => ({
      agent_ref: String(agent._id),
      display_name: agent.name.trim(),
      description: (agent.description ?? "").trim(),
      in_room: inRoom.has(String(agent._id)),
    }))
    .sort((left, right) => {
      const leftExact = left.display_name.toLowerCase() === needle ? 0 : 1;
      const rightExact = right.display_name.toLowerCase() === needle ? 0 : 1;
      return leftExact - rightExact || left.display_name.localeCompare(right.display_name);
    })
    .slice(0, AGENT_DIRECTORY_LIMIT);
}

export const listForDirectory = internalQuery({
  args: { externalOrgId: v.string(), spaceRef: v.string(), query: v.string() },
  handler: async (ctx, args): Promise<DirectoryAgent[]> => {
    const organization = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", args.externalOrgId))
      .first();
    if (!organization) return [];
    const agents = await ctx.db
      .query("agents")
      .withIndex("by_org_and_status", (q: any) => q.eq("orgId", organization._id).eq("status", "active"))
      .collect();
    const bindings = await ctx.db
      .query("spaceAgentBindings")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .collect();
    return projectAgentDirectory(
      agents as unknown as AgentRow[],
      bindings as unknown as BindingRow[],
      args.externalOrgId,
      args.spaceRef,
      args.query,
    );
  },
});

async function digest(value: string): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)));
}

function constantTimeEqual(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) difference |= left[index] ^ right[index];
  return difference === 0;
}

/**
 * Whether the request carries execution-core's directory key. Throws while the key is unset (the route answers 503),
 * so an unconfigured deployment is never an open one.
 */
export async function authorizeAgentDirectoryRequest(
  request: Request,
  environment: Record<string, string | undefined> = process.env,
): Promise<boolean> {
  const expected = environment[KEY_ENV];
  if (!expected || expected.length < 32) {
    throw new Error("Convex agent directory key is not configured");
  }
  const match = /^Bearer ([^\s]+)$/.exec(request.headers.get("authorization") || "");
  if (!match) return false;
  const [provided, wanted] = await Promise.all([digest(match[1]), digest(expected)]);
  return constantTimeEqual(provided, wanted);
}

/** The request body: the run's organization and room, and the model's query. */
export type DirectoryRequest = { orgId: string; spaceRef: string; query: string };

/** Parses and bounds the request body; `null` is a 400. Unknown fields are refused. */
export function parseDirectoryRequest(raw: string): DirectoryRequest | null {
  let body: unknown;
  try {
    body = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  const record = body as Record<string, unknown>;
  if (Object.keys(record).some((key) => !["org_id", "space_ref", "query"].includes(key))) return null;
  const { org_id: orgId, space_ref: spaceRef, query } = record;
  if (typeof orgId !== "string" || typeof spaceRef !== "string" || typeof query !== "string") return null;
  const bounded = (value: string, max: number) => value.trim().length > 0 && value.length <= max && !value.includes("\0");
  if (!bounded(orgId, MAX_REF_CHARS) || !bounded(spaceRef, MAX_REF_CHARS) || !bounded(query, MAX_QUERY_CHARS)) {
    return null;
  }
  return { orgId: orgId.trim(), spaceRef: spaceRef.trim(), query: query.trim() };
}

function envelope(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", "Cache-Control": "no-store" },
  });
}

function refusal(status: number, code: string, message: string): Response {
  return envelope(status, { error: { code, message, details: {} } });
}

export const agentDirectory = httpAction(async (ctx, request) => {
  let authorized: boolean;
  try {
    authorized = await authorizeAgentDirectoryRequest(request);
  } catch {
    return refusal(503, "directory_unavailable", "The agent directory is not configured.");
  }
  if (!authorized) return refusal(401, "not_authorized", "The agent directory key is missing or wrong.");
  const raw = await request.text();
  if (raw.length > MAX_BODY_BYTES) return refusal(413, "invalid_request", "The request is too large.");
  const parsed = parseDirectoryRequest(raw);
  if (!parsed) return refusal(400, "invalid_request", "The request body is not a directory lookup.");
  try {
    const agents = await ctx.runQuery(internal.spaceAgentDirectory.listForDirectory, {
      externalOrgId: parsed.orgId,
      spaceRef: parsed.spaceRef,
      query: parsed.query,
    });
    return envelope(200, { data: { agents } });
  } catch {
    return refusal(503, "directory_unavailable", "The agent directory could not be read.");
  }
});
