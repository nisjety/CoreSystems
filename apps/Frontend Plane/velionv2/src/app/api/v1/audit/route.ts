import { NextResponse } from "next/server"
import { fail, ok } from "@/lib/api/envelope"
import {
  RequestActorError,
  requireRequestActor,
} from "@/lib/integrations/request-actor"
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core"
import { AuditCoreError, fetchAuditEvents } from "@/lib/integrations/audit-core"

export const runtime = "nodejs"
export const dynamic = "force-dynamic"

type SessionContext = { orgId?: string | null }

export async function GET(request: Request) {
  try {
    const actor = await requireRequestActor()

    // Derive the org from the authenticated session-context, NOT from a
    // client-supplied param, so a user can only read their own org's audit log.
    const session = await fetchUserCoreJson<SessionContext>(
      actor,
      "/api/v1/me/session-context",
    ).catch((error: unknown) => {
      if (error instanceof UserCoreError) return null
      throw error
    })

    const orgId = session?.orgId
    if (!orgId) {
      return NextResponse.json(ok([]))
    }

    const limitParam = new URL(request.url).searchParams.get("limit")
    const limit = limitParam ? Math.min(Math.max(Number(limitParam) || 0, 1), 100) : 25

    const events = await fetchAuditEvents(actor, orgId, { limit })
    return NextResponse.json(ok(events))
  } catch (error) {
    if (
      error instanceof RequestActorError ||
      error instanceof UserCoreError ||
      error instanceof AuditCoreError
    ) {
      // Tolerable read failures degrade to an empty list rather than erroring
      // the settings page; only auth failures surface as non-200.
      if (error instanceof RequestActorError) {
        return NextResponse.json(
          fail({ code: error.code, message: error.message }),
          { status: error.status },
        )
      }
      return NextResponse.json(ok([]))
    }
    return NextResponse.json(
      fail({ code: "audit_load_failed", message: "Audit log could not be loaded." }),
      { status: 500 },
    )
  }
}
