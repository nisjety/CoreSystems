import { NextResponse } from "next/server";
import { z } from "zod";

import { fail, ok } from "@/lib/api/envelope";
import { isReadIntegrationUnavailable } from "@/lib/integrations/optional-service";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const HEX_COLOR = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

const brandThemeSchema = z.object({
  mode: z.enum(["verevon", "brand"]),
  primaryColor: z.string().regex(HEX_COLOR),
});

type AppearanceSettings = {
  theme?: "light" | "dark" | "system" | "auto";
  colorScheme?: string;
  fontSize?: string;
  compactMode?: boolean;
};

export async function PUT(request: Request) {
  const parsed = brandThemeSchema.safeParse(await request.json().catch(() => null));
  if (!parsed.success) {
    return NextResponse.json(
      fail({ code: "invalid_brand_theme", message: "Brand theme must include mode and a hex color." }),
      { status: 400 },
    );
  }

  try {
    const actor = await requireRequestActor();
    await ensureUserCoreProfile(actor);
    const current = await fetchUserCoreJson<AppearanceSettings>(actor, "/api/v1/settings/appearance").catch(() => ({
      theme: "light",
      colorScheme: "blue",
      fontSize: "medium",
      compactMode: false,
    }));
    await fetchUserCoreJson<AppearanceSettings>(actor, "/api/v1/settings/appearance", {
      method: "PUT",
      body: JSON.stringify({
        theme: current.theme === "system" ? "auto" : current.theme ?? "light",
        colorScheme: parsed.data.mode === "brand" ? parsed.data.primaryColor.toLowerCase() : "blue",
        fontSize: current.fontSize ?? "medium",
        compactMode: current.compactMode ?? false,
      }),
    });
    return NextResponse.json(ok({ persisted: true, ...parsed.data }));
  } catch (error) {
    if (isReadIntegrationUnavailable(error)) {
      return NextResponse.json(ok({ configured: false, persisted: false, ...parsed.data }));
    }
    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(ok({ configured: false, persisted: false, ...parsed.data }));
    }
    return NextResponse.json(
      ok({ configured: false, persisted: false, ...parsed.data }),
    );
  }
}

async function ensureUserCoreProfile(actor: Parameters<typeof fetchUserCoreJson>[0]) {
  await fetchUserCoreJson(actor, "/api/v1/users/me").catch(() => undefined);
}
