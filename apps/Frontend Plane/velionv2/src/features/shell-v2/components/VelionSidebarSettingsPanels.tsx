"use client";

import Link from "next/link";
import type { Route } from "next";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import {
  Building2,
  CheckCircle2,
  CircleUserRound,
  CreditCard,
  Globe2,
  KeyRound,
  Plug,
  Settings,
  ShieldCheck,
  User,
  UsersRound,
  type LucideIcon,
} from "lucide-react";
import { SidebarPanelTitle } from "@/features/shell-v2/components/VerevonSidebarPrimitives";
import { sidebarType } from "@/features/shell-v2/lib/sidebar-style";
import { cn } from "@/lib/utils";

type SettingsLinkSection = { id: string; label: string; Icon: LucideIcon; href: string };

const accountSidebarSections: SettingsLinkSection[] = [
  { id: "profile", label: "Profile", Icon: User, href: "#profile" },
  { id: "contact", label: "Contact", Icon: CircleUserRound, href: "#contact" },
  { id: "preferences", label: "Preferences", Icon: Settings, href: "#preferences" },
  { id: "availability", label: "Availability", Icon: CheckCircle2, href: "#availability" },
  { id: "connected-accounts", label: "Connected accounts", Icon: Plug, href: "#connected-accounts" },
  { id: "privacy", label: "Privacy", Icon: Globe2, href: "#privacy" },
];

const settingsSidebarSections: SettingsLinkSection[] = [
  { id: "workspace", label: "Workspace", Icon: Building2, href: "/settings/workspace" },
  { id: "members", label: "Members & roles", Icon: UsersRound, href: "/settings/members" },
  { id: "billing", label: "Billing", Icon: CreditCard, href: "/settings/billing" },
  { id: "sso", label: "SSO", Icon: KeyRound, href: "/settings/sso" },
  { id: "org-security", label: "Org security", Icon: ShieldCheck, href: "/settings/org-security" },
  { id: "integrations", label: "Integrations", Icon: Plug, href: "/settings/integrations" },
];

export function AccountExpandedSidebarPanel({ onCollapse }: { onCollapse: () => void }) {
  const activeSectionId = useActiveAccountSection();

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-5 pb-5 pt-6 dark:bg-[#101114]">
      <SidebarPanelTitle spacing="mb-8" onCollapse={onCollapse}>Account</SidebarPanelTitle>

      <SettingsSectionLinks
        ariaLabel="Account sections"
        sections={accountSidebarSections}
        activeSectionId={activeSectionId}
      />
    </div>
  );
}

export function SettingsExpandedSidebarPanel({ onCollapse }: { onCollapse: () => void }) {
  const pathname = usePathname();
  const activeSectionId = getSettingsActiveSectionId(pathname);

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-5 pb-5 pt-6 dark:bg-[#101114]">
      <SidebarPanelTitle spacing="mb-8" onCollapse={onCollapse}>Settings</SidebarPanelTitle>

      <SettingsSectionLinks
        ariaLabel="Settings sections"
        sections={settingsSidebarSections}
        activeSectionId={activeSectionId}
      />
    </div>
  );
}

function SettingsSectionLinks({
  activeSectionId,
  ariaLabel,
  sections,
}: {
  activeSectionId: string;
  ariaLabel: string;
  sections: SettingsLinkSection[];
}) {
  return (
    <nav aria-label={ariaLabel} className="space-y-1">
      {sections.map((section) => (
        <SettingsSectionLink
          key={section.id}
          active={activeSectionId === section.id}
          section={section}
        />
      ))}
    </nav>
  );
}

function SettingsSectionLink({
  active,
  section,
}: {
  active: boolean;
  section: SettingsLinkSection;
}) {
  const { href, Icon, label } = section;
  const className = cn(
    "flex h-9 w-full items-center gap-2.5 rounded-[9px] px-2 text-left transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]",
    sidebarType.row,
    active
      ? "bg-[#F0F1F5] text-[#1C1C1E] dark:bg-[#202228] dark:text-white"
      : "text-[#626772] hover:bg-[#F0F2F5] hover:text-[#111827] dark:text-[#AEB4C0] dark:hover:bg-white/5 dark:hover:text-white",
  );
  const content = (
    <>
      <Icon className={cn("shrink-0", sidebarType.icon)} strokeWidth={1.7} />
      <span className="truncate">{label}</span>
    </>
  );

  if (href.startsWith("#")) {
    return (
      <a href={href} className={className} aria-current={active ? "location" : undefined}>
        {content}
      </a>
    );
  }

  return (
    <Link href={href as Route} className={className} aria-current={active ? "page" : undefined}>
      {content}
    </Link>
  );
}

function getSettingsActiveSectionId(pathname: string) {
  const section = pathname.split("/")[2];
  return settingsSidebarSections.some((item) => item.id === section) ? section : "workspace";
}

function useActiveAccountSection() {
  const [activeSectionId, setActiveSectionId] = useState(getActiveAccountSectionId);

  useEffect(() => {
    const ids = accountSidebarSections.map((section) => section.id);
    let frame = 0;

    const updateActiveSection = () => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        const scrollRoot = document.querySelector<HTMLElement>("[data-account-settings-scroll]");
        if (scrollRoot && scrollRoot.scrollTop + scrollRoot.clientHeight >= scrollRoot.scrollHeight - 24) {
          setActiveSectionId((current) => (current === ids[ids.length - 1] ? current : ids[ids.length - 1]));
          return;
        }

        const rootTop = scrollRoot?.getBoundingClientRect().top ?? 0;
        const activationLine = rootTop + 180;
        let nextActiveSectionId = ids[0];

        ids.forEach((id) => {
          const section = document.getElementById(id);
          if (!section) {
            return;
          }

          if (section.getBoundingClientRect().top <= activationLine) {
            nextActiveSectionId = id;
          }
        });

        setActiveSectionId((current) => (current === nextActiveSectionId ? current : nextActiveSectionId));
      });
    };

    const scrollRoot = document.querySelector<HTMLElement>("[data-account-settings-scroll]");
    scrollRoot?.addEventListener("scroll", updateActiveSection, { passive: true });
    window.addEventListener("resize", updateActiveSection);

    return () => {
      window.cancelAnimationFrame(frame);
      scrollRoot?.removeEventListener("scroll", updateActiveSection);
      window.removeEventListener("resize", updateActiveSection);
    };
  }, []);

  return activeSectionId;
}

function getActiveAccountSectionId() {
  if (typeof document === "undefined") {
    return accountSidebarSections[0].id;
  }

  const ids = accountSidebarSections.map((section) => section.id);
  const scrollRoot = document.querySelector<HTMLElement>("[data-account-settings-scroll]");
  if (scrollRoot && scrollRoot.scrollTop + scrollRoot.clientHeight >= scrollRoot.scrollHeight - 24) {
    return ids[ids.length - 1];
  }

  const rootTop = scrollRoot?.getBoundingClientRect().top ?? 0;
  const activationLine = rootTop + 180;
  return ids.reduce((activeId, id) => {
    const section = document.getElementById(id);
    if (!section || section.getBoundingClientRect().top > activationLine) {
      return activeId;
    }

    return id;
  }, ids[0]);
}
