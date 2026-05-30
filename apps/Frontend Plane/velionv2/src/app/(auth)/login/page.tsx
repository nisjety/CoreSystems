import type { Metadata } from "next";
import { VelionAuthPage } from "@/features/auth/components/VelionAuthPage";
import { redirectAuthenticatedUserFromAuth } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Sign in | Velion v2",
  description: "Velion secure authentication.",
};

export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<{ callbackUrl?: string | string[] }>;
}) {
  await redirectAuthenticatedUserFromAuth();
  const { callbackUrl } = await searchParams;
  const normalizedCallbackUrl = Array.isArray(callbackUrl) ? callbackUrl[0] : callbackUrl;

  return <VelionAuthPage callbackUrl={normalizedCallbackUrl} />;
}
