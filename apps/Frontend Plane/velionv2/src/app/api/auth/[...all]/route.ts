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

  const { auth } = await import("@/lib/auth/auth");
  return auth.handler(request);
}

export const GET = handleAuthRequest;
export const POST = handleAuthRequest;
export const PUT = handleAuthRequest;
export const PATCH = handleAuthRequest;
export const DELETE = handleAuthRequest;
export const OPTIONS = handleAuthRequest;
