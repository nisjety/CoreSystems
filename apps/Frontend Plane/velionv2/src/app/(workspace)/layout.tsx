import type { Route } from "next";
import { redirect } from "next/navigation";
import { getControlPlaneContext } from "@/lib/auth/control-plane-context";
import { ControlPlaneProvider } from "@/features/shell-v2/lib/control-plane-provider";

export const dynamic = "force-dynamic";

export default async function WorkspaceLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  // Auth-presence gate (defense-in-depth with proxy.ts + per-page guards) and
  // Control Plane context provider for all client components in the workspace.
  // Onboarding-completeness is intentionally NOT enforced here: the onboarding
  // route lives under (workspace), and each page enforces completion itself.
  const context = await getControlPlaneContext();

  if (!context.user) {
    redirect("/login" as Route);
  }

  return <ControlPlaneProvider value={context}>{children}</ControlPlaneProvider>;
}
