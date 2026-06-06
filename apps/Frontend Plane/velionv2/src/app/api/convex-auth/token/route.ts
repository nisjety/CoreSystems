import {
  isControlPlaneAuthConfigured,
  proxyControlPlaneAuthServiceRequest,
} from "@/lib/auth/control-plane";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  if (!isControlPlaneAuthConfigured()) {
    return new Response(
      JSON.stringify({
        error: {
          code: "control_plane_auth_unconfigured",
          message:
            "AUTH_CORE_URL (or CONTROL_PLANE_AUTH_URL) must be set to mint Convex tokens.",
        },
      }),
      { status: 503, headers: { "content-type": "application/json" } },
    );
  }

  return proxyControlPlaneAuthServiceRequest(request, "/api/convex-auth/token");
}
