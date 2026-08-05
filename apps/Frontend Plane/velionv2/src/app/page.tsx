import type { Route } from "next";
import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { getAuthGateState } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Verevon v2",
  description: "Verevon v2 workspace entry.",
};

export const dynamic = "force-dynamic";

export default async function Home() {
  const gate = await getAuthGateState();

  if (!gate.user) {
    redirect("/login" as Route);
  }

  redirect((gate.onboardingComplete ? "/dashboard" : "/onboarding") as Route);
}
