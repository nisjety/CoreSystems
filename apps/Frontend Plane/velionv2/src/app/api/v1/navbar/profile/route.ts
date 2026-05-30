import { NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import { isReadIntegrationUnavailable } from "@/lib/integrations/optional-service";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type UserCoreProfileResponse = {
  user?: {
    id: string;
    email?: string;
    name?: string;
    display_name?: string;
    avatar?: string;
    status?: string;
  };
};

export async function GET() {
  try {
    const actor = await requireRequestActor();
    const payload = await fetchUserCoreJson<UserCoreProfileResponse>(actor, "/api/v1/users/me");
    const profile = payload.user;

    return NextResponse.json(
      ok({
        id: profile?.id ?? actor.userId,
        name: profile?.display_name ?? profile?.name ?? actor.name ?? "Account",
        email: profile?.email ?? actor.email,
        avatar: profile?.avatar ?? actor.avatar,
        status: profile?.status ?? "online",
      }),
    );
  } catch (error) {
    if (isReadIntegrationUnavailable(error)) {
      return NextResponse.json(ok(null));
    }

    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(fail({ code: "profile_load_failed", message: "Profile could not be loaded." }), { status: 500 });
  }
}
