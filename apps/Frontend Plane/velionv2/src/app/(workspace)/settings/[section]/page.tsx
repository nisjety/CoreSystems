import type { Metadata } from "next";
import { notFound } from "next/navigation";
import {
  getWorkspaceSettingsSection,
  isWorkspaceSettingsSection,
  workspaceSettingsSectionIds,
} from "@/features/settings-v2/lib/settings-sections";
import { VelionWorkspaceSettingsPage } from "@/features/settings-v2/components/VelionWorkspaceSettingsPage";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
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
      title: "Settings | Velion v2",
    };
  }

  const details = getWorkspaceSettingsSection(section);
  return {
    title: `${details.label} settings | Velion v2`,
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
    <VelionProductShell activeRoute="/settings" defaultSidebarExpanded>
      <VelionWorkspaceSettingsPage section={section} />
    </VelionProductShell>
  );
}
