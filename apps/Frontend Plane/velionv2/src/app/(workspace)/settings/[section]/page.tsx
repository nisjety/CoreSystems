import type { Metadata } from "next";
import { notFound } from "next/navigation";
import {
  getWorkspaceSettingsSection,
  isWorkspaceSettingsSection,
  workspaceSettingsSectionIds,
} from "@/features/settings-v2/lib/settings-sections";
import { VerevonWorkspaceSettingsPage } from "@/features/settings-v2/components/VerevonWorkspaceSettingsPage";
import { VerevonProductShell } from "@/features/shell-v2/components/VerevonProductShell";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

type SettingsSectionPageProps = {
  params: Promise<{ section: string }>;
};

export function generateStaticParams() {
  return workspaceSettingsSectionIds.map((section) => ({ section }));
}

export async function generateMetadata({ params }: SettingsSectionPageProps): Promise<Metadata> {
  const { section } = await params;

  if (!isWorkspaceSettingsSection(section)) {
    return {
      title: "Settings | Verevon v2",
    };
  }

  const details = getWorkspaceSettingsSection(section);
  return {
    title: `${details.label} settings | Verevon v2`,
    description: details.description,
  };
}

export default async function SettingsSectionPage({ params }: SettingsSectionPageProps) {
  await requireCompletedOnboarding("/settings");

  const { section } = await params;

  if (!isWorkspaceSettingsSection(section)) {
    notFound();
  }

  return (
    <VerevonProductShell activeRoute="/settings" defaultSidebarExpanded>
      <VerevonWorkspaceSettingsPage section={section} />
    </VerevonProductShell>
  );
}
