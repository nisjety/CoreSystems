import { NextResponse } from "next/server";
import { z } from "zod";
import { fail, ok } from "@/lib/api/envelope";
import { isReadIntegrationUnavailable } from "@/lib/integrations/optional-service";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const themeSchema = z.object({
  theme: z.enum(["light", "dark", "system"]),
});

type AppearanceSettings = {
  theme?: "light" | "dark" | "system" | "auto";
  colorScheme?: string;
  fontSize?: string;
  compactMode?: boolean;
};

function normalizeTheme(theme: AppearanceSettings["theme"]) {
  return theme === "auto" ? "system" : theme ?? "system";
}

export async function GET() {
  try {
    const actor = await requireRequestActor();
    const settings = await fetchUserCoreJson<AppearanceSettings>(actor, "/api/v1/settings/appearance");
    return NextResponse.json(ok({ theme: normalizeTheme(settings.theme) }));
  } catch (error) {
    if (isReadIntegrationUnavailable(error)) {
      return NextResponse.json(ok({ configured: false, theme: "system" }));
    }

    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(fail({ code: "theme_load_failed", message: "Theme could not be loaded." }), { status: 500 });
  }
}

export async function PUT(request: Request) {
  const parsed = themeSchema.safeParse(await request.json().catch(() => null));

  if (!parsed.success) {
    return NextResponse.json(
      fail({
        code: "invalid_theme",
        message: "Theme must be light, dark, or system.",
      }),
      { status: 400 },
    );
  }

  try {
    const actor = await requireRequestActor();
    const themeForUserCore = parsed.data.theme === "system" ? "auto" : parsed.data.theme;
    await fetchUserCoreJson<AppearanceSettings>(actor, "/api/v1/settings/appearance", {
      method: "PUT",
      body: JSON.stringify({
        theme: themeForUserCore,
        colorScheme: "blue",
        fontSize: "medium",
        compactMode: false,
      }),
    });
    return NextResponse.json(ok({ theme: parsed.data.theme }));
  } catch (error) {
    if (isReadIntegrationUnavailable(error)) {
      return NextResponse.json(ok({ configured: false, persisted: false, theme: parsed.data.theme }));
    }

    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(fail({ code: "theme_save_failed", message: "Theme could not be saved." }), { status: 500 });
  }
}
