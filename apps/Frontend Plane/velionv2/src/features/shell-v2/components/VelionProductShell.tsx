"use client";

import dynamic from "next/dynamic";
import Link from "next/link";
import type { Route } from "next";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useEffectEvent, useRef, useState } from "react";
import { toast } from "sonner";
import {
  Bell,
  Building2,
  CalendarDays,
  Check,
  MessageSquareMore,
  MoonStar,
  Search,
  Sparkles,
  Sun,
  UserRound,
} from "lucide-react";
import { authClient } from "@/lib/auth/auth-client";
import type { ControlPlaneContextValue } from "@/lib/control-plane/context-types";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import {
  BadgeButton,
  Breadcrumb,
  HistoryNav,
  NavbarActionButton,
  NavDivider,
  SearchTrigger,
} from "@/features/shell-v2/components/VelionNavbarControls";
import {
  CalendarDropdown,
  MessagesDropdown,
  NotificationsDropdown,
  ProfileDropdown,
} from "@/features/shell-v2/components/VelionNavbarPanels";
import { VelionSidebar, SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from "@/features/shell-v2/components/VelionSidebar";
import {
  fallbackWorkspaceIdentity,
  formatPlanLabel,
  type VelionRoute,
  type WorkspaceIdentity,
} from "@/features/shell-v2/lib/shell-data";
import { useControlPlaneContext } from "@/features/shell-v2/lib/control-plane-provider";
import {
  markNavbarNotificationRead,
  saveNavbarTheme,
  useNavbarData,
  type NavbarProfile,
} from "@/features/shell-v2/lib/navbar-data";
import { isLocalIntegrationUnavailable } from "@/lib/api/client-envelope";
import { useTheme } from "@/lib/theme/theme-provider";

type OpenPanel = "assistant" | "messages" | "notifications" | "calendar" | "profile" | "support" | "workspace" | null;

const LazyGlobalSearchDialog = dynamic(
  () => import("@/features/shell-v2/components/VelionNavbarOverlays").then((module) => module.GlobalSearchDialog),
  { ssr: false },
);
const LazyAssistantModal = dynamic(
  () => import("@/features/shell-v2/components/VelionNavbarOverlays").then((module) => module.AssistantModal),
  { ssr: false },
);
const LazySupportModal = dynamic(
  () => import("@/features/shell-v2/components/VelionNavbarOverlays").then((module) => module.SupportModal),
  { ssr: false },
);

function getNavbarLabels(activeRoute: VelionRoute) {
  switch (activeRoute) {
    case "/chat":
      return { moduleLabel: "Chat", tabLabel: "Oppgaver" };
    case "/inbox":
      return { moduleLabel: "Inbox", tabLabel: "Your inbox" };
    case "/agents":
      return { moduleLabel: "Agenter", tabLabel: "Studio" };
    case "/knowledge":
      return { moduleLabel: "Kunnskap", tabLabel: "Kilder" };
    case "/account":
      return { moduleLabel: "Account", tabLabel: "Profile" };
    case "/settings":
    case "/settings/workspace":
    case "/settings/members":
    case "/settings/billing":
    case "/settings/sso":
    case "/settings/org-security":
    case "/settings/integrations":
      return { moduleLabel: "Innstillinger", tabLabel: "Workspace" };
    case "/dashboard":
    default:
      return { moduleLabel: "Oversikt", tabLabel: "Hjem" };
  }
}

const HEX_COLOR = /^#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

function cleanOrganizationName(name: string) {
  const trimmed = name.trim();
  if (!trimmed) return fallbackWorkspaceIdentity.name;

  if (!/^[A-ZÆØÅ0-9 .&-]+$/.test(trimmed)) {
    return trimmed;
  }

  return trimmed
    .toLowerCase()
    .split(" ")
    .map((word) => {
      if (["as", "asa", "ab", "sa", "ba", "llc", "inc"].includes(word)) {
        return word.toUpperCase();
      }
      return word.charAt(0).toUpperCase() + word.slice(1);
    })
    .join(" ");
}

function firstInitial(...values: Array<string | null | undefined>) {
  const first = values.find((value) => value?.trim());
  return (first?.trim().charAt(0) || fallbackWorkspaceIdentity.initial).toUpperCase();
}

function safeAccentColor(...values: Array<string | null | undefined>) {
  const found = values.find((value) => value && HEX_COLOR.test(value));
  return found?.toLowerCase() ?? null;
}

function resolveWorkspaceIdentity(
  context: ControlPlaneContextValue,
  profile: NavbarProfile | null,
): WorkspaceIdentity {
  const organization = context.organization;
  const userName = profile?.name ?? context.user?.name ?? null;
  const userEmail = profile?.email ?? context.user?.email ?? null;
  const userAvatar = profile?.avatar ?? context.user?.image ?? null;
  const plan = context.entitlements?.plan ?? organization?.plan ?? "free";
  const name = organization?.name ?? userName ?? fallbackWorkspaceIdentity.name;

  return {
    accentColor: safeAccentColor(context.appearance?.colorScheme, organization?.accentColor),
    domain: organization?.primaryDomain ?? null,
    initial: firstInitial(organization?.name, userName, userEmail),
    logoUrl: organization?.logoUrl ?? null,
    name: cleanOrganizationName(name),
    plan: formatPlanLabel(plan),
    role: context.role,
    userAvatar,
    userEmail,
    userName,
  };
}

function useWorkspaceAccent(accentColor: string | null | undefined) {
  useEffect(() => {
    if (!accentColor || !HEX_COLOR.test(accentColor)) {
      return;
    }

    const root = document.documentElement;
    root.style.setProperty("--velion-accent", accentColor);
  }, [accentColor]);
}

export function VelionProductShell({
  activeRoute,
  children,
  defaultSidebarExpanded = false,
  expandedSidebarWidth = SIDEBAR_EXPANDED_WIDTH,
  lockSidebarCollapsed = false,
}: {
  activeRoute: VelionRoute;
  children: React.ReactNode;
  defaultSidebarExpanded?: boolean;
  expandedSidebarWidth?: number;
  lockSidebarCollapsed?: boolean;
}) {
  const [sidebarExpanded, setSidebarExpanded] = useState(defaultSidebarExpanded);
  const [searchOpen, setSearchOpen] = useState(false);
  const [profile, setProfile] = useState<NavbarProfile | null>(null);
  const controlPlane = useControlPlaneContext();
  const { setTheme } = useTheme();
  const workspace = resolveWorkspaceIdentity(controlPlane, profile);
  const effectiveSidebarExpanded = lockSidebarCollapsed ? false : sidebarExpanded;
  const sidebarWidth = effectiveSidebarExpanded ? expandedSidebarWidth : SIDEBAR_MINIMIZED_WIDTH;
  useWorkspaceAccent(workspace.accentColor);

  // Apply the server-side theme preference only when the user has no explicit
  // local override stored in localStorage (key "theme").
  const serverTheme = controlPlane.appearance?.theme;
  useEffect(() => {
    if (!serverTheme) return;
    try {
      const stored = window.localStorage.getItem("theme");
      if (!stored) {
        setTheme(serverTheme);
      }
    } catch {
      // localStorage unavailable — skip silently.
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverTheme]);

  return (
    <div
      className="relative h-dvh overflow-hidden bg-[#F7F7F8] text-[#1A1A1A] transition-colors dark:bg-[#1C1E24] dark:text-[#ECEEF2]"
      style={{
        ["--dashboard-navbar-height" as string]: "56px",
        ["--dashboard-rail-width" as string]: `${sidebarWidth}px`,
      }}
    >
      <TopNavbar
        activeRoute={activeRoute}
        profile={profile}
        searchOpen={searchOpen}
        workspace={workspace}
        onProfileChange={setProfile}
        onSearchOpenChange={setSearchOpen}
      />
      <VelionSidebar
        activeRoute={activeRoute}
        expandedWidth={expandedSidebarWidth}
        expanded={effectiveSidebarExpanded}
        expansionLocked={lockSidebarCollapsed}
        onExpandedChange={lockSidebarCollapsed ? () => undefined : setSidebarExpanded}
        onOpenSearch={() => setSearchOpen(true)}
      />
      <main className="relative h-full overflow-hidden pt-14 md:pl-[var(--dashboard-rail-width)]">
        <div className="velion-workspace-panel dashboard-main-panel relative h-full overflow-hidden bg-[#FCFCFD] text-[#1A1A1A] transition-colors dark:bg-[#1C1E24] dark:text-[#ECEEF2]">
          {children}
        </div>
      </main>
    </div>
  );
}

function TopNavbar({
  activeRoute,
  profile,
  searchOpen,
  workspace,
  onProfileChange,
  onSearchOpenChange,
}: {
  activeRoute: VelionRoute;
  profile: NavbarProfile | null;
  searchOpen: boolean;
  workspace: WorkspaceIdentity;
  onProfileChange: (profile: NavbarProfile) => void;
  onSearchOpenChange: (open: boolean) => void;
}) {
  const { back, forward, push } = useRouter();
  const pathname = usePathname();
  const { resolvedTheme, setTheme } = useTheme();
  const headerRef = useRef<HTMLElement>(null);
  const [openPanel, setOpenPanel] = useState<OpenPanel>(null);
  const { calendar, notifications, setCalendar, setNotifications } = useNavbarData({
    onProfileChange,
    onThemeChange: setTheme,
  });
  const openSearch = useEffectEvent(() => {
    onSearchOpenChange(true);
  });
  const closeShellOverlays = useEffectEvent(() => {
    setOpenPanel(null);
    onSearchOpenChange(false);
  });

  const { moduleLabel, tabLabel } = getNavbarLabels(activeRoute);
  const unreadMessageCount = notifications.messages.filter((message) => !message.read).length;
  const unreadNotificationCount = notifications.notifications.filter((notification) => !notification.read).length;
  const profileAvatar = workspace.userAvatar ?? profile?.avatar;
  const profileInitial = firstInitial(profile?.name, workspace.userName, workspace.userEmail);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const isTyping = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.isContentEditable;

      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        openSearch();
      }

      if (!isTyping && event.key === "/") {
        event.preventDefault();
        openSearch();
      }

      if (event.key === "Escape") {
        closeShellOverlays();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    if (!openPanel) {
      return;
    }

    const closePanelOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (target && headerRef.current?.contains(target)) {
        return;
      }

      setOpenPanel(null);
    };

    document.addEventListener("pointerdown", closePanelOnOutsidePointer, true);
    return () => document.removeEventListener("pointerdown", closePanelOnOutsidePointer, true);
  }, [openPanel]);

  const toggleTheme = async () => {
    const nextTheme = resolvedTheme === "dark" ? "light" : "dark";
    setTheme(nextTheme);

    await saveNavbarTheme(nextTheme, workspace.accentColor).catch((error: unknown) => {
      if (isLocalIntegrationUnavailable(error)) {
        return;
      }

      toast.error(error instanceof Error ? error.message : "Theme could not be saved.");
    });
  };

  const markNotificationRead = async (notificationId: string) => {
    setNotifications((current) => ({
      ...current,
      notifications: current.notifications.map((item) => item.id === notificationId ? { ...item, read: true } : item),
      messages: current.messages.map((item) => item.id === notificationId ? { ...item, read: true } : item),
    }));
    await markNavbarNotificationRead(notificationId).catch(() => undefined);
  };

  const openExclusivePanel = (panel: OpenPanel) => {
    setOpenPanel((current) => (current === panel ? null : panel));
  };

  return (
    <>
      <header ref={headerRef} className="dashboard-navbar-bg fixed inset-x-0 top-0 z-[var(--velion-z-navbar)] bg-[#F7F7F8]/95 backdrop-blur transition-colors dark:bg-[#1E2027]/88">
        <div className="flex h-14 items-center justify-between gap-4 pl-3 pr-5">
          <div className="flex min-w-0 items-center gap-3">
            <TopLayerTooltip label="Home">
              <Link
                href={"/dashboard" as Route}
                aria-label="Go to home"
                title="Go to home"
                className="hidden size-9 shrink-0 items-center justify-center overflow-hidden rounded-[10px] border border-black/[0.05] bg-white text-[var(--velion-accent)] shadow-sm transition-colors hover:bg-black/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--velion-accent)] dark:border-white/[0.08] dark:bg-[#202229] dark:hover:bg-white/10 md:flex"
              >
                {workspace.logoUrl ? (
                  <img
                    src={workspace.logoUrl}
                    alt=""
                    referrerPolicy="no-referrer"
                    className="size-full object-cover"
                  />
                ) : (
                  <span className="text-[11px] font-semibold">{workspace.initial}</span>
                )}
              </Link>
            </TopLayerTooltip>

            <HistoryNav onBack={back} onForward={forward} />

            <Breadcrumb
              moduleLabel={moduleLabel}
              moduleHref={activeRoute}
              onWorkspaceClick={() => openExclusivePanel("workspace")}
              tabLabel={tabLabel}
              tabHref={activeRoute}
              workspace={workspace}
              workspaceActive={openPanel === "workspace"}
            />
          </div>

          <div className="hidden min-w-0 max-w-[500px] flex-1 md:block">
            <SearchTrigger onOpen={() => onSearchOpenChange(true)} />
          </div>

          <div className="relative flex min-w-0 items-center justify-end gap-2">
            <NavbarActionButton
              label="Open knowledge search"
              tooltip="Open knowledge search"
              onClick={() => onSearchOpenChange(true)}
              className="md:hidden"
            >
              <Search className="size-4" strokeWidth={1.9} />
            </NavbarActionButton>

            <div className="flex items-center gap-1 pl-2">
              <NavDivider />

              <NavbarActionButton label="Toggle dark mode" tooltip="Toggle dark mode" onClick={toggleTheme}>
                <span className="grid size-[18px] place-items-center">
                  <Sun className="hidden size-[18px] dark:block" strokeWidth={1.85} />
                  <MoonStar className="size-[18px] dark:hidden" strokeWidth={1.85} />
                </span>
              </NavbarActionButton>

              <NavDivider />

              <NavbarActionButton
                label="Open AI assistant"
                tooltip="AI assistant for current page"
                active={openPanel === "assistant"}
                onClick={() => openExclusivePanel("assistant")}
              >
                <Sparkles className="size-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              <BadgeButton count={unreadMessageCount} label="Quick messages">
                <NavbarActionButton
                  label={unreadMessageCount > 0 ? `${unreadMessageCount} unread messages` : "Quick messages"}
                  active={openPanel === "messages"}
                  onClick={() => openExclusivePanel("messages")}
                >
                  <MessageSquareMore className="size-[18px]" strokeWidth={1.85} />
                </NavbarActionButton>
              </BadgeButton>

              <BadgeButton count={unreadNotificationCount} label="Notifications">
                <NavbarActionButton
                  label={unreadNotificationCount > 0 ? `${unreadNotificationCount} unread notifications` : "Notifications"}
                  active={openPanel === "notifications"}
                  onClick={() => openExclusivePanel("notifications")}
                >
                  <Bell className="size-[18px]" strokeWidth={1.85} />
                </NavbarActionButton>
              </BadgeButton>

              <NavbarActionButton
                label="Calendar"
                active={openPanel === "calendar"}
                onClick={() => openExclusivePanel("calendar")}
              >
                <CalendarDays className="size-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              <NavDivider />

              <button
                type="button"
                aria-label="Open profile menu"
                title="Open profile menu"
                onClick={() => openExclusivePanel("profile")}
                className="relative flex size-8 shrink-0 items-center justify-center rounded-full focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/20 dark:focus-visible:ring-white/30"
              >
                <span
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-0 rounded-full"
                  style={{
                    background: "linear-gradient(135deg, #F97316, #EC4899, #8B5CF6)",
                    padding: "2px",
                    WebkitMask: "linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0)",
                    WebkitMaskComposite: "xor",
                    maskComposite: "exclude",
                  }}
                />
                <span className="flex size-[26px] items-center justify-center rounded-full bg-[linear-gradient(135deg,#F2DFC2,#E6B783)] text-[10px] font-bold text-[#4A341A]">
                  {profileAvatar ? (
                    <img
                      src={profileAvatar}
                      alt=""
                      referrerPolicy="no-referrer"
                      className="size-full rounded-full object-cover"
                    />
                  ) : (
                    profileInitial
                  )}
                </span>
              </button>
            </div>

            {openPanel === "messages" ? (
              <MessagesDropdown messages={notifications.messages} configured={notifications.configured} onOpen={markNotificationRead} />
            ) : null}
            {openPanel === "notifications" ? (
              <NotificationsDropdown notifications={notifications.notifications} configured={notifications.configured} onOpen={markNotificationRead} />
            ) : null}
            {openPanel === "calendar" ? (
              <CalendarDropdown
                state={calendar}
                onStateChange={setCalendar}
                onSaved={(message) => toast.success(message)}
              />
            ) : null}
            {openPanel === "profile" ? (
              <ProfileDropdown
                profile={profile}
                planLabel={workspace.plan}
                onSupport={() => openExclusivePanel("support")}
                onSignOut={async () => {
                  await authClient.signOut();
                  push("/login");
                }}
              />
            ) : null}
          </div>
        </div>
        {openPanel === "workspace" ? (
          <WorkspaceSwitcher workspace={workspace} onClose={() => setOpenPanel(null)} />
        ) : null}
      </header>

      {searchOpen ? <LazyGlobalSearchDialog onClose={() => onSearchOpenChange(false)} /> : null}
      {openPanel === "assistant" ? <LazyAssistantModal pathname={pathname} onClose={() => setOpenPanel(null)} /> : null}
      {openPanel === "support" ? <LazySupportModal pathname={pathname} onClose={() => setOpenPanel(null)} /> : null}
    </>
  );
}

