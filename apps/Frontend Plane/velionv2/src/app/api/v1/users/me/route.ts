import { NextResponse } from "next/server"
import { fail, ok } from "@/lib/api/envelope"
import { isReadIntegrationUnavailable } from "@/lib/integrations/optional-service"
import {
  RequestActorError,
  requireRequestActor,
} from "@/lib/integrations/request-actor"
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core"

export const runtime = "nodejs"
export const dynamic = "force-dynamic"

type UserCoreUserResponse = {
  user?: {
    id: string
    email?: string
    name?: string
    display_name?: string
    avatar?: string
    status?: string
    metadata?: Record<string, unknown>
  }
}

type UpdateProfileBody = {
  name?: string
  display_name?: string
  avatar?: string
  metadata?: Record<string, unknown>
}

function isSameOriginRequest(request: Request) {
  const origin = request.headers.get("origin")
  const host = request.headers.get("host")
  if (!origin || !host) return true
  try {
    return new URL(origin).host === host
  } catch {
    return false
  }
}

export async function GET() {
  try {
    const actor = await requireRequestActor()
    const payload = await fetchUserCoreJson<UserCoreUserResponse>(
      actor,
      "/api/v1/users/me",
    )
    return NextResponse.json(ok(payload.user ?? null))
  } catch (error) {
    if (isReadIntegrationUnavailable(error)) {
      return NextResponse.json(ok(null))
    }
    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      )
    }
    return NextResponse.json(
      fail({ code: "profile_load_failed", message: "Profile could not be loaded." }),
      { status: 500 },
    )
  }
}

export async function PATCH(request: Request) {
  if (!isSameOriginRequest(request)) {
    return NextResponse.json(
      fail({ code: "invalid_origin", message: "Request origin is not allowed." }),
      { status: 403 },
    )
  }

  try {
    const actor = await requireRequestActor()
    const body = (await request.json().catch(() => ({}))) as UpdateProfileBody
    const payload = await fetchUserCoreJson<UserCoreUserResponse>(
      actor,
      "/api/v1/users/me",
      {
        method: "PATCH",
        body: JSON.stringify(body),
      },
    )
    return NextResponse.json(ok(payload.user ?? payload))
  } catch (error) {
    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      )
    }
    return NextResponse.json(
      fail({ code: "profile_update_failed", message: "Profile could not be updated." }),
      { status: 500 },
    )
  }
}
