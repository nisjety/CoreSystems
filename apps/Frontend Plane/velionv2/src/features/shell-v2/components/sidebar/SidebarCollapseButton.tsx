"use client";

import { ChevronLeft } from "lucide-react";
import { sidebarFocusClass } from "@/features/shell-v2/lib/sidebar-style";
import { cn } from "@/lib/utils";

export function SidebarCollapseButton({ onCollapse }: { onCollapse: () => void }) {
  return (
    <button
      type="button"
      onClick={onCollapse}
      className={cn(
        "verevon-sidebar-collapse-button",
        sidebarFocusClass,
      )}
      aria-label="Collapse sidebar"
      title="Collapse sidebar"
    >
      <ChevronLeft className="size-4" strokeWidth={1.9} />
    </button>
  );
}
