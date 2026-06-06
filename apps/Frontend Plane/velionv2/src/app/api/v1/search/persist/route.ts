import { type NextRequest, NextResponse } from "next/server";

import { ok, fail } from "@/lib/api/envelope";
import {
  requireSession,
  authErrorResponse,
} from "@/app/api/_lib/control-plane-auth";
import { resolveActiveOrgId } from "@/app/api/onboarding/_lib/onboarding-proxy";
import { convexMutation } from "@/app/api/_lib/convex-client";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

// BFF — POST /api/v1/search/persist
//
// Persists a completed AI search to Convex (convex-core `searches.*`). The
// browser calls this after a turn-0 answer (or a follow-up) finishes; the
// write itself happens here, server-side, with the service key — the browser
// never holds it. Org/user are server-asserted from the Better Auth session,
// so a client cannot persist into another tenant.
//
// Body (turn 0):     { query, answer, citations }            -> createThread
// Body (follow-up):  { threadId, role, text, citations? }    -> appendTurn

type RawCitation = { url?: unknown; title?: unknown };
type CleanCitation = { url: string; title?: string | null };

const MAX_CITATIONS = 50;
const MAX_TEXT = 100_000;

function isSameOriginRequest(request: Request): boolean {
  const origin = request.headers.get("origin");
  const host = request.headers.get("host");
  if (!origin || !host) return true;
  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

/**
 * Convex validators are strict: citations must be EXACTLY { url, title? } with
 * title a string|null. Strip everything else (snippet, favicon, score, …) so
 * the mutation isn't rejected for unexpected fields.
 */
function cleanCitations(input: unknown): CleanCitation[] {
  if (!Array.isArray(input)) return [];
  const out: CleanCitation[] = [];
  for (const raw of input as RawCitation[]) {
    if (!raw || typeof raw !== "object") continue;
    const url = typeof raw.url === "string" ? raw.url.slice(0, 2048) : "";
    if (!url) continue;
    const title =
      typeof raw.title === "string"
        ? raw.title.slice(0, 1024)
        : raw.title === null
          ? null
          : undefined;
    out.push(title === undefined ? { url } : { url, title });
    if (out.length >= MAX_CITATIONS) break;
  }
  return out;
}

export async function POST(request: NextRequest) {
  if (!isSameOriginRequest(request)) {
    return NextResponse.json(
      fail({ code: "invalid_origin", message: "Request origin is not allowed." }),
      { status: 403 },
    );
  }

  try {
    const session = await requireSession(request);
    const externalUserId = session.user.id;
    const externalOrgId = await resolveActiveOrgId(request, session);

    // No active org → degrade gracefully. Search history is org-scoped; without
    // an org there's nowhere to file it, but the search itself must not break.
    if (!externalOrgId) {
      return NextResponse.json(ok({ persisted: false, reason: "no_active_org" }), {
        status: 200,
      });
    }

    const body = (await request.json().catch(() => null)) as Record<
      string,
      unknown
    > | null;
    if (!body) {
      return NextResponse.json(
        fail({ code: "invalid_body", message: "A JSON body is required." }),
        { status: 400 },
      );
    }

    const threadId = typeof body.threadId === "string" ? body.threadId : "";

    // ---- Follow-up turn -----------------------------------------------------
    if (threadId) {
      const role = body.role === "assistant" ? "assistant" : "user";
      const text = typeof body.text === "string" ? body.text.slice(0, MAX_TEXT) : "";
      if (!text.trim()) {
        return NextResponse.json(
          fail({ code: "invalid_text", message: "A non-empty `text` is required." }),
          { status: 400 },
        );
      }
      const citations = cleanCitations(body.citations);
      const turnId = await convexMutation<string>("searches:appendTurn", {
        threadId,
        role,
        text,
        ...(citations.length ? { citations } : {}),
      });
      return NextResponse.json(ok({ persisted: true, threadId, turnId }), {
        status: 200,
      });
    }

    // ---- Turn 0: create thread ---------------------------------------------
    const query = typeof body.query === "string" ? body.query.slice(0, MAX_TEXT) : "";
    if (!query.trim()) {
      return NextResponse.json(
        fail({ code: "invalid_query", message: "A non-empty `query` is required." }),
        { status: 400 },
      );
    }
    const answer = typeof body.answer === "string" ? body.answer.slice(0, MAX_TEXT) : "";
    const citations = cleanCitations(body.citations);

    const newThreadId = await convexMutation<string>("searches:createThread", {
      externalOrgId,
      externalUserId,
      query,
      answer,
      citations,
    });

    return NextResponse.json(ok({ persisted: true, threadId: newThreadId }), {
      status: 200,
    });
  } catch (error) {
    // Persistence is best-effort — never surface a 500 that would look like the
    // search failed. Auth errors keep their status; everything else is a soft
    // "not persisted" so the UI can ignore it.
    if (error && typeof error === "object" && "status" in error) {
      return authErrorResponse(error);
    }
    console.error("[search/persist] convex write failed:", error);
    return NextResponse.json(
      ok({ persisted: false, reason: "write_failed" }),
      { status: 200 },
    );
  }
}
