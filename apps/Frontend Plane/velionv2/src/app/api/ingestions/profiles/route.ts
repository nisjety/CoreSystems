import { NextResponse, type NextRequest } from "next/server";
import { authErrorResponse } from "@/app/api/_lib/control-plane-auth";
import { fetchQuarry } from "@/app/api/ingestions/_lib/quarry-ingestions";
import { ok } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type QuarryProfileList = {
  profiles: string[];
};

type QuarryRestoreProbe = {
  profile_id: string;
  restorable: boolean;
  cookies_count: number;
  local_storage_count: number;
  session_storage_count: number;
  indexed_db_count: number;
  has_user_agent: boolean;
  has_viewport: boolean;
  locale?: string | null;
  timezone?: string | null;
};

export async function GET(request: NextRequest) {
  try {
    const profiles = await fetchQuarry<QuarryProfileList>(request, "/v1/profiles");
    const details = await Promise.all(
      profiles.profiles.slice(0, 10).map(async (profileId) => {
        const probe = await fetchQuarry<QuarryRestoreProbe>(
          request,
          `/v1/profiles/${profileId}/restore_probe`,
          {
            method: "POST",
            body: { url: "https://example.com" },
          },
        ).catch(() => null);

        return {
          id: profileId,
          restorable: probe?.restorable ?? false,
          cookies: probe?.cookies_count ?? 0,
          storage:
            (probe?.local_storage_count ?? 0) +
            (probe?.session_storage_count ?? 0) +
            (probe?.indexed_db_count ?? 0),
          locale: probe?.locale ?? null,
          timezone: probe?.timezone ?? null,
        };
      }),
    );

    return NextResponse.json(ok({ profiles: details }));
  } catch (error) {
    return authErrorResponse(error);
  }
}
