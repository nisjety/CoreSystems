"use client";

import dynamic from "next/dynamic";
import Link from "next/link";
import type { Route } from "next";
import { usePathname, useRouter } from "next/navigation";
import { useTheme } from "next-themes";
import { useEffect, useEffectEvent, useRef, useState } from "react";
import { toast } from "sonner";
import {
  Bell,
  CalendarDays,
  CircleDashed,
  MessageSquareMore,
  MoonStar,
  Search,
  Sparkles,
  Sun,
} from "lucide-react";
import { authClient } from "@/lib/auth/auth-client";
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
import { workspaceIdentity, type VelionRoute } from "@/features/shell-v2/lib/shell-data";
import {
  markNavbarNotificationRead,
  saveNavbarTheme,
  useNavbarData,
  type NavbarProfile,
} from "@/features/shell-v2/lib/navbar-data";
import { isLocalIntegrationUnavailable } from "@/lib/api/client-envelope";

type OpenPanel = "assistant" | "messages" | "notifications" | "calendar" | "profile" | "support" | null;

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
  const effectiveSidebarExpanded = lockSidebarCollapsed ? false : sidebarExpanded;
  const sidebarWidth = effectiveSidebarExpanded ? expandedSidebarWidth : SIDEBAR_MINIMIZED_WIDTH;

  return (
    <div
      className="relative h-dvh overflow-hidden bg-[#F7F7F8] text-[#1A1A1A] transition-colors dark:bg-[#101114] dark:text-[#F7F8F8]"
      style={{
        ["--dashboard-navbar-height" as string]: "56px",
        ["--dashboard-rail-width" as string]: `${sidebarWidth}px`,
      }}
    >
      <TopNavbar
        activeRoute={activeRoute}
        profile={profile}
        searchOpen={searchOpen}
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
        <div className="velion-workspace-panel dashboard-main-panel relative h-full overflow-hidden bg-[#FCFCFD] text-[#1A1A1A] transition-colors dark:bg-[#101114] dark:text-[#F7F8F8]">
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
  onProfileChange,
  onSearchOpenChange,
}: {
  activeRoute: VelionRoute;
  profile: NavbarProfile | null;
  searchOpen: boolean;
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
  const profileInitial = profile?.name ? (profile.name.trim().charAt(0) || workspaceIdentity.profile).toUpperCase() : workspaceIdentity.profile;

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

    await saveNavbarTheme(nextTheme).catch((error: unknown) => {
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
      <header ref={headerRef} className="dashboard-navbar-bg fixed inset-x-0 top-0 z-[var(--velion-z-navbar)] bg-[#F7F7F8]/95 backdrop-blur transition-colors dark:bg-[#101114]/92">
        <div className="flex h-14 items-center justify-between gap-4 pl-3 pr-5">
          <div className="flex min-w-0 items-center gap-3">
            <TopLayerTooltip label="Home">
              <Link
                href={"/dashboard" as Route}
                aria-label="Go to home"
                title="Go to home"
                className="hidden size-9 shrink-0 items-center justify-center rounded-[10px] text-[#DD7A1F] transition-colors hover:bg-black/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[#DD7A1F]/40 dark:hover:bg-white/10 md:flex"
              >
                <CircleDashed className="size-4" strokeWidth={1.8} />
              </Link>
            </TopLayerTooltip>

            <HistoryNav onBack={back} onForward={forward} />

            <Breadcrumb
              moduleLabel={moduleLabel}
              moduleHref={activeRoute}
              tabLabel={tabLabel}
              tabHref={activeRoute}
            />
          </div>

          <div className="hidden min-w-0 max-w-[500px] flex-1 md:block">
            <SearchTrigger onOpen={() => onSearchOpenChange(true)} />
          </div>

          <div className="relative flex min-w-0 items-center justify-end gap-2">
            <NavbarActionButton
              label="Open global search"
              tooltip="Open global search"
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
                  {profileInitial}
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
                onSupport={() => openExclusivePanel("support")}
                onSignOut={async () => {
                  await authClient.signOut();
                  push("/login");
                }}
              />
            ) : null}
          </div>
        </div>
      </header>

      {searchOpen ? <LazyGlobalSearchDialog onClose={() => onSearchOpenChange(false)} /> : null}
      {openPanel === "assistant" ? <LazyAssistantModal pathname={pathname} onClose={() => setOpenPanel(null)} /> : null}
      {openPanel === "support" ? <LazySupportModal pathname={pathname} onClose={() => setOpenPanel(null)} /> : null}
    </>
  );
}
