import { NextResponse } from "next/server";
import { ok } from "@/lib/api/envelope";
import { isControlPlaneAuthConfigured } from "@/lib/auth/control-plane";
import { authRuntimePlan, getAuthBaseUrl, getTrustedOrigins } from "@/lib/auth/runtime";
import { hasAuthDatabaseConfig } from "@/lib/auth/database";

export const runtime = "nodejs";

export async function GET() {
  const controlPlaneConfigured = isControlPlaneAuthConfigured();

  return NextResponse.json(
    ok({
      mode: controlPlaneConfigured ? "control-plane" : "standalone",
      baseUrl: getAuthBaseUrl(),
      trustedOrigins: getTrustedOrigins(),
      betterAuthRoute: "/api/auth/[...all]",
      controlPlaneAuthConfigured: controlPlaneConfigured,
      localDatabaseConfigured: hasAuthDatabaseConfig(),
      security: authRuntimePlan,
      requiredProductionEnv: [
        "CONTROL_PLANE_AUTH_URL or AUTH_CORE_URL",
        "USER_CORE_URL or USER_SERVICE_URL",
        "INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET",
        "NEXT_PUBLIC_APP_URL",
      ],
    }),
  );
}
