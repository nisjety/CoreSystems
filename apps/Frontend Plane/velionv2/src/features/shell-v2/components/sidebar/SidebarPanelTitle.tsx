"use client";

import type { ReactNode } from "react";
import { SidebarCollapseButton } from "@/features/shell-v2/components/sidebar/SidebarCollapseButton";
import { sidebarType } from "@/features/shell-v2/lib/sidebar-style";
import { cn } from "@/lib/utils";

export function SidebarPanelTitle({
  children,
  onCollapse,
  spacing = "mb-5",
  titleClassName = "text-[#17181C] dark:text-white",
}: {
  children: ReactNode;
  onCollapse: () => void;
  spacing?: string;
  titleClassName?: string;
}) {
  return (
    <div className={cn("flex items-center justify-between gap-3", spacing)}>
      <div className={cn("truncate", titleClassName, sidebarType.title)}>{children}</div>
      <SidebarCollapseButton onCollapse={onCollapse} />
    </div>
  );
}