type ActiveWorkspace = "org" | "personal";

function WorkspaceSwitcher({
  onClose,
  workspace,
}: {
  onClose: () => void;
  workspace: WorkspaceIdentity;
}) {
  const [active, setActive] = useState<ActiveWorkspace>("org");

  return (
    <div className="fixed left-[150px] top-12 z-[var(--velion-z-popover)] w-[340px] overflow-hidden rounded-[18px] border border-black/[0.08] bg-white/96 p-2 shadow-[0_22px_70px_rgba(15,16,20,0.18)] backdrop-blur-xl dark:border-white/[0.07] dark:bg-[#232630]/96">
      {/* Organization workspace */}
      <button
        type="button"
        aria-pressed={active === "org"}
        onClick={() => { setActive("org"); onClose(); }}
        className="flex w-full items-center gap-3 rounded-[14px] bg-[#F7F7F8] px-3 py-3 text-left transition-colors hover:bg-[#EFEFEF] dark:bg-white/[0.06] dark:hover:bg-white/[0.10]"
      >
        <WorkspaceMark workspace={workspace} />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-semibold text-[#111111] dark:text-white">{workspace.name}</p>
          <p className="mt-0.5 truncate text-[12px] text-[#777] dark:text-[#B8BEC8]">
            Organization workspace · {workspace.plan}
          </p>
        </div>
        {active === "org" ? (
          <Check className="size-4 shrink-0 text-[var(--velion-accent)]" aria-hidden="true" />
        ) : null}
      </button>

      {/* Personal workspace */}
      <button
        type="button"
        aria-pressed={active === "personal"}
        onClick={() => { setActive("personal"); onClose(); }}
        className="mt-1 flex w-full items-center gap-3 rounded-[14px] border border-black/[0.06] px-3 py-3 text-left transition-colors hover:bg-[#F7F7F8] dark:border-white/[0.08] dark:hover:bg-white/[0.06]"
      >
        <span className="grid size-9 shrink-0 place-items-center rounded-[12px] bg-[#F2F3F5] text-[#4B5563] dark:bg-[#2A2D35] dark:text-[#D7DBE3]">
          <UserRound className="size-4" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-semibold text-[#111111] dark:text-white">
            {workspace.userName ?? "Personal workspace"}
          </p>
          <p className="mt-0.5 truncate text-[12px] text-[#777] dark:text-[#B8BEC8]">
            {workspace.userEmail ?? "Signed in"}
          </p>
        </div>
        {active === "personal" ? (
          <Check className="size-4 shrink-0 text-[var(--velion-accent)]" aria-hidden="true" />
        ) : null}
      </button>

      <Link
        href={"/settings/workspace" as Route}
        onClick={onClose}
        className="mt-2 flex items-center gap-2 rounded-[12px] px-3 py-2 text-[12px] font-medium text-[#555] transition-colors hover:bg-black/[0.04] dark:text-[#D0D6E0] dark:hover:bg-white/[0.08]"
      >
        <Building2 className="size-4" />
        Manage workspaces and members
      </Link>
    </div>
  );
}

function WorkspaceMark({ workspace }: { workspace: WorkspaceIdentity }) {
  if (workspace.logoUrl) {
    return (
      <span className="size-10 shrink-0 overflow-hidden rounded-[13px] bg-white shadow-sm ring-1 ring-black/[0.06] dark:bg-[#17191F] dark:ring-white/[0.08]">
        <img src={workspace.logoUrl} alt="" referrerPolicy="no-referrer" className="size-full object-cover" />
      </span>
    );
  }

  return (
    <span
      className="grid size-10 shrink-0 place-items-center rounded-[13px] text-[13px] font-semibold text-white shadow-sm"
      style={{ backgroundColor: workspace.accentColor ?? "var(--velion-accent)" }}
    >
      {workspace.initial}
    </span>
  );
}
