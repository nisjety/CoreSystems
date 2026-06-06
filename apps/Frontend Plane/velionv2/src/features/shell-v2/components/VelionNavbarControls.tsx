"use client";

import Link from "next/link";
import type { Route } from "next";
import { ChevronLeft, ChevronRight, Search, Slash } from "lucide-react";
import type { ReactNode } from "react";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import type { VelionRoute, WorkspaceIdentity } from "@/features/shell-v2/lib/shell-data";
import { cn } from "@/lib/utils";

export function SearchTrigger({ onOpen }: { onOpen: () => void }) {
  return (
    <button
      type="button"
      onClick={onOpen}
      title="Open knowledge search"
      className="velion-navbar-search-trigger"
      aria-label="Open knowledge search"
    >
      <Search className="velion-navbar-search-icon" strokeWidth={1.8} />
      <span className="velion-navbar-search-label">Search knowledge base</span>
      <span className="velion-navbar-shortcut-key velion-navbar-shortcut-key-min">
        /
      </span>
      <span className="velion-navbar-shortcut-key velion-navbar-shortcut-key-wide">
        CMD+K
      </span>
    </button>
  );
}

export function Breadcrumb({
  moduleLabel,
  moduleHref,
  onWorkspaceClick,
  tabLabel,
  tabHref,
  workspace,
  workspaceActive = false,
}: {
  moduleLabel: string;
  moduleHref: VelionRoute;
  onWorkspaceClick?: () => void;
  tabLabel: string;
  tabHref: VelionRoute;
  workspace: WorkspaceIdentity;
  workspaceActive?: boolean;
}) {
  return (
    <div className="hidden min-w-0 items-center gap-2 text-[14px] text-[#6D717B] dark:text-[#8A8F98] lg:flex">
      <BreadcrumbSeparator />
      <button
        type="button"
        onClick={onWorkspaceClick}
        aria-expanded={workspaceActive}
        className="group inline-flex min-w-0 items-center gap-2 rounded-[10px] px-1.5 py-1 transition-colors hover:bg-black/[0.04] hover:text-[#111111] dark:hover:bg-white/[0.08] dark:hover:text-white"
      >
        <span className="truncate font-medium text-[#2A2D35] group-hover:text-[#111111] dark:text-[#F2F4F8] dark:group-hover:text-white">{workspace.name}</span>
        <span className="shrink-0 rounded-full border border-[#DDE0E7] bg-white px-2 py-0.5 text-[12px] font-medium leading-none text-[#4B5563] dark:border-[#3A3D46] dark:bg-[#202229] dark:text-[#D0D6E0]">
          {workspace.plan}
        </span>
      </button>
      <BreadcrumbSeparator />
      <Link href={moduleHref as Route} className="truncate font-medium text-[#2A2D35] transition-colors hover:text-[#111111] dark:text-[#F7F8F8] dark:hover:text-white">
        {moduleLabel}
      </Link>
      <BreadcrumbSeparator />
      <Link href={tabHref as Route} className="truncate font-medium text-[#7A7F89] transition-colors hover:text-[#2A2D35] dark:text-[#AEB4C0] dark:hover:text-white">
        {tabLabel}
      </Link>
    </div>
  );
}

export function HistoryNav({
  onBack,
  onForward,
}: {
  onBack: () => void;
  onForward: () => void;
}) {
  return (
    <div className="flex items-center gap-0.5 rounded-[12px] bg-[#F4F5F1] p-1 dark:bg-[#17181C]">
      <NavbarActionButton label="Go back" tooltip="Go back" onClick={onBack}>
        <ChevronLeft className="size-4" strokeWidth={2.1} />
      </NavbarActionButton>
      <NavbarActionButton label="Go forward" tooltip="Go forward" onClick={onForward}>
        <ChevronRight className="size-4" strokeWidth={2.1} />
      </NavbarActionButton>
    </div>
  );
}

export function NavbarActionButton({
  active,
  children,
  className,
  disabled = false,
  label,
  onClick,
  tooltip,
}: {
  active?: boolean;
  children: ReactNode;
  className?: string;
  disabled?: boolean;
  label: string;
  onClick?: () => void;
  tooltip?: string;
}) {
  const button = (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={tooltip ?? label}
      data-active={active ? "true" : undefined}
      className={cn(
        "velion-navbar-action-button",
        className,
      )}
    >
      {children}
    </button>
  );

  if (!tooltip) {
    return button;
  }

  return (
    <TopLayerTooltip label={tooltip} placement="bottom">
      {button}
    </TopLayerTooltip>
  );
}

export function BadgeButton({
  children,
  count,
  label,
}: {
  children: ReactNode;
  count: number;
  label: string;
}) {
  return (
    <div className="relative">
      {children}
      {count > 0 ? (
        <span
          aria-label={`${count} unread ${label.toLowerCase()}`}
          className="pointer-events-none absolute right-0.5 top-0.5 inline-flex min-w-4 items-center justify-center rounded-full bg-[#E8F1FF] px-1 text-[9px] font-semibold leading-4 text-[#3578F6]"
        >
          {Math.min(count, 9)}
          {count > 9 ? "+" : null}
        </span>
      ) : null}
    </div>
  );
}

export function NavDivider() {
  return <div className="mx-2 h-7 w-px bg-[#E4E0D8] dark:bg-[#2E3038]" aria-hidden="true" />;
}

function BreadcrumbSeparator() {
  return <Slash className="size-3.5 shrink-0 text-[#C0C4CC] dark:text-[#62666D]" strokeWidth={2} />;
}
