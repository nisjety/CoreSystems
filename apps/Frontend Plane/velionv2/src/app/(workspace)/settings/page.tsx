import type { Metadata } from "next";
import type { Route } from "next";
import { redirect } from "next/navigation";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Settings | Velion v2",
  description: "Velion workspace and admin settings.",
};

export default async function SettingsPage() {
  await requireCompletedOnboarding("/settings");

  redirect("/settings/workspace" as Route);
}
