"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "@/lib/utils";

type TooltipPlacement = "top" | "right" | "bottom" | "left";

type TooltipPosition = {
  left: number;
  top: number;
};

const tooltipOffset = 10;
const viewportPadding = 8;

export function TopLayerTooltip({
  children,
  className,
  label,
  placement = "top",
}: {
  children: ReactNode;
  className?: string;
  label: string;
  placement?: TooltipPlacement;
}) {
  const triggerRef = useRef<HTMLSpanElement>(null);
  const tooltipRef = useRef<HTMLSpanElement>(null);
  const [visible, setVisible] = useState(false);
  const [position, setPosition] = useState<TooltipPosition | null>(null);

  useEffect(() => {
    if (!visible) {
      return;
    }

    const updatePosition = () => {
      const trigger = triggerRef.current;
      const tooltip = tooltipRef.current;

      if (!trigger || !tooltip) {
        return;
      }

      const triggerRect = trigger.getBoundingClientRect();
      const tooltipRect = tooltip.getBoundingClientRect();
      const next = getTooltipPosition(triggerRect, tooltipRect, placement);
      setPosition(next);
    };

    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [placement, visible]);

  return (
    <span
      ref={triggerRef}
      className={cn("inline-flex", className)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) {
          setVisible(false);
        }
      }}
      onFocus={() => setVisible(true)}
      onMouseEnter={() => setVisible(true)}
      onMouseLeave={() => setVisible(false)}
    >
      {children}
      {visible && typeof document !== "undefined"
        ? createPortal(
            <span
              ref={tooltipRef}
              className={cn(
                "velion-top-layer-tooltip pointer-events-none fixed z-[var(--velion-z-tooltip)] whitespace-nowrap rounded-[12px] border border-[#E7E7EA] bg-white px-3 py-1.5 text-[11px] font-semibold text-[#3A3C44] opacity-0 shadow-[0_12px_34px_rgba(17,17,17,0.16)] transition-opacity duration-150 dark:border-[#2A2C31] dark:bg-[#17181C] dark:text-[#F7F8F8]",
                position ? "opacity-100" : "",
              )}
              style={{
                left: position?.left ?? -9999,
                top: position?.top ?? -9999,
              }}
              role="tooltip"
            >
              {label}
              <TooltipCaret placement={placement} />
            </span>,
            document.body,
          )
        : null}
    </span>
  );
}

function getTooltipPosition(
  triggerRect: DOMRect,
  tooltipRect: DOMRect,
  placement: TooltipPlacement,
): TooltipPosition {
  const centeredLeft = triggerRect.left + triggerRect.width / 2 - tooltipRect.width / 2;
  const centeredTop = triggerRect.top + triggerRect.height / 2 - tooltipRect.height / 2;

  const rawPosition =
    placement === "right"
      ? {
          left: triggerRect.right + tooltipOffset,
          top: centeredTop,
        }
      : placement === "left"
        ? {
            left: triggerRect.left - tooltipRect.width - tooltipOffset,
            top: centeredTop,
          }
        : placement === "bottom"
          ? {
              left: centeredLeft,
              top: triggerRect.bottom + tooltipOffset,
            }
          : {
              left: centeredLeft,
              top: triggerRect.top - tooltipRect.height - tooltipOffset,
            };

  return {
    left: clamp(rawPosition.left, viewportPadding, window.innerWidth - tooltipRect.width - viewportPadding),
    top: clamp(rawPosition.top, viewportPadding, window.innerHeight - tooltipRect.height - viewportPadding),
  };
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(value, max));
}

function TooltipCaret({ placement }: { placement: TooltipPlacement }) {
  if (placement === "right") {
    return (
      <span className="absolute right-full top-1/2 h-4 w-2 -translate-y-1/2 overflow-hidden">
        <span className="absolute -right-1 top-1/2 size-2.5 -translate-y-1/2 rotate-45 border border-[#E7E7EA] bg-white dark:border-[#2A2C31] dark:bg-[#17181C]" />
      </span>
    );
  }

  if (placement === "left") {
    return (
      <span className="absolute left-full top-1/2 h-4 w-2 -translate-y-1/2 overflow-hidden">
        <span className="absolute -left-1 top-1/2 size-2.5 -translate-y-1/2 rotate-45 border border-[#E7E7EA] bg-white dark:border-[#2A2C31] dark:bg-[#17181C]" />
      </span>
    );
  }

  if (placement === "bottom") {
    return (
      <span className="absolute bottom-full left-1/2 h-2 w-4 -translate-x-1/2 overflow-hidden">
        <span className="absolute -bottom-1 left-1/2 size-2.5 -translate-x-1/2 rotate-45 border border-[#E7E7EA] bg-white dark:border-[#2A2C31] dark:bg-[#17181C]" />
      </span>
    );
  }

  return (
    <span className="absolute left-1/2 top-full h-2 w-4 -translate-x-1/2 overflow-hidden">
      <span className="absolute -top-1 left-1/2 size-2.5 -translate-x-1/2 rotate-45 border border-[#E7E7EA] bg-white dark:border-[#2A2C31] dark:bg-[#17181C]" />
    </span>
  );
}
