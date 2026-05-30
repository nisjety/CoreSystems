import {
  isControlPlaneAuthConfigured,
  proxyControlPlaneAuthRequest,
} from "@/lib/auth/control-plane";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

async function handleAuthRequest(request: Request) {
  if (isControlPlaneAuthConfigured()) {
    return proxyControlPlaneAuthRequest(request);
  }

  // Fail closed in production: never silently fall back to the standalone
  // Better Auth instance, which would issue sessions disconnected from the
  // Control Plane (auth-core). Standalone auth is a local-dev convenience only.
  if (process.env.NODE_ENV === "production") {
    return new Response(
      JSON.stringify({
        error: {
          code: "control_plane_auth_unconfigured",
          message:
            "AUTH_CORE_URL (or CONTROL_PLANE_AUTH_URL) must be set in production. Refusing to fall back to standalone auth.",
        },
      }),
      { status: 503, headers: { "content-type": "application/json" } },
    );
  }

  const { auth } = await import("@/lib/auth/auth");
  return auth.handler(request);
}

export const GET = handleAuthRequest;
export const POST = handleAuthRequest;
export const PUT = handleAuthRequest;
export const PATCH = handleAuthRequest;
export const DELETE = handleAuthRequest;
export const OPTIONS = handleAuthRequest;
