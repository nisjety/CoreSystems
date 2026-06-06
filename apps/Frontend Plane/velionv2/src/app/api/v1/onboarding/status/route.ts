import { after, NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import {
  AuthGateError,
  getAuthGateState,
  markCurrentUserOnboardingComplete,
} from "@/lib/auth/onboarding-access";
import { sendOnboardingCompletedNotification } from "@/lib/integrations/notification-core";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import { UserCoreError } from "@/lib/integrations/user-core";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type CompletionMetadata = {
  selected_theme?: unknown;
  websites?: unknown;
  connectors?: unknown;
  recommendation?: unknown;
};

function isSameOriginRequest(request: Request) {
  const origin = request.headers.get("origin");
  const host = request.headers.get("host");

  if (!origin || !host) {
    return true;
  }

  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

export async function GET() {
  try {
    const gate = await getAuthGateState();

    if (!gate.user) {
      return NextResponse.json(
        fail({ code: "authentication_required", message: "Sign in is required." }),
        { status: 401 },
      );
    }

    return NextResponse.json(ok({
      onboardingComplete: gate.onboardingComplete,
      onboardingStatus: gate.onboardingStatus,
      source: gate.source,
    }));
  } catch (error) {
    if (error instanceof UserCoreError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }

    return NextResponse.json(
      fail({ code: "onboarding_status_failed", message: "Onboarding status could not be loaded." }),
      { status: 500 },
    );
  }
}

export async function POST(request: Request) {
  if (!isSameOriginRequest(request)) {
    return NextResponse.json(fail({ code: "invalid_origin", message: "Request origin is not allowed." }), { status: 403 });
  }

  try {
    const actor = await requireRequestActor();
    const body = (await request.json().catch(() => null)) as
      | { orgId?: string; plan?: string; source?: string; metadata?: CompletionMetadata }
      | null;
    const result = await markCurrentUserOnboardingComplete(body?.metadata ? { metadata: body.metadata } : undefined);
    after(() =>
      sendOnboardingCompletedNotification(actor, {
        orgId: body?.orgId,
        plan: body?.plan,
        source: body?.source,
        metadata: body?.metadata,
      }).catch(() => undefined),
    );
    return NextResponse.json(ok(result), { status: 200 });
  } catch (error) {
    if (error instanceof AuthGateError || error instanceof RequestActorError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }

    return NextResponse.json(
      fail({ code: "onboarding_completion_failed", message: "Onboarding completion could not be saved." }),
      { status: 500 },
    );
  }
}
