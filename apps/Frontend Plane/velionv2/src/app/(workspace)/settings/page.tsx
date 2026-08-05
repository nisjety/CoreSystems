import type { Metadata } from "next";
import type { Route } from "next";
import { redirect } from "next/navigation";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Settings | Verevon v2",
  description: "Verevon workspace and admin settings.",
};

export default async function SettingsPage() {
  await requireCompletedOnboarding("/settings");

  redirect("/settings/workspace" as Route);
}
